use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

use serde_json::Value;
use tokio::sync::{mpsc, oneshot};
use tracing::debug;

use crate::context::RuntimeInvocationContext;
use crate::error::{NeovexRuntimeError, Result};
use crate::host::HostCallCancellation;
use crate::limits::RuntimePolicy;
use crate::runtime::{InvocationRequest, NeovexRuntime, RuntimeBundle};

struct RuntimeWorkerJob {
    runtime: NeovexRuntime,
    bundle: RuntimeBundle,
    request: InvocationRequest,
    context: RuntimeInvocationContext,
    cancellation: Option<HostCallCancellation>,
    enqueued_at: Instant,
    result_tx: oneshot::Sender<Result<Value>>,
}

#[derive(Clone)]
pub struct RuntimeExecutor {
    inner: Arc<RuntimeExecutorInner>,
}

struct RuntimeExecutorInner {
    policy: Arc<RuntimePolicy>,
    sender: Mutex<Option<mpsc::Sender<RuntimeWorkerJob>>>,
    worker_count: usize,
    queue_capacity: usize,
    worker_handles: Mutex<Vec<JoinHandle<()>>>,
}

impl std::fmt::Debug for RuntimeExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeExecutor")
            .field("worker_count", &self.inner.worker_count)
            .field("queue_capacity", &self.inner.queue_capacity)
            .finish()
    }
}

impl RuntimeExecutor {
    pub fn new(policy: Arc<RuntimePolicy>) -> Self {
        let worker_count = policy.limits().max_concurrent_isolates.max(1);
        let queue_capacity = worker_count.saturating_mul(4).max(1);
        let (sender, receiver) = mpsc::channel::<RuntimeWorkerJob>(queue_capacity);
        let receiver = Arc::new(Mutex::new(receiver));
        let mut worker_handles = Vec::with_capacity(worker_count);

        for worker_id in 0..worker_count {
            let receiver = receiver.clone();
            let policy = policy.clone();
            let handle = std::thread::Builder::new()
                .name(format!("neovex-runtime-worker-{worker_id}"))
                .spawn(move || {
                    loop {
                        let job = {
                            let mut receiver = receiver
                                .lock()
                                .expect("runtime executor receiver lock should not be poisoned");
                            receiver.blocking_recv()
                        };
                        let Some(job) = job else {
                            break;
                        };

                        if job
                            .cancellation
                            .as_ref()
                            .is_some_and(HostCallCancellation::is_cancelled)
                        {
                            policy
                                .metrics()
                                .record_queued_canceled_invocation_for_tenant(
                                    job.context.tenant_label.as_deref(),
                                );
                            let _ = job.result_tx.send(Err(NeovexRuntimeError::Cancelled));
                            continue;
                        }

                        policy.metrics().record_worker_dispatch();

                        let tokio_runtime = match tokio::runtime::Builder::new_current_thread()
                            .enable_all()
                            .build()
                        {
                            Ok(runtime) => runtime,
                            Err(error) => {
                                let _ =
                                    job.result_tx.send(Err(NeovexRuntimeError::Contract(format!(
                                        "runtime worker failed to build tokio runtime: {error}"
                                    ))));
                                continue;
                            }
                        };

                        let result = tokio_runtime.block_on(Self::invoke_job(
                            policy.clone(),
                            job.runtime,
                            job.bundle,
                            job.request,
                            job.context,
                            job.cancellation,
                            job.enqueued_at,
                        ));
                        let _ = job.result_tx.send(result);
                    }
                })
                .expect("runtime executor worker thread should start");
            worker_handles.push(handle);
        }

        Self {
            inner: Arc::new(RuntimeExecutorInner {
                policy,
                sender: Mutex::new(Some(sender)),
                worker_count,
                queue_capacity,
                worker_handles: Mutex::new(worker_handles),
            }),
        }
    }

    pub fn policy(&self) -> Arc<RuntimePolicy> {
        self.inner.policy.clone()
    }

    async fn invoke_job(
        policy: Arc<RuntimePolicy>,
        runtime: NeovexRuntime,
        bundle: RuntimeBundle,
        request: InvocationRequest,
        context: RuntimeInvocationContext,
        cancellation: Option<HostCallCancellation>,
        queue_started_at: Instant,
    ) -> Result<Value> {
        let metrics = policy.metrics();
        let _permit = if runtime.bypasses_concurrency_limit() {
            None
        } else {
            metrics.increment_queued_invocations();
            Some(
                policy
                    .isolate_semaphore()
                    .acquire_owned()
                    .await
                    .map_err(|_| {
                        NeovexRuntimeError::Contract(
                            "runtime isolate semaphore unexpectedly closed".to_string(),
                        )
                    })?,
            )
        };
        if !runtime.bypasses_concurrency_limit() {
            metrics.decrement_queued_invocations();
            let queue_wait = queue_started_at.elapsed();
            metrics.record_queue_wait_for_tenant(context.tenant_label.as_deref(), queue_wait);
            debug!(
                invocation_id = context.invocation_id,
                tenant = context.tenant_label.as_deref().unwrap_or("unknown"),
                function = %context.function_name,
                kind = context.kind,
                queue_wait_ms = queue_wait.as_secs_f64() * 1000.0,
                queued_invocations = metrics.snapshot().queued_invocations,
                "runtime invocation admitted"
            );
        }

        metrics.increment_active_isolates_for_tenant(context.tenant_label.as_deref());
        let execution_started_at = Instant::now();
        runtime
            .invoke_bundle_unmanaged(&bundle, &request, &context, cancellation)
            .await
            .inspect_err(|error| match error {
                NeovexRuntimeError::ExecutionTimeout(_) => metrics.record_timeout(),
                NeovexRuntimeError::Cancelled => metrics
                    .record_in_flight_canceled_invocation_for_tenant(
                        context.tenant_label.as_deref(),
                    ),
                _ => {}
            })
            .inspect(|_| {
                let execution = execution_started_at.elapsed();
                metrics.record_execution_for_tenant(context.tenant_label.as_deref(), execution);
                debug!(
                    invocation_id = context.invocation_id,
                    tenant = context.tenant_label.as_deref().unwrap_or("unknown"),
                    function = %context.function_name,
                    kind = context.kind,
                    execution_ms = execution.as_secs_f64() * 1000.0,
                    active_isolates = metrics.snapshot().active_isolates,
                    "runtime invocation completed"
                );
            })
            .inspect_err(|_| {
                let execution = execution_started_at.elapsed();
                metrics.record_execution_for_tenant(context.tenant_label.as_deref(), execution);
            })
            .inspect(|_| {
                metrics.decrement_active_isolates_for_tenant(context.tenant_label.as_deref())
            })
            .inspect_err(|_| {
                metrics.decrement_active_isolates_for_tenant(context.tenant_label.as_deref())
            })
    }

    pub async fn invoke(
        &self,
        runtime: NeovexRuntime,
        bundle: RuntimeBundle,
        request: InvocationRequest,
        context: RuntimeInvocationContext,
    ) -> Result<Value> {
        self.invoke_with_cancellation(runtime, bundle, request, context, None)
            .await
    }

    pub async fn invoke_with_cancellation(
        &self,
        runtime: NeovexRuntime,
        bundle: RuntimeBundle,
        request: InvocationRequest,
        context: RuntimeInvocationContext,
        cancellation: Option<HostCallCancellation>,
    ) -> Result<Value> {
        Self::invoke_job(
            self.inner.policy.clone(),
            runtime,
            bundle,
            request,
            context,
            cancellation,
            Instant::now(),
        )
        .await
    }

    pub async fn invoke_on_worker(
        &self,
        runtime: NeovexRuntime,
        bundle: RuntimeBundle,
        request: InvocationRequest,
        context: RuntimeInvocationContext,
        cancellation: Option<HostCallCancellation>,
    ) -> Result<Value> {
        if cancellation
            .as_ref()
            .is_some_and(HostCallCancellation::is_cancelled)
        {
            self.inner
                .policy
                .metrics()
                .record_queued_canceled_invocation_for_tenant(context.tenant_label.as_deref());
            return Err(NeovexRuntimeError::Cancelled);
        }

        let sender = self
            .inner
            .sender
            .lock()
            .expect("runtime executor sender lock should not be poisoned")
            .as_ref()
            .cloned()
            .ok_or_else(|| {
                NeovexRuntimeError::Contract("runtime executor unexpectedly closed".to_string())
            })?;
        let (result_tx, result_rx) = oneshot::channel();
        sender
            .send(RuntimeWorkerJob {
                runtime,
                bundle,
                request,
                context,
                cancellation: cancellation.clone(),
                enqueued_at: Instant::now(),
                result_tx,
            })
            .await
            .map_err(|_| {
                NeovexRuntimeError::Contract("runtime executor unexpectedly closed".to_string())
            })?;

        match cancellation {
            Some(cancellation) => {
                tokio::select! {
                    _ = cancellation.cancelled() => Err(NeovexRuntimeError::Cancelled),
                    result = result_rx => result.map_err(|_| {
                        NeovexRuntimeError::Contract(
                            "runtime executor dropped an invocation result".to_string(),
                        )
                    })?,
                }
            }
            None => result_rx.await.map_err(|_| {
                NeovexRuntimeError::Contract(
                    "runtime executor dropped an invocation result".to_string(),
                )
            })?,
        }
    }

    pub fn invoke_blocking(
        &self,
        runtime: NeovexRuntime,
        bundle: RuntimeBundle,
        request: InvocationRequest,
        context: RuntimeInvocationContext,
    ) -> Result<Value> {
        self.invoke_blocking_with_cancellation(runtime, bundle, request, context, None)
    }

    pub fn invoke_blocking_with_cancellation(
        &self,
        runtime: NeovexRuntime,
        bundle: RuntimeBundle,
        request: InvocationRequest,
        context: RuntimeInvocationContext,
        cancellation: Option<HostCallCancellation>,
    ) -> Result<Value> {
        let executor = self.clone();
        let invoke = move || {
            let tokio_runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            tokio_runtime.block_on(executor.invoke_on_worker(
                runtime,
                bundle,
                request,
                context,
                cancellation,
            ))
        };

        if tokio::runtime::Handle::try_current().is_ok() {
            std::thread::spawn(invoke).join().map_err(|_| {
                NeovexRuntimeError::Contract(
                    "runtime executor invocation thread panicked".to_string(),
                )
            })?
        } else {
            invoke()
        }
    }
}

impl Drop for RuntimeExecutorInner {
    fn drop(&mut self) {
        self.sender
            .lock()
            .expect("runtime executor sender lock should not be poisoned")
            .take();
        let mut worker_handles = self
            .worker_handles
            .lock()
            .expect("runtime executor worker handle lock should not be poisoned");
        for handle in worker_handles.drain(..) {
            let _ = handle.join();
        }
    }
}

impl Default for RuntimeExecutor {
    fn default() -> Self {
        let policy = Arc::new(RuntimePolicy::default());
        Self::new(policy)
    }
}
