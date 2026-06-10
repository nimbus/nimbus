use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

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
    fn lock(&self) -> MutexGuard<'_, PendingBootstrapCancellations> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub(super) fn track_request(&self, request_id: String, cancellation: HostCallCancellation) {
        self.lock().by_request_id.insert(request_id, cancellation);
    }

    pub(super) fn link_subscription(&self, subscription_id: u64, request_id: &str) {
        let mut pending = self.lock();
        if let Some(cancellation) = pending.by_request_id.get(request_id).cloned() {
            pending
                .by_subscription_id
                .insert(subscription_id, cancellation);
        }
    }

    pub(super) fn finish_request(&self, request_id: &str, registered_subscription_id: Option<u64>) {
        let mut pending = self.lock();
        pending.by_request_id.remove(request_id);
        if let Some(subscription_id) = registered_subscription_id {
            pending.by_subscription_id.remove(&subscription_id);
        }
    }

    pub(super) fn cancel_subscription(&self, subscription_id: u64) {
        if let Some(cancellation) = self.lock().by_subscription_id.remove(&subscription_id) {
            cancellation.cancel();
        }
    }

    pub(super) fn clear(&self) {
        let mut pending = self.lock();
        pending.by_request_id.clear();
        pending.by_subscription_id.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_subscription_cancels_linked_request_once() {
        let registry = PendingBootstrapCancellationRegistry::default();
        let cancellation = HostCallCancellation::default();

        registry.track_request("request-1".to_string(), cancellation.clone());
        registry.link_subscription(7, "request-1");
        registry.cancel_subscription(7);
        registry.cancel_subscription(7);

        assert!(cancellation.is_cancelled());
        assert_eq!(
            cancellation.cause(),
            Some(nimbus_runtime::HostCallCancellationCause::Explicit)
        );
    }

    #[test]
    fn finish_request_removes_request_and_linked_subscription() {
        let registry = PendingBootstrapCancellationRegistry::default();
        let cancellation = HostCallCancellation::default();

        registry.track_request("request-1".to_string(), cancellation.clone());
        registry.link_subscription(9, "request-1");
        registry.finish_request("request-1", Some(9));
        registry.cancel_subscription(9);
        registry.link_subscription(10, "request-1");
        registry.cancel_subscription(10);

        assert!(!cancellation.is_cancelled());
    }

    #[test]
    fn clear_drops_pending_requests_without_cancelling_them() {
        let registry = PendingBootstrapCancellationRegistry::default();
        let cancellation = HostCallCancellation::default();

        registry.track_request("request-1".to_string(), cancellation.clone());
        registry.link_subscription(11, "request-1");
        registry.clear();
        registry.cancel_subscription(11);

        assert!(!cancellation.is_cancelled());
    }
}
