use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Shared OS-thread lifecycle scaffold for tenant background workers.
///
/// Every tenant background pipeline (subscription delivery, trigger
/// candidates, trigger execution) spawns exactly one dedicated named thread,
/// guarded so a second `start` is a no-op while one is running, and shuts
/// down by letting the caller flip the shutdown flag and wake whatever
/// blocking wait the worker is parked on as one locked operation, then
/// joining (unless the calling thread *is* the worker thread, in which case
/// joining would deadlock).
///
/// `BackgroundWorker` owns only this lifecycle. It knows nothing about the
/// work queue or the tenant runtime the spawned closure closes over; callers
/// build their run-loop closure (typically capturing a `Weak<TenantRuntime>`
/// so the worker never keeps a tenant alive) and hand it to `start`.
pub(crate) struct BackgroundWorker {
    worker: Mutex<Option<std::thread::JoinHandle<()>>>,
    shutdown: Arc<AtomicBool>,
    start_count: AtomicU64,
}

impl BackgroundWorker {
    pub(crate) fn new() -> Self {
        Self {
            worker: Mutex::new(None),
            shutdown: Arc::new(AtomicBool::new(false)),
            start_count: AtomicU64::new(0),
        }
    }

    /// Starts the worker thread with the given name, running `run` with the
    /// worker's shutdown flag. Returns `false` without spawning if a worker
    /// is already running.
    pub(crate) fn start(
        &self,
        thread_name: &str,
        run: impl FnOnce(Arc<AtomicBool>) + Send + 'static,
    ) -> bool {
        let mut worker = self
            .worker
            .lock()
            .expect("background worker lock should not be poisoned");
        if worker.is_some() {
            return false;
        }
        self.shutdown.store(false, Ordering::Release);
        self.start_count.fetch_add(1, Ordering::Relaxed);
        let shutdown = self.shutdown.clone();
        *worker = Some(
            std::thread::Builder::new()
                .name(thread_name.to_string())
                .spawn(move || run(shutdown))
                .expect("background worker should spawn"),
        );
        true
    }

    /// Signals shutdown and joins the worker thread unless called from the
    /// worker thread itself.
    ///
    /// `signal` receives this worker's shutdown flag and is responsible for
    /// *both* setting it and waking whatever the run loop is parked on
    /// (typically the owning queue's `signal_shutdown`), in that order,
    /// under the same lock the run loop's blocking pop holds across its
    /// shutdown check. `BackgroundWorker` does not set the flag itself: a
    /// bare `store` here, before or after `signal` runs, would let the flip
    /// race the run loop's check-then-park sequence and could leave a
    /// worker parked forever on an empty queue with no one left to wake it.
    pub(crate) fn shutdown(&self, signal: impl FnOnce(&AtomicBool)) {
        signal(&self.shutdown);
        if let Some(worker) = self
            .worker
            .lock()
            .expect("background worker lock should not be poisoned")
            .take()
        {
            if worker.thread().id() == std::thread::current().id() {
                return;
            }
            let _ = worker.join();
        }
    }

    pub(crate) fn running(&self) -> bool {
        self.worker
            .lock()
            .expect("background worker lock should not be poisoned")
            .is_some()
    }

    pub(crate) fn start_count(&self) -> u64 {
        self.start_count.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;

    use super::*;

    #[test]
    fn start_is_a_no_op_while_already_running() {
        let worker = BackgroundWorker::new();
        let started = Arc::new(std::sync::Barrier::new(2));
        let release = Arc::new(AtomicBool::new(false));

        let started_clone = started.clone();
        let release_clone = release.clone();
        let spawned = worker.start("bg-worker-test", move |shutdown| {
            started_clone.wait();
            while !shutdown.load(Ordering::Acquire) && !release_clone.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(1));
            }
        });
        assert!(spawned, "first start should spawn the worker");
        started.wait();

        let second_spawn = worker.start("bg-worker-test-again", |_shutdown| {});
        assert!(!second_spawn, "second start should be a no-op");
        assert_eq!(worker.start_count(), 1);

        release.store(true, Ordering::Release);
        worker.shutdown(|shutdown| shutdown.store(true, Ordering::Release));
        assert!(!worker.running());
    }

    #[test]
    fn shutdown_joins_worker_and_notify_runs() {
        let worker = BackgroundWorker::new();
        let notified = Arc::new(AtomicUsize::new(0));

        worker.start("bg-worker-shutdown-test", |shutdown| {
            while !shutdown.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(1));
            }
        });
        assert!(worker.running());

        let notified_clone = notified.clone();
        worker.shutdown(move |shutdown| {
            shutdown.store(true, Ordering::Release);
            notified_clone.fetch_add(1, Ordering::SeqCst);
        });

        assert_eq!(notified.load(Ordering::SeqCst), 1);
        assert!(!worker.running());
    }

    #[test]
    fn start_count_increments_across_restarts() {
        let worker = BackgroundWorker::new();
        worker.start("bg-worker-restart-test-1", |shutdown| {
            while !shutdown.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(1));
            }
        });
        worker.shutdown(|shutdown| shutdown.store(true, Ordering::Release));
        assert_eq!(worker.start_count(), 1);

        worker.start("bg-worker-restart-test-2", |shutdown| {
            while !shutdown.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(1));
            }
        });
        worker.shutdown(|shutdown| shutdown.store(true, Ordering::Release));
        assert_eq!(worker.start_count(), 2);
    }
}
