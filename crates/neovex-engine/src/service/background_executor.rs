use std::future::Future;
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use neovex_core::{Error, Result};
use tokio::runtime::{
    Builder as TokioRuntimeBuilder, Handle as TokioRuntimeHandle, Runtime as TokioRuntime,
};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

/// Owned Tokio runtime with explicit quiesce semantics and tracked task lifecycle.
pub(crate) struct BackgroundExecutor {
    runtime: Option<TokioRuntime>,
    handle: TokioRuntimeHandle,
    spawn_gate: RwLock<()>,
    closed: AtomicBool,
    shutdown: CancellationToken,
    tracker: TaskTracker,
    name: &'static str,
}

impl BackgroundExecutor {
    pub(crate) fn new(name: &'static str, worker_threads: usize) -> Self {
        let runtime = TokioRuntimeBuilder::new_multi_thread()
            .worker_threads(worker_threads.max(1))
            .thread_name(name)
            .enable_all()
            .build()
            .expect("background runtime should build");
        let handle = runtime.handle().clone();
        Self {
            runtime: Some(runtime),
            handle,
            spawn_gate: RwLock::new(()),
            closed: AtomicBool::new(false),
            shutdown: CancellationToken::new(),
            tracker: TaskTracker::new(),
            name,
        }
    }

    pub(crate) fn handle(&self) -> TokioRuntimeHandle {
        self.handle.clone()
    }

    pub(crate) fn shutdown_token(&self) -> CancellationToken {
        self.shutdown.clone()
    }

    pub(crate) fn spawn<F>(&self, future: F) -> Result<JoinHandle<F::Output>>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        let _guard = self
            .spawn_gate
            .read()
            .expect("background executor spawn gate should not be poisoned");
        self.ensure_open()?;
        Ok(self.tracker.spawn_on(future, &self.handle))
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
        if let Some(runtime) = self.runtime.take() {
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
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    use tokio::sync::Notify;

    use super::BackgroundExecutor;

    #[tokio::test]
    async fn quiesce_rejects_new_work() {
        let executor = BackgroundExecutor::new("quiesce-rejects", 1);
        executor
            .spawn(async {})
            .expect("executor should accept initial task")
            .await
            .expect("initial task should finish");

        executor.quiesce().await;

        let error = executor
            .spawn(async {})
            .expect_err("executor should reject tasks after quiesce");
        assert!(matches!(error, neovex_core::Error::ResourceExhausted(_)));
    }

    #[tokio::test]
    async fn quiesce_waits_for_tracked_tasks() {
        let executor = Arc::new(BackgroundExecutor::new("quiesce-blocking", 1));
        let entered = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let finished = Arc::new(AtomicBool::new(false));

        let entered_for_task = entered.clone();
        let release_for_task = release.clone();
        let finished_for_task = finished.clone();
        let blocking = executor
            .spawn(async move {
                entered_for_task.notify_one();
                release_for_task.notified().await;
                finished_for_task.store(true, Ordering::SeqCst);
            })
            .expect("tracked task should spawn");

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
}
