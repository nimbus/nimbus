use std::sync::Arc;
use std::time::Duration;

use crate::tenant::pause_barrier::{PauseBarrier, PauseBarrierHandle};

pub(super) type SubscriptionDeliveryPauseState = PauseBarrier;

#[derive(Debug, Clone)]
pub(crate) struct SubscriptionDeliveryPauseHandle {
    inner: PauseBarrierHandle,
}

impl SubscriptionDeliveryPauseHandle {
    pub(super) fn new(state: Arc<SubscriptionDeliveryPauseState>) -> Self {
        Self {
            inner: PauseBarrierHandle::new(state),
        }
    }

    pub(crate) fn arm(&self) {
        self.inner.arm();
    }

    pub(crate) fn wait_until_entered(&self, timeout: Duration) -> bool {
        self.inner.wait_until_entered(timeout).is_some()
    }

    pub(crate) fn release(&self) {
        self.inner.release();
    }
}
