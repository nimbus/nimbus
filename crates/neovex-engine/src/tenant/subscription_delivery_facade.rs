use std::sync::Arc;

use crate::subscriptions::{QueuedSubscriptionWork, SubscriptionDispatchStats};

use super::*;

impl TenantRuntime {
    pub(crate) fn ensure_subscription_delivery_worker_started(self: &Arc<Self>) {
        self.subscription_delivery.start_worker(self);
    }

    pub(crate) fn enqueue_subscription_work(
        &self,
        work: QueuedSubscriptionWork,
    ) -> std::result::Result<(), QueuedSubscriptionWork> {
        self.subscription_delivery.enqueue(work)
    }

    pub(crate) fn record_subscription_dispatch_stats(&self, stats: SubscriptionDispatchStats) {
        self.subscription_delivery.record_dispatch_stats(stats);
    }

    pub(crate) fn record_subscription_overflow_sync_fallback(&self) {
        self.subscription_delivery.record_overflow_sync_fallback();
    }

    pub(crate) fn record_subscription_coalesced_batch(
        &self,
        commit_count: u64,
        merged_subscription_wakeup_count: u64,
    ) {
        self.subscription_delivery
            .record_coalesced_batch(commit_count, merged_subscription_wakeup_count);
    }

    pub(crate) fn shutdown_subscription_delivery(&self) {
        self.subscription_delivery.shutdown();
    }

    pub(crate) fn subscription_delivery_stats(&self) -> SubscriptionDeliveryStats {
        self.subscription_delivery.stats()
    }

    #[cfg(test)]
    pub(crate) fn set_subscription_delivery_queue_capacity_for_testing(&self, capacity: usize) {
        self.subscription_delivery
            .set_capacity_for_testing(capacity);
    }

    #[cfg(test)]
    pub(crate) fn subscription_delivery_pause_handle_for_testing(
        &self,
    ) -> SubscriptionDeliveryPauseHandle {
        self.subscription_delivery.pause_handle()
    }
}
