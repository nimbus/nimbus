use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::Notify;

/// The signal that tells a runtime worker to stop.
///
/// A worker reads [`RuntimeWorkerShutdown::is_cancelled`] between jobs, and it
/// awaits [`RuntimeWorkerShutdown::cancelled`] at each point inside a job where
/// it would otherwise wait without a bound. Both forms report the same state,
/// so a worker that waits for a permit still leaves when the executor closes,
/// and the executor drop can join it.
#[derive(Clone)]
pub(crate) struct RuntimeWorkerShutdown {
    cancelled: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

impl RuntimeWorkerShutdown {
    pub(in crate::executor) fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            notify: Arc::new(Notify::new()),
        }
    }

    pub(in crate::executor) fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// Completes when the executor cancels this signal.
    ///
    /// The future registers with the notify before it reads the flag again, so
    /// a cancel that lands between the two is not lost.
    pub(crate) async fn cancelled(&self) {
        while !self.is_cancelled() {
            let notified = self.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.is_cancelled() {
                return;
            }
            notified.await;
        }
    }
}
