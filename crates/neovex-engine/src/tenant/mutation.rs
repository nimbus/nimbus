use std::collections::VecDeque;
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use neovex_core::{DocumentId, Error, Mutation, PrincipalContext, Result, SequenceNumber};
use neovex_storage::JournalProgress;
use serde::Serialize;
use tokio::sync::{Notify, oneshot};

use super::TenantOperationGuard;

pub(crate) const DEFAULT_MUTATION_ADMISSION_QUEUE_CAPACITY: usize = 256;
pub(crate) const DEFAULT_MUTATION_JOURNAL_QUEUE_CAPACITY: usize = 256;

pub(crate) enum QueuedMutationResult {
    Immediate(Option<DocumentId>),
    Scheduled(bool),
}

pub(crate) struct QueuedMutationRequest {
    pub mutation: Mutation,
    pub principal: PrincipalContext,
    pub scheduled_execution_id: Option<String>,
    pub cancelled: Arc<AtomicBool>,
    pub _operation: TenantOperationGuard,
    pub response: oneshot::Sender<Result<QueuedMutationResult>>,
    #[cfg_attr(not(test), allow(dead_code))]
    pub enqueued_at: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum MutationAdmissionPhase {
    Idle,
    Dropping,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MutationAdmissionStats {
    pub queue_depth: usize,
    pub queue_capacity: usize,
    pub oldest_queue_age_nanos: u64,
    pub admitted_count: u64,
    pub shed_count: u64,
    pub queue_rejection_count: u64,
    pub codel_phase: MutationAdmissionPhase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MutationJournalStats {
    pub durable_head: SequenceNumber,
    pub applied_head: SequenceNumber,
    pub apply_lag: u64,
    pub queue_depth: usize,
    pub queue_capacity: usize,
    pub oldest_queue_age_nanos: u64,
    pub pending_response_count: u64,
    pub worker_running: bool,
    pub worker_start_count: u64,
    pub worker_restart_count: u64,
    pub queue_rejection_count: u64,
    pub worker_failure_count: u64,
    pub read_wait_count: u64,
    pub total_read_wait_nanos: u64,
}

pub(super) struct MutationAdmissionGate {
    state: Mutex<MutationAdmissionGateState>,
    capacity: AtomicUsize,
    admitted_count: AtomicU64,
    shed_count: AtomicU64,
    queue_rejection_count: AtomicU64,
}

struct MutationAdmissionGateState {
    queue: VecDeque<QueuedMutationRequest>,
    codel: CoDelState,
}

struct CoDelState {
    target: Duration,
    interval: Duration,
    phase: CoDelPhase,
    first_above_time: Option<Instant>,
}

enum CoDelPhase {
    Idle,
    Dropping { drop_next: Instant, drop_count: u32 },
}

pub(super) enum MutationAdmissionDecision {
    Admit(QueuedMutationRequest),
    Reject {
        request: QueuedMutationRequest,
        error: Error,
    },
    Empty,
}

#[cfg(any(test, feature = "test-hooks"))]
#[derive(Debug, Clone)]
pub(crate) struct MutationJournalPauseHandle {
    state: Arc<MutationJournalPauseState>,
}

#[cfg(any(test, feature = "test-hooks"))]
#[derive(Debug, Default)]
pub(super) struct MutationJournalPauseState {
    control: Mutex<MutationJournalPauseControl>,
    entered: Condvar,
    released: Notify,
}

#[cfg(any(test, feature = "test-hooks"))]
#[derive(Debug, Default)]
struct MutationJournalPauseControl {
    armed: bool,
    entered: bool,
    released: bool,
}

pub(super) struct MutationJournalState {
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

impl MutationAdmissionGate {
    pub(super) fn new() -> Self {
        Self {
            state: Mutex::new(MutationAdmissionGateState {
                queue: VecDeque::new(),
                codel: CoDelState::new(Duration::from_millis(5), Duration::from_millis(100)),
            }),
            capacity: AtomicUsize::new(DEFAULT_MUTATION_ADMISSION_QUEUE_CAPACITY),
            admitted_count: AtomicU64::new(0),
            shed_count: AtomicU64::new(0),
            queue_rejection_count: AtomicU64::new(0),
        }
    }

    pub(super) fn enqueue(&self, request: QueuedMutationRequest) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .expect("mutation admission gate lock should not be poisoned");
        let capacity = self.capacity.load(Ordering::Acquire).max(1);
        if state.queue.len() >= capacity {
            self.queue_rejection_count.fetch_add(1, Ordering::Relaxed);
            return Err(Error::ResourceExhausted(format!(
                "mutation admission gate full (capacity {capacity})"
            )));
        }
        state.queue.push_back(request);
        self.admitted_count.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    pub(super) fn pop_next_at(&self, now: Instant) -> MutationAdmissionDecision {
        let mut state = self
            .state
            .lock()
            .expect("mutation admission gate lock should not be poisoned");
        let Some(request) = state.queue.pop_front() else {
            state.codel.reset();
            return MutationAdmissionDecision::Empty;
        };

        let should_drop = state.codel.should_drop(now, request.enqueued_at);
        if state.queue.is_empty() {
            state.codel.reset();
        }

        if should_drop {
            self.shed_count.fetch_add(1, Ordering::Relaxed);
            return MutationAdmissionDecision::Reject {
                request,
                error: Error::ResourceExhausted("mutation shed by admission gate".to_string()),
            };
        }

        MutationAdmissionDecision::Admit(request)
    }

    pub(super) fn has_pending(&self) -> bool {
        !self
            .state
            .lock()
            .expect("mutation admission gate lock should not be poisoned")
            .queue
            .is_empty()
    }

    pub(super) fn stats(&self) -> MutationAdmissionStats {
        let state = self
            .state
            .lock()
            .expect("mutation admission gate lock should not be poisoned");
        let oldest_queue_age_nanos = state
            .queue
            .front()
            .map(|request| request.enqueued_at.elapsed().as_nanos())
            .unwrap_or(0)
            .min(u128::from(u64::MAX)) as u64;
        MutationAdmissionStats {
            queue_depth: state.queue.len(),
            queue_capacity: self.capacity.load(Ordering::Relaxed),
            oldest_queue_age_nanos,
            admitted_count: self.admitted_count.load(Ordering::Relaxed),
            shed_count: self.shed_count.load(Ordering::Relaxed),
            queue_rejection_count: self.queue_rejection_count.load(Ordering::Relaxed),
            codel_phase: state.codel.phase_stats(),
        }
    }

    #[cfg(test)]
    pub(super) fn set_codel_for_testing(&self, target: Duration, interval: Duration) {
        let mut state = self
            .state
            .lock()
            .expect("mutation admission gate lock should not be poisoned");
        state.codel = CoDelState::new(target, interval);
    }
}

impl CoDelState {
    fn new(target: Duration, interval: Duration) -> Self {
        Self {
            target,
            interval,
            phase: CoDelPhase::Idle,
            first_above_time: None,
        }
    }

    fn should_drop(&mut self, now: Instant, enqueued_at: Instant) -> bool {
        let sojourn = now.saturating_duration_since(enqueued_at);
        if sojourn < self.target {
            self.reset();
            return false;
        }

        match &mut self.phase {
            CoDelPhase::Idle => match self.first_above_time {
                None => {
                    self.first_above_time = Some(now + self.interval);
                    false
                }
                Some(first_above_time) if now < first_above_time => false,
                Some(_) => {
                    self.phase = CoDelPhase::Dropping {
                        drop_next: now + codel_drop_interval(self.interval, 1),
                        drop_count: 1,
                    };
                    true
                }
            },
            CoDelPhase::Dropping {
                drop_next,
                drop_count,
            } => {
                if sojourn < self.target {
                    self.reset();
                    return false;
                }
                if now < *drop_next {
                    return false;
                }
                *drop_count = drop_count.saturating_add(1);
                *drop_next = now + codel_drop_interval(self.interval, *drop_count);
                true
            }
        }
    }

    fn reset(&mut self) {
        self.phase = CoDelPhase::Idle;
        self.first_above_time = None;
    }

    fn phase_stats(&self) -> MutationAdmissionPhase {
        match self.phase {
            CoDelPhase::Idle => MutationAdmissionPhase::Idle,
            CoDelPhase::Dropping { .. } => MutationAdmissionPhase::Dropping,
        }
    }
}

fn codel_drop_interval(interval: Duration, drop_count: u32) -> Duration {
    let divisor = f64::from(drop_count.max(1)).sqrt();
    Duration::from_secs_f64((interval.as_secs_f64() / divisor).max(0.000_001))
}

#[cfg(any(test, feature = "test-hooks"))]
impl MutationJournalPauseState {
    pub(super) async fn wait_if_armed(&self) {
        {
            let mut control = self
                .control
                .lock()
                .expect("mutation journal pause lock should not be poisoned");
            if !control.armed {
                return;
            }
            control.entered = true;
            self.entered.notify_all();
            if control.released {
                *control = MutationJournalPauseControl::default();
                return;
            }
        }

        loop {
            let notified = self.released.notified();
            {
                let mut control = self
                    .control
                    .lock()
                    .expect("mutation journal pause lock should not be poisoned");
                if control.released {
                    *control = MutationJournalPauseControl::default();
                    return;
                }
            }
            notified.await;
        }
    }
}

#[cfg(any(test, feature = "test-hooks"))]
impl MutationJournalPauseHandle {
    pub(super) fn from_state(state: Arc<MutationJournalPauseState>) -> Self {
        Self { state }
    }

    pub(crate) fn arm(&self) {
        let mut control = self
            .state
            .control
            .lock()
            .expect("mutation journal pause lock should not be poisoned");
        *control = MutationJournalPauseControl {
            armed: true,
            entered: false,
            released: false,
        };
    }

    pub(crate) fn wait_until_entered(&self, timeout: std::time::Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut control = self
            .state
            .control
            .lock()
            .expect("mutation journal pause lock should not be poisoned");
        while !control.entered {
            let now = Instant::now();
            let Some(remaining) = deadline.checked_duration_since(now) else {
                return false;
            };
            let (next, result) = self
                .state
                .entered
                .wait_timeout(control, remaining)
                .expect("mutation journal pause wait should not be poisoned");
            control = next;
            if result.timed_out() && !control.entered {
                return false;
            }
        }
        true
    }

    pub(crate) fn release(&self) {
        let mut control = self
            .state
            .control
            .lock()
            .expect("mutation journal pause lock should not be poisoned");
        control.released = true;
        self.state.released.notify_waiters();
    }
}

impl MutationJournalState {
    pub(super) fn new(progress: JournalProgress) -> Self {
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

    pub(super) fn enqueue(
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

    pub(super) async fn drain_batch(&self, max_batch_size: usize) -> Vec<QueuedMutationRequest> {
        let mut queue = self
            .queue
            .lock()
            .expect("mutation journal queue lock should not be poisoned");
        let batch_size = queue.len().min(max_batch_size.max(1));
        queue.drain(..batch_size).collect()
    }

    #[cfg(test)]
    pub(super) async fn wait_before_drain(&self) {
        self.pause_before_drain.wait_if_armed().await;
    }

    pub(super) fn release_worker(&self, gate_has_more: bool) -> bool {
        self.worker_running.store(false, Ordering::Release);
        let queue_has_more = gate_has_more
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

    pub(super) fn try_start_worker(&self) -> bool {
        self.worker_running
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    pub(super) fn record_worker_start(&self) {
        self.worker_start_count.fetch_add(1, Ordering::Relaxed);
    }

    pub(super) fn record_worker_failure(&self) {
        self.worker_failure_count.fetch_add(1, Ordering::Relaxed);
    }

    pub(super) fn begin_pending_response(&self) {
        self.pending_response_count.fetch_add(1, Ordering::Relaxed);
    }

    pub(super) fn finish_pending_response(&self) {
        self.pending_response_count.fetch_sub(1, Ordering::Relaxed);
    }

    pub(super) fn durable_head(&self) -> SequenceNumber {
        SequenceNumber(self.durable_head.load(Ordering::Acquire))
    }

    pub(super) fn lock_sequence_gate(&self) -> std::sync::MutexGuard<'_, ()> {
        self.sequence_gate
            .lock()
            .expect("mutation journal sequence gate should not be poisoned")
    }

    pub(super) fn applied_head(&self) -> SequenceNumber {
        SequenceNumber(self.applied_head.load(Ordering::Acquire))
    }

    pub(super) fn mark_durable_head(&self, sequence: SequenceNumber) {
        self.durable_head.store(sequence.0, Ordering::Release);
    }

    pub(super) fn mark_applied_head(&self, sequence: SequenceNumber) {
        let _guard = self
            .applied_wait_lock
            .lock()
            .expect("mutation journal applied wait lock should not be poisoned");
        self.applied_head.store(sequence.0, Ordering::Release);
        self.applied_wait.notify_all();
        self.applied_notify.notify_waiters();
    }

    pub(super) async fn wait_for_applied_sequence_cancellable<Fut>(
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
            if self.applied_head().0 >= required.0 {
                self.record_read_wait(started);
                return Ok(());
            }
            let notified = self.applied_notify.notified();
            tokio::pin!(notified);
            tokio::select! {
                _ = &mut cancel_wait => {
                    self.record_read_wait(started);
                    return Err(Error::Cancelled);
                }
                _ = &mut notified => {}
            }
        }
    }

    pub(super) fn wait_for_applied_sequence_blocking(&self, required: SequenceNumber) {
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

    pub(super) fn stats(&self) -> MutationJournalStats {
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
    pub(super) fn set_capacity_for_testing(&self, capacity: usize) {
        self.capacity.store(capacity.max(1), Ordering::Release);
    }

    #[cfg(test)]
    pub(super) fn pause_handle(&self) -> MutationJournalPauseHandle {
        MutationJournalPauseHandle {
            state: self.pause_before_drain.clone(),
        }
    }
}
