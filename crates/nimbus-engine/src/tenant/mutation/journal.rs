use std::collections::VecDeque;
use std::future::Future;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::Instant;

use nimbus_core::{Error, Result, SequenceNumber};
use nimbus_storage::JournalProgress;
use tokio::sync::Notify;

#[cfg(any(test, feature = "test-hooks"))]
use super::pause::{MutationJournalPauseHandle, MutationJournalPauseState};
use super::requests::{DEFAULT_MUTATION_JOURNAL_QUEUE_CAPACITY, QueuedMutationRequest};
use super::stats::MutationJournalStats;
#[cfg(any(test, feature = "test-hooks"))]
use std::sync::Arc;

pub(in crate::tenant) struct MutationJournalState {
    queue: Mutex<VecDeque<QueuedMutationRequest>>,
    queue_depth: AtomicUsize,
    capacity: AtomicUsize,
    worker_running: AtomicUsize,
    worker_start_count: AtomicU64,
    queue_rejection_count: AtomicU64,
    worker_failure_count: AtomicU64,
    pending_response_count: AtomicU64,
    applied_wait_lock: Mutex<()>,
    applied_wait: Condvar,
    durable_head: AtomicU64,
    applied_head: AtomicU64,
    read_wait_count: AtomicU64,
    total_read_wait_nanos: AtomicU64,
    applied_notify: Notify,
    #[cfg(any(test, feature = "test-hooks"))]
    pause_before_drain: Arc<MutationJournalPauseState>,
}

pub(super) type MutationJournalEnqueueError = Box<(QueuedMutationRequest, Error)>;

impl MutationJournalState {
    pub(in crate::tenant) fn new(progress: JournalProgress) -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
            queue_depth: AtomicUsize::new(0),
            capacity: AtomicUsize::new(DEFAULT_MUTATION_JOURNAL_QUEUE_CAPACITY),
            worker_running: AtomicUsize::new(0),
            worker_start_count: AtomicU64::new(0),
            queue_rejection_count: AtomicU64::new(0),
            worker_failure_count: AtomicU64::new(0),
            pending_response_count: AtomicU64::new(0),
            applied_wait_lock: Mutex::new(()),
            applied_wait: Condvar::new(),
            durable_head: AtomicU64::new(progress.durable_head.0),
            applied_head: AtomicU64::new(progress.applied_head.0),
            read_wait_count: AtomicU64::new(0),
            total_read_wait_nanos: AtomicU64::new(0),
            applied_notify: Notify::new(),
            #[cfg(any(test, feature = "test-hooks"))]
            pause_before_drain: Arc::new(MutationJournalPauseState::default()),
        }
    }

    pub(in crate::tenant) fn enqueue(
        &self,
        request: QueuedMutationRequest,
    ) -> std::result::Result<(), MutationJournalEnqueueError> {
        let mut queue = self
            .queue
            .lock()
            .expect("mutation journal queue lock should not be poisoned");
        let capacity = self.capacity.load(Ordering::Acquire).max(1);
        if queue.len() >= capacity {
            self.queue_rejection_count.fetch_add(1, Ordering::Relaxed);
            return Err(Box::new((
                request,
                Error::committer_full(
                    format!("mutation journal queue full (capacity {capacity})"),
                    capacity,
                ),
            )));
        }
        queue.push_back(request);
        self.queue_depth.fetch_add(1, Ordering::Release);
        Ok(())
    }

    pub(in crate::tenant) async fn drain_batch(
        &self,
        max_batch_size: usize,
    ) -> Vec<QueuedMutationRequest> {
        let mut queue = self
            .queue
            .lock()
            .expect("mutation journal queue lock should not be poisoned");
        let batch_size = queue.len().min(max_batch_size.max(1));
        let batch = queue.drain(..batch_size).collect::<Vec<_>>();
        self.queue_depth.fetch_sub(batch.len(), Ordering::Release);
        batch
    }

    pub(in crate::tenant) fn drain_all(&self) -> VecDeque<QueuedMutationRequest> {
        let mut queue = self
            .queue
            .lock()
            .expect("mutation journal queue lock should not be poisoned");
        let drained = std::mem::take(&mut *queue);
        self.queue_depth.store(0, Ordering::Release);
        drained
    }

    pub(in crate::tenant) fn queue_depth(&self) -> usize {
        self.queue_depth.load(Ordering::Acquire)
    }

    #[cfg(any(test, feature = "test-hooks"))]
    pub(in crate::tenant) async fn wait_before_drain(&self) {
        self.pause_before_drain.wait_if_armed().await;
    }

    pub(in crate::tenant) fn record_worker_start(&self) {
        let _ = self
            .worker_start_count
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire);
    }

    pub(in crate::tenant) fn set_worker_running(&self, running: bool) {
        if running {
            self.worker_running.fetch_add(1, Ordering::AcqRel);
        } else {
            let previous = self.worker_running.fetch_sub(1, Ordering::AcqRel);
            debug_assert!(previous > 0, "mutation worker activity cannot underflow");
        }
    }

    pub(in crate::tenant) fn record_worker_failure(&self) {
        self.worker_failure_count.fetch_add(1, Ordering::Relaxed);
    }

    pub(in crate::tenant) fn begin_pending_response(&self) {
        self.pending_response_count.fetch_add(1, Ordering::Relaxed);
    }

    pub(in crate::tenant) fn finish_pending_response(&self) {
        self.pending_response_count.fetch_sub(1, Ordering::Relaxed);
    }

    pub(in crate::tenant) fn durable_head(&self) -> SequenceNumber {
        SequenceNumber(self.durable_head.load(Ordering::Acquire))
    }

    pub(in crate::tenant) fn applied_head(&self) -> SequenceNumber {
        SequenceNumber(self.applied_head.load(Ordering::Acquire))
    }

    pub(in crate::tenant) fn mark_durable_head(&self, sequence: SequenceNumber) {
        self.durable_head.fetch_max(sequence.0, Ordering::AcqRel);
    }

    pub(in crate::tenant) fn mark_applied_head(&self, sequence: SequenceNumber) {
        let _guard = self
            .applied_wait_lock
            .lock()
            .expect("mutation journal applied wait lock should not be poisoned");
        let previous = self.applied_head.fetch_max(sequence.0, Ordering::AcqRel);
        if sequence.0 > previous {
            self.applied_wait.notify_all();
            self.applied_notify.notify_waiters();
        }
    }

    pub(in crate::tenant) async fn wait_for_applied_sequence_cancellable<Fut>(
        &self,
        required: SequenceNumber,
        cancel_wait: Fut,
    ) -> Result<()>
    where
        Fut: Future<Output = ()>,
    {
        if self.applied_head().0 >= required.0 {
            return Ok(());
        }

        let started = Instant::now();
        tokio::pin!(cancel_wait);
        loop {
            // Create the notified future before re-checking the head:
            // `notify_waiters` only reaches futures that already exist, so an
            // apply landing between a head check and a later `notified()` call
            // would be lost and this wait would hang until the next mutation.
            let notified = self.applied_notify.notified();
            tokio::pin!(notified);
            if self.applied_head().0 >= required.0 {
                self.record_read_wait(started);
                return Ok(());
            }
            tokio::select! {
                _ = &mut cancel_wait => {
                    self.record_read_wait(started);
                    return Err(Error::Cancelled);
                }
                _ = &mut notified => {}
            }
        }
    }

    pub(in crate::tenant) fn wait_for_applied_sequence_blocking(&self, required: SequenceNumber) {
        if self.applied_head().0 >= required.0 {
            return;
        }

        let started = Instant::now();
        let mut guard = self
            .applied_wait_lock
            .lock()
            .expect("mutation journal applied wait lock should not be poisoned");
        while self.applied_head().0 < required.0 {
            guard = self
                .applied_wait
                .wait(guard)
                .expect("mutation journal applied wait should not be poisoned");
        }
        drop(guard);
        self.record_read_wait(started);
    }

    fn record_read_wait(&self, started: Instant) {
        self.read_wait_count.fetch_add(1, Ordering::Relaxed);
        self.total_read_wait_nanos.fetch_add(
            started.elapsed().as_nanos().try_into().unwrap_or(u64::MAX),
            Ordering::Relaxed,
        );
    }

    pub(in crate::tenant) fn stats(&self) -> MutationJournalStats {
        let durable_head = self.durable_head();
        let applied_head = self.applied_head();
        let queue = self
            .queue
            .lock()
            .expect("mutation journal queue lock should not be poisoned");
        let oldest_queue_age_nanos = queue
            .front()
            .map(|request| request.enqueued_at.elapsed().as_nanos())
            .unwrap_or(0)
            .min(u128::from(u64::MAX)) as u64;
        let worker_start_count = self.worker_start_count.load(Ordering::Relaxed);
        MutationJournalStats {
            durable_head,
            applied_head,
            apply_lag: durable_head.0.saturating_sub(applied_head.0),
            queue_depth: queue.len(),
            queue_capacity: self.capacity.load(Ordering::Relaxed),
            oldest_queue_age_nanos,
            pending_response_count: self.pending_response_count.load(Ordering::Relaxed),
            worker_running: self.worker_running.load(Ordering::Relaxed) > 0,
            worker_start_count,
            worker_restart_count: worker_start_count.saturating_sub(1),
            queue_rejection_count: self.queue_rejection_count.load(Ordering::Relaxed),
            worker_failure_count: self.worker_failure_count.load(Ordering::Relaxed),
            read_wait_count: self.read_wait_count.load(Ordering::Relaxed),
            total_read_wait_nanos: self.total_read_wait_nanos.load(Ordering::Relaxed),
            committer_inbox_depth: 0,
            committer_inbox_capacity: 0,
            committer_send_timeout_count: 0,
            publisher_queue_depth: 0,
            publisher_queue_capacity: 0,
            publisher_send_timeout_count: 0,
            publisher_transient_error_count: 0,
            publisher_fatal_error_count: 0,
            publisher_ambiguous_error_count: 0,
            publisher_mode: super::CommitterPipelineMode::Serial,
            publisher_mode_transition_count: 0,
            publisher_mode_transition_failure_count: 0,
            observer_queue_depth: 0,
            observer_queue_capacity: 0,
            observer_queue_high_watermark: 0,
            observer_queue_high_water_warning_count: 0,
            observer_queue_cap_breach_count: 0,
            observer_dispatch_poisoned: false,
            observer_spawned_work_depth: 0,
            observer_spawned_work_capacity: 0,
            observer_spawned_work_high_watermark: 0,
            observer_spawned_work_high_water_warning_count: 0,
            observer_spawned_work_cap_breach_count: 0,
            observer_spawned_work_poisoned: false,
        }
    }

    #[cfg(test)]
    pub(in crate::tenant) fn set_capacity_for_testing(&self, capacity: usize) {
        self.capacity.store(capacity.max(1), Ordering::Release);
    }

    #[cfg(any(test, feature = "test-hooks"))]
    pub(in crate::tenant) fn pause_handle(&self) -> MutationJournalPauseHandle {
        MutationJournalPauseHandle::from_state(self.pause_before_drain.clone())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn empty_state() -> Arc<MutationJournalState> {
        Arc::new(MutationJournalState::new(JournalProgress {
            durable_head: SequenceNumber(0),
            applied_head: SequenceNumber(0),
        }))
    }

    /// The apply worker advances the head from a blocking-pool thread, in
    /// true parallelism with async waiters. `notify_waiters` only reaches
    /// `Notified` futures that already exist, so the wait loop must create
    /// its notified future before checking the head — otherwise an apply
    /// landing in that gap is lost and the waiter hangs until an unrelated
    /// mutation re-notifies. This hammers that interleaving from both sides
    /// of the window; with the wrong ordering it hangs a round and trips
    /// the per-round timeout.
    #[tokio::test]
    async fn applied_sequence_wait_observes_apply_racing_the_wait_setup() {
        for round in 0..200u32 {
            let state = empty_state();
            let marker = {
                let state = Arc::clone(&state);
                std::thread::spawn(move || {
                    for _ in 0..round {
                        std::hint::spin_loop();
                    }
                    state.mark_applied_head(SequenceNumber(1));
                })
            };
            tokio::time::timeout(
                Duration::from_secs(10),
                state.wait_for_applied_sequence_cancellable(
                    SequenceNumber(1),
                    std::future::pending(),
                ),
            )
            .await
            .unwrap_or_else(|_| {
                panic!("round {round}: the wait must observe the racing apply, not hang")
            })
            .expect("applied-visibility wait should succeed");
            marker.join().expect("apply marker thread should join");
        }
    }

    #[tokio::test]
    async fn applied_sequence_wait_returns_immediately_when_already_applied() {
        let state = empty_state();
        state.mark_applied_head(SequenceNumber(3));
        state
            .wait_for_applied_sequence_cancellable(SequenceNumber(2), std::future::pending())
            .await
            .expect("an already-applied sequence should not wait");
    }
}
