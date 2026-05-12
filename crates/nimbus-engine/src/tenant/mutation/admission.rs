use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use nimbus_core::{Error, Result};

use super::codel::CoDelState;
use super::requests::{DEFAULT_MUTATION_ADMISSION_QUEUE_CAPACITY, QueuedMutationRequest};
use super::stats::MutationAdmissionStats;

pub(in crate::tenant) struct MutationAdmissionGate {
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

pub(in crate::tenant) enum MutationAdmissionDecision {
    Admit(QueuedMutationRequest),
    Reject {
        request: QueuedMutationRequest,
        error: Error,
    },
    Empty,
}

impl MutationAdmissionGate {
    pub(in crate::tenant) fn new() -> Self {
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

    pub(in crate::tenant) fn enqueue(&self, request: QueuedMutationRequest) -> Result<()> {
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

    pub(in crate::tenant) fn pop_next_at(&self, now: Instant) -> MutationAdmissionDecision {
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

    pub(in crate::tenant) fn has_pending(&self) -> bool {
        !self
            .state
            .lock()
            .expect("mutation admission gate lock should not be poisoned")
            .queue
            .is_empty()
    }

    pub(in crate::tenant) fn stats(&self) -> MutationAdmissionStats {
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
    pub(in crate::tenant) fn set_codel_for_testing(&self, target: Duration, interval: Duration) {
        let mut state = self
            .state
            .lock()
            .expect("mutation admission gate lock should not be poisoned");
        state.codel = CoDelState::new(target, interval);
    }
}
