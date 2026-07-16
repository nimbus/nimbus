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
    runtime: &Arc<TenantRuntime>,
    records: &[nimbus_core::TriggerInvocationRecord],
    cursor: nimbus_core::TriggerDeliveryCursor,
) -> nimbus_core::Result<()> {
    let runtime_for_commit = runtime.clone();
    let records = records.to_vec();
    runtime.submit_internal_committer(move || {
        materialize_trigger_invocations_and_sync_in_actor(
            &runtime_for_commit,
            records.as_slice(),
            cursor,
        )
    })
}

fn materialize_trigger_invocations_and_sync_in_actor(
    runtime: &TenantRuntime,
    records: &[nimbus_core::TriggerInvocationRecord],
    cursor: nimbus_core::TriggerDeliveryCursor,
) -> nimbus_core::Result<()> {
    runtime
        .store
        .check_fault(nimbus_storage::FaultPoint::TriggerInvocationMaterializeBeforeCommit)?;
    runtime
        .store
        .materialize_trigger_invocations(records, cursor)?;
    let progress = runtime.store.journal_progress()?;
    // The cursor-advance commit `materialize_trigger_invocations` just
    // appended is zero-write by construction, so it can never change what a
    // materialized-serving snapshot serves. Carry every loaded table's
    // coverage frontier through to the new durable head *before* publishing
    // that head below: a racing query derives `required_sequence` from the
    // durable head, and if the head became visible first, the query could
    // observe it ahead of a table's `covered_sequence` and pay for a
    // spurious reload that changes nothing.
    //
    // That widening is only sound across the span `(floor, head]` we can
    // *prove* is inert. `floor` is the sequence of the commit we just
    // materialized invocations for. The committer actor is process-local, so
    // on a provider-backed tenant a foreign engine
    // process can append -- and apply -- its own commit between our
    // cursor-record write and the `journal_progress()` read above. If that
    // happened, `head` has moved past a real write this process has not
    // folded into its materialized tables, and blindly widening every
    // loaded table's `covered_sequence` to `head` would mark that write as
    // already covered: future queries would then serve stale documents
    // instead of reloading. So before widening anything, re-read the gap
    // and verify every record in it is provably inert (our own
    // cursor-advance commit always is; anything else means a foreign
    // commit landed). If verification fails or the re-read itself errors,
    // skip the widening entirely and fail closed -- loaded tables simply
    // behave as they did pre-TI7 and reload on their next query.
    // Correctness over optimization.
    let floor = cursor.materialized_through;
    // On a process-local sequence authority the durable head is either
    // unchanged or the cursor record just appended above: the committer actor
    // excludes every document writer, and no foreign process can assign a
    // sequence. Account that known zero-write record independently of the
    // wider materialized-read gap below. The wider gap may contain an already
    // staged document commit when this worker lags, which must not strand
    // write-log coverage behind the cursor sequence.
    if runtime.store().has_process_local_sequence_authority() {
        runtime.advance_write_log_zero_write_coverage(progress.durable_head);
    }
    if progress.durable_head.0 > floor.0 {
        let gap_is_inert = runtime
            .store
            .read_durable_journal_from(nimbus_core::SequenceNumber(floor.0.saturating_add(1)))
            .map(|gap_records| gap_is_provably_inert(&gap_records))
            .unwrap_or(false);
        if gap_is_inert {
            runtime.advance_materialized_read_coverage_for_zero_write_commit(
                floor,
                progress.durable_head,
            );
        }
    }
    runtime.sync_mutation_journal_progress_in_actor(progress);
    Ok(())
}

/// True when every record in a re-read journal gap is provably inert, i.e.
/// safe for `materialize_trigger_invocations_and_sync` to assume that
/// widening a loaded table's coverage across it changes nothing it serves.
/// See `TenantEventRecord::is_provably_inert_trigger_delivery_only` for what
/// "inert" means. An empty gap is vacuously inert; in practice this is only
/// called when the gap is non-empty (it always contains at least the
/// cursor-advance commit this call just appended).
fn gap_is_provably_inert(records: &[nimbus_core::TenantEventRecord]) -> bool {
    records
        .iter()
        .all(nimbus_core::TenantEventRecord::is_provably_inert_trigger_delivery_only)
}

impl Drop for TriggerCandidateFeed {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod gap_inertness_tests {
    use nimbus_core::{
        DocumentId, SequenceNumber, TableId, TableName, TenantEventKind, TenantEventRecord,
        Timestamp, TriggerDeliveryCursor, WriteOp, WriteOpType,
    };

    use super::gap_is_provably_inert;

    fn trigger_delivery_record(sequence: u64) -> TenantEventRecord {
        TenantEventRecord::from_events(
            SequenceNumber(sequence),
            Timestamp(sequence * 100),
            vec![TenantEventKind::TriggerDelivery {
                cursor: TriggerDeliveryCursor::new(SequenceNumber(sequence - 1)),
            }],
        )
        .expect("trigger-delivery-only record should construct")
    }

    fn document_write_record(sequence: u64) -> TenantEventRecord {
        let table = TableName::new("tasks").expect("table name should be valid");
        TenantEventRecord::new(
            SequenceNumber(sequence),
            Timestamp(sequence * 100),
            vec![WriteOp {
                table: table.clone(),
                table_id: TableId::new(),
                op_type: WriteOpType::Insert,
                doc_id: DocumentId::new(),
                resource_path_binding: None,
                trigger_write_origin: None,
                previous: None,
                current: None,
            }],
            None,
        )
        .expect("document-write record should construct")
    }

    fn barrier_record(sequence: u64) -> TenantEventRecord {
        TenantEventRecord::barrier(
            SequenceNumber(sequence),
            Timestamp(sequence * 100),
            "foreign-schema-migration".to_string(),
        )
        .expect("barrier record should construct")
    }

    #[test]
    fn gap_containing_only_the_own_cursor_advance_is_inert() {
        // This is the common case: the gap re-read after
        // `materialize_trigger_invocations` sees exactly the zero-write
        // TriggerDelivery record this call just appended, and nothing else
        // landed concurrently.
        assert!(gap_is_provably_inert(&[trigger_delivery_record(2)]));
    }

    #[test]
    fn gap_containing_a_foreign_document_write_is_not_inert() {
        // A foreign engine process appended (and applied) a real document
        // commit between our cursor-record write and the journal_progress
        // read -- this is the exact race this P1 guards against. Widening
        // coverage across this gap would hide that write from queries.
        assert!(!gap_is_provably_inert(&[
            trigger_delivery_record(2),
            document_write_record(3),
        ]));
    }

    #[test]
    fn gap_containing_a_foreign_non_inert_zero_write_record_is_not_inert() {
        // A foreign zero-write record that is NOT a TriggerDelivery advance
        // (e.g. a schema/table-lifecycle change, represented here by a
        // Barrier for minimal construction) is just as unsafe to widen
        // across as a real document write: it is a real state transition
        // this process has not folded in.
        assert!(!gap_is_provably_inert(&[
            trigger_delivery_record(2),
            barrier_record(3),
        ]));
    }
}
