use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex};

/// Shared bounded, blocking work queue for tenant background workers.
///
/// This is the common shape behind the tenant's dedicated-thread delivery
/// pipelines: a `Mutex<VecDeque<T>>` guarded by a `Condvar`, with a capacity
/// check on the normal enqueue path and shutdown-aware blocking pop/drain
/// helpers. Callers that have no real capacity bound (e.g. trigger
/// candidates) construct with `usize::MAX`, which makes the capacity check a
/// no-op while still sharing the pop/drain/notify machinery.
pub(crate) struct WorkQueue<T> {
    queue: Mutex<VecDeque<T>>,
    queue_ready: Condvar,
    capacity: AtomicUsize,
}

impl<T> WorkQueue<T> {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
            queue_ready: Condvar::new(),
            capacity: AtomicUsize::new(capacity),
        }
    }

    pub(crate) fn unbounded() -> Self {
        Self::new(usize::MAX)
    }

    /// Enqueues a single item, rejecting it (returning it back) when the
    /// queue is at capacity.
    pub(crate) fn enqueue(&self, item: T) -> Result<(), T> {
        let mut queue = self
            .queue
            .lock()
            .expect("work queue lock should not be poisoned");
        if queue.len() >= self.capacity.load(Ordering::Acquire).max(1) {
            return Err(item);
        }
        queue.push_back(item);
        self.queue_ready.notify_one();
        Ok(())
    }

    /// Requeues previously-dequeued items at the front of the queue,
    /// bypassing the capacity check. This is a recovery path (e.g. after a
    /// transient store failure) for work that was already admitted once.
    pub(crate) fn requeue_front(&self, items: Vec<T>) {
        if items.is_empty() {
            return;
        }
        let mut queue = self
            .queue
            .lock()
            .expect("work queue lock should not be poisoned");
        for item in items.into_iter().rev() {
            queue.push_front(item);
        }
        self.queue_ready.notify_one();
    }

    pub(crate) fn pop_next(&self, shutdown: &AtomicBool) -> Option<T> {
        let mut queue = self
            .queue
            .lock()
            .expect("work queue lock should not be poisoned");
        loop {
            if shutdown.load(Ordering::Acquire) {
                queue.clear();
                return None;
            }
            if let Some(item) = queue.pop_front() {
                return Some(item);
            }
            queue = self
                .queue_ready
                .wait(queue)
                .expect("work queue wait should not be poisoned");
        }
    }

    /// Drains additional ready items beyond the one already popped by
    /// `pop_next`, up to `max_additional` more (`usize::MAX` for "drain
    /// everything ready").
    pub(crate) fn drain_ready_batch(
        &self,
        shutdown: &AtomicBool,
        max_additional: usize,
    ) -> Option<Vec<T>> {
        let mut queue = self
            .queue
            .lock()
            .expect("work queue lock should not be poisoned");
        if shutdown.load(Ordering::Acquire) {
            queue.clear();
            return None;
        }
        let mut drained = Vec::new();
        while drained.len() < max_additional {
            let Some(item) = queue.pop_front() else {
                break;
            };
            drained.push(item);
        }
        Some(drained)
    }

    /// Sets `shutdown` and wakes every waiter, both while holding the same
    /// queue lock that `pop_next`/`drain_ready_batch` hold across their
    /// shutdown check. Setting the flag and notifying outside that lock (as
    /// a bare `AtomicBool::store` + `Condvar::notify_all`) leaves a lost-
    /// wakeup window: a worker that has just checked `shutdown` (saw false)
    /// but has not yet called `wait` would see neither the flag flip nor the
    /// notification, and park forever on an empty queue. Serializing through
    /// the queue mutex closes that window: the flag can only change while no
    /// waiter is mid-check, so a waiter either observes it before parking or
    /// is already parked and gets woken.
    pub(crate) fn signal_shutdown(&self, shutdown: &AtomicBool) {
        let _queue = self
            .queue
            .lock()
            .expect("work queue lock should not be poisoned");
        shutdown.store(true, Ordering::Release);
        self.queue_ready.notify_all();
    }

    pub(crate) fn len(&self) -> usize {
        self.queue
            .lock()
            .expect("work queue lock should not be poisoned")
            .len()
    }

    pub(crate) fn capacity(&self) -> usize {
        self.capacity.load(Ordering::Relaxed)
    }

    /// Applies `f` to the front item without removing it, for stats
    /// snapshots that need to inspect the oldest queued entry.
    pub(crate) fn with_front<R>(&self, f: impl FnOnce(Option<&T>) -> R) -> R {
        let queue = self
            .queue
            .lock()
            .expect("work queue lock should not be poisoned");
        f(queue.front())
    }

    #[cfg(test)]
    pub(crate) fn set_capacity_for_testing(&self, capacity: usize) {
        self.capacity.store(capacity.max(1), Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enqueue_rejects_when_at_capacity() {
        let queue: WorkQueue<u32> = WorkQueue::new(1);
        queue.enqueue(1).expect("first enqueue should succeed");
        let rejected = queue
            .enqueue(2)
            .expect_err("second enqueue should be rejected");
        assert_eq!(rejected, 2);
        assert_eq!(queue.len(), 1);
    }

    #[test]
    fn unbounded_queue_never_rejects() {
        let queue: WorkQueue<u32> = WorkQueue::unbounded();
        for value in 0..10_000u32 {
            queue
                .enqueue(value)
                .expect("unbounded queue should never reject");
        }
        assert_eq!(queue.len(), 10_000);
    }

    #[test]
    fn pop_next_returns_none_after_shutdown_and_clears_queue() {
        let queue: WorkQueue<u32> = WorkQueue::new(4);
        queue.enqueue(1).expect("enqueue should succeed");
        let shutdown = AtomicBool::new(true);
        assert_eq!(queue.pop_next(&shutdown), None);
        assert_eq!(queue.len(), 0);
    }

    #[test]
    fn drain_ready_batch_respects_max_additional() {
        let queue: WorkQueue<u32> = WorkQueue::new(16);
        for value in 0..5u32 {
            queue.enqueue(value).expect("enqueue should succeed");
        }
        let shutdown = AtomicBool::new(false);
        let drained = queue
            .drain_ready_batch(&shutdown, 2)
            .expect("drain should return Some while not shut down");
        assert_eq!(drained, vec![0, 1]);
        assert_eq!(queue.len(), 3);
    }

    #[test]
    fn requeue_front_preserves_relative_order_ahead_of_existing_items() {
        let queue: WorkQueue<u32> = WorkQueue::unbounded();
        queue.enqueue(3).expect("enqueue should succeed");
        queue.requeue_front(vec![1, 2]);
        let shutdown = AtomicBool::new(false);
        assert_eq!(queue.pop_next(&shutdown), Some(1));
        assert_eq!(queue.pop_next(&shutdown), Some(2));
        assert_eq!(queue.pop_next(&shutdown), Some(3));
    }

    #[test]
    fn signal_shutdown_wakes_a_worker_parked_on_pop_next() {
        use std::sync::Arc;

        let queue: Arc<WorkQueue<u32>> = Arc::new(WorkQueue::new(4));
        let shutdown = Arc::new(AtomicBool::new(false));

        let pop_queue = queue.clone();
        let pop_shutdown = shutdown.clone();
        let worker = std::thread::spawn(move || pop_queue.pop_next(&pop_shutdown));

        // Deterministically wait for the worker to reach the empty-queue
        // wait branch: `pop_next` only releases the queue lock once it is
        // parked in `Condvar::wait`, so a successful `try_lock` here proves
        // the worker is parked rather than still mid-check. This avoids a
        // sleep-based race between the spawn above and the shutdown signal
        // below.
        loop {
            if let Ok(guard) = queue.queue.try_lock() {
                drop(guard);
                break;
            }
            std::thread::yield_now();
        }

        queue.signal_shutdown(&shutdown);

        let result = worker.join().expect("parked worker should not panic");
        assert_eq!(
            result, None,
            "a worker parked on an empty queue must be released by signal_shutdown"
        );
    }
}
