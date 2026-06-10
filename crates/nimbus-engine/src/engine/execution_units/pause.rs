use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

#[derive(Debug, Clone)]
pub(crate) struct ExecutionUnitCommitPauseHandle {
    state: Arc<ExecutionUnitCommitPauseState>,
}

#[derive(Debug, Default)]
struct ExecutionUnitCommitPauseControl {
    armed: bool,
    entered: bool,
    released: bool,
}

#[derive(Debug, Default)]
pub(crate) struct ExecutionUnitCommitPauseState {
    control: Mutex<ExecutionUnitCommitPauseControl>,
    condvar: Condvar,
}

impl ExecutionUnitCommitPauseHandle {
    pub(crate) fn new(state: Arc<ExecutionUnitCommitPauseState>) -> Self {
        Self { state }
    }

    pub(crate) fn arm(&self) {
        let mut control = self
            .state
            .control
            .lock()
            .expect("execution unit commit pause lock should not be poisoned");
        *control = ExecutionUnitCommitPauseControl {
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
            .expect("execution unit commit pause lock should not be poisoned");
        while !control.entered {
            let now = Instant::now();
            let Some(remaining) = deadline.checked_duration_since(now) else {
                return false;
            };
            let (next, result) = self
                .state
                .condvar
                .wait_timeout(control, remaining)
                .expect("execution unit commit pause wait should not be poisoned");
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
            .expect("execution unit commit pause lock should not be poisoned");
        control.released = true;
        self.state.condvar.notify_all();
    }
}

impl ExecutionUnitCommitPauseState {
    pub(crate) fn wait_if_armed(&self) {
        let mut control = self
            .control
            .lock()
            .expect("execution unit commit pause lock should not be poisoned");
        if !control.armed {
            return;
        }
        control.armed = false;
        control.entered = true;
        self.condvar.notify_all();
        while !control.released {
            control = self
                .condvar
                .wait(control)
                .expect("execution unit commit pause wait should not be poisoned");
        }
        *control = ExecutionUnitCommitPauseControl::default();
    }
}
