use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use nimbus_core::{
    Timestamp, TriggerInvocationKey, TriggerInvocationRecord, TriggerInvocationState, WallClock,
};
use nimbus_storage::FaultPoint;
use tracing::warn;

use crate::triggers::execution::{SharedTriggerInvocationExecutor, TriggerInvocationExecution};

use super::TenantRuntime;
use super::background::BackgroundWorker;

const TRIGGER_MAX_ATTEMPTS: u32 = 5;
const TRIGGER_RETRY_POLL_INTERVAL: Duration = Duration::from_millis(25);
/// Backoff before retrying a trigger invocation whose *store* interaction
/// failed (e.g. a transient I/O error), as opposed to a business-level
/// execution failure. Store retries are unbounded and never count against
/// `TRIGGER_MAX_ATTEMPTS`, which governs business retries only. Mirrors
/// `trigger_candidates`'s `TRIGGER_CANDIDATE_RETRY_BACKOFF` order of
/// magnitude.
const TRIGGER_EXECUTION_STORE_RETRY_BACKOFF: Duration = Duration::from_millis(10);
/// Bound on in-place retries when persisting an already-computed execution
/// outcome (`persist_execution_outcome`). Unlike the pre-execution store
/// retry path above, this cannot be unbounded: the handler has already run
/// by this point, so retrying forever would hold the worker thread hostage
/// to one key's outcome save instead of logging and moving on to the rest
/// of the queue.
const TRIGGER_EXECUTION_OUTCOME_SAVE_MAX_ATTEMPTS: u32 = 5;

#[derive(Clone, PartialEq, Eq)]
struct QueuedTriggerInvocation {
    key: TriggerInvocationKey,
    ready_at: Timestamp,
}

struct TriggerExecutionQueueState {
    queue: Mutex<VecDeque<QueuedTriggerInvocation>>,
    queue_ready: Condvar,
}

struct TriggerExecutionWorker {
    worker: BackgroundWorker,
}

pub(super) struct TriggerExecutionQueue {
    queue: Arc<TriggerExecutionQueueState>,
    worker: Arc<TriggerExecutionWorker>,
}

impl TriggerExecutionQueueState {
    fn new() -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
            queue_ready: Condvar::new(),
        }
    }

    fn enqueue(&self, entries: Vec<QueuedTriggerInvocation>) {
        if entries.is_empty() {
            return;
        }
        let mut queue = self
            .queue
            .lock()
            .expect("trigger execution queue lock should not be poisoned");
        for entry in entries {
            if let Some(existing) = queue.iter_mut().find(|queued| queued.key == entry.key) {
                if entry.ready_at.0 < existing.ready_at.0 {
                    existing.ready_at = entry.ready_at;
                }
                continue;
            }
            queue.push_back(entry);
        }
        queue.make_contiguous().sort_by(|left, right| {
            left.ready_at
                .cmp(&right.ready_at)
                .then(left.key.cmp(&right.key))
        });
        self.queue_ready.notify_all();
    }

    fn pop_next_ready(
        &self,
        shutdown: &AtomicBool,
        clock: &dyn WallClock,
    ) -> Option<TriggerInvocationKey> {
        let mut queue = self
            .queue
            .lock()
            .expect("trigger execution queue lock should not be poisoned");
        loop {
            if shutdown.load(Ordering::Acquire) {
                queue.clear();
                return None;
            }
            if let Some(entry) = queue.front() {
                let now = clock.now();
                if entry.ready_at.0 <= now.0 {
                    return queue.pop_front().map(|queued| queued.key);
                }
                let wait_ms = (entry.ready_at.0 - now.0)
                    .min(TRIGGER_RETRY_POLL_INTERVAL.as_millis() as u64)
                    .max(1);
                let (next_queue, _) = self
                    .queue_ready
                    .wait_timeout(queue, Duration::from_millis(wait_ms))
                    .expect("trigger execution queue timed wait should not be poisoned");
                queue = next_queue;
                continue;
            }
            queue = self
                .queue_ready
                .wait(queue)
                .expect("trigger execution queue wait should not be poisoned");
        }
    }

    /// Sets `shutdown` and wakes every waiter while holding the same queue
    /// lock `pop_next_ready` holds across its shutdown check, so the flag
    /// flip can never land in the gap between that check and the waiter
    /// actually parking on the condvar. See `WorkQueue::signal_shutdown` for
    /// the full lost-wakeup rationale this mirrors.
    fn signal_shutdown(&self, shutdown: &AtomicBool) {
        let _queue = self
            .queue
            .lock()
            .expect("trigger execution queue lock should not be poisoned");
        shutdown.store(true, Ordering::Release);
        self.queue_ready.notify_all();
    }
}

impl TriggerExecutionWorker {
    fn new() -> Self {
        Self {
            worker: BackgroundWorker::new(),
        }
    }

    fn start(
        &self,
        runtime: &Arc<TenantRuntime>,
        queue: Arc<TriggerExecutionQueueState>,
        clock: Arc<dyn WallClock>,
        executor: SharedTriggerInvocationExecutor,
    ) {
        let runtime = Arc::downgrade(runtime);
        self.worker
            .start("nimbus-trigger-execution", move |shutdown| {
                run_trigger_execution_worker(runtime, queue, shutdown, clock, executor)
            });
    }

    fn shutdown(&self, queue: &Arc<TriggerExecutionQueueState>) {
        let queue = queue.clone();
        self.worker
            .shutdown(move |shutdown| queue.signal_shutdown(shutdown));
    }
}

impl TriggerExecutionQueue {
    pub(super) fn new() -> Self {
        Self {
            queue: Arc::new(TriggerExecutionQueueState::new()),
            worker: Arc::new(TriggerExecutionWorker::new()),
        }
    }

    pub(super) fn start_worker(
        &self,
        runtime: &Arc<TenantRuntime>,
        clock: Arc<dyn WallClock>,
        executor: SharedTriggerInvocationExecutor,
    ) {
        self.worker
            .start(runtime, self.queue.clone(), clock, executor);
    }

    pub(super) fn enqueue(&self, keys: Vec<TriggerInvocationKey>) {
        self.enqueue_scheduled(keys.into_iter().map(|key| (key, Timestamp(0))).collect());
    }

    pub(super) fn enqueue_scheduled(&self, entries: Vec<(TriggerInvocationKey, Timestamp)>) {
        self.queue.enqueue(
            entries
                .into_iter()
                .map(|(key, ready_at)| QueuedTriggerInvocation { key, ready_at })
                .collect(),
        );
    }

    pub(super) fn shutdown(&self) {
        self.worker.shutdown(&self.queue);
    }
}

fn run_trigger_execution_worker(
    runtime: std::sync::Weak<TenantRuntime>,
    queue: Arc<TriggerExecutionQueueState>,
    shutdown: Arc<AtomicBool>,
    clock: Arc<dyn WallClock>,
    executor: SharedTriggerInvocationExecutor,
) {
    loop {
        let Some(key) = queue.pop_next_ready(&shutdown, clock.as_ref()) else {
            return;
        };

        let Some(runtime) = runtime.upgrade() else {
            return;
        };

        // Phase 1 (pre-execution): load the record and, for a fresh
        // Pending/RetryPending attempt, mark it Running before the handler
        // runs. The handler has not executed anywhere in this phase, so any
        // failure here is a plain store retry: re-enqueueing the key for
        // another pass cannot cause a duplicate handler invocation.
        let pre_execution: nimbus_core::Result<Option<TriggerInvocationRecord>> = (|| {
            let Some(mut record) = runtime.store.trigger_invocation(&key)? else {
                return Ok(None);
            };
            match record.state {
                TriggerInvocationState::Pending | TriggerInvocationState::RetryPending { .. } => {
                    record.begin_attempt(clock.now())?;
                    runtime.persist_trigger_invocation_transition(&record)?;
                }
                TriggerInvocationState::Running { .. } => {
                    // A Running record is crash/takeover replay. Persisting the
                    // identical record is an idempotent durable claim that
                    // proves this worker still owns the provider lease before
                    // it can invoke the external handler.
                    runtime.persist_trigger_invocation_transition(&record)?;
                }
                _ => return Ok(None),
            }
            Ok(Some(record))
        })();

        let mut record = match pre_execution {
            Ok(Some(record)) => record,
            Ok(None) => continue,
            Err(error) if matches!(error, nimbus_core::Error::CommitterFenced { .. }) => {
                warn!(
                    error = %error,
                    key = ?key,
                    "trigger execution worker lost tenant authority before handler execution"
                );
                continue;
            }
            Err(error) => {
                requeue_for_store_retry(&queue, &key, clock.as_ref(), &error);
                continue;
            }
        };

        // Phase 2 (post-execution): run the handler exactly once and
        // compute the resulting state transition in memory. `record.state`
        // is guaranteed `Running` here (just set above, or loaded as such),
        // so `complete`/`schedule_retry`/`fail_terminal` cannot fail; the
        // handler itself has now run, so from here on a persistence failure
        // must retry the *save* in place (`persist_execution_outcome`)
        // rather than re-enqueue the key, which would re-run the handler.
        match executor.execute_invocation(runtime.tenant_id(), &record) {
            TriggerInvocationExecution::Completed => {
                record
                    .complete(clock.now())
                    .expect("record must be Running immediately after execute_invocation");
                persist_execution_outcome(&runtime, &record);
            }
            TriggerInvocationExecution::RetryableFailure { error } => {
                let attempt = record.state.attempt();
                if let Some(next_attempt_at) = next_retry_attempt_at(attempt, clock.now()) {
                    record
                        .schedule_retry(clock.now(), next_attempt_at, error)
                        .expect("record must be Running immediately after execute_invocation");
                    if persist_execution_outcome(&runtime, &record) {
                        runtime.enqueue_trigger_invocation_scheduled(vec![(
                            record.key.clone(),
                            next_attempt_at,
                        )]);
                    }
                } else {
                    record
                        .fail_terminal(clock.now(), error)
                        .expect("record must be Running immediately after execute_invocation");
                    persist_execution_outcome(&runtime, &record);
                }
            }
            TriggerInvocationExecution::TerminalFailure { error } => {
                record
                    .fail_terminal(clock.now(), error)
                    .expect("record must be Running immediately after execute_invocation");
                persist_execution_outcome(&runtime, &record);
            }
        }
    }
}

/// Re-enqueues `key` for a plain store retry after a **pre-execution**
/// failure (loading the record, or persisting a fresh Running attempt). The
/// handler has not run yet at this point, so replaying the key is safe and
/// matches GR4's at-least-once store-retry contract. Store retries are
/// unbounded and never count against `TRIGGER_MAX_ATTEMPTS`.
fn requeue_for_store_retry(
    queue: &TriggerExecutionQueueState,
    key: &TriggerInvocationKey,
    clock: &dyn WallClock,
    error: &nimbus_core::Error,
) {
    let retry_at = store_retry_ready_at(clock.now());
    queue.enqueue(vec![QueuedTriggerInvocation {
        key: key.clone(),
        ready_at: retry_at,
    }]);
    warn!(
        error = %error,
        key = ?key,
        "trigger execution worker failed to prepare invocation for execution; re-enqueued for store retry"
    );
}

/// Persists an already-computed execution outcome. The handler has already
/// run by the time this is called, so — unlike `requeue_for_store_retry` —
/// a failure here must never re-enqueue the key: that would run
/// `execute_invocation` again for an attempt that already completed.
/// Instead this retries the save itself, in place, up to
/// `TRIGGER_EXECUTION_OUTCOME_SAVE_MAX_ATTEMPTS` times; if every attempt
/// fails, it logs and leaves the durable record `Running`. Either way the
/// handler ran exactly once: this is no worse than a pre-GR4 warn-and-drop,
/// and never double-executes.
///
/// Returns whether the outcome was durably saved.
fn persist_execution_outcome(
    runtime: &Arc<TenantRuntime>,
    record: &TriggerInvocationRecord,
) -> bool {
    for attempt in 1..=TRIGGER_EXECUTION_OUTCOME_SAVE_MAX_ATTEMPTS {
        let result: nimbus_core::Result<()> = (|| {
            runtime
                .store
                .check_fault(FaultPoint::TriggerExecutionBeforeSave)?;
            runtime.persist_trigger_invocation_transition(record)?;
            Ok(())
        })();
        match result {
            Ok(()) => return true,
            Err(error) if matches!(error, nimbus_core::Error::CommitterFenced { .. }) => {
                warn!(
                    error = %error,
                    key = ?record.key,
                    "trigger execution worker lost tenant authority while persisting the computed outcome"
                );
                return false;
            }
            Err(error) if attempt < TRIGGER_EXECUTION_OUTCOME_SAVE_MAX_ATTEMPTS => {
                warn!(
                    error = %error,
                    key = ?record.key,
                    attempt,
                    "trigger execution worker failed to persist a computed outcome; retrying the save in place without re-running the handler"
                );
                std::thread::sleep(TRIGGER_EXECUTION_STORE_RETRY_BACKOFF);
            }
            Err(error) => {
                warn!(
                    error = %error,
                    key = ?record.key,
                    "trigger execution worker exhausted outcome-save retries; the handler already ran exactly once but its outcome could not be persisted, leaving the durable record Running"
                );
            }
        }
    }
    false
}

fn store_retry_ready_at(now: Timestamp) -> Timestamp {
    now.saturating_add_duration(TRIGGER_EXECUTION_STORE_RETRY_BACKOFF)
}

fn next_retry_attempt_at(attempt: u32, now: Timestamp) -> Option<Timestamp> {
    retry_delay_for_attempt(attempt).map(|delay| now.saturating_add_duration(delay))
}

fn retry_delay_for_attempt(attempt: u32) -> Option<Duration> {
    if attempt >= TRIGGER_MAX_ATTEMPTS {
        return None;
    }
    let delay_ms = match attempt {
        1 => 50,
        2 => 100,
        3 => 250,
        _ => 500,
    };
    Some(Duration::from_millis(delay_ms))
}

impl Drop for TriggerExecutionQueue {
    fn drop(&mut self) {
        self.shutdown();
    }
}
