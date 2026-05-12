use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

#[derive(Debug, Clone)]
pub(crate) struct SubscriptionDeliveryPauseHandle {
    state: Arc<SubscriptionDeliveryPauseState>,
}

#[derive(Debug, Default)]
struct SubscriptionDeliveryPauseControl {
    armed: bool,
    entered: bool,
    released: bool,
}

#[derive(Debug, Default)]
pub(super) struct SubscriptionDeliveryPauseState {
    control: Mutex<SubscriptionDeliveryPauseControl>,
    condvar: Condvar,
}

impl SubscriptionDeliveryPauseHandle {
    pub(super) fn new(state: Arc<SubscriptionDeliveryPauseState>) -> Self {
        Self { state }
    }

    pub(crate) fn arm(&self) {
        let mut control = self
            .state
            .control
            .lock()
            .expect("subscription delivery pause lock should not be poisoned");
        control.armed = true;
        control.entered = false;
        control.released = false;
    }

    pub(crate) fn wait_until_entered(&self, timeout: std::time::Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut control = self
            .state
            .control
            .lock()
            .expect("subscription delivery pause lock should not be poisoned");
        while control.armed && !control.entered {
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                return false;
            };
            let (next_control, wait_result) = self
                .state
                .condvar
                .wait_timeout(control, remaining)
                .expect("subscription delivery pause wait should not be poisoned");
            control = next_control;
            if wait_result.timed_out() {
                return control.entered;
            }
        }
        control.entered
    }

    pub(crate) fn release(&self) {
        let mut control = self
            .state
            .control
            .lock()
            .expect("subscription delivery pause lock should not be poisoned");
        control.released = true;
        self.state.condvar.notify_all();
    }
}

impl SubscriptionDeliveryPauseState {
    pub(super) fn wait_if_armed(&self) {
        let mut control = self
            .control
            .lock()
            .expect("subscription delivery pause lock should not be poisoned");
        if !control.armed {
            return;
        }
        control.entered = true;
        self.condvar.notify_all();
        while !control.released {
            control = self
                .condvar
                .wait(control)
                .expect("subscription delivery pause wait should not be poisoned");
        }
        *control = SubscriptionDeliveryPauseControl::default();
    }
}
