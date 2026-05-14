use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use nimbus_core::CommitEntry;
use tracing::warn;

#[cfg(test)]
use crate::triggers::dispatch::TriggerCommitCandidate;
use crate::triggers::dispatch::build_trigger_commit_candidates;
use crate::triggers::materialize::build_trigger_invocation_records;

use super::TenantRuntime;

struct QueuedTriggerCommitBatch {
    commits: Vec<CommitEntry>,
}

struct TriggerCandidateQueueState {
    queue: Mutex<VecDeque<QueuedTriggerCommitBatch>>,
    queue_ready: Condvar,
}

#[cfg(test)]
struct PendingTriggerCandidateState {
    queue: Mutex<VecDeque<TriggerCommitCandidate>>,
}

struct TriggerCandidateWorker {
    worker: Mutex<Option<std::thread::JoinHandle<()>>>,
    shutdown: Arc<AtomicBool>,
}

pub(super) struct TriggerCandidateFeed {
    queue: Arc<TriggerCandidateQueueState>,
    #[cfg(test)]
    pending: Arc<PendingTriggerCandidateState>,
    worker: Arc<TriggerCandidateWorker>,
    #[cfg(test)]
    pause: Arc<TriggerCandidatePauseState>,
}

#[cfg(test)]
#[derive(Debug, Clone)]
pub(crate) struct TriggerCandidatePauseHandle {
    state: Arc<TriggerCandidatePauseState>,
}

#[cfg(test)]
#[derive(Debug, Default)]
struct TriggerCandidatePauseControl {
    armed: bool,
    entered: bool,
    released: bool,
}

#[cfg(test)]
#[derive(Debug, Default)]
struct TriggerCandidatePauseState {
    control: Mutex<TriggerCandidatePauseControl>,
    condvar: Condvar,
}

impl TriggerCandidateQueueState {
    fn new() -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
            queue_ready: Condvar::new(),
        }
    }

    fn enqueue(&self, commits: Vec<CommitEntry>) {
        if commits.is_empty() {
            return;
        }
        let mut queue = self
            .queue
            .lock()
            .expect("trigger candidate queue lock should not be poisoned");
        queue.push_back(QueuedTriggerCommitBatch { commits });
        self.queue_ready.notify_one();
    }

    fn pop_next(&self, shutdown: &AtomicBool) -> Option<QueuedTriggerCommitBatch> {
        let mut queue = self
            .queue
            .lock()
            .expect("trigger candidate queue lock should not be poisoned");
        loop {
            if shutdown.load(Ordering::Acquire) {
                queue.clear();
                return None;
            }
            if let Some(batch) = queue.pop_front() {
                return Some(batch);
            }
            queue = self
                .queue_ready
                .wait(queue)
                .expect("trigger candidate queue wait should not be poisoned");
        }
    }

    fn drain_ready_batches(&self, shutdown: &AtomicBool) -> Option<Vec<QueuedTriggerCommitBatch>> {
        let mut queue = self
            .queue
            .lock()
            .expect("trigger candidate queue lock should not be poisoned");
        if shutdown.load(Ordering::Acquire) {
            queue.clear();
            return None;
        }
        let mut drained = Vec::new();
        while let Some(batch) = queue.pop_front() {
            drained.push(batch);
        }
        Some(drained)
    }

    fn notify_all(&self) {
        self.queue_ready.notify_all();
    }
}

#[cfg(test)]
impl PendingTriggerCandidateState {
    fn new() -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
        }
    }

    fn push_all(&self, candidates: Vec<TriggerCommitCandidate>) {
        if candidates.is_empty() {
            return;
        }
        let mut queue = self
            .queue
            .lock()
            .expect("pending trigger candidate queue lock should not be poisoned");
        queue.extend(candidates);
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.queue
            .lock()
            .expect("pending trigger candidate queue lock should not be poisoned")
            .len()
    }

    #[cfg(test)]
    fn drain_all(&self) -> Vec<TriggerCommitCandidate> {
        self.queue
            .lock()
            .expect("pending trigger candidate queue lock should not be poisoned")
            .drain(..)
            .collect()
    }
}

impl TriggerCandidateWorker {
    fn new() -> Self {
        Self {
            worker: Mutex::new(None),
            shutdown: Arc::new(AtomicBool::new(false)),
        }
    }

    #[cfg(test)]
    fn start(
        &self,
        runtime: &Arc<TenantRuntime>,
        queue: Arc<TriggerCandidateQueueState>,
        pending: Arc<PendingTriggerCandidateState>,
        pause: Arc<TriggerCandidatePauseState>,
    ) {
        self.start_inner(runtime, queue, pending, Some(pause));
    }

    #[cfg(not(test))]
    fn start(&self, runtime: &Arc<TenantRuntime>, queue: Arc<TriggerCandidateQueueState>) {
        self.start_inner(runtime, queue);
    }

    #[cfg(test)]
    fn start_inner(
        &self,
        runtime: &Arc<TenantRuntime>,
        queue: Arc<TriggerCandidateQueueState>,
        pending: Arc<PendingTriggerCandidateState>,
        pause: Option<Arc<TriggerCandidatePauseState>>,
    ) {
        let mut worker = self
            .worker
            .lock()
            .expect("trigger candidate worker lock should not be poisoned");
        if worker.is_some() {
            return;
        }
        self.shutdown.store(false, Ordering::Release);
        let runtime = Arc::downgrade(runtime);
        let shutdown = self.shutdown.clone();
        *worker = Some(
            std::thread::Builder::new()
                .name("nimbus-trigger-candidates".to_string())
                .spawn(move || {
                    run_trigger_candidate_worker(runtime, queue, pending, shutdown, pause)
                })
                .expect("trigger candidate worker should spawn"),
        );
    }

    #[cfg(not(test))]
    fn start_inner(&self, runtime: &Arc<TenantRuntime>, queue: Arc<TriggerCandidateQueueState>) {
        let mut worker = self
            .worker
            .lock()
            .expect("trigger candidate worker lock should not be poisoned");
        if worker.is_some() {
            return;
        }
        self.shutdown.store(false, Ordering::Release);
        let runtime = Arc::downgrade(runtime);
        let shutdown = self.shutdown.clone();
        *worker = Some(
            std::thread::Builder::new()
                .name("nimbus-trigger-candidates".to_string())
                .spawn(move || run_trigger_candidate_worker(runtime, queue, shutdown))
                .expect("trigger candidate worker should spawn"),
        );
    }

    fn request_shutdown(&self, queue: &Arc<TriggerCandidateQueueState>) {
        self.shutdown.store(true, Ordering::Release);
        queue.notify_all();
    }

    fn join(&self) {
        if let Some(worker) = self
            .worker
            .lock()
            .expect("trigger candidate worker lock should not be poisoned")
            .take()
        {
            if worker.thread().id() == std::thread::current().id() {
                return;
            }
            let _ = worker.join();
        }
    }
}

impl TriggerCandidateFeed {
    pub(super) fn new() -> Self {
        Self {
            queue: Arc::new(TriggerCandidateQueueState::new()),
            #[cfg(test)]
            pending: Arc::new(PendingTriggerCandidateState::new()),
            worker: Arc::new(TriggerCandidateWorker::new()),
            #[cfg(test)]
            pause: Arc::new(TriggerCandidatePauseState::default()),
        }
    }

    pub(super) fn start_worker(&self, runtime: &Arc<TenantRuntime>) {
        self.worker.start(
            runtime,
            self.queue.clone(),
            #[cfg(test)]
            self.pending.clone(),
            #[cfg(test)]
            self.pause.clone(),
        );
    }

    pub(super) fn enqueue_commits(&self, commits: Vec<CommitEntry>) {
        self.queue.enqueue(commits);
    }

    pub(super) fn shutdown(&self) {
        self.worker.request_shutdown(&self.queue);
        #[cfg(test)]
        self.pause.release_for_shutdown();
        self.worker.join();
    }

    #[cfg(test)]
    pub(super) fn pending_count(&self) -> usize {
        self.pending.len()
    }

    #[cfg(test)]
    pub(super) fn drain_pending(&self) -> Vec<TriggerCommitCandidate> {
        self.pending.drain_all()
    }

    #[cfg(test)]
    pub(super) fn pause_handle(&self) -> TriggerCandidatePauseHandle {
        TriggerCandidatePauseHandle {
            state: self.pause.clone(),
        }
    }
}

impl TenantRuntime {
    pub(crate) fn ensure_trigger_candidate_worker_started(self: &Arc<Self>) {
        self.trigger_candidates.start_worker(self);
    }

    pub(crate) fn enqueue_trigger_commit_batch(&self, commits: Vec<CommitEntry>) {
        self.trigger_candidates.enqueue_commits(commits);
    }

    pub(crate) fn shutdown_trigger_candidates(&self) {
        self.trigger_candidates.shutdown();
    }

    #[cfg(test)]
    pub(crate) fn pending_trigger_candidate_count_for_testing(&self) -> usize {
        self.trigger_candidates.pending_count()
    }

    #[cfg(test)]
    pub(crate) fn drain_trigger_candidates_for_testing(&self) -> Vec<TriggerCommitCandidate> {
        self.trigger_candidates.drain_pending()
    }

    #[cfg(test)]
    pub(crate) fn trigger_candidate_pause_handle_for_testing(&self) -> TriggerCandidatePauseHandle {
        self.trigger_candidates.pause_handle()
    }
}

#[cfg(test)]
impl TriggerCandidatePauseHandle {
    pub(crate) fn arm(&self) {
        let mut control = self
            .state
            .control
            .lock()
            .expect("trigger candidate pause lock should not be poisoned");
        control.armed = true;
        control.entered = false;
        control.released = false;
    }

    pub(crate) fn wait_until_entered(&self, timeout: std::time::Duration) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        let mut control = self
            .state
            .control
            .lock()
            .expect("trigger candidate pause lock should not be poisoned");
        while control.armed && !control.entered {
            let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now()) else {
                return false;
            };
            let (next_control, wait_result) = self
                .state
                .condvar
                .wait_timeout(control, remaining)
                .expect("trigger candidate pause wait should not be poisoned");
            control = next_control;
            if wait_result.timed_out() {
                return control.entered;
            }
        }
        control.entered
    }

    pub(crate) fn release(&self) {
        let mut control = self
            .state
            .control
            .lock()
            .expect("trigger candidate pause lock should not be poisoned");
        control.released = true;
        self.state.condvar.notify_all();
    }
}

#[cfg(test)]
impl TriggerCandidatePauseState {
    fn release_for_shutdown(&self) {
        let mut control = self
            .control
            .lock()
            .expect("trigger candidate pause lock should not be poisoned");
        if control.armed && !control.released {
            control.released = true;
            self.condvar.notify_all();
        }
    }

    fn wait_if_armed(&self) {
        let mut control = self
            .control
            .lock()
            .expect("trigger candidate pause lock should not be poisoned");
        if !control.armed {
            return;
        }
        control.entered = true;
        self.condvar.notify_all();
        while !control.released {
            control = self
                .condvar
                .wait(control)
                .expect("trigger candidate pause wait should not be poisoned");
        }
        *control = TriggerCandidatePauseControl::default();
    }
}

#[cfg(test)]
fn run_trigger_candidate_worker(
    runtime: std::sync::Weak<TenantRuntime>,
    queue: Arc<TriggerCandidateQueueState>,
    #[cfg(test)] pending: Arc<PendingTriggerCandidateState>,
    shutdown: Arc<AtomicBool>,
    pause: Option<Arc<TriggerCandidatePauseState>>,
) {
    loop {
        let Some(first_batch) = queue.pop_next(&shutdown) else {
            return;
        };
        if let Some(pause) = pause.as_ref() {
            pause.wait_if_armed();
        }
        let Some(mut ready_batches) = queue.drain_ready_batches(&shutdown) else {
            return;
        };
        ready_batches.insert(0, first_batch);

        let Some(runtime) = runtime.upgrade() else {
            return;
        };
        let mut candidates = Vec::new();
        let result: nimbus_core::Result<()> = (|| {
            for batch in ready_batches {
                for commit in batch.commits {
                    let commit_candidates = build_trigger_commit_candidates(&commit, |locator| {
                        runtime.store.resource_path_binding(locator)
                    })?;
                    candidates.extend(commit_candidates.clone());
                    if !runtime.trigger_registry().is_ready() {
                        continue;
                    }
                    let mut records = Vec::new();
                    for candidate in &commit_candidates {
                        records.extend(build_trigger_invocation_records(
                            runtime.tenant_id(),
                            runtime.trigger_registry(),
                            candidate,
                        )?);
                    }
                    runtime.store.materialize_trigger_invocations(
                        records.as_slice(),
                        nimbus_core::TriggerDeliveryCursor::new(commit.sequence),
                    )?;
                    runtime.enqueue_trigger_invocation_keys(
                        records.iter().map(|record| record.key.clone()).collect(),
                    );
                }
            }
            Ok(())
        })();
        match result {
            Ok(()) => pending.push_all(candidates),
            Err(error) => {
                warn!(error = %error, "trigger candidate worker failed to build candidates")
            }
        }
    }
}

#[cfg(not(test))]
fn run_trigger_candidate_worker(
    runtime: std::sync::Weak<TenantRuntime>,
    queue: Arc<TriggerCandidateQueueState>,
    shutdown: Arc<AtomicBool>,
) {
    loop {
        let Some(first_batch) = queue.pop_next(&shutdown) else {
            return;
        };
        let Some(mut ready_batches) = queue.drain_ready_batches(&shutdown) else {
            return;
        };
        ready_batches.insert(0, first_batch);

        let Some(runtime) = runtime.upgrade() else {
            return;
        };
        let result: nimbus_core::Result<()> = (|| {
            for batch in ready_batches {
                for commit in batch.commits {
                    let commit_candidates = build_trigger_commit_candidates(&commit, |locator| {
                        runtime.store.resource_path_binding(locator)
                    })?;
                    if !runtime.trigger_registry().is_ready() {
                        continue;
                    }
                    let mut records = Vec::new();
                    for candidate in &commit_candidates {
                        records.extend(build_trigger_invocation_records(
                            runtime.tenant_id(),
                            runtime.trigger_registry(),
                            candidate,
                        )?);
                    }
                    runtime.store.materialize_trigger_invocations(
                        records.as_slice(),
                        nimbus_core::TriggerDeliveryCursor::new(commit.sequence),
                    )?;
                    runtime.enqueue_trigger_invocation_keys(
                        records.iter().map(|record| record.key.clone()).collect(),
                    );
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            warn!(error = %error, "trigger candidate worker failed to build candidates");
        }
    }
}

impl Drop for TriggerCandidateFeed {
    fn drop(&mut self) {
        self.shutdown();
    }
}
