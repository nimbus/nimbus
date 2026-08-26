use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use nimbus_core::Error;

use crate::{FaultInjector, FaultPoint};

pub(crate) struct PauseAfterRetentionReadPage {
    pause_next_page: AtomicBool,
    rows_read: mpsc::SyncSender<()>,
    resume: Mutex<mpsc::Receiver<()>>,
}

impl FaultInjector for PauseAfterRetentionReadPage {
    fn check(&self, point: FaultPoint) -> nimbus_core::Result<()> {
        if point == FaultPoint::RetentionReadAfterPage
            && self.pause_next_page.swap(false, Ordering::SeqCst)
        {
            self.rows_read
                .send(())
                .map_err(|error| Error::Internal(error.to_string()))?;
            self.resume
                .lock()
                .expect("test resume lock should hold")
                .recv_timeout(Duration::from_secs(5))
                .map_err(|error| Error::Internal(error.to_string()))?;
        }
        Ok(())
    }
}

pub(crate) fn pause_after_retention_read_page() -> (
    Arc<PauseAfterRetentionReadPage>,
    mpsc::Receiver<()>,
    mpsc::SyncSender<()>,
) {
    let (rows_read_tx, rows_read_rx) = mpsc::sync_channel(1);
    let (resume_tx, resume_rx) = mpsc::sync_channel(1);
    (
        Arc::new(PauseAfterRetentionReadPage {
            pause_next_page: AtomicBool::new(true),
            rows_read: rows_read_tx,
            resume: Mutex::new(resume_rx),
        }),
        rows_read_rx,
        resume_tx,
    )
}
