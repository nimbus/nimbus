//! Deterministic barrier after provision admission and before its first effect.

use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use crate::{Result, SandboxError};

#[derive(Clone)]
pub(in crate::backends::container::runtime) struct ProvisionAdmissionTestProbe {
    state: Arc<(Mutex<ProbeState>, Condvar)>,
    timeout: Duration,
}

#[derive(Default)]
struct ProbeState {
    entered: bool,
    released: bool,
}

impl ProvisionAdmissionTestProbe {
    pub(in crate::backends::container::runtime) fn new(timeout: Duration) -> Self {
        Self {
            state: Arc::new((Mutex::new(ProbeState::default()), Condvar::new())),
            timeout,
        }
    }

    pub(in crate::backends::container::runtime) fn pause(&self) -> Result<()> {
        let (state, changed) = &*self.state;
        let mut state = state.lock().map_err(|_| SandboxError::OperationFailed {
            message: "provision admission test probe was poisoned".to_owned(),
        })?;
        state.entered = true;
        changed.notify_all();
        let (state, timeout) = changed
            .wait_timeout_while(state, self.timeout, |state| !state.released)
            .map_err(|_| SandboxError::OperationFailed {
                message: "provision admission test probe wait was poisoned".to_owned(),
            })?;
        if timeout.timed_out() && !state.released {
            return Err(SandboxError::OperationFailed {
                message: "timed out waiting to release provision admission test probe".to_owned(),
            });
        }
        Ok(())
    }

    pub(in crate::backends::container::runtime) fn wait_until_entered(&self) -> bool {
        let (state, changed) = &*self.state;
        let state = state
            .lock()
            .expect("provision admission test probe should not be poisoned");
        let (state, _) = changed
            .wait_timeout_while(state, self.timeout, |state| !state.entered)
            .expect("provision admission test probe wait should not be poisoned");
        state.entered
    }

    pub(in crate::backends::container::runtime) fn release(&self) {
        let (state, changed) = &*self.state;
        let mut state = state
            .lock()
            .expect("provision admission test probe should not be poisoned");
        state.released = true;
        changed.notify_all();
    }
}
