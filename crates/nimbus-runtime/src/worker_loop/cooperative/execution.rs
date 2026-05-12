use std::sync::Arc;
use std::time::Instant;

use tracing::debug;

use crate::error::NimbusRuntimeError;
use crate::executor::{RuntimeWorkerJob, RuntimeWorkerQueue, SharedInvocationPermit};
use crate::host::HostCallCancellation;
use crate::limits::RuntimePolicy;
use crate::runtime::{CooperativeRuntimeSlotStart, RuntimeInvocationExecution};

use super::{CooperativeInvocation, CooperativeWorkerLoop};

impl CooperativeWorkerLoop {
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
        if let Err(error) = &result {
            match error {
                NimbusRuntimeError::ExecutionTimeout(_) => metrics.record_timeout(),
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

    pub(super) async fn finish_failed_start(
        policy: Arc<RuntimePolicy>,
        worker_id: usize,
        job: RuntimeWorkerJob,
        permit: SharedInvocationPermit,
        cancellation_for_metrics: Option<HostCallCancellation>,
        execution_started_at: Instant,
        result: crate::error::Result<serde_json::Value>,
    ) -> (
        RuntimeWorkerJob,
        crate::error::Result<serde_json::Value>,
        Vec<RuntimeWorkerJob>,
    ) {
        Self::finish_invocation(
            policy,
            worker_id,
            job,
            permit,
            execution_started_at,
            cancellation_for_metrics,
            result,
        )
        .await
    }

    pub(super) fn admit_job(&mut self, queue: &Arc<dyn RuntimeWorkerQueue>, job: RuntimeWorkerJob) {
        let cancellation_for_metrics = job.cancellation.clone();
        let permit = SharedInvocationPermit::new(
            self.policy.clone(),
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
            self.policy
                .metrics()
                .record_queued_canceled_invocation_for_tenant(
                    job.context.tenant_label.as_deref(),
                    Self::cancellation_cause(&job.cancellation),
                );
            let ready_jobs = self.worker_runtime.block_on(permit.finish_invocation());
            queue.complete_job(job, Err(NimbusRuntimeError::Cancelled), ready_jobs);
            return;
        }

        self.policy.metrics().record_worker_dispatch();
        let runtime = job.runtime.clone().into_policy(self.policy.clone());
        let worker_runtime = &self.worker_runtime;
        let v8_runtime_pool = &mut self.v8_runtime_pool;
        let watchdog = self.watchdog.clone();
        let activity_signal = self.activity_signal.clone();
        let worker_id = self.worker_id;
        let start = worker_runtime.block_on(async {
            let execution_started_at = Instant::now();
            permit
                .clone()
                .acquire_initial(job.enqueued_at)
                .await
                .map_err(|error| (error, execution_started_at))?;
            debug!(
                worker_id,
                invocation_id = job.context.invocation_id,
                request_id = ?job.context.server_request_id,
                tenant = job.context.tenant_label.as_deref().unwrap_or("unknown"),
                function = %job.context.function_name,
                kind = job.context.kind,
                "runtime worker invocation started"
            );
            let slot = runtime
                .start_cooperative_locker_runtime_slot(
                    v8_runtime_pool,
                    CooperativeRuntimeSlotStart {
                        invocation: RuntimeInvocationExecution {
                            watchdog,
                            bundle: job.bundle.clone(),
                            request: job.request.clone(),
                            context: job.context.clone(),
                            external_cancellation: job.cancellation.clone(),
                            permit: permit.clone(),
                        },
                        activity_signal,
                    },
                )
                .await
                .map_err(|error| (error, execution_started_at))?;
            Ok::<_, (NimbusRuntimeError, Instant)>((slot, execution_started_at))
        });

        match start {
            Ok((slot, execution_started_at)) => {
                self.scheduler.admit_runnable(CooperativeInvocation {
                    job,
                    permit,
                    slot,
                    execution_started_at,
                    cancellation_for_metrics,
                });
            }
            Err((error, execution_started_at)) => {
                let (job, result, ready_jobs) =
                    self.worker_runtime.block_on(Self::finish_failed_start(
                        self.policy.clone(),
                        self.worker_id,
                        job,
                        permit,
                        cancellation_for_metrics,
                        execution_started_at,
                        Err(error),
                    ));
                queue.complete_job(job, result, ready_jobs);
            }
        }
    }
}
