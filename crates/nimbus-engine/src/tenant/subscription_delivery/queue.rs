use std::sync::atomic::AtomicBool;

use crate::subscriptions::QueuedSubscriptionWork;
use crate::tenant::background::WorkQueue;

pub(crate) const DEFAULT_SUBSCRIPTION_WORK_QUEUE_CAPACITY: usize = 256;
const SUBSCRIPTION_DELIVERY_DRAIN_BATCH_SIZE: usize = 8;

pub(super) struct SubscriptionDeliveryQueueState {
    queue: WorkQueue<QueuedSubscriptionWork>,
}

pub(super) struct SubscriptionDeliveryQueueSnapshot {
    pub(super) depth: usize,
    pub(super) capacity: usize,
    pub(super) oldest_queue_age_nanos: u64,
}

impl SubscriptionDeliveryQueueState {
    pub(super) fn new() -> Self {
        Self {
            queue: WorkQueue::new(DEFAULT_SUBSCRIPTION_WORK_QUEUE_CAPACITY),
        }
    }

    pub(super) fn enqueue(
        &self,
        work: QueuedSubscriptionWork,
    ) -> std::result::Result<(), QueuedSubscriptionWork> {
        self.queue.enqueue(work)
    }

    pub(super) fn pop_next(&self, shutdown: &AtomicBool) -> Option<QueuedSubscriptionWork> {
        self.queue.pop_next(shutdown)
    }

    pub(super) fn drain_ready_batch(
        &self,
        shutdown: &AtomicBool,
    ) -> Option<Vec<QueuedSubscriptionWork>> {
        self.queue
            .drain_ready_batch(shutdown, SUBSCRIPTION_DELIVERY_DRAIN_BATCH_SIZE - 1)
    }

    pub(super) fn signal_shutdown(&self, shutdown: &AtomicBool) {
        self.queue.signal_shutdown(shutdown);
    }

    pub(super) fn snapshot(&self) -> SubscriptionDeliveryQueueSnapshot {
        let oldest_queue_age_nanos = self.queue.with_front(|work| {
            work.map(|work| work.enqueued_at.elapsed().as_nanos())
                .unwrap_or(0)
                .min(u128::from(u64::MAX)) as u64
        });
        SubscriptionDeliveryQueueSnapshot {
            depth: self.queue.len(),
            capacity: self.queue.capacity(),
            oldest_queue_age_nanos,
        }
    }

    #[cfg(test)]
    pub(super) fn set_capacity_for_testing(&self, capacity: usize) {
        self.queue.set_capacity_for_testing(capacity);
    }
}
