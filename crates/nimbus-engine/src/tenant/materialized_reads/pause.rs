use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

const MATERIALIZED_READ_PAUSE_RELEASE_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone)]
pub(crate) struct MaterializedReadPublishPauseHandle {
    pub(super) state: Arc<MaterializedReadPublishPauseState>,
}

#[derive(Debug, Default)]
struct MaterializedReadPublishPauseControl {
    armed: bool,
    entered: bool,
    released: bool,
}

#[derive(Debug, Default)]
pub(super) struct MaterializedReadPublishPauseState {
    control: Mutex<MaterializedReadPublishPauseControl>,
    condvar: Condvar,
}

impl MaterializedReadPublishPauseHandle {
    pub(crate) fn arm(&self) {
        let mut control = self
            .state
            .control
            .lock()
            .expect("materialized publish pause lock should not be poisoned");
        *control = MaterializedReadPublishPauseControl {
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
            .expect("materialized publish pause lock should not be poisoned");
        while !control.entered {
            let now = Instant::now();
            let Some(remaining) = deadline.checked_duration_since(now) else {
                return false;
            };
            let (next, result) = self
                .state
                .condvar
                .wait_timeout(control, remaining)
                .expect("materialized publish pause wait should not be poisoned");
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
            .expect("materialized publish pause lock should not be poisoned");
        control.released = true;
        self.state.condvar.notify_all();
    }
}

impl MaterializedReadPublishPauseState {
    pub(super) fn wait_if_armed(&self) {
        let mut control = self
            .control
            .lock()
            .expect("materialized publish pause lock should not be poisoned");
        if !control.armed {
            return;
        }
        control.armed = false;
        control.entered = true;
        self.condvar.notify_all();
        let (next, _) = self
            .condvar
            .wait_timeout_while(
                control,
                MATERIALIZED_READ_PAUSE_RELEASE_TIMEOUT,
                |control| !control.released,
            )
            .expect("materialized publish pause wait should not be poisoned");
        control = next;
        assert!(
            control.released,
            "materialized read publish pause was not released within \
             {MATERIALIZED_READ_PAUSE_RELEASE_TIMEOUT:?}; the test likely exited before calling \
             release()"
        );
        *control = MaterializedReadPublishPauseControl::default();
    }
}
