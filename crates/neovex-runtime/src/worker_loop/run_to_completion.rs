use std::sync::Arc;

use crate::backends::v8::V8RuntimeBackendFactory;
use crate::backends::{RuntimeBackend, RuntimeBackendFactory, RuntimeBackendInvocation};
use crate::error::NeovexRuntimeError;
use crate::executor::{
    RuntimeWorkerQueue, RuntimeWorkerShutdown, SharedInvocationPermit, run_invocation_lifecycle,
};
use crate::host::HostCallCancellation;
use crate::limits::RuntimePolicy;
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
    backend_factory: Arc<dyn RuntimeBackendFactory>,
    watchdog: WatchdogTimer,
    #[cfg(test)]
    test_state: Option<Arc<crate::executor::RuntimeExecutorTestState>>,
}

impl RunToCompletionWorkerLoopFactory {
    pub(crate) fn new(watchdog: WatchdogTimer) -> Self {
        Self {
            backend_factory: Arc::new(V8RuntimeBackendFactory),
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
            policy,
            self.watchdog.clone(),
            self.backend_factory.create(),
            #[cfg(test)]
            self.test_state.clone(),
        ))
    }
}

struct RunToCompletionWorkerLoop {
    worker_id: usize,
    policy: Arc<RuntimePolicy>,
    watchdog: WatchdogTimer,
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
            policy,
            watchdog,
            backend,
            worker_runtime,
        }
    }

    fn cancellation_cause(
        cancellation: &Option<HostCallCancellation>,
    ) -> Option<crate::host::HostCallCancellationCause> {
        cancellation.as_ref().and_then(HostCallCancellation::cause)
    }
}

impl WorkerLoop for RunToCompletionWorkerLoop {
    fn run(&mut self, queue: Arc<dyn RuntimeWorkerQueue>, shutdown: RuntimeWorkerShutdown) {
        while !shutdown.is_cancelled() {
            let Some(job) = queue.try_recv().or_else(|| queue.recv_blocking()) else {
                break;
            };
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
                queue.complete_job(job, Err(NeovexRuntimeError::Cancelled), ready_jobs);
                continue;
            }

            self.policy.metrics().record_worker_dispatch();
            let (result, ready_jobs) = self.worker_runtime.block_on(run_invocation_lifecycle(
                permit,
                self.policy.clone(),
                job.context.clone(),
                cancellation_for_metrics,
                job.enqueued_at,
                Some(self.worker_id),
                |permit| {
                    self.backend.invoke(RuntimeBackendInvocation {
                        watchdog: self.watchdog.clone(),
                        runtime: job.runtime.clone().into_policy(self.policy.clone()),
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
