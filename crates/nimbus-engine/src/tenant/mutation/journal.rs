use std::collections::VecDeque;
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::Instant;

use nimbus_core::{Error, Result, SequenceNumber};
use nimbus_storage::JournalProgress;
use tokio::sync::Notify;

#[cfg(test)]
use super::pause::{MutationJournalPauseHandle, MutationJournalPauseState};
use super::requests::{DEFAULT_MUTATION_JOURNAL_QUEUE_CAPACITY, QueuedMutationRequest};
use super::stats::MutationJournalStats;
#[cfg(test)]
use std::sync::Arc;

pub(in crate::tenant) struct MutationJournalState {
    queue: Mutex<VecDeque<QueuedMutationRequest>>,
    capacity: AtomicUsize,
    worker_running: AtomicBool,
    worker_start_count: AtomicU64,
    queue_rejection_count: AtomicU64,
    worker_failure_count: AtomicU64,
    pending_response_count: AtomicU64,
    sequence_gate: Mutex<()>,
    applied_wait_lock: Mutex<()>,
    applied_wait: Condvar,
    durable_head: AtomicU64,
    applied_head: AtomicU64,
    read_wait_count: AtomicU64,
    total_read_wait_nanos: AtomicU64,
    applied_notify: Notify,
    #[cfg(test)]
    pause_before_drain: Arc<MutationJournalPauseState>,
}

pub(super) type MutationJournalEnqueueError = Box<(QueuedMutationRequest, Error)>;

impl MutationJournalState {
    pub(in crate::tenant) fn new(progress: JournalProgress) -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
            capacity: AtomicUsize::new(DEFAULT_MUTATION_JOURNAL_QUEUE_CAPACITY),
            worker_running: AtomicBool::new(false),
            worker_start_count: AtomicU64::new(0),
            queue_rejection_count: AtomicU64::new(0),
            worker_failure_count: AtomicU64::new(0),
            pending_response_count: AtomicU64::new(0),
            sequence_gate: Mutex::new(()),
            applied_wait_lock: Mutex::new(()),
            applied_wait: Condvar::new(),
            durable_head: AtomicU64::new(progress.durable_head.0),
            applied_head: AtomicU64::new(progress.applied_head.0),
            read_wait_count: AtomicU64::new(0),
            total_read_wait_nanos: AtomicU64::new(0),
            applied_notify: Notify::new(),
            #[cfg(test)]
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
                Error::ResourceExhausted(format!(
                    "mutation journal queue full (capacity {capacity})"
                )),
            )));
        }
        queue.push_back(request);
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
        queue.drain(..batch_size).collect()
    }

    #[cfg(test)]
    pub(in crate::tenant) async fn wait_before_drain(&self) {
        self.pause_before_drain.wait_if_armed().await;
    }

    /// Attempts to retire the drain worker. Clears `worker_running` FIRST, then
    /// evaluates whether more work is pending.
    ///
    /// `gate_has_more` (the admission-queue check) MUST be a closure evaluated
    /// AFTER the store, never a pre-computed `bool` argument. Ordering matters:
    /// if the admission queue is read before `worker_running` is cleared, a
    /// producer that enqueues in the window between that read and the store is
    /// invisible to both sides — its `try_start_worker` CAS fails against the
    /// not-yet-cleared flag (so it declines to spawn, trusting this worker), and
    /// this worker's stale check misses the request (so it retires) — leaving the
    /// mutation stranded with no drainer: a lost-wakeup deadlock. With the store
    /// sequenced-before the check, any missed enqueue is guaranteed to observe
    /// `worker_running == false` and spawn its own worker (happens-before:
    /// store → check → producer-enqueue → producer-CAS forces the CAS to read the
    /// cleared flag). Existing Release/AcqRel orderings are sufficient; the fix is
    /// purely program order.
    pub(in crate::tenant) fn release_worker(&self, gate_has_more: impl FnOnce() -> bool) -> bool {
        self.worker_running.store(false, Ordering::Release);
        let queue_has_more = gate_has_more()
            || !self
                .queue
                .lock()
                .expect("mutation journal queue lock should not be poisoned")
                .is_empty();
        queue_has_more
            && self
                .worker_running
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
    }

    pub(in crate::tenant) fn try_start_worker(&self) -> bool {
        self.worker_running
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    pub(in crate::tenant) fn record_worker_start(&self) {
        self.worker_start_count.fetch_add(1, Ordering::Relaxed);
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

    pub(in crate::tenant) fn lock_sequence_gate(&self) -> std::sync::MutexGuard<'_, ()> {
        self.sequence_gate
            .lock()
            .expect("mutation journal sequence gate should not be poisoned")
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
            worker_running: self.worker_running.load(Ordering::Relaxed),
            worker_start_count,
            worker_restart_count: worker_start_count.saturating_sub(1),
            queue_rejection_count: self.queue_rejection_count.load(Ordering::Relaxed),
            worker_failure_count: self.worker_failure_count.load(Ordering::Relaxed),
            read_wait_count: self.read_wait_count.load(Ordering::Relaxed),
            total_read_wait_nanos: self.total_read_wait_nanos.load(Ordering::Relaxed),
        }
    }

    #[cfg(test)]
    pub(in crate::tenant) fn set_capacity_for_testing(&self, capacity: usize) {
        self.capacity.store(capacity.max(1), Ordering::Release);
    }

    #[cfg(test)]
    pub(in crate::tenant) fn worker_running_for_testing(&self) -> bool {
        self.worker_running.load(Ordering::Acquire)
    }

    #[cfg(test)]
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

    /// Regression for a lost-wakeup deadlock in the drain worker's release path.
    ///
    /// `release_worker` must clear `worker_running` BEFORE it evaluates the
    /// pending gate. If the gate (the admission-queue check, threaded in as the
    /// closure) is read first, a producer that enqueues in the window between
    /// that read and the store is invisible to both sides — its
    /// `try_start_worker` CAS fails against the still-set flag (so it declines to
    /// spawn, trusting the running worker) and the stale gate misses its request
    /// (so the worker retires) — stranding the mutation with no drainer forever.
    /// This was originally a `bool` argument evaluated *before* the call, and was
    /// found by the concurrent write-throughput benchmark hanging ~1 run in 4.
    ///
    /// The check is exact and deterministic: from inside the gate closure, a
    /// racing producer could only ever start its own worker once `worker_running`
    /// is already `false`, so the gate MUST observe it cleared.
    #[test]
    fn release_worker_clears_running_before_evaluating_the_gate() {
        let state = empty_state();
        assert!(state.try_start_worker(), "a fresh worker should start");
        assert!(state.worker_running_for_testing());

        let running_when_gate_ran = std::cell::Cell::new(true);
        let re_armed = state.release_worker(|| {
            running_when_gate_ran.set(state.worker_running_for_testing());
            false // no work pending from this worker's perspective
        });

        assert!(
            !running_when_gate_ran.get(),
            "release_worker must clear worker_running BEFORE evaluating the pending \
             gate; evaluating it earlier reopens the lost-wakeup deadlock"
        );
        assert!(!re_armed, "with no pending work the worker retires");
        assert!(
            !state.worker_running_for_testing(),
            "a retired worker leaves worker_running cleared"
        );
    }
}
