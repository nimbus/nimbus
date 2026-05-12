#[cfg(test)]
use std::collections::HashMap;
#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
#[cfg(test)]
use std::thread::ThreadId;
use std::time::Duration;

use crate::limits::RuntimePolicy;
use crate::watchdog::WatchdogTimer;
use crate::worker_loop::{WorkerLoopFactory, create_worker_loop_factory};

use super::admission::RuntimeExecutorAdmission;
use super::queue::{RuntimeWorkerQueue, RuntimeWorkerRouter, RuntimeWorkerShutdown};

#[derive(Clone)]
pub struct RuntimeExecutor {
    pub(super) inner: Arc<RuntimeExecutorInner>,
}

pub(super) struct RuntimeExecutorInner {
    pub(super) policy: Arc<RuntimePolicy>,
    pub(super) router: Arc<RuntimeWorkerRouter>,
    pub(super) admission: Arc<RuntimeExecutorAdmission>,
    pub(super) shutdown: RuntimeWorkerShutdown,
    pub(super) watchdog: WatchdogTimer,
    pub(super) worker_count: usize,
    pub(super) queue_capacity: usize,
    pub(super) worker_handles: Mutex<Vec<JoinHandle<()>>>,
    #[cfg(test)]
    pub(super) test_state: Arc<RuntimeExecutorTestState>,
}

#[cfg(test)]
pub(crate) struct RuntimeExecutorTestState {
    next_worker_runtime_id: AtomicUsize,
    worker_runtime_builds: AtomicUsize,
    worker_thread_runtime_ids: Mutex<HashMap<ThreadId, usize>>,
}

#[cfg(test)]
impl RuntimeExecutorTestState {
    fn new() -> Self {
        Self {
            next_worker_runtime_id: AtomicUsize::new(1),
            worker_runtime_builds: AtomicUsize::new(0),
            worker_thread_runtime_ids: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn register_current_worker_runtime(&self) {
        let worker_runtime_id = self.next_worker_runtime_id.fetch_add(1, Ordering::Relaxed);
        self.worker_runtime_builds.fetch_add(1, Ordering::Relaxed);
        self.worker_thread_runtime_ids
            .lock()
            .expect("runtime executor test state lock should not be poisoned")
            .insert(std::thread::current().id(), worker_runtime_id);
    }

    pub(crate) fn worker_runtime_builds(&self) -> usize {
        self.worker_runtime_builds.load(Ordering::Relaxed)
    }

    pub(crate) fn worker_runtime_id_for_current_thread(&self) -> Option<usize> {
        self.worker_thread_runtime_ids
            .lock()
            .expect("runtime executor test state lock should not be poisoned")
            .get(&std::thread::current().id())
            .copied()
    }
}

pub(super) const BLOCKING_RESULT_POLL_INTERVAL: Duration = Duration::from_millis(1);

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
        let worker_count = policy.limits().worker_threads.max(1);
        let queue_capacity = worker_count.saturating_mul(4).max(1);
        let per_worker_queue_capacity = queue_capacity.div_ceil(worker_count).max(1);
        let (router, worker_queues) = RuntimeWorkerRouter::new(
            worker_count,
            per_worker_queue_capacity,
            policy.metrics(),
            policy.limits().routing_affinity,
            policy.limits().routing_affinity_max_entries,
        );
        let admission = Arc::new(RuntimeExecutorAdmission::new(policy.clone()));
        let shutdown = RuntimeWorkerShutdown::new();
        let watchdog = WatchdogTimer::new();
        #[cfg(test)]
        let test_state = Arc::new(RuntimeExecutorTestState::new());
        let worker_loop_factory: Arc<dyn WorkerLoopFactory> = create_worker_loop_factory(
            policy.clone(),
            watchdog.clone(),
            #[cfg(test)]
            test_state.clone(),
        );
        let mut worker_handles = Vec::with_capacity(worker_count);

        for (worker_id, queue) in worker_queues.into_iter().enumerate() {
            let queue: Arc<dyn RuntimeWorkerQueue> = queue;
            let policy = policy.clone();
            let shutdown = shutdown.clone();
            let worker_loop_factory = worker_loop_factory.clone();
            let handle = std::thread::Builder::new()
                .name(format!("neovex-runtime-worker-{worker_id}"))
                .stack_size(8 * 1024 * 1024)
                .spawn(move || {
                    let mut worker_loop = worker_loop_factory.create(worker_id, policy);
                    worker_loop.run(queue, shutdown);
                })
                .expect("runtime executor worker thread should start");
            worker_handles.push(handle);
        }

        Self {
            inner: Arc::new(RuntimeExecutorInner {
                policy,
                router,
                admission,
                shutdown,
                watchdog,
                worker_count,
                queue_capacity,
                worker_handles: Mutex::new(worker_handles),
                #[cfg(test)]
                test_state,
            }),
        }
    }

    pub fn policy(&self) -> Arc<RuntimePolicy> {
        self.inner.policy.clone()
    }

    #[cfg(test)]
    pub(super) fn test_state(&self) -> Arc<RuntimeExecutorTestState> {
        self.inner.test_state.clone()
    }
}

impl Drop for RuntimeExecutorInner {
    fn drop(&mut self) {
        self.shutdown.cancel();
        self.router.close();
        for queued_job in self.admission.drain_queued_jobs() {
            queued_job
                .result_tx
                .send(Err(crate::error::NeovexRuntimeError::Contract(
                    "runtime executor unexpectedly closed".to_string(),
                )));
        }
        let mut worker_handles = self
            .worker_handles
            .lock()
            .expect("runtime executor worker handle lock should not be poisoned");
        for handle in worker_handles.drain(..) {
            let _ = handle.join();
        }
        self.watchdog.shutdown();
    }
}

impl Default for RuntimeExecutor {
    fn default() -> Self {
        let policy = Arc::new(RuntimePolicy::default());
        Self::new(policy)
    }
}
