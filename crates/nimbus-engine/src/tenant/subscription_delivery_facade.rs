use std::sync::Arc;

use crate::subscriptions::QueuedSubscriptionWork;

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

    pub(crate) fn subscription_delivery_metrics(&self) -> &Arc<SubscriptionDeliveryMetrics> {
        self.subscription_delivery.metrics()
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

    #[cfg(test)]
    pub(crate) fn subscription_delivery_publish_pause_handle_for_testing(
        &self,
    ) -> SubscriptionDeliveryPublishPauseHandle {
        self.subscriptions.delivery_publish_pause_handle()
    }
}
