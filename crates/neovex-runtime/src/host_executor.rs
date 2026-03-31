use std::sync::{Arc, Mutex};

use serde_json::Value;
use tokio::sync::{mpsc, oneshot};

use crate::error::{NeovexRuntimeError, Result};
use crate::host::HostCallCancellation;
use crate::limits::RuntimePolicy;
use crate::metrics::RuntimeMetrics;

type RuntimeHostTask = Box<dyn FnOnce() -> Result<Value> + Send + 'static>;

struct RuntimeHostJob {
    cancellation: HostCallCancellation,
    task: RuntimeHostTask,
    result_tx: oneshot::Sender<Result<Value>>,
}

#[derive(Clone)]
pub struct RuntimeHostExecutor {
    inner: Arc<RuntimeHostExecutorInner>,
}

struct RuntimeHostExecutorInner {
    sender: Mutex<Option<mpsc::Sender<RuntimeHostJob>>>,
    worker_count: usize,
    queue_capacity: usize,
    metrics: Arc<RuntimeMetrics>,
    worker_handles: Mutex<Vec<std::thread::JoinHandle<()>>>,
}

impl std::fmt::Debug for RuntimeHostExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeHostExecutor")
            .field("worker_count", &self.inner.worker_count)
            .field("queue_capacity", &self.inner.queue_capacity)
            .finish()
    }
}

impl RuntimeHostExecutor {
    pub fn new(policy: Arc<RuntimePolicy>) -> Self {
        let worker_count = policy.limits().max_concurrent_isolates.max(1);
        let queue_capacity = worker_count.saturating_mul(4).max(1);
        let (sender, receiver) = mpsc::channel::<RuntimeHostJob>(queue_capacity);
        let receiver = Arc::new(Mutex::new(receiver));
        let mut worker_handles = Vec::with_capacity(worker_count);

        for worker_id in 0..worker_count {
            let receiver = receiver.clone();
            let handle = std::thread::Builder::new()
                .name(format!("neovex-runtime-host-{worker_id}"))
                .spawn(move || {
                    loop {
                        let job = {
                            let mut receiver = receiver.lock().expect(
                                "runtime host executor receiver lock should not be poisoned",
                            );
                            receiver.blocking_recv()
                        };
                        let Some(job) = job else {
                            break;
                        };

                        if job.cancellation.is_cancelled() {
                            let _ = job.result_tx.send(Err(NeovexRuntimeError::Cancelled));
                            continue;
                        }

                        let result = (job.task)();
                        if job.cancellation.is_cancelled() {
                            let _ = job.result_tx.send(Err(NeovexRuntimeError::Cancelled));
                        } else {
                            let _ = job.result_tx.send(result);
                        }
                    }
                })
                .expect("runtime host executor worker thread should start");
            worker_handles.push(handle);
        }

        Self {
            inner: Arc::new(RuntimeHostExecutorInner {
                sender: Mutex::new(Some(sender)),
                worker_count,
                queue_capacity,
                metrics: policy.metrics(),
                worker_handles: Mutex::new(worker_handles),
            }),
        }
    }

    pub async fn submit<F>(&self, cancellation: HostCallCancellation, task: F) -> Result<Value>
    where
        F: FnOnce() -> Result<Value> + Send + 'static,
    {
        if cancellation.is_cancelled() {
            self.inner.metrics.record_canceled_host_op();
            return Err(NeovexRuntimeError::Cancelled);
        }

        let (result_tx, result_rx) = oneshot::channel();
        let sender = self
            .inner
            .sender
            .lock()
            .expect("runtime host executor sender lock should not be poisoned")
            .as_ref()
            .cloned()
            .ok_or_else(|| {
                NeovexRuntimeError::Contract(
                    "runtime host executor unexpectedly closed".to_string(),
                )
            })?;
        sender
            .send(RuntimeHostJob {
                cancellation: cancellation.clone(),
                task: Box::new(task),
                result_tx,
            })
            .await
            .map_err(|_| {
                NeovexRuntimeError::Contract(
                    "runtime host executor unexpectedly closed".to_string(),
                )
            })?;

        tokio::select! {
            _ = cancellation.cancelled() => {
                self.inner.metrics.record_canceled_host_op();
                Err(NeovexRuntimeError::Cancelled)
            },
            result = result_rx => result.map_err(|_| {
                NeovexRuntimeError::Contract(
                    "runtime host executor dropped a job result".to_string(),
                )
            })?,
        }
    }
}

impl Drop for RuntimeHostExecutorInner {
    fn drop(&mut self) {
        self.sender
            .lock()
            .expect("runtime host executor sender lock should not be poisoned")
            .take();
        let mut worker_handles = self
            .worker_handles
            .lock()
            .expect("runtime host executor worker handle lock should not be poisoned");
        for handle in worker_handles.drain(..) {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc as std_mpsc;
    use std::time::Duration;

    use serde_json::json;
    use tokio::sync::oneshot;

    use super::RuntimeHostExecutor;
    use crate::{HostCallCancellation, NeovexRuntimeError, RuntimeLimits, RuntimePolicy};

    #[tokio::test]
    async fn runtime_host_executor_skips_canceled_queued_jobs() {
        let policy = Arc::new(RuntimePolicy::new(RuntimeLimits {
            max_concurrent_isolates: 1,
            ..RuntimeLimits::default()
        }));
        let executor = RuntimeHostExecutor::new(policy.clone());

        let (started_tx, started_rx) = oneshot::channel();
        let (release_tx, release_rx) = std_mpsc::channel();

        let first = executor.clone();
        let first_handle = tokio::spawn(async move {
            first
                .submit(HostCallCancellation::default(), move || {
                    started_tx
                        .send(())
                        .expect("first host task should signal start");
                    release_rx
                        .recv()
                        .expect("first host task should wait for release");
                    Ok(json!("first"))
                })
                .await
        });

        tokio::time::timeout(Duration::from_secs(1), started_rx)
            .await
            .expect("first host task should begin promptly")
            .expect("first host task should signal start");

        let canceled = HostCallCancellation::default();
        let queued_runs = Arc::new(AtomicUsize::new(0));
        let queued_runs_clone = queued_runs.clone();
        let canceled_future = executor.submit(canceled.clone(), move || {
            queued_runs_clone.fetch_add(1, Ordering::SeqCst);
            Ok(json!("second"))
        });
        canceled.cancel();

        let canceled_result = tokio::time::timeout(Duration::from_secs(1), canceled_future)
            .await
            .expect("canceled host task should resolve promptly");
        assert!(matches!(
            canceled_result,
            Err(NeovexRuntimeError::Cancelled)
        ));

        release_tx
            .send(())
            .expect("first host task should be releasable");
        let first_result = tokio::time::timeout(Duration::from_secs(1), first_handle)
            .await
            .expect("first host task should finish promptly after release")
            .expect("first host task join should succeed");
        assert_eq!(
            first_result.expect("first host task should succeed"),
            json!("first")
        );

        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(queued_runs.load(Ordering::SeqCst), 0);
        assert_eq!(policy.metrics_snapshot().canceled_host_ops, 1);
    }
}
