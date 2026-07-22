use std::sync::Arc;

use crate::backends::{
    RuntimeBackend, RuntimeBackendInvocation, create_runtime_backend_for_policy,
};
use crate::error::NimbusRuntimeError;
use crate::executor::{
    RuntimeWorkerControl, RuntimeWorkerControlCommand, RuntimeWorkerMessage, RuntimeWorkerQueue,
    RuntimeWorkerRetirementAck, RuntimeWorkerShutdown, SharedInvocationPermit,
    run_invocation_lifecycle,
};
use crate::host::HostCallCancellation;
use crate::limits::{RuntimeBackendKind, RuntimePolicy};
use crate::watchdog::WatchdogTimer;

pub(crate) trait WorkerLoopFactory: Send + Sync + 'static {
    fn create(&self, worker_id: usize, policy: Arc<RuntimePolicy>) -> Box<dyn WorkerLoop>;
}

/// Worker loops are created inside their worker thread and may therefore own
/// thread-affine runtime state such as `JsRuntime`.
pub(crate) trait WorkerLoop: 'static {
    fn run(&mut self, queue: Arc<dyn RuntimeWorkerQueue>, shutdown: RuntimeWorkerShutdown);
}

pub(crate) struct RunToCompletionWorkerLoopFactory {
    watchdog: WatchdogTimer,
    #[cfg(test)]
    test_state: Option<Arc<crate::executor::RuntimeExecutorTestState>>,
}

impl RunToCompletionWorkerLoopFactory {
    pub(crate) fn new(watchdog: WatchdogTimer) -> Self {
        Self {
            watchdog,
            #[cfg(test)]
            test_state: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_test_state(
        mut self,
        test_state: Arc<crate::executor::RuntimeExecutorTestState>,
    ) -> Self {
        self.test_state = Some(test_state);
        self
    }
}

impl WorkerLoopFactory for RunToCompletionWorkerLoopFactory {
    fn create(&self, worker_id: usize, policy: Arc<RuntimePolicy>) -> Box<dyn WorkerLoop> {
        Box::new(RunToCompletionWorkerLoop::new(
            worker_id,
            policy.clone(),
            self.watchdog.clone(),
            create_runtime_backend_for_policy(&policy),
            #[cfg(test)]
            self.test_state.clone(),
        ))
    }
}

struct RunToCompletionWorkerLoop {
    worker_id: usize,
    watchdog: WatchdogTimer,
    policy: Arc<RuntimePolicy>,
    backend_kind: RuntimeBackendKind,
    backend: Box<dyn RuntimeBackend>,
    worker_runtime: tokio::runtime::Runtime,
}

impl RunToCompletionWorkerLoop {
    fn new(
        worker_id: usize,
        policy: Arc<RuntimePolicy>,
        watchdog: WatchdogTimer,
        backend: Box<dyn RuntimeBackend>,
        #[cfg(test)] test_state: Option<Arc<crate::executor::RuntimeExecutorTestState>>,
    ) -> Self {
        let worker_runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap_or_else(|error| {
                panic!("runtime worker failed to build tokio runtime: {error}")
            });
        #[cfg(test)]
        if let Some(test_state) = &test_state {
            test_state.register_current_worker_runtime();
        }
        Self {
            worker_id,
            watchdog,
            policy: policy.clone(),
            backend_kind: policy.limits().backend_kind,
            backend,
            worker_runtime,
        }
    }

    fn cancellation_cause(
        cancellation: &Option<HostCallCancellation>,
    ) -> Option<crate::host::HostCallCancellationCause> {
        cancellation.as_ref().and_then(HostCallCancellation::cause)
    }

    fn process_control(&mut self, control: RuntimeWorkerControl) {
        let retained_entries_purged = match &control.command {
            RuntimeWorkerControlCommand::RetireOwner(owner_id) => {
                self.backend.retire_owner(owner_id)
            }
            RuntimeWorkerControlCommand::RetireDeploymentAuthority(authority_id) => {
                self.backend.retire_deployment_authority(authority_id)
            }
        };
        if self.backend_kind == RuntimeBackendKind::V8 {
            for _ in 0..retained_entries_purged {
                self.policy
                    .metrics()
                    .decrement_retained_runtime_pool_entries();
                self.policy
                    .metrics()
                    .record_retained_runtime_pool_retirement();
            }
        }
        let _ = control.acknowledged.send(RuntimeWorkerRetirementAck {
            worker_id: self.worker_id,
            retained_entries_purged,
        });
    }
}

impl WorkerLoop for RunToCompletionWorkerLoop {
    fn run(&mut self, queue: Arc<dyn RuntimeWorkerQueue>, shutdown: RuntimeWorkerShutdown) {
        while !shutdown.is_cancelled() {
            let Some(message) = queue.try_recv().or_else(|| queue.recv_blocking()) else {
                break;
            };
            let job = match message {
                RuntimeWorkerMessage::Job(job) => *job,
                RuntimeWorkerMessage::Control(control) => {
                    self.process_control(control);
                    continue;
                }
            };
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
                continue;
            }

            if let Err(error) =
                crate::retained_state::validate_retained_state_admission(&job_policy, &job.context)
            {
                let ready_jobs = self.worker_runtime.block_on(permit.finish_invocation());
                queue.complete_job(job, Err(error), ready_jobs);
                continue;
            }

            job_policy.metrics().record_worker_dispatch();
            if self.backend_kind != job_policy.limits().backend_kind {
                self.backend_kind = job_policy.limits().backend_kind;
                self.backend = create_runtime_backend_for_policy(&job_policy);
            }
            let (result, ready_jobs) = self.worker_runtime.block_on(run_invocation_lifecycle(
                permit,
                job_policy.clone(),
                job.context.clone(),
                cancellation_for_metrics,
                job.enqueued_at,
                Some(self.worker_id),
                |permit| {
                    self.backend.invoke(RuntimeBackendInvocation {
                        watchdog: self.watchdog.clone(),
                        host: job.host.clone(),
                        policy: job_policy.clone(),
                        bundle: job.bundle.clone(),
                        request: job.request.clone(),
                        context: job.context.clone(),
                        cancellation: job.cancellation.clone(),
                        permit,
                    })
                },
            ));
            queue.complete_job(job, result, ready_jobs);
        }
    }
}
