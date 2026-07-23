use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, RwLock, RwLockReadGuard};
use std::time::Duration;

use nimbus_core::{Error, Result};
use tokio::runtime::{
    Builder as TokioRuntimeBuilder, Handle as TokioRuntimeHandle, Runtime as TokioRuntime,
};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

/// Owned Tokio runtime with explicit quiesce semantics and tracked task lifecycle.
pub(crate) struct BackgroundExecutor {
    runtime: Mutex<Option<TokioRuntime>>,
    handle: TokioRuntimeHandle,
    spawn_gate: RwLock<()>,
    closed: AtomicBool,
    shutdown: CancellationToken,
    tracker: TaskTracker,
    name: &'static str,
}

pub(crate) struct BackgroundSpawnPermit<'a> {
    executor: &'a BackgroundExecutor,
    _guard: RwLockReadGuard<'a, ()>,
}

impl BackgroundSpawnPermit<'_> {
    pub(crate) fn spawn<F>(&self, future: F) -> JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.executor
            .tracker
            .spawn_on(future, &self.executor.handle)
    }
}

impl BackgroundExecutor {
    pub(crate) fn new(name: &'static str, worker_threads: usize) -> std::io::Result<Self> {
        let runtime = TokioRuntimeBuilder::new_multi_thread()
            .worker_threads(worker_threads.max(1))
            .thread_name(name)
            .enable_all()
            .build()?;
        let handle = runtime.handle().clone();
        Ok(Self {
            runtime: Mutex::new(Some(runtime)),
            handle,
            spawn_gate: RwLock::new(()),
            closed: AtomicBool::new(false),
            shutdown: CancellationToken::new(),
            tracker: TaskTracker::new(),
            name,
        })
    }

    pub(crate) fn handle(&self) -> TokioRuntimeHandle {
        self.handle.clone()
    }

    pub(crate) fn shutdown_token(&self) -> CancellationToken {
        self.shutdown.clone()
    }

    pub(crate) fn spawn_mapped<F, M, Mapped>(
        &self,
        future: F,
        map: M,
    ) -> std::result::Result<JoinHandle<Mapped::Output>, (Error, F)>
    where
        F: Future + Send + 'static,
        M: FnOnce(F) -> Mapped,
        Mapped: Future + Send + 'static,
        Mapped::Output: Send + 'static,
    {
        let permit = match self.acquire_spawn_permit() {
            Ok(permit) => permit,
            Err(error) => return Err((error, future)),
        };
        Ok(permit.spawn(map(future)))
    }

    /// Holds the quiesce write gate while one logical worker group is installed.
    ///
    /// A caller may spawn several mutually dependent tasks through the permit.
    /// Quiescence therefore observes either the complete group or none of it;
    /// it cannot close the tracker between receiver extraction and the final
    /// spawn.
    pub(crate) fn acquire_spawn_permit(&self) -> Result<BackgroundSpawnPermit<'_>> {
        let guard = self
            .spawn_gate
            .read()
            .expect("background executor spawn gate should not be poisoned");
        self.ensure_open()?;
        Ok(BackgroundSpawnPermit {
            executor: self,
            _guard: guard,
        })
    }

    pub(crate) async fn quiesce(&self) {
        let guard = self
            .spawn_gate
            .write()
            .expect("background executor spawn gate should not be poisoned");
        self.closed.store(true, Ordering::Release);
        self.shutdown.cancel();
        self.tracker.close();
        drop(guard);
        self.tracker.wait().await;
        self.shutdown_runtime().await;
    }

    async fn shutdown_runtime(&self) {
        let runtime = self
            .runtime
            .lock()
            .expect("background executor runtime lock should not be poisoned")
            .take();
        let Some(runtime) = runtime else {
            return;
        };

        let called_from_owned_runtime =
            TokioRuntimeHandle::try_current().is_ok_and(|current| current.id() == self.handle.id());
        if called_from_owned_runtime {
            // Waiting for this runtime from one of its own workers would
            // deadlock. Initiate cancellation and let the worker return.
            runtime.shutdown_background();
            return;
        }

        let (finished_tx, finished_rx) = tokio::sync::oneshot::channel();
        std::thread::Builder::new()
            .name(format!("{}-shutdown", self.name))
            .spawn(move || {
                runtime.shutdown_timeout(Duration::from_secs(5));
                let _ = finished_tx.send(());
            })
            .expect("background executor shutdown thread should spawn");
        finished_rx
            .await
            .expect("background executor shutdown thread should finish");
    }

    fn ensure_open(&self) -> Result<()> {
        if self.closed.load(Ordering::Acquire) {
            return Err(Error::ResourceExhausted(format!(
                "{} executor is quiescing",
                self.name
            )));
        }
        Ok(())
    }
}

impl Drop for BackgroundExecutor {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::Release);
        self.shutdown.cancel();
        self.tracker.close();
        if let Some(runtime) = self
            .runtime
            .get_mut()
            .expect("background executor runtime lock should not be poisoned")
            .take()
        {
            if tokio::runtime::Handle::try_current().is_ok() {
                runtime.shutdown_background();
            } else {
                runtime.shutdown_timeout(Duration::from_secs(5));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::future::pending;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    use tokio::sync::Notify;

    use super::BackgroundExecutor;

    struct CancellationSignal(Arc<AtomicBool>);

    impl Drop for CancellationSignal {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn quiesce_rejects_new_work() {
        let executor =
            BackgroundExecutor::new("quiesce-rejects", 1).expect("test runtime should build");
        let permit = executor
            .acquire_spawn_permit()
            .expect("executor should accept initial task");
        let initial = permit.spawn(async {});
        drop(permit);
        initial.await.expect("initial task should finish");

        executor.quiesce().await;

        let error = executor
            .acquire_spawn_permit()
            .err()
            .expect("executor should reject tasks after quiesce");
        assert!(matches!(error, nimbus_core::Error::ResourceExhausted(_)));
    }

    #[tokio::test]
    async fn spawn_permit_holds_quiesce_gate_for_complete_worker_group() {
        let executor =
            BackgroundExecutor::new("spawn-group", 1).expect("test runtime should build");
        let permit = executor
            .acquire_spawn_permit()
            .expect("open executor should grant a spawn permit");

        assert!(
            executor.spawn_gate.try_write().is_err(),
            "the group permit must exclude quiesce until every dependent worker is installed"
        );
        let first = permit.spawn(async { "publisher" });
        let second = permit.spawn(async { "committer" });
        drop(permit);
        assert!(
            executor.spawn_gate.try_write().is_ok(),
            "releasing the group permit must make the quiesce gate available"
        );

        executor.quiesce().await;
        assert_eq!(
            first.await.expect("first group worker should finish"),
            "publisher"
        );
        assert_eq!(
            second.await.expect("second group worker should finish"),
            "committer"
        );
    }

    #[tokio::test]
    async fn quiesce_waits_for_tracked_tasks() {
        let executor = Arc::new(
            BackgroundExecutor::new("quiesce-blocking", 1).expect("test runtime should build"),
        );
        let entered = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let finished = Arc::new(AtomicBool::new(false));

        let entered_for_task = entered.clone();
        let release_for_task = release.clone();
        let finished_for_task = finished.clone();
        let permit = executor
            .acquire_spawn_permit()
            .expect("tracked task should spawn");
        let blocking = permit.spawn(async move {
            entered_for_task.notify_one();
            release_for_task.notified().await;
            finished_for_task.store(true, Ordering::SeqCst);
        });
        drop(permit);

        tokio::time::timeout(Duration::from_secs(1), entered.notified())
            .await
            .expect("tracked task should start");

        let quiesce = tokio::spawn({
            let executor = executor.clone();
            async move {
                executor.quiesce().await;
            }
        });

        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(
            !finished.load(Ordering::SeqCst),
            "quiesce should still be waiting for the tracked task"
        );

        release.notify_one();
        blocking.await.expect("tracked task should join");
        quiesce.await.expect("quiesce task should join");
        assert!(
            finished.load(Ordering::SeqCst),
            "tracked task should complete before quiesce returns"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn quiesce_cancels_untracked_runtime_tasks_before_returning() {
        let executor =
            BackgroundExecutor::new("quiesce-untracked", 1).expect("test runtime should build");
        let started = Arc::new(Notify::new());
        let cancelled = Arc::new(AtomicBool::new(false));
        let started_for_task = started.clone();
        let cancelled_for_task = cancelled.clone();

        let _untracked = executor.handle().spawn(async move {
            let _signal = CancellationSignal(cancelled_for_task);
            started_for_task.notify_one();
            pending::<()>().await;
        });
        tokio::time::timeout(Duration::from_secs(1), started.notified())
            .await
            .expect("untracked runtime task should start");

        executor.quiesce().await;

        assert!(
            cancelled.load(Ordering::SeqCst),
            "quiesce must cancel runtime-owned transport tasks before it returns"
        );
    }
}
