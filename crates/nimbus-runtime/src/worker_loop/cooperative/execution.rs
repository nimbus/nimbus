use std::sync::Arc;
use std::time::Instant;

use tracing::debug;

use crate::error::NimbusRuntimeError;
use crate::executor::{
    RuntimeWorkerJob, RuntimeWorkerQueue, SharedInvocationPermit, SharedInvocationPermitAcquire,
};
use crate::host::HostCallCancellation;
use crate::limits::RuntimePolicy;

use super::backend::{CooperativeBackendDriver, CooperativeBackendInvocationStart};
use super::{CooperativeInvocation, CooperativeWorkerLoop};

enum CooperativeAdmissionStart<S> {
    Slot(S),
    DirectResult(crate::error::Result<serde_json::Value>),
    Deferred,
}

impl<D: CooperativeBackendDriver> CooperativeWorkerLoop<D> {
    pub(super) fn cancellation_cause(
        cancellation: &Option<HostCallCancellation>,
    ) -> Option<crate::host::HostCallCancellationCause> {
        cancellation.as_ref().and_then(HostCallCancellation::cause)
    }

    pub(super) async fn finish_invocation(
        policy: Arc<RuntimePolicy>,
        worker_id: usize,
        job: RuntimeWorkerJob,
        permit: SharedInvocationPermit,
        execution_started_at: Instant,
        cancellation_for_metrics: Option<HostCallCancellation>,
        result: crate::error::Result<serde_json::Value>,
    ) -> (
        RuntimeWorkerJob,
        crate::error::Result<serde_json::Value>,
        Vec<RuntimeWorkerJob>,
    ) {
        let metrics = policy.metrics();
        let runtime_profile = policy.runtime_profile();
        if let Err(error) = &result {
            match error {
                NimbusRuntimeError::ExecutionTimeout(_) | NimbusRuntimeError::SystemTimeout(_) => {
                    metrics.record_timeout()
                }
                NimbusRuntimeError::Cancelled => {
                    metrics.record_in_flight_canceled_invocation_for_tenant(
                        job.context.tenant_label.as_deref(),
                        cancellation_for_metrics
                            .as_ref()
                            .and_then(HostCallCancellation::cause),
                    );
                }
                _ => {}
            }
        }

        let execution = execution_started_at.elapsed();
        metrics.record_execution_for_tenant(job.context.tenant_label.as_deref(), execution);
        metrics.record_profile_execution(runtime_profile, execution);
        if result.is_ok() {
            debug!(
                worker_id,
                invocation_id = job.context.invocation_id,
                request_id = ?job.context.server_request_id,
                tenant = job.context.tenant_label.as_deref().unwrap_or("unknown"),
                function = %job.context.function_name,
                kind = job.context.kind,
                execution_ms = execution.as_secs_f64() * 1000.0,
                active_runtime_instances = metrics.snapshot().active_runtime_instances,
                "runtime worker invocation completed"
            );
        }
        let ready_jobs = permit.finish_invocation().await;
        (job, result, ready_jobs)
    }

    pub(super) fn admit_job(&mut self, queue: &Arc<dyn RuntimeWorkerQueue>, job: RuntimeWorkerJob) {
        let deferred = self.admit_job_inner(queue, job, true);
        debug_assert!(
            deferred.is_none(),
            "blocking cooperative admission should not defer jobs"
        );
        if let Some(job) = deferred {
            self.pending_admissions.push_front(job);
        }
    }

    pub(super) fn try_admit_job(
        &mut self,
        queue: &Arc<dyn RuntimeWorkerQueue>,
        job: RuntimeWorkerJob,
    ) -> Option<RuntimeWorkerJob> {
        self.admit_job_inner(queue, job, false)
    }

    fn admit_job_inner(
        &mut self,
        queue: &Arc<dyn RuntimeWorkerQueue>,
        mut job: RuntimeWorkerJob,
        allow_blocking_acquire: bool,
    ) -> Option<RuntimeWorkerJob> {
        let cancellation_for_metrics = job.cancellation.clone();
        let job_policy = job.policy.clone();
        let permit = SharedInvocationPermit::new(
            job_policy.clone(),
            job.context.tenant_label.clone(),
            job.dispatch_handle.clone(),
            job.context.bypasses_concurrency_limit(),
            job.cancellation.clone(),
        );

        if job
            .cancellation
            .as_ref()
            .is_some_and(HostCallCancellation::is_cancelled)
        {
            job_policy
                .metrics()
                .record_queued_canceled_invocation_for_tenant(
                    job.context.tenant_label.as_deref(),
                    Self::cancellation_cause(&job.cancellation),
                );
            let ready_jobs = self.worker_runtime.block_on(permit.finish_invocation());
            queue.complete_job(job, Err(NimbusRuntimeError::Cancelled), ready_jobs);
            return None;
        }

        if let Err(error) =
            crate::retained_state::validate_retained_state_admission(&job_policy, &job.context)
        {
            let ready_jobs = self.worker_runtime.block_on(permit.finish_invocation());
            queue.complete_job(job, Err(error), ready_jobs);
            return None;
        }

        let worker_runtime = &self.worker_runtime;
        let watchdog = self.watchdog.clone();
        let activity_signal = self.activity_signal.clone();
        let worker_id = self.worker_id;
        let start = worker_runtime.block_on(async {
            let execution_started_at = Instant::now();
            let mut permit_for_acquire = permit.clone();
            if allow_blocking_acquire {
                permit_for_acquire
                    .acquire_initial(job.enqueued_at)
                    .await
                    .map_err(|error| (error, execution_started_at))?;
            } else {
                match permit_for_acquire
                    .try_acquire_initial(job.enqueued_at)
                    .map_err(|error| (error, execution_started_at))?
                {
                    SharedInvocationPermitAcquire::Acquired => {}
                    SharedInvocationPermitAcquire::WouldBlock => {
                        return Ok::<_, (NimbusRuntimeError, Instant)>((
                            CooperativeAdmissionStart::Deferred,
                            execution_started_at,
                        ));
                    }
                }
            }
            job_policy.metrics().record_worker_dispatch();
            debug!(
                worker_id,
                invocation_id = job.context.invocation_id,
                request_id = ?job.context.server_request_id,
                tenant = job.context.tenant_label.as_deref().unwrap_or("unknown"),
                function = %job.context.function_name,
                kind = job.context.kind,
                "runtime worker invocation started"
            );

            let start = CooperativeBackendInvocationStart {
                watchdog,
                host: job.host.clone(),
                policy: job_policy.clone(),
                bundle: job.bundle.clone(),
                request: job.request.clone(),
                context: job.context.clone(),
                execution_plan: job.execution_plan.clone(),
                cancellation: job.cancellation.clone(),
                response_ready_tx: job.response_ready_tx.take(),
                permit: permit.clone(),
                activity_signal,
            };
            if !self.driver.permits_scheduler_admission(&job.execution_plan) {
                let result = self.driver.invoke_direct(start).await;
                return Ok::<_, (NimbusRuntimeError, Instant)>((
                    CooperativeAdmissionStart::DirectResult(result),
                    execution_started_at,
                ));
            }

            let slot = self
                .driver
                .start_slot(start)
                .await
                .map_err(|error| (error, execution_started_at))?;
            Ok::<_, (NimbusRuntimeError, Instant)>((
                CooperativeAdmissionStart::Slot(slot),
                execution_started_at,
            ))
        });

        match start {
            Ok((CooperativeAdmissionStart::Slot(slot), execution_started_at)) => {
                self.scheduler.admit_runnable(CooperativeInvocation {
                    job,
                    permit,
                    slot,
                    execution_started_at,
                    cancellation_for_metrics,
                });
                None
            }
            Ok((CooperativeAdmissionStart::DirectResult(result), execution_started_at)) => {
                let (job, result, ready_jobs) =
                    self.worker_runtime.block_on(Self::finish_invocation(
                        job_policy.clone(),
                        self.worker_id,
                        job,
                        permit,
                        execution_started_at,
                        cancellation_for_metrics,
                        result,
                    ));
                queue.complete_job(job, result, ready_jobs);
                None
            }
            Ok((CooperativeAdmissionStart::Deferred, _execution_started_at)) => {
                debug!(
                    worker_id,
                    invocation_id = job.context.invocation_id,
                    request_id = ?job.context.server_request_id,
                    tenant = job.context.tenant_label.as_deref().unwrap_or("unknown"),
                    function = %job.context.function_name,
                    kind = job.context.kind,
                        "runtime worker deferred admission behind parked cooperative slots"
                );
                Some(job)
            }
            Err((error, execution_started_at)) => {
                let (job, result, ready_jobs) =
                    self.worker_runtime.block_on(Self::finish_invocation(
                        job_policy,
                        self.worker_id,
                        job,
                        permit,
                        execution_started_at,
                        cancellation_for_metrics,
                        Err(error),
                    ));
                queue.complete_job(job, result, ready_jobs);
                None
            }
        }
    }
}
