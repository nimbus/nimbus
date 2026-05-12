#[cfg(test)]
mod pause;
mod queue;
mod stats;
#[cfg(test)]
mod tests;
mod worker;

use std::sync::Arc;

use crate::subscriptions::{QueuedSubscriptionWork, SubscriptionDispatchStats};

use super::TenantRuntime;

#[cfg(test)]
pub(crate) use pause::SubscriptionDeliveryPauseHandle;
#[cfg(test)]
use pause::SubscriptionDeliveryPauseState;
#[cfg(test)]
pub(crate) use queue::DEFAULT_SUBSCRIPTION_WORK_QUEUE_CAPACITY;
use queue::SubscriptionDeliveryQueueState;
use stats::SubscriptionDeliveryMetrics;
pub use stats::SubscriptionDeliveryStats;
use worker::SubscriptionDeliveryWorker;

pub(super) struct SubscriptionDeliveryQueue {
    queue: Arc<SubscriptionDeliveryQueueState>,
    worker: Arc<SubscriptionDeliveryWorker>,
    metrics: Arc<SubscriptionDeliveryMetrics>,
    #[cfg(test)]
    pause: Arc<SubscriptionDeliveryPauseState>,
}

impl SubscriptionDeliveryQueue {
    pub(super) fn new() -> Self {
        Self {
            queue: Arc::new(SubscriptionDeliveryQueueState::new()),
            worker: Arc::new(SubscriptionDeliveryWorker::new()),
            metrics: Arc::new(SubscriptionDeliveryMetrics::new()),
            #[cfg(test)]
            pause: Arc::new(SubscriptionDeliveryPauseState::default()),
        }
    }

    pub(super) fn start_worker(&self, runtime: &Arc<TenantRuntime>) {
        self.worker.start(
            runtime,
            self.queue.clone(),
            self.metrics.clone(),
            #[cfg(test)]
            self.pause.clone(),
        );
    }

    pub(super) fn enqueue(
        &self,
        work: QueuedSubscriptionWork,
    ) -> std::result::Result<(), QueuedSubscriptionWork> {
        self.queue.enqueue(work)
    }

    pub(super) fn record_overflow_sync_fallback(&self) {
        self.metrics.record_overflow_sync_fallback();
    }

    pub(super) fn record_coalesced_batch(
        &self,
        commit_count: u64,
        merged_subscription_wakeup_count: u64,
    ) {
        self.metrics
            .record_coalesced_batch(commit_count, merged_subscription_wakeup_count);
    }

    pub(super) fn record_dispatch_stats(&self, stats: SubscriptionDispatchStats) {
        self.metrics.record_dispatch_stats(stats);
    }

    pub(super) fn shutdown(&self) {
        self.worker.shutdown(&self.queue);
    }

    pub(super) fn stats(&self) -> SubscriptionDeliveryStats {
        self.metrics.snapshot(
            &self.queue,
            self.worker.running(),
            self.worker.start_count(),
        )
    }

    #[cfg(test)]
    pub(super) fn set_capacity_for_testing(&self, capacity: usize) {
        self.queue.set_capacity_for_testing(capacity);
    }

    #[cfg(test)]
    pub(super) fn pause_handle(&self) -> SubscriptionDeliveryPauseHandle {
        SubscriptionDeliveryPauseHandle::new(self.pause.clone())
    }
}

impl Drop for SubscriptionDeliveryQueue {
    fn drop(&mut self) {
        self.shutdown();
    }
}
