#[cfg(test)]
use std::collections::VecDeque;
use std::sync::Arc;
#[cfg(test)]
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use nimbus_core::CommitEntry;
use tracing::warn;

#[cfg(test)]
use crate::triggers::dispatch::TriggerCommitCandidate;
use crate::triggers::dispatch::build_trigger_commit_candidates;
use crate::triggers::materialize::build_trigger_invocation_records;

use super::TenantRuntime;
use super::background::{BackgroundWorker, WorkQueue};
#[cfg(test)]
use super::pause_barrier::{PauseBarrier, PauseBarrierHandle};

const TRIGGER_CANDIDATE_RETRY_BACKOFF: Duration = Duration::from_millis(10);

struct QueuedTriggerCommitBatch {
    commits: Vec<CommitEntry>,
}

struct TriggerCandidateQueueState {
    queue: WorkQueue<QueuedTriggerCommitBatch>,
}

#[cfg(test)]
struct PendingTriggerCandidateState {
    queue: Mutex<VecDeque<TriggerCommitCandidate>>,
}

struct TriggerCandidateWorker {
    worker: BackgroundWorker,
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
type TriggerCandidatePauseState = PauseBarrier;

#[cfg(test)]
#[derive(Debug, Clone)]
pub(crate) struct TriggerCandidatePauseHandle {
    inner: PauseBarrierHandle,
}

impl TriggerCandidateQueueState {
    fn new() -> Self {
        Self {
            queue: WorkQueue::unbounded(),
        }
    }

    fn enqueue(&self, commits: Vec<CommitEntry>) {
        if commits.is_empty() {
            return;
        }
        let _ = self.queue.enqueue(QueuedTriggerCommitBatch { commits });
    }

    fn requeue_front(&self, commits: Vec<CommitEntry>) {
        if commits.is_empty() {
            return;
        }
        self.queue
            .requeue_front(vec![QueuedTriggerCommitBatch { commits }]);
    }

    fn pop_next(&self, shutdown: &AtomicBool) -> Option<QueuedTriggerCommitBatch> {
        self.queue.pop_next(shutdown)
    }

    fn drain_ready_batches(&self, shutdown: &AtomicBool) -> Option<Vec<QueuedTriggerCommitBatch>> {
        self.queue.drain_ready_batch(shutdown, usize::MAX)
    }

    fn signal_shutdown(&self, shutdown: &AtomicBool) {
        self.queue.signal_shutdown(shutdown);
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
            worker: BackgroundWorker::new(),
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

    fn start_inner(
        &self,
        runtime: &Arc<TenantRuntime>,
        queue: Arc<TriggerCandidateQueueState>,
        #[cfg(test)] pending: Arc<PendingTriggerCandidateState>,
        #[cfg(test)] pause: Option<Arc<TriggerCandidatePauseState>>,
    ) {
        let runtime = Arc::downgrade(runtime);
        self.worker
            .start("nimbus-trigger-candidates", move |shutdown| {
                run_trigger_candidate_worker(
                    runtime,
                    queue,
                    #[cfg(test)]
                    pending,
                    shutdown,
                    #[cfg(test)]
                    pause,
                )
            });
    }

    fn request_shutdown(
        &self,
        queue: &Arc<TriggerCandidateQueueState>,
        #[cfg(test)] pause: &Arc<TriggerCandidatePauseState>,
    ) {
        let queue = queue.clone();
        #[cfg(test)]
        let pause = pause.clone();
        self.worker.shutdown(move |shutdown| {
            // Signal shutdown *before* releasing a worker parked in the test
            // pause barrier: `BackgroundWorker::shutdown` runs this closure
            // synchronously before it joins, so both orderings the two
            // mechanisms need are satisfied by this single sequence — the
            // flag is visible before the paused worker wakes (it won't
            // process or advance past the point it paused at), and the
            // worker is released before `shutdown` attempts to join it (no
            // deadlock on a still-paused worker).
            queue.signal_shutdown(shutdown);
            #[cfg(test)]
            pause.release_for_shutdown();
        });
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
        self.worker.request_shutdown(
            &self.queue,
            #[cfg(test)]
            &self.pause,
        );
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
        TriggerCandidatePauseHandle::new(self.pause.clone())
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
    fn new(state: Arc<TriggerCandidatePauseState>) -> Self {
        Self {
            inner: PauseBarrierHandle::new(state),
        }
    }

    pub(crate) fn arm(&self) {
        self.inner.arm();
    }

    pub(crate) fn wait_until_entered(&self, timeout: Duration) -> bool {
        self.inner.wait_until_entered(timeout).is_some()
    }

    pub(crate) fn release(&self) {
        self.inner.release();
    }
}

fn run_trigger_candidate_worker(
    runtime: std::sync::Weak<TenantRuntime>,
    queue: Arc<TriggerCandidateQueueState>,
    #[cfg(test)] pending: Arc<PendingTriggerCandidateState>,
    shutdown: Arc<AtomicBool>,
    #[cfg(test)] pause: Option<Arc<TriggerCandidatePauseState>>,
) {
    loop {
        let Some(first_batch) = queue.pop_next(&shutdown) else {
            return;
        };
        #[cfg(test)]
        if let Some(pause) = pause.as_ref() {
            pause.wait_if_armed(());
        }
        let Some(mut ready_batches) = queue.drain_ready_batches(&shutdown) else {
            return;
        };
        ready_batches.insert(0, first_batch);
        let mut commits = ready_batches
            .into_iter()
            .flat_map(|batch| batch.commits)
            .collect::<Vec<_>>();

        let Some(runtime) = runtime.upgrade() else {
            return;
        };
        #[cfg(test)]
        let mut candidates = Vec::new();
        let mut processed_count = 0usize;
        let result: nimbus_core::Result<()> = (|| {
            for commit in &commits {
                let commit_candidates = build_trigger_commit_candidates(commit, |locator| {
                    runtime.store.resource_path_binding(locator)
                })?;
                if !runtime.trigger_registry().is_ready() {
                    #[cfg(test)]
                    candidates.extend(commit_candidates);
                    processed_count = processed_count.saturating_add(1);
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
                materialize_trigger_invocations_and_sync(
                    &runtime,
                    records.as_slice(),
                    nimbus_core::TriggerDeliveryCursor::new(commit.sequence),
                )?;
                runtime.enqueue_trigger_invocation_keys(
                    records.iter().map(|record| record.key.clone()).collect(),
                );
                #[cfg(test)]
                candidates.extend(commit_candidates);
                processed_count = processed_count.saturating_add(1);
            }
            Ok(())
        })();
        if let Err(error) = result {
            let unprocessed_count = commits.len().saturating_sub(processed_count);
            let retry_commits = commits.split_off(processed_count);
            queue.requeue_front(retry_commits);
            #[cfg(test)]
            pending.push_all(candidates);
            warn!(
                error = %error,
                unprocessed_count,
                "trigger candidate worker failed to process candidates; requeued unprocessed commits"
            );
            std::thread::sleep(TRIGGER_CANDIDATE_RETRY_BACKOFF);
        } else {
            #[cfg(test)]
            pending.push_all(candidates);
        }
    }
}

fn materialize_trigger_invocations_and_sync(
    runtime: &TenantRuntime,
    records: &[nimbus_core::TriggerInvocationRecord],
    cursor: nimbus_core::TriggerDeliveryCursor,
) -> nimbus_core::Result<()> {
    let _sequence_guard = runtime.lock_mutation_sequence();
    runtime
        .store
        .check_fault(nimbus_storage::FaultPoint::TriggerInvocationMaterializeBeforeCommit)?;
    runtime
        .store
        .materialize_trigger_invocations(records, cursor)?;
    runtime.sync_mutation_journal_progress(runtime.store.journal_progress()?);
    Ok(())
}

impl Drop for TriggerCandidateFeed {
    fn drop(&mut self) {
        self.shutdown();
    }
}
