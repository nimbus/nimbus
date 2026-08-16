use std::collections::{HashMap, HashSet};
use std::ops::Bound::{Excluded, Included};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nimbus_core::ResourcePathBinding;
use nimbus_core::{
    CommitEntry, DependencySet, Document, DocumentId, Error, Result, SequenceNumber, TableId,
    TableName, TenantEventRecord, Timestamp, commit_intersects_dependency_set,
};
use rpds::RedBlackTreeMapSync;

use crate::tenant::WriteLogFrontierSample;

const DEFAULT_MIN_RETENTION_SECS: usize = 30;
const DEFAULT_MAX_RETENTION_SECS: usize = 300;
const DEFAULT_SOFT_MAX_BYTES: usize = 32 * 1024 * 1024;

type PersistentOrdMap<K, V> = RedBlackTreeMapSync<K, V>;

/// Retention policy for one tenant's in-memory full-image conflict window.
///
/// The minimum is hard: entries younger than it are never removed, even when
/// the byte budget is exceeded. The maximum and byte budget force progress
/// past a stalled reader once the minimum has elapsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WriteLogConfig {
    min_retention_ms: u64,
    max_retention_ms: u64,
    soft_max_bytes: usize,
}

impl WriteLogConfig {
    pub(crate) fn from_env() -> Self {
        let min_retention_secs = crate::config::env_positive_usize(
            "NIMBUS_WRITE_LOG_MIN_RETENTION_SECS",
            DEFAULT_MIN_RETENTION_SECS,
        );
        let max_retention_secs = crate::config::env_positive_usize(
            "NIMBUS_WRITE_LOG_MAX_RETENTION_SECS",
            DEFAULT_MAX_RETENTION_SECS,
        )
        .max(min_retention_secs);
        Self::for_tests(
            min_retention_secs,
            max_retention_secs,
            crate::config::env_positive_usize(
                "NIMBUS_WRITE_LOG_SOFT_MAX_BYTES",
                DEFAULT_SOFT_MAX_BYTES,
            ),
        )
    }

    fn for_tests(
        min_retention_secs: usize,
        max_retention_secs: usize,
        soft_max_bytes: usize,
    ) -> Self {
        Self {
            min_retention_ms: seconds_to_millis(min_retention_secs),
            max_retention_ms: seconds_to_millis(max_retention_secs)
                .max(seconds_to_millis(min_retention_secs)),
            soft_max_bytes: soft_max_bytes.max(1),
        }
    }
}

fn seconds_to_millis(seconds: usize) -> u64 {
    u64::try_from(seconds)
        .unwrap_or(u64::MAX)
        .saturating_mul(1_000)
}

#[derive(Debug, Clone)]
struct WindowEntry {
    sequence: SequenceNumber,
    change: WindowChange,
    observed_at: Timestamp,
    accounted_bytes: usize,
}

#[derive(Debug, Clone)]
enum WindowChange {
    DocumentCommit(Arc<CommitEntry>),
    WholeTables(Arc<HashSet<TableName>>),
}

impl WindowEntry {
    fn document_commit(commit: CommitEntry, observed_at: Timestamp) -> Self {
        // Serialized size deliberately includes both full document images in
        // every WriteOp. It is a stable conservative payload budget rather
        // than a key-only estimate; fixed map/Arc overhead is added as well.
        let image_bytes = serde_json::to_vec(&commit)
            .expect("CommitEntry values must always serialize")
            .len();
        let accounted_bytes = image_bytes
            .saturating_add(std::mem::size_of::<CommitEntry>())
            .saturating_add(std::mem::size_of::<WindowEntry>());
        Self {
            sequence: commit.sequence,
            change: WindowChange::DocumentCommit(Arc::new(commit)),
            observed_at,
            accounted_bytes,
        }
    }

    fn whole_tables(record: &TenantEventRecord, observed_at: Timestamp) -> Option<Self> {
        let tables = record.schema_epoch_tables();
        if tables.is_empty() {
            return None;
        }
        let accounted_bytes = serde_json::to_vec(record)
            .expect("TenantEventRecord values must always serialize")
            .len()
            .saturating_add(std::mem::size_of::<WindowEntry>());
        Some(Self {
            sequence: record.sequence,
            change: WindowChange::WholeTables(Arc::new(tables)),
            observed_at,
            accounted_bytes,
        })
    }
}

#[derive(Debug)]
struct WriteLogState {
    published: PersistentOrdMap<SequenceNumber, Arc<WindowEntry>>,
    pending: PersistentOrdMap<SequenceNumber, Arc<WindowEntry>>,
    /// O(1) lookup of the latest retained published full image for caller-side
    /// single-document prepare. The Arc points back into `published`, so this
    /// index does not duplicate document payloads.
    published_documents: HashMap<(TableName, DocumentId), PublishedDocumentPointer>,
    pending_documents: HashMap<(TableName, DocumentId), PublishedDocumentPointer>,
    accounted_bytes: usize,
    /// All sequences at or below this point are known to the runtime. Some
    /// may be zero-write records and therefore have no map entry.
    covered_through: SequenceNumber,
    /// The runtime started after this sequence was already applied, so older
    /// snapshots require a storage scan even though the live suffix is known.
    bootstrap_sequence: SequenceNumber,
    /// Highest assigned sequence observed by this runtime, including the
    /// durable-but-unapplied startup tail.
    assigned_through: SequenceNumber,
    /// Monotonic history of assignment, including suffixes later proven not to
    /// have committed and removed from `assigned_through`.
    assigned_high_water: SequenceNumber,
    /// Highest sequence whose publish position has been crossed. This tracks
    /// zero-write/recovered positions too, while entry publication below is
    /// still asserted strictly monotonic.
    published_through: SequenceNumber,
    /// Highest storage-side applied watermark observed by this runtime.
    ///
    /// Storage transactions for later zero-write records can report a value
    /// beyond an earlier document entry that is still pending in the engine.
    /// This is therefore only a candidate frontier: `published_through` may
    /// catch up to it once every lower pending image is explicitly published.
    observed_applied_through: SequenceNumber,
    /// Highest sequence removed from the front of the retained window.
    purged_sequence: SequenceNumber,
    /// False after a persistence attempt may have committed without staging
    /// its full image. Such a runtime uses authoritative storage validation
    /// until restart rather than risking a false in-memory pass.
    coverage_known: bool,
    /// Assignment epochs are retained independently from conflict-window
    /// trimming. Schema changes are rare and an execution unit may need to
    /// compare its published schema snapshot with a newer assigned epoch even
    /// after the corresponding conflict marker ages out.
    schema_epoch_history: HashMap<TableName, PersistentOrdMap<SequenceNumber, SequenceNumber>>,
    published_schema_epochs: HashMap<TableName, SequenceNumber>,
}

/// Per-tenant, structurally shared mirror of recent full commit images.
///
/// Mutation publication takes the mutex only to replace persistent-map roots.
/// Validation clones those roots in O(1), releases the mutex, then scans its
/// stable view. The pending map is intentionally present before publish is
/// pipelined: today's serial committer normally makes it empty whenever a
/// validator runs, while PPSC5 will consume the same API directly.
pub(crate) struct WriteLog {
    config: WriteLogConfig,
    state: Mutex<WriteLogState>,
}

#[derive(Debug, Clone)]
struct PublishedDocumentPointer {
    sequence: SequenceNumber,
    entry: Arc<WindowEntry>,
    write_index: usize,
}

impl WriteLog {
    pub(crate) fn new(
        config: WriteLogConfig,
        covered_through: SequenceNumber,
        assigned_through: SequenceNumber,
    ) -> Self {
        // Shared providers read durable/applied heads in separate statements,
        // so a foreign commit can transiently produce applied > durable. Treat
        // the applied observation as assignment too; this only widens the
        // bootstrap/fallback boundary and never claims historical images.
        let assigned_through = assigned_through.max(covered_through);
        Self {
            config,
            state: Mutex::new(WriteLogState {
                published: PersistentOrdMap::default(),
                pending: PersistentOrdMap::default(),
                published_documents: HashMap::new(),
                pending_documents: HashMap::new(),
                accounted_bytes: 0,
                covered_through,
                bootstrap_sequence: covered_through,
                assigned_through,
                assigned_high_water: assigned_through,
                published_through: covered_through,
                observed_applied_through: covered_through,
                purged_sequence: SequenceNumber(0),
                coverage_known: true,
                schema_epoch_history: HashMap::new(),
                published_schema_epochs: HashMap::new(),
            }),
        }
    }

    /// Adds assigned commits to the unpublished stage.
    ///
    /// Sequence assignment is a structural invariant, so regression is a hard
    /// assertion instead of a recoverable post-durability error.
    pub(crate) fn stage_pending(
        &self,
        commits: impl IntoIterator<Item = CommitEntry>,
        observed_at: Timestamp,
    ) {
        let entries = commits
            .into_iter()
            .map(|commit| Arc::new(WindowEntry::document_commit(commit, observed_at)))
            .collect::<Vec<_>>();
        self.stage_entries(entries);
    }

    /// Removes an assigned suffix after storage proves that append never
    /// advanced. Ambiguous exits keep the suffix and mark coverage unknown.
    pub(crate) fn discard_unpersisted_suffix(&self, first: SequenceNumber) {
        let mut state = self
            .state
            .lock()
            .expect("write-log lock should not be poisoned");
        let removed = state
            .pending
            .range(first..)
            .map(|(sequence, entry)| (*sequence, entry.clone()))
            .collect::<Vec<_>>();
        for (sequence, entry) in removed {
            state.pending.remove_mut(&sequence);
            state.accounted_bytes = state.accounted_bytes.saturating_sub(entry.accounted_bytes);
            if let WindowChange::WholeTables(tables) = &entry.change {
                for table in tables.iter() {
                    if let Some(history) = state.schema_epoch_history.get_mut(table) {
                        history.remove_mut(&sequence);
                    }
                }
            }
        }
        state.pending_documents.clear();
        let retained_pending = state.pending.values().cloned().collect::<Vec<_>>();
        for entry in retained_pending {
            let WindowChange::DocumentCommit(commit) = &entry.change else {
                continue;
            };
            for (write_index, write) in commit.writes.iter().enumerate() {
                state.pending_documents.insert(
                    (write.table.clone(), write.doc_id.clone()),
                    PublishedDocumentPointer {
                        sequence: entry.sequence,
                        entry: entry.clone(),
                        write_index,
                    },
                );
            }
        }
        state.assigned_through = state
            .pending
            .last()
            .map_or(state.published_through, |(sequence, _)| *sequence);
        state.covered_through = state.covered_through.min(state.assigned_through);
    }

    /// Stages a zero-write schema/table-lifecycle record as a dedicated
    /// whole-table marker. Inert zero-write records only advance coverage.
    pub(crate) fn stage_zero_write_record(
        &self,
        record: &TenantEventRecord,
        observed_at: Timestamp,
    ) {
        assert!(
            record.writes.is_empty(),
            "zero-write window staging requires an empty writes projection"
        );
        let Some(entry) = WindowEntry::whole_tables(record, observed_at) else {
            self.advance_known_zero_write_through(record.sequence);
            return;
        };
        self.stage_entries(vec![Arc::new(entry)]);
    }

    fn stage_entries(&self, entries: Vec<Arc<WindowEntry>>) {
        if entries.is_empty() {
            return;
        }

        let mut state = self
            .state
            .lock()
            .expect("write-log lock should not be poisoned");
        let mut previous = state.assigned_through;
        for entry in &entries {
            let expected = SequenceNumber(
                previous
                    .0
                    .checked_add(1)
                    .expect("write-log sequence must not overflow"),
            );
            assert_eq!(
                entry.sequence, expected,
                "write-log sequences must append without interior holes"
            );
            previous = entry.sequence;
        }

        for entry in entries {
            let sequence = entry.sequence;
            if let WindowChange::WholeTables(tables) = &entry.change {
                for table in tables.iter() {
                    state
                        .schema_epoch_history
                        .entry(table.clone())
                        .or_default()
                        .insert_mut(sequence, sequence);
                }
            }
            if let WindowChange::DocumentCommit(commit) = &entry.change {
                for (write_index, write) in commit.writes.iter().enumerate() {
                    state.pending_documents.insert(
                        (write.table.clone(), write.doc_id.clone()),
                        PublishedDocumentPointer {
                            sequence,
                            entry: entry.clone(),
                            write_index,
                        },
                    );
                }
            }
            state.accounted_bytes = state.accounted_bytes.saturating_add(entry.accounted_bytes);
            state.pending.insert_mut(sequence, entry);
            if sequence.0 == state.covered_through.0.saturating_add(1) {
                state.covered_through = sequence;
            }
            state.assigned_through = sequence;
            state.assigned_high_water = state.assigned_high_water.max(sequence);
        }
    }

    pub(crate) fn published_schema_epoch_snapshot(&self) -> HashMap<TableName, SequenceNumber> {
        self.state
            .lock()
            .expect("write-log lock should not be poisoned")
            .published_schema_epochs
            .clone()
    }

    pub(crate) fn current_schema_epoch(&self, table: &TableName) -> SequenceNumber {
        self.schema_epoch_at(table, SequenceNumber(u64::MAX))
    }

    pub(crate) fn schema_epoch_at(
        &self,
        table: &TableName,
        sequence: SequenceNumber,
    ) -> SequenceNumber {
        self.state
            .lock()
            .expect("write-log lock should not be poisoned")
            .schema_epoch_history
            .get(table)
            .and_then(|history| history.range(..=sequence).next_back())
            .map_or(SequenceNumber(0), |(_, epoch)| *epoch)
    }

    /// Re-bases a still-empty startup window after recovery/catch-up.
    ///
    /// No historical images are claimed: snapshots below the new bootstrap
    /// sequence still fall back to storage. This only prevents an
    /// assigned-before-applied startup gap from disabling all future live
    /// suffix coverage after recovery catches up.
    pub(crate) fn rebase_empty_after_recovery(
        &self,
        applied_through: SequenceNumber,
        assigned_through: SequenceNumber,
    ) {
        let mut state = self
            .state
            .lock()
            .expect("write-log lock should not be poisoned");
        if !state.published.is_empty()
            || !state.pending.is_empty()
            || state.purged_sequence != SequenceNumber(0)
            || state.covered_through >= applied_through
        {
            return;
        }
        let assigned_through = assigned_through.max(applied_through);
        state.bootstrap_sequence = applied_through;
        state.covered_through = applied_through;
        state.assigned_through = state.assigned_through.max(assigned_through);
        state.assigned_high_water = state.assigned_high_water.max(state.assigned_through);
        state.published_through = state.published_through.max(applied_through);
        state.observed_applied_through = state.observed_applied_through.max(applied_through);
    }

    /// Observes sequence assignment without claiming conflict-image coverage.
    ///
    /// Recovery can reveal an ambiguously committed document record that
    /// failed before the live append path staged its full image. A non-empty
    /// window must remember that hole so a later zero-write schema/cursor
    /// record cannot bridge over it. Empty startup windows may subsequently
    /// rebase at the applied head, raising their bootstrap boundary instead of
    /// claiming the missing history.
    pub(crate) fn observe_assigned_through_without_coverage(&self, sequence: SequenceNumber) {
        let mut state = self
            .state
            .lock()
            .expect("write-log lock should not be poisoned");
        // Progress may have been read before this caller waited for the
        // committer actor, so ignore a stale observation rather than regressing.
        state.assigned_through = state.assigned_through.max(sequence);
        state.assigned_high_water = state.assigned_high_water.max(sequence);
    }

    pub(crate) fn mark_coverage_unknown(&self) {
        self.state
            .lock()
            .expect("write-log lock should not be poisoned")
            .coverage_known = false;
    }

    /// Records a proven zero-write assignment span without allocating entries.
    ///
    /// This must not advance `published_through`: an earlier document record
    /// can still be staged in `pending` while a later zero-write record becomes
    /// durable and applied in the same storage catch-up. Publication remains
    /// owned by `publish_pending_through`, which publishes those pending images
    /// before crossing the applied zero-write suffix.
    pub(crate) fn advance_known_zero_write_through(&self, sequence: SequenceNumber) {
        let mut state = self
            .state
            .lock()
            .expect("write-log lock should not be poisoned");
        assert!(
            sequence >= state.assigned_through,
            "write-log zero-write coverage cannot regress sequence"
        );
        if sequence == state.assigned_through {
            return;
        }
        if state.covered_through == state.assigned_through {
            state.covered_through = sequence;
        }
        state.assigned_through = sequence;
        state.assigned_high_water = state.assigned_high_water.max(sequence);
    }

    /// Publishes entries that this caller has applied through `applied_head`.
    ///
    /// The returned sequence is the contiguous published frontier. It can be
    /// higher than `applied_head` when an earlier progress sync observed a
    /// later zero-write record: once this call removes the lower pending
    /// barrier, that already-observed suffix becomes publishable too.
    pub(crate) fn publish_pending_through(
        &self,
        applied_head: SequenceNumber,
        now: Timestamp,
        reader_frontier: SequenceNumber,
    ) -> SequenceNumber {
        let mut state = self
            .state
            .lock()
            .expect("write-log lock should not be poisoned");
        state.observed_applied_through = state.observed_applied_through.max(applied_head);
        let publish_sequences = state
            .pending
            .range((Included(SequenceNumber(0)), Included(applied_head)))
            .map(|(sequence, _)| *sequence)
            .collect::<Vec<_>>();
        let mut previous_published_entry = state
            .published
            .last()
            .map_or(state.published_through, |(sequence, _)| *sequence);
        for sequence in publish_sequences {
            assert!(
                sequence > previous_published_entry,
                "write-log publish order must follow assignment order: {} after {}",
                sequence,
                previous_published_entry
            );
            previous_published_entry = sequence;
            let entry = state
                .pending
                .get(&sequence)
                .cloned()
                .expect("selected pending write-log entry must still exist");
            assert!(
                state.pending.remove_mut(&sequence),
                "selected pending write-log entry must still exist"
            );
            if let WindowChange::WholeTables(tables) = &entry.change {
                for table in tables.iter() {
                    state
                        .published_schema_epochs
                        .insert(table.clone(), sequence);
                }
                state
                    .published_documents
                    .retain(|(table, _), _| !tables.contains(table));
            } else if let WindowChange::DocumentCommit(commit) = &entry.change {
                let indexed = commit
                    .writes
                    .iter()
                    .enumerate()
                    .map(|(write_index, write)| {
                        (
                            (write.table.clone(), write.doc_id.clone()),
                            PublishedDocumentPointer {
                                sequence,
                                entry: entry.clone(),
                                write_index,
                            },
                        )
                    })
                    .collect::<Vec<_>>();
                state.published_documents.extend(indexed);
                for write in &commit.writes {
                    let key = (write.table.clone(), write.doc_id.clone());
                    if state
                        .pending_documents
                        .get(&key)
                        .is_some_and(|pointer| pointer.sequence == sequence)
                    {
                        state.pending_documents.remove(&key);
                    }
                }
            }
            state.published.insert_mut(sequence, entry);
        }
        Self::advance_published_frontier_locked(&mut state);
        let published_through = state.published_through;
        self.trim_locked(&mut state, now, reader_frontier.min(published_through));
        published_through
    }

    /// Records a storage-side applied observation without publishing entries
    /// that remain owned by another apply path.
    ///
    /// A later zero-write transaction can advance the storage watermark past
    /// a lower pending document entry. The pending entry is a hard barrier:
    /// callers waiting to re-prepare must not observe the later sequence until
    /// its owning apply path explicitly publishes the lower image.
    pub(crate) fn observe_applied_through(
        &self,
        applied_head: SequenceNumber,
        now: Timestamp,
        reader_frontier: SequenceNumber,
    ) -> SequenceNumber {
        let mut state = self
            .state
            .lock()
            .expect("write-log lock should not be poisoned");
        state.observed_applied_through = state.observed_applied_through.max(applied_head);
        Self::advance_published_frontier_locked(&mut state);
        let published_through = state.published_through;
        self.trim_locked(&mut state, now, reader_frontier.min(published_through));
        published_through
    }

    fn advance_published_frontier_locked(state: &mut WriteLogState) {
        // Full-image coverage and journal progress are independent. Shared
        // providers can apply records this runtime never staged; those spans
        // force window validation to storage but must not stall applied-head
        // waiters. Only an entry still owned by a local apply path is a hard
        // publication barrier.
        let mut frontier = state.observed_applied_through;
        if let Some((first_pending, _)) = state.pending.first()
            && *first_pending <= frontier
        {
            frontier = SequenceNumber(first_pending.0.saturating_sub(1));
        }
        state.published_through = state.published_through.max(frontier);
    }

    pub(crate) fn published_through(&self) -> SequenceNumber {
        self.state
            .lock()
            .expect("write-log lock should not be poisoned")
            .published_through
    }

    pub(crate) fn frontier_sample(&self) -> WriteLogFrontierSample {
        let state = self
            .state
            .lock()
            .expect("write-log lock should not be poisoned");
        WriteLogFrontierSample {
            assigned_high_water: state.assigned_high_water,
            active_assigned_head: state.assigned_through,
            storage_applied_head: state.observed_applied_through,
            published_head: state.published_through,
        }
    }

    pub(crate) fn validation_source(
        &self,
        snapshot_sequence: SequenceNumber,
        head: SequenceNumber,
    ) -> Result<ValidationSource> {
        let state = self
            .state
            .lock()
            .expect("write-log lock should not be poisoned");
        if snapshot_sequence < state.purged_sequence {
            return Err(Error::out_of_retention(
                format!(
                    "transaction snapshot {} is older than the in-memory write-log retention horizon {}; retry from a fresh snapshot",
                    snapshot_sequence, state.purged_sequence
                ),
                Some(state.purged_sequence),
            ));
        }
        if !state.coverage_known {
            return Ok(ValidationSource::StorageFallback);
        }
        if snapshot_sequence < state.bootstrap_sequence
            || snapshot_sequence > state.covered_through
            || head > state.covered_through
        {
            return Ok(ValidationSource::StorageFallback);
        }
        Ok(ValidationSource::InMemory(WriteLogView {
            published: state.published.clone(),
            pending: state.pending.clone(),
            snapshot_sequence,
            head,
        }))
    }

    pub(crate) fn assigned_through(&self) -> SequenceNumber {
        self.state
            .lock()
            .expect("write-log lock should not be poisoned")
            .assigned_through
    }

    #[cfg(test)]
    pub(crate) fn pending_sequences_for_testing(&self) -> Vec<SequenceNumber> {
        self.state
            .lock()
            .expect("write-log lock should not be poisoned")
            .pending
            .keys()
            .copied()
            .collect()
    }

    /// True when `head` names a fully published in-memory prefix. Newer
    /// published or assigned-pending suffixes are intentionally allowed:
    /// caller prepare pins the applied prefix and actor validation folds them
    /// in.
    pub(crate) fn current_prepare_view_available(&self, head: SequenceNumber) -> bool {
        let state = self
            .state
            .lock()
            .expect("write-log lock should not be poisoned");
        state.coverage_known
            && head >= state.bootstrap_sequence
            && head <= state.covered_through
            && head <= state.published_through
    }

    /// Returns the latest retained published image for a document at the
    /// current applied head. `None` is deliberately ambiguous (not retained or
    /// not present), so update/delete callers fall back to storage.
    pub(crate) fn current_document_state(
        &self,
        head: SequenceNumber,
        table: &TableName,
        document_id: &DocumentId,
    ) -> Option<WindowDocumentState> {
        let state = self
            .state
            .lock()
            .expect("write-log lock should not be poisoned");
        if !state.coverage_known
            || head < state.bootstrap_sequence
            || head > state.covered_through
            || head > state.published_through
        {
            return None;
        }
        let current_pointer = state
            .published_documents
            .get(&(table.clone(), document_id.clone()));
        let historical_pointer;
        let pointer = if let Some(pointer) =
            current_pointer.filter(|pointer| pointer.sequence <= head)
        {
            pointer
        } else {
            historical_pointer = state
                .published
                .range((Included(SequenceNumber(0)), Included(head)))
                .rev()
                .find_map(|(sequence, entry)| {
                    let WindowChange::DocumentCommit(commit) = &entry.change else {
                        return None;
                    };
                    commit
                        .writes
                        .iter()
                        .enumerate()
                        .rev()
                        .find(|(_, write)| write.table == *table && write.doc_id == *document_id)
                        .map(|(write_index, _)| PublishedDocumentPointer {
                            sequence: *sequence,
                            entry: entry.clone(),
                            write_index,
                        })
                })?;
            &historical_pointer
        };
        let WindowChange::DocumentCommit(commit) = &pointer.entry.change else {
            return None;
        };
        let write = commit.writes.get(pointer.write_index)?;
        Some(WindowDocumentState {
            sequence: pointer.sequence,
            table_id: write.table_id.clone(),
            document: write.current.clone(),
            resource_path_binding: write.resource_path_binding.clone(),
        })
    }

    /// Actor-local O(1) validation and latest-image lookup for one document.
    /// The result covers both published and assigned-pending full images.
    pub(crate) fn single_document_change_since(
        &self,
        snapshot_sequence: SequenceNumber,
        table: &TableName,
        document_id: &DocumentId,
    ) -> Result<Option<SingleDocumentWindowChange>> {
        let state = self
            .state
            .lock()
            .expect("write-log lock should not be poisoned");
        if snapshot_sequence < state.purged_sequence {
            return Err(Error::out_of_retention(
                format!(
                    "transaction snapshot {} is older than the in-memory write-log retention horizon {}; retry from a fresh snapshot",
                    snapshot_sequence, state.purged_sequence
                ),
                Some(state.purged_sequence),
            ));
        }
        if !state.coverage_known
            || snapshot_sequence < state.bootstrap_sequence
            || snapshot_sequence > state.covered_through
        {
            return Ok(None);
        }
        if let Some(sequence) = state
            .schema_epoch_history
            .get(table)
            .and_then(|history| {
                history
                    .range((
                        Excluded(snapshot_sequence),
                        Included(state.assigned_through),
                    ))
                    .next()
            })
            .map(|(sequence, _)| *sequence)
        {
            return Ok(Some(SingleDocumentWindowChange::WholeTable { sequence }));
        }
        let key = (table.clone(), document_id.clone());
        let pointer = state
            .pending_documents
            .get(&key)
            .filter(|pointer| pointer.sequence > snapshot_sequence)
            .or_else(|| {
                state
                    .published_documents
                    .get(&key)
                    .filter(|pointer| pointer.sequence > snapshot_sequence)
            });
        let Some(pointer) = pointer else {
            return Ok(Some(SingleDocumentWindowChange::Unchanged));
        };
        let WindowChange::DocumentCommit(commit) = &pointer.entry.change else {
            return Ok(None);
        };
        let Some(write) = commit.writes.get(pointer.write_index) else {
            return Ok(None);
        };
        Ok(Some(SingleDocumentWindowChange::Changed {
            latest: Box::new(WindowDocumentState {
                sequence: pointer.sequence,
                table_id: write.table_id.clone(),
                document: write.current.clone(),
                resource_path_binding: write.resource_path_binding.clone(),
            }),
        }))
    }

    fn trim_locked(
        &self,
        state: &mut WriteLogState,
        now: Timestamp,
        reader_frontier: SequenceNumber,
    ) {
        loop {
            let Some((sequence, entry)) = state.published.first().map(|(k, v)| (*k, v.clone()))
            else {
                break;
            };
            if sequence > state.published_through {
                break;
            }
            let age = now.saturating_duration_since(entry.observed_at);
            if age < Duration::from_millis(self.config.min_retention_ms) {
                break;
            }
            let reader_has_advanced = sequence <= reader_frontier;
            let exceeded_max_retention = age >= Duration::from_millis(self.config.max_retention_ms);
            let exceeded_byte_budget = state.accounted_bytes > self.config.soft_max_bytes;
            if !(reader_has_advanced || exceeded_max_retention || exceeded_byte_budget) {
                break;
            }

            let removed = state
                .published
                .get(&sequence)
                .cloned()
                .expect("oldest published write-log entry must still exist");
            assert!(
                state.published.remove_mut(&sequence),
                "oldest published write-log entry must still exist"
            );
            state.accounted_bytes = state
                .accounted_bytes
                .saturating_sub(removed.accounted_bytes);
            if let WindowChange::DocumentCommit(commit) = &removed.change {
                for write in &commit.writes {
                    let key = (write.table.clone(), write.doc_id.clone());
                    if state
                        .published_documents
                        .get(&key)
                        .is_some_and(|pointer| pointer.sequence == sequence)
                    {
                        state.published_documents.remove(&key);
                    }
                }
            }
            state.purged_sequence = state.purged_sequence.max(sequence);
        }
    }

    #[cfg(test)]
    fn inspection(&self) -> WriteLogInspection {
        let state = self
            .state
            .lock()
            .expect("write-log lock should not be poisoned");
        WriteLogInspection {
            published: state.published.keys().copied().collect(),
            pending: state.pending.keys().copied().collect(),
            accounted_bytes: state.accounted_bytes,
            purged_sequence: state.purged_sequence,
        }
    }
}

pub(crate) enum ValidationSource {
    InMemory(WriteLogView),
    StorageFallback,
}

pub(crate) struct WriteLogView {
    published: PersistentOrdMap<SequenceNumber, Arc<WindowEntry>>,
    pending: PersistentOrdMap<SequenceNumber, Arc<WindowEntry>>,
    snapshot_sequence: SequenceNumber,
    head: SequenceNumber,
}

#[derive(Debug, Clone)]
pub(crate) struct WindowDocumentState {
    pub(crate) sequence: SequenceNumber,
    pub(crate) table_id: TableId,
    pub(crate) document: Option<Document>,
    pub(crate) resource_path_binding: Option<ResourcePathBinding>,
}

pub(crate) enum SingleDocumentWindowChange {
    Unchanged,
    Changed { latest: Box<WindowDocumentState> },
    WholeTable { sequence: SequenceNumber },
}

impl WriteLogView {
    fn entries(&self) -> Vec<Arc<WindowEntry>> {
        let pending_head = self
            .pending
            .last()
            .map_or(self.head, |(sequence, _)| self.head.max(*sequence));
        let mut entries = self
            .published
            .range((Excluded(self.snapshot_sequence), Included(self.head)))
            .map(|(_, entry)| entry.clone())
            .chain(
                self.pending
                    .range((Excluded(self.snapshot_sequence), Included(pending_head)))
                    .map(|(_, entry)| entry.clone()),
            )
            .collect::<Vec<_>>();
        entries.sort_unstable_by_key(|entry| entry.sequence);
        entries
    }

    pub(crate) fn first_conflicting_sequence<F>(
        &self,
        dependencies: &DependencySet,
        mut resolve_document: F,
    ) -> Option<SequenceNumber>
    where
        F: FnMut(&TableName, DocumentId) -> Result<Option<Document>>,
    {
        self.entries().into_iter().find_map(|entry| {
            let intersects = match &entry.change {
                WindowChange::DocumentCommit(commit) => commit_intersects_dependency_set(
                    commit,
                    dependencies,
                    &[],
                    |table, document_id| resolve_document(table, document_id),
                ),
                WindowChange::WholeTables(tables) => {
                    tables.iter().any(|table| dependencies.touches_table(table))
                }
            };
            intersects.then_some(entry.sequence)
        })
    }
}

#[cfg(test)]
struct WriteLogInspection {
    published: Vec<SequenceNumber>,
    pending: Vec<SequenceNumber>,
    accounted_bytes: usize,
    purged_sequence: SequenceNumber,
}

#[cfg(test)]
mod tests;
