use std::fmt;
use std::future::Future;

use tokio::task::JoinSet;

#[derive(Default)]
pub(crate) struct OwnedTaskSet {
    tasks: JoinSet<()>,
}

impl OwnedTaskSet {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn spawn<F>(&mut self, task: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.tasks.spawn(task);
    }

    pub(crate) async fn shutdown_and_drain(mut self) {
        self.tasks.abort_all();
        while self.tasks.join_next().await.is_some() {}
    }

    pub(crate) fn task_count(&self) -> usize {
        self.tasks.len()
    }
}

impl fmt::Debug for OwnedTaskSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OwnedTaskSet")
            .field("task_count", &self.task_count())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::sync::oneshot;
    use tokio::time::timeout;

    use super::OwnedTaskSet;

    struct DropSignal(Option<oneshot::Sender<()>>);

    impl Drop for DropSignal {
        fn drop(&mut self) {
            if let Some(sender) = self.0.take() {
                let _ = sender.send(());
            }
        }
    }

    #[tokio::test]
    async fn shutdown_and_drain_aborts_pending_children_deterministically() {
        let (started_tx, started_rx) = oneshot::channel();
        let (dropped_tx, dropped_rx) = oneshot::channel();
        let mut tasks = OwnedTaskSet::new();

        tasks.spawn(async move {
            let _guard = DropSignal(Some(dropped_tx));
            started_tx
                .send(())
                .expect("task should notify that it started");
            std::future::pending::<()>().await;
        });

        started_rx.await.expect("task should start");

        tasks.shutdown_and_drain().await;

        timeout(Duration::from_secs(1), dropped_rx)
            .await
            .expect("shutdown should cancel and drain the pending child")
            .expect("task should signal drop when it is aborted");
    }
}
