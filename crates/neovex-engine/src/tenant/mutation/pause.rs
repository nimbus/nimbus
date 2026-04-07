#[cfg(any(test, feature = "test-hooks"))]
use std::sync::{Arc, Condvar, Mutex};
#[cfg(any(test, feature = "test-hooks"))]
use std::time::Instant;

#[cfg(any(test, feature = "test-hooks"))]
use tokio::sync::Notify;

#[cfg(any(test, feature = "test-hooks"))]
#[derive(Debug, Clone)]
pub(crate) struct MutationJournalPauseHandle {
    state: Arc<MutationJournalPauseState>,
}

#[cfg(any(test, feature = "test-hooks"))]
#[derive(Debug, Default)]
pub(in crate::tenant) struct MutationJournalPauseState {
    control: Mutex<MutationJournalPauseControl>,
    entered: Condvar,
    released: Notify,
}

#[cfg(any(test, feature = "test-hooks"))]
#[derive(Debug, Default)]
struct MutationJournalPauseControl {
    armed: bool,
    entered: bool,
    released: bool,
}

#[cfg(any(test, feature = "test-hooks"))]
impl MutationJournalPauseState {
    pub(in crate::tenant) async fn wait_if_armed(&self) {
        {
            let mut control = self
                .control
                .lock()
                .expect("mutation journal pause lock should not be poisoned");
            if !control.armed {
                return;
            }
            control.entered = true;
            self.entered.notify_all();
            if control.released {
                *control = MutationJournalPauseControl::default();
                return;
            }
        }

        loop {
            let notified = self.released.notified();
            {
                let mut control = self
                    .control
                    .lock()
                    .expect("mutation journal pause lock should not be poisoned");
                if control.released {
                    *control = MutationJournalPauseControl::default();
                    return;
                }
            }
            notified.await;
        }
    }
}

#[cfg(any(test, feature = "test-hooks"))]
impl MutationJournalPauseHandle {
    pub(in crate::tenant) fn from_state(state: Arc<MutationJournalPauseState>) -> Self {
        Self { state }
    }

    pub(crate) fn arm(&self) {
        let mut control = self
            .state
            .control
            .lock()
            .expect("mutation journal pause lock should not be poisoned");
        *control = MutationJournalPauseControl {
            armed: true,
            entered: false,
            released: false,
        };
    }

    pub(crate) fn wait_until_entered(&self, timeout: std::time::Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut control = self
            .state
            .control
            .lock()
            .expect("mutation journal pause lock should not be poisoned");
        while !control.entered {
            let now = Instant::now();
            let Some(remaining) = deadline.checked_duration_since(now) else {
                return false;
            };
            let (next, result) = self
                .state
                .entered
                .wait_timeout(control, remaining)
                .expect("mutation journal pause wait should not be poisoned");
            control = next;
            if result.timed_out() && !control.entered {
                return false;
            }
        }
        true
    }

    pub(crate) fn release(&self) {
        let mut control = self
            .state
            .control
            .lock()
            .expect("mutation journal pause lock should not be poisoned");
        control.released = true;
        self.state.released.notify_waiters();
    }
}
