use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use neovex_core::{Timestamp, TriggerInvocationKey, TriggerInvocationState};
use neovex_storage::Clock;
use tracing::warn;

use crate::triggers::execution::{SharedTriggerInvocationExecutor, TriggerInvocationExecution};

use super::TenantRuntime;

const TRIGGER_MAX_ATTEMPTS: u32 = 5;
const TRIGGER_RETRY_POLL_INTERVAL: Duration = Duration::from_millis(25);

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
    worker: Mutex<Option<std::thread::JoinHandle<()>>>,
    shutdown: Arc<AtomicBool>,
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
        clock: &dyn Clock,
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

    fn notify_all(&self) {
        self.queue_ready.notify_all();
    }
}

impl TriggerExecutionWorker {
    fn new() -> Self {
        Self {
            worker: Mutex::new(None),
            shutdown: Arc::new(AtomicBool::new(false)),
        }
    }

    fn start(
        &self,
        runtime: &Arc<TenantRuntime>,
        queue: Arc<TriggerExecutionQueueState>,
        clock: Arc<dyn Clock>,
        executor: SharedTriggerInvocationExecutor,
    ) {
        let mut worker = self
            .worker
            .lock()
            .expect("trigger execution worker lock should not be poisoned");
        if worker.is_some() {
            return;
        }
        self.shutdown.store(false, Ordering::Release);
        let runtime = Arc::downgrade(runtime);
        let shutdown = self.shutdown.clone();
        *worker = Some(
            std::thread::Builder::new()
                .name("neovex-trigger-execution".to_string())
                .spawn(move || {
                    run_trigger_execution_worker(runtime, queue, shutdown, clock, executor)
                })
                .expect("trigger execution worker should spawn"),
        );
    }

    fn shutdown(&self, queue: &Arc<TriggerExecutionQueueState>) {
        self.shutdown.store(true, Ordering::Release);
        queue.notify_all();
        if let Some(worker) = self
            .worker
            .lock()
            .expect("trigger execution worker lock should not be poisoned")
            .take()
        {
            if worker.thread().id() == std::thread::current().id() {
                return;
            }
            let _ = worker.join();
        }
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
        clock: Arc<dyn Clock>,
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
    clock: Arc<dyn Clock>,
    executor: SharedTriggerInvocationExecutor,
) {
    loop {
        let Some(key) = queue.pop_next_ready(&shutdown, clock.as_ref()) else {
            return;
        };

        let Some(runtime) = runtime.upgrade() else {
            return;
        };
        let result: neovex_core::Result<()> = (|| {
            let Some(mut record) = runtime.store.trigger_invocation(&key)? else {
                return Ok(());
            };
            if !matches!(
                record.state,
                TriggerInvocationState::Pending | TriggerInvocationState::RetryPending { .. }
            ) {
                return Ok(());
            }
            record.begin_attempt(clock.now())?;
            runtime.store.save_trigger_invocation(&record)?;
            match executor.execute_invocation(runtime.tenant_id(), &record) {
                TriggerInvocationExecution::Completed => {
                    record.complete(clock.now())?;
                }
                TriggerInvocationExecution::RetryableFailure { error } => {
                    let attempt = record.state.attempt();
                    if let Some(next_attempt_at) = next_retry_attempt_at(attempt, clock.now()) {
                        record.schedule_retry(clock.now(), next_attempt_at, error)?;
                        runtime.store.save_trigger_invocation(&record)?;
                        runtime.enqueue_trigger_invocation_scheduled(vec![(
                            record.key.clone(),
                            next_attempt_at,
                        )]);
                        return Ok(());
                    }
                    record.fail_terminal(clock.now(), error)?;
                }
                TriggerInvocationExecution::TerminalFailure { error } => {
                    record.fail_terminal(clock.now(), error)?;
                }
            }
            runtime.store.save_trigger_invocation(&record)?;
            Ok(())
        })();
        if let Err(error) = result {
            warn!(error = %error, "trigger execution worker failed to execute invocation");
        }
    }
}

fn next_retry_attempt_at(attempt: u32, now: Timestamp) -> Option<Timestamp> {
    retry_delay_for_attempt(attempt).map(|delay| {
        Timestamp(
            now.0
                .saturating_add(delay.as_millis().try_into().unwrap_or(u64::MAX)),
        )
    })
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
