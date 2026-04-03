use std::sync::Arc;
use std::time::Instant;

use tracing::debug;

use crate::backend::{DenoRuntimeBackendFactory, RuntimeBackendFactory, RuntimeBackendInvocation};
use crate::error::NeovexRuntimeError;
use crate::executor::{RuntimeWorkerQueue, RuntimeWorkerShutdown, SharedInvocationPermit};
use crate::host::HostCallCancellation;
use crate::limits::RuntimePolicy;
use crate::watchdog::WatchdogTimer;

pub(crate) trait WorkerLoopFactory: Send + Sync + 'static {
    fn create(&self, worker_id: usize, policy: Arc<RuntimePolicy>) -> Box<dyn WorkerLoop>;
}

pub(crate) trait WorkerLoop: Send + 'static {
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
            backend_factory: Arc::new(DenoRuntimeBackendFactory),
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
    backend: Box<dyn crate::backend::RuntimeBackend>,
    worker_runtime: tokio::runtime::Runtime,
}

impl RunToCompletionWorkerLoop {
    fn new(
        worker_id: usize,
        policy: Arc<RuntimePolicy>,
        watchdog: WatchdogTimer,
        backend: Box<dyn crate::backend::RuntimeBackend>,
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
            let Some(job) = queue.recv_blocking() else {
                break;
            };
            let cancellation_for_metrics = job.cancellation.clone();
            let mut permit = SharedInvocationPermit::new(
                self.policy.clone(),
                job.context.tenant_label.clone(),
                job.dispatch_handle.clone(),
                job.runtime.bypasses_concurrency_limit(),
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
            let (result, ready_jobs) = self.worker_runtime.block_on(async {
                let execution_started_at = Instant::now();
                let result = async {
                    permit.acquire_initial(job.enqueued_at).await?;
                    debug!(
                        worker_id = self.worker_id,
                        invocation_id = job.context.invocation_id,
                        request_id = ?job.context.server_request_id,
                        tenant = job.context.tenant_label.as_deref().unwrap_or("unknown"),
                        function = %job.context.function_name,
                        kind = job.context.kind,
                        "runtime worker invocation started"
                    );
                    self.backend
                        .invoke(RuntimeBackendInvocation {
                            watchdog: self.watchdog.clone(),
                            runtime: job.runtime.clone().into_policy(self.policy.clone()),
                            bundle: job.bundle.clone(),
                            request: job.request.clone(),
                            context: job.context.clone(),
                            cancellation: job.cancellation.clone(),
                            permit: permit.clone(),
                        })
                        .await
                }
                .await
                .inspect(|_| {
                    let execution = execution_started_at.elapsed();
                    self.policy.metrics().record_execution_for_tenant(
                        job.context.tenant_label.as_deref(),
                        execution,
                    );
                    debug!(
                        worker_id = self.worker_id,
                        invocation_id = job.context.invocation_id,
                        request_id = ?job.context.server_request_id,
                        tenant = job.context.tenant_label.as_deref().unwrap_or("unknown"),
                        function = %job.context.function_name,
                        kind = job.context.kind,
                        execution_ms = execution.as_secs_f64() * 1000.0,
                        active_isolates = self.policy.metrics().snapshot().active_isolates,
                        "runtime worker invocation completed"
                    );
                })
                .inspect_err(|error| match error {
                    NeovexRuntimeError::ExecutionTimeout(_) => {
                        self.policy.metrics().record_timeout();
                        let execution = execution_started_at.elapsed();
                        self.policy.metrics().record_execution_for_tenant(
                            job.context.tenant_label.as_deref(),
                            execution,
                        );
                    }
                    NeovexRuntimeError::Cancelled => {
                        self.policy
                            .metrics()
                            .record_in_flight_canceled_invocation_for_tenant(
                                job.context.tenant_label.as_deref(),
                                cancellation_for_metrics
                                    .as_ref()
                                    .and_then(HostCallCancellation::cause),
                            );
                        let execution = execution_started_at.elapsed();
                        self.policy.metrics().record_execution_for_tenant(
                            job.context.tenant_label.as_deref(),
                            execution,
                        );
                    }
                    _ => {
                        let execution = execution_started_at.elapsed();
                        self.policy.metrics().record_execution_for_tenant(
                            job.context.tenant_label.as_deref(),
                            execution,
                        );
                    }
                });
                let ready_jobs = permit.finish_invocation().await;
                (result, ready_jobs)
            });
            queue.complete_job(job, result, ready_jobs);
        }
    }
}
