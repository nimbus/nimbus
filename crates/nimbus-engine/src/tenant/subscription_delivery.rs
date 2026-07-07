#[cfg(test)]
mod pause;
mod queue;
mod stats;
#[cfg(test)]
mod tests;
mod worker;

use std::sync::Arc;

use crate::subscriptions::QueuedSubscriptionWork;

use super::TenantRuntime;

#[cfg(test)]
pub(crate) use pause::SubscriptionDeliveryPauseHandle;
#[cfg(test)]
use pause::SubscriptionDeliveryPauseState;
#[cfg(test)]
pub(crate) use queue::DEFAULT_SUBSCRIPTION_WORK_QUEUE_CAPACITY;
use queue::SubscriptionDeliveryQueueState;
pub(crate) use stats::SubscriptionDeliveryMetrics;
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

    pub(super) fn metrics(&self) -> &Arc<SubscriptionDeliveryMetrics> {
        &self.metrics
    }

    pub(super) fn shutdown(&self) {
        self.worker.shutdown(
            &self.queue,
            #[cfg(test)]
            &self.pause,
        );
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
