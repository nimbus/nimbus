use std::collections::HashMap;
use std::sync::Mutex;

use nimbus_runtime::HostCallCancellation;

#[derive(Default)]
struct PendingBootstrapCancellations {
    by_request_id: HashMap<String, HostCallCancellation>,
    by_subscription_id: HashMap<u64, HostCallCancellation>,
}

#[derive(Default)]
pub(super) struct PendingBootstrapCancellationRegistry {
    inner: Mutex<PendingBootstrapCancellations>,
}

impl PendingBootstrapCancellationRegistry {
    pub(super) fn track_request(&self, request_id: String, cancellation: HostCallCancellation) {
        self.inner
            .lock()
            .expect("pending bootstrap cancellation lock should not be poisoned")
            .by_request_id
            .insert(request_id, cancellation);
    }

    pub(super) fn link_subscription(&self, subscription_id: u64, request_id: &str) {
        let mut pending = self
            .inner
            .lock()
            .expect("pending bootstrap cancellation lock should not be poisoned");
        if let Some(cancellation) = pending.by_request_id.get(request_id).cloned() {
            pending
                .by_subscription_id
                .insert(subscription_id, cancellation);
        }
    }

    pub(super) fn finish_request(&self, request_id: &str, registered_subscription_id: Option<u64>) {
        let mut pending = self
            .inner
            .lock()
            .expect("pending bootstrap cancellation lock should not be poisoned");
        pending.by_request_id.remove(request_id);
        if let Some(subscription_id) = registered_subscription_id {
            pending.by_subscription_id.remove(&subscription_id);
        }
    }

    pub(super) fn cancel_subscription(&self, subscription_id: u64) {
        if let Some(cancellation) = self
            .inner
            .lock()
            .expect("pending bootstrap cancellation lock should not be poisoned")
            .by_subscription_id
            .remove(&subscription_id)
        {
            cancellation.cancel();
        }
    }

    pub(super) fn clear(&self) {
        let mut pending = self
            .inner
            .lock()
            .expect("pending bootstrap cancellation lock should not be poisoned");
        pending.by_request_id.clear();
        pending.by_subscription_id.clear();
    }
}
