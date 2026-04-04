use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex};

use crate::subscriptions::QueuedSubscriptionWork;

pub(crate) const DEFAULT_SUBSCRIPTION_WORK_QUEUE_CAPACITY: usize = 256;
const SUBSCRIPTION_DELIVERY_DRAIN_BATCH_SIZE: usize = 8;

pub(super) struct SubscriptionDeliveryQueueState {
    queue: Mutex<VecDeque<QueuedSubscriptionWork>>,
    queue_ready: Condvar,
    capacity: AtomicUsize,
}

pub(super) struct SubscriptionDeliveryQueueSnapshot {
    pub(super) depth: usize,
    pub(super) capacity: usize,
    pub(super) oldest_queue_age_nanos: u64,
}

impl SubscriptionDeliveryQueueState {
    pub(super) fn new() -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
            queue_ready: Condvar::new(),
            capacity: AtomicUsize::new(DEFAULT_SUBSCRIPTION_WORK_QUEUE_CAPACITY),
        }
    }

    pub(super) fn enqueue(
        &self,
        work: QueuedSubscriptionWork,
    ) -> std::result::Result<(), QueuedSubscriptionWork> {
        let mut queue = self
            .queue
            .lock()
            .expect("subscription delivery queue lock should not be poisoned");
        if queue.len() >= self.capacity.load(Ordering::Acquire).max(1) {
            return Err(work);
        }
        queue.push_back(work);
        self.queue_ready.notify_one();
        Ok(())
    }

    pub(super) fn pop_next(&self, shutdown: &AtomicBool) -> Option<QueuedSubscriptionWork> {
        let mut queue = self
            .queue
            .lock()
            .expect("subscription delivery queue lock should not be poisoned");
        loop {
            if shutdown.load(Ordering::Acquire) {
                queue.clear();
                return None;
            }
            if let Some(work) = queue.pop_front() {
                return Some(work);
            }
            queue = self
                .queue_ready
                .wait(queue)
                .expect("subscription delivery wait should not be poisoned");
        }
    }

    pub(super) fn drain_ready_batch(
        &self,
        shutdown: &AtomicBool,
    ) -> Option<Vec<QueuedSubscriptionWork>> {
        let mut queue = self
            .queue
            .lock()
            .expect("subscription delivery queue lock should not be poisoned");
        if shutdown.load(Ordering::Acquire) {
            queue.clear();
            return None;
        }
        let mut work_batch = Vec::new();
        while work_batch.len() < SUBSCRIPTION_DELIVERY_DRAIN_BATCH_SIZE.saturating_sub(1) {
            let Some(work) = queue.pop_front() else {
                break;
            };
            work_batch.push(work);
        }
        Some(work_batch)
    }

    pub(super) fn notify_all(&self) {
        self.queue_ready.notify_all();
    }

    pub(super) fn snapshot(&self) -> SubscriptionDeliveryQueueSnapshot {
        let queue = self
            .queue
            .lock()
            .expect("subscription delivery queue lock should not be poisoned");
        let oldest_queue_age_nanos = queue
            .front()
            .map(|work| work.enqueued_at.elapsed().as_nanos())
            .unwrap_or(0)
            .min(u128::from(u64::MAX)) as u64;
        SubscriptionDeliveryQueueSnapshot {
            depth: queue.len(),
            capacity: self.capacity.load(Ordering::Relaxed),
            oldest_queue_age_nanos,
        }
    }

    #[cfg(test)]
    pub(super) fn set_capacity_for_testing(&self, capacity: usize) {
        self.capacity.store(capacity.max(1), Ordering::Release);
    }
}
