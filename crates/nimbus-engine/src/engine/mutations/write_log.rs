use std::ops::Bound::{Excluded, Included};
use std::sync::{Arc, Mutex};

use imbl::OrdMap;
use nimbus_core::{
    CommitEntry, DependencySet, Document, DocumentId, Error, Result, SequenceNumber, TableName,
    Timestamp, commit_intersects_dependency_set,
};

const DEFAULT_MIN_RETENTION_SECS: usize = 30;
const DEFAULT_MAX_RETENTION_SECS: usize = 300;
const DEFAULT_SOFT_MAX_BYTES: usize = 32 * 1024 * 1024;

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
        let min_retention_secs = env_positive_usize(
            "NIMBUS_WRITE_LOG_MIN_RETENTION_SECS",
            DEFAULT_MIN_RETENTION_SECS,
        );
        let max_retention_secs = env_positive_usize(
            "NIMBUS_WRITE_LOG_MAX_RETENTION_SECS",
            DEFAULT_MAX_RETENTION_SECS,
        )
        .max(min_retention_secs);
        Self::for_tests(
            min_retention_secs,
            max_retention_secs,
            env_positive_usize("NIMBUS_WRITE_LOG_SOFT_MAX_BYTES", DEFAULT_SOFT_MAX_BYTES),
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

fn env_positive_usize(key: &str, default: usize) -> usize {
    std::env::var_os(key)
        .and_then(|value| value.into_string().ok())
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

#[derive(Debug, Clone)]
struct WindowEntry {
    commit: Arc<CommitEntry>,
    observed_at: Timestamp,
    accounted_bytes: usize,
}

impl WindowEntry {
    fn new(commit: CommitEntry, observed_at: Timestamp) -> Self {
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
            commit: Arc::new(commit),
            observed_at,
            accounted_bytes,
        }
    }
}

#[derive(Debug)]
struct WriteLogState {
    published: OrdMap<SequenceNumber, Arc<WindowEntry>>,
    pending: OrdMap<SequenceNumber, Arc<WindowEntry>>,
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
    /// Highest sequence removed from the front of the retained window.
    purged_sequence: SequenceNumber,
}

/// Per-tenant, structurally shared mirror of recent full commit images.
///
/// Mutation publication takes the mutex only to replace persistent-map roots.
/// Validation clones those roots in O(1), releases the mutex, then scans its
/// stable view. The pending map is intentionally present before publish is
/// pipelined: today's serial sequence gate normally makes it empty whenever a
/// validator runs, while PPSC5 will consume the same API directly.
pub(crate) struct WriteLog {
    config: WriteLogConfig,
    state: Mutex<WriteLogState>,
}

impl WriteLog {
    pub(crate) fn new(
        config: WriteLogConfig,
        covered_through: SequenceNumber,
        assigned_through: SequenceNumber,
    ) -> Self {
        assert!(
            covered_through <= assigned_through,
            "write-log applied coverage cannot exceed assigned sequence"
        );
        Self {
            config,
            state: Mutex::new(WriteLogState {
                published: OrdMap::new(),
                pending: OrdMap::new(),
                accounted_bytes: 0,
                covered_through,
                bootstrap_sequence: covered_through,
                assigned_through,
                purged_sequence: SequenceNumber(0),
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
            .map(|commit| Arc::new(WindowEntry::new(commit, observed_at)))
            .collect::<Vec<_>>();
        if entries.is_empty() {
            return;
        }

        let mut state = self
            .state
            .lock()
            .expect("write-log lock should not be poisoned");
        let mut previous = state.assigned_through;
        for entry in &entries {
            assert!(
                entry.commit.sequence > previous,
                "write-log sequences must append strictly monotonically: {} after {}",
                entry.commit.sequence,
                previous
            );
            previous = entry.commit.sequence;
        }

        for entry in entries {
            let sequence = entry.commit.sequence;
            state.accounted_bytes = state.accounted_bytes.saturating_add(entry.accounted_bytes);
            state.pending.insert(sequence, entry);
            if sequence.0 == state.covered_through.0.saturating_add(1) {
                state.covered_through = sequence;
            }
            state.assigned_through = sequence;
        }
    }

    /// Records a proven zero-write sequence span without allocating entries.
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
    }

    /// Publishes pending entries through `applied_head`, then applies retention.
    pub(crate) fn publish_pending_through(
        &self,
        applied_head: SequenceNumber,
        now: Timestamp,
        reader_frontier: SequenceNumber,
    ) {
        let mut state = self
            .state
            .lock()
            .expect("write-log lock should not be poisoned");
        let publish_sequences = state
            .pending
            .range((Included(SequenceNumber(0)), Included(applied_head)))
            .map(|(sequence, _)| *sequence)
            .collect::<Vec<_>>();
        for sequence in publish_sequences {
            let entry = state
                .pending
                .remove(&sequence)
                .expect("selected pending write-log entry must still exist");
            state.published.insert(sequence, entry);
        }
        self.trim_locked(&mut state, now, reader_frontier.min(applied_head));
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
            return Err(Error::retryable_conflict(
                format!(
                    "transaction snapshot {} is older than the in-memory write-log retention horizon {}; retry from a fresh snapshot",
                    snapshot_sequence, state.purged_sequence
                ),
                Some(state.purged_sequence),
            ));
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

    fn trim_locked(
        &self,
        state: &mut WriteLogState,
        now: Timestamp,
        reader_frontier: SequenceNumber,
    ) {
        loop {
            let Some((sequence, entry)) = state.published.get_min().map(|(k, v)| (*k, v.clone()))
            else {
                break;
            };
            let age_ms = now.0.saturating_sub(entry.observed_at.0);
            if age_ms < self.config.min_retention_ms {
                break;
            }
            let reader_has_advanced = sequence <= reader_frontier;
            let exceeded_max_retention = age_ms >= self.config.max_retention_ms;
            let exceeded_byte_budget = state.accounted_bytes > self.config.soft_max_bytes;
            if !(reader_has_advanced || exceeded_max_retention || exceeded_byte_budget) {
                break;
            }

            let removed = state
                .published
                .remove(&sequence)
                .expect("oldest published write-log entry must still exist");
            state.accounted_bytes = state
                .accounted_bytes
                .saturating_sub(removed.accounted_bytes);
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
    published: OrdMap<SequenceNumber, Arc<WindowEntry>>,
    pending: OrdMap<SequenceNumber, Arc<WindowEntry>>,
    snapshot_sequence: SequenceNumber,
    head: SequenceNumber,
}

impl WriteLogView {
    fn commits(&self) -> Vec<Arc<CommitEntry>> {
        let pending_head = self
            .pending
            .get_max()
            .map_or(self.head, |(sequence, _)| self.head.max(*sequence));
        let mut commits = self
            .published
            .range((Excluded(self.snapshot_sequence), Included(self.head)))
            .map(|(_, entry)| entry.commit.clone())
            .chain(
                self.pending
                    .range((Excluded(self.snapshot_sequence), Included(pending_head)))
                    .map(|(_, entry)| entry.commit.clone()),
            )
            .collect::<Vec<_>>();
        commits.sort_unstable_by_key(|commit| commit.sequence);
        commits
    }

    pub(crate) fn first_conflicting_sequence<F>(
        &self,
        dependencies: &DependencySet,
        mut resolve_document: F,
    ) -> Option<SequenceNumber>
    where
        F: FnMut(&TableName, DocumentId) -> Result<Option<Document>>,
    {
        self.commits().into_iter().find_map(|commit| {
            commit_intersects_dependency_set(&commit, dependencies, &[], |table, document_id| {
                resolve_document(table, document_id)
            })
            .then_some(commit.sequence)
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
mod tests {
    use std::sync::Arc;

    use nimbus_core::{
        Filter, FilterOp, IndexId, IndexRangeDependency, PaginatedWindowDependency,
        PredicateDependency, TableId, WriteOp, WriteOpType,
    };
    use nimbus_storage::{ManualClock, MemoryTenantStore, NoopFaultInjector};
    use rand::rngs::StdRng;
    use rand::{Rng, SeedableRng};
    use serde_json::json;

    use super::*;

    fn commit(sequence: u64, body_bytes: usize) -> CommitEntry {
        let table = TableName::new("messages").expect("table name should be valid");
        let id =
            DocumentId::from_key(format!("doc-{sequence}")).expect("document id should be valid");
        let document = Document {
            id: id.clone(),
            table: table.clone(),
            creation_time: Timestamp(sequence),
            update_time: Timestamp(sequence),
            fields: serde_json::Map::from_iter([(
                "body".to_string(),
                json!("x".repeat(body_bytes)),
            )]),
            typed_fields: Default::default(),
        };
        CommitEntry {
            sequence: SequenceNumber(sequence),
            timestamp: Timestamp(sequence),
            writes: vec![WriteOp {
                table,
                table_id: TableId::new(),
                op_type: WriteOpType::Insert,
                doc_id: id,
                resource_path_binding: None,
                trigger_write_origin: None,
                previous: None,
                current: Some(document),
            }],
        }
    }

    fn test_log(config: WriteLogConfig) -> WriteLog {
        WriteLog::new(config, SequenceNumber(0), SequenceNumber(0))
    }

    #[test]
    fn pending_stage_entries_staged_to_published_lifecycle() {
        let log = test_log(WriteLogConfig::for_tests(30, 300, usize::MAX));
        log.stage_pending([commit(1, 8)], Timestamp(1_000));
        let staged = log.inspection();
        assert_eq!(staged.pending, vec![SequenceNumber(1)]);
        assert!(staged.published.is_empty());

        log.publish_pending_through(SequenceNumber(1), Timestamp(1_000), SequenceNumber(0));
        let published = log.inspection();
        assert!(published.pending.is_empty());
        assert_eq!(published.published, vec![SequenceNumber(1)]);
    }

    #[test]
    fn zero_write_sequence_advances_coverage_without_allocating_an_entry() {
        let log = test_log(WriteLogConfig::for_tests(30, 300, usize::MAX));
        log.advance_known_zero_write_through(SequenceNumber(1));

        let inspection = log.inspection();
        assert!(inspection.pending.is_empty());
        assert!(inspection.published.is_empty());
        assert!(matches!(
            log.validation_source(SequenceNumber(0), SequenceNumber(1)),
            Ok(ValidationSource::InMemory(_))
        ));
    }

    #[test]
    fn pending_stage_validation_sees_pending_entries() {
        let log = test_log(WriteLogConfig::for_tests(30, 300, usize::MAX));
        let pending = commit(1, 8);
        let table = pending.writes[0].table.clone();
        let table_id = pending.writes[0].table_id.clone();
        log.stage_pending([pending], Timestamp(1_000));
        let mut dependencies = DependencySet::default();
        dependencies.record_table(&table, &table_id);

        let ValidationSource::InMemory(view) = log
            .validation_source(SequenceNumber(0), SequenceNumber(1))
            .expect("pending window should cover the validation range")
        else {
            panic!("covered pending entry should not require storage fallback");
        };
        assert_eq!(
            view.first_conflicting_sequence(&dependencies, |_, _| Ok(None)),
            Some(SequenceNumber(1))
        );
    }

    #[test]
    fn out_of_retention_validation_fails_closed() {
        let log = test_log(WriteLogConfig::for_tests(1, 2, usize::MAX));
        log.stage_pending([commit(1, 8), commit(2, 8)], Timestamp(0));
        log.publish_pending_through(SequenceNumber(2), Timestamp(3_000), SequenceNumber(2));
        let inspection = log.inspection();
        assert_eq!(inspection.purged_sequence, SequenceNumber(2));

        let error = log
            .validation_source(SequenceNumber(1), SequenceNumber(2))
            .err()
            .expect("snapshot older than the purge horizon must fail closed");
        assert!(matches!(
            error,
            Error::Conflict {
                ref message,
                conflicting_sequence: Some(SequenceNumber(2)),
                retryable: true,
                ..
            } if message.contains("retention horizon")
        ));
    }

    #[test]
    fn stalled_reader_size_cap_trim_respects_min_retention_floor() {
        let probe = WindowEntry::new(commit(1, 1_024), Timestamp(0)).accounted_bytes;
        let log = test_log(WriteLogConfig::for_tests(30, 300, probe + 1));
        log.stage_pending([commit(1, 1_024), commit(2, 1_024)], Timestamp(0));
        log.publish_pending_through(SequenceNumber(2), Timestamp(29_999), SequenceNumber(0));
        let before_min = log.inspection();
        assert_eq!(before_min.published.len(), 2);
        assert!(before_min.accounted_bytes > probe + 1);

        log.publish_pending_through(SequenceNumber(2), Timestamp(30_000), SequenceNumber(0));
        let after_min = log.inspection();
        assert!(after_min.accounted_bytes <= probe + 1);
        assert_eq!(after_min.purged_sequence, SequenceNumber(1));
        assert_eq!(after_min.published, vec![SequenceNumber(2)]);
    }

    #[test]
    #[should_panic(expected = "write-log sequences must append strictly monotonically")]
    fn window_append_asserts_sequence_monotonicity() {
        let log = test_log(WriteLogConfig::for_tests(30, 300, usize::MAX));
        log.stage_pending([commit(2, 8)], Timestamp(0));
        log.stage_pending([commit(1, 8)], Timestamp(0));
    }

    #[test]
    fn bootstrap_fallback_uses_storage_scan() {
        let store = MemoryTenantStore::with_simulation(
            Arc::new(ManualClock::new(Timestamp(1_000))),
            Arc::new(NoopFaultInjector),
        );
        let document = document(1, "active", 1);
        let commit = store.insert(&document).expect("seed insert should commit");
        let log = WriteLog::new(
            WriteLogConfig::for_tests(30, 300, usize::MAX),
            commit.sequence,
            commit.sequence,
        );
        let mut dependencies = DependencySet::default();
        dependencies.record_table(&commit.writes[0].table, &commit.writes[0].table_id);

        assert!(matches!(
            log.validation_source(SequenceNumber(0), commit.sequence)
                .expect("bootstrap coverage decision should succeed"),
            ValidationSource::StorageFallback
        ));
        let storage_conflict = store
            .read_commit_log_from(SequenceNumber(1))
            .expect("bootstrap fallback scan should read memory persistence")
            .into_iter()
            .any(|entry| {
                commit_intersects_dependency_set(&entry, &dependencies, &[], |table, id| {
                    store.get(table, &id)
                })
            });
        assert!(
            storage_conflict,
            "the mandatory bootstrap fallback must preserve the storage-scan abort decision"
        );
    }

    #[test]
    fn window_vs_storage_scan_differential() {
        const HISTORIES: usize = 20;
        const CASES_PER_HISTORY: usize = 25;
        const CORPUS_SIZE: usize = HISTORIES * CASES_PER_HISTORY;

        let mut rng = StdRng::seed_from_u64(0x5050_5343_3257_4c47);
        let mut checked = 0;
        for history_index in 0..HISTORIES {
            let store = MemoryTenantStore::with_simulation(
                Arc::new(ManualClock::new(Timestamp(
                    10_000 + u64::try_from(history_index).expect("history index fits u64"),
                ))),
                Arc::new(NoopFaultInjector),
            );
            let log = test_log(WriteLogConfig::for_tests(30, 300, usize::MAX));
            let mut active = Vec::<Document>::new();
            let mut history = Vec::<CommitEntry>::new();

            for operation_index in 0..CASES_PER_HISTORY {
                let choose_insert = active.is_empty() || rng.gen_bool(0.45);
                let commit = if choose_insert {
                    let document = document(
                        history_index * 1_000 + operation_index,
                        if rng.gen_bool(0.5) {
                            "active"
                        } else {
                            "archived"
                        },
                        rng.gen_range(0..100),
                    );
                    let commit = store
                        .insert(&document)
                        .expect("generated insert should commit");
                    active.push(document);
                    commit
                } else {
                    let target = rng.gen_range(0..active.len());
                    if active.len() > 1 && rng.gen_bool(0.25) {
                        let document = active.swap_remove(target);
                        store
                            .delete_validated_returning_document(
                                &document.table,
                                &document.id,
                                |_| Ok(()),
                            )
                            .expect("generated delete should commit")
                            .0
                    } else {
                        let document = &mut active[target];
                        let status = if rng.gen_bool(0.5) {
                            "active"
                        } else {
                            "archived"
                        };
                        let rank = rng.gen_range(0..100);
                        let patch = serde_json::Map::from_iter([
                            ("status".to_string(), json!(status)),
                            ("rank".to_string(), json!(rank)),
                        ]);
                        let commit = store
                            .update_validated(&document.table, &document.id, &patch, |_, _| Ok(()))
                            .expect("generated update should commit");
                        document.fields.extend(patch);
                        document.update_time = commit.timestamp;
                        commit
                    }
                };
                log.stage_pending([commit.clone()], Timestamp(1_000));
                log.publish_pending_through(commit.sequence, Timestamp(1_000), SequenceNumber(0));
                history.push(commit);
            }

            let head = history
                .last()
                .expect("generated history should be non-empty")
                .sequence;
            for _ in 0..CASES_PER_HISTORY {
                let snapshot = SequenceNumber(rng.gen_range(0..head.0));
                let target = &history[rng.gen_range(0..history.len())];
                let dependencies = generated_dependencies(&mut rng, target);
                let storage_conflict = store
                    .read_commit_log_from(SequenceNumber(snapshot.0.saturating_add(1)))
                    .expect("differential storage scan should read")
                    .into_iter()
                    .find_map(|entry| {
                        commit_intersects_dependency_set(&entry, &dependencies, &[], |table, id| {
                            store.get(table, &id)
                        })
                        .then_some(entry.sequence)
                    });
                let ValidationSource::InMemory(view) = log
                    .validation_source(snapshot, head)
                    .expect("generated in-memory validation should be retained")
                else {
                    panic!("fully populated generated history should not fall back");
                };
                let window_conflict = view
                    .first_conflicting_sequence(&dependencies, |table, id| store.get(table, &id));
                assert_eq!(
                    window_conflict.is_some(),
                    storage_conflict.is_some(),
                    "differential case {checked} disagreed at snapshot {snapshot}"
                );
                checked += 1;
            }
        }
        assert_eq!(checked, CORPUS_SIZE);
    }

    fn document(slot: usize, status: &str, rank: u64) -> Document {
        let table = TableName::new("differential_messages").expect("table name should be valid");
        Document {
            id: DocumentId::from_key(format!("generated-{slot}"))
                .expect("generated document id should be valid"),
            table,
            creation_time: Timestamp(u64::try_from(slot).unwrap_or(u64::MAX)),
            update_time: Timestamp(u64::try_from(slot).unwrap_or(u64::MAX)),
            fields: serde_json::Map::from_iter([
                ("status".to_string(), json!(status)),
                ("rank".to_string(), json!(rank)),
            ]),
            typed_fields: Default::default(),
        }
    }

    fn generated_dependencies(rng: &mut StdRng, commit: &CommitEntry) -> DependencySet {
        let write = &commit.writes[0];
        let mut dependencies = DependencySet::default();
        match rng.gen_range(0..8) {
            0 => dependencies.record_table(&write.table, &write.table_id),
            1 => dependencies.record_document(&write.table, &write.table_id, write.doc_id.clone()),
            2 => dependencies.record_missing_table(&write.table),
            3 => dependencies.record_predicate(PredicateDependency {
                table: write.table.clone(),
                table_id: write.table_id.clone(),
                filters: vec![Filter {
                    field: "status".to_string(),
                    op: FilterOp::Eq,
                    value: json!(if rng.gen_bool(0.5) {
                        "active"
                    } else {
                        "archived"
                    }),
                }],
            }),
            4 => dependencies.record_paginated_window(PaginatedWindowDependency {
                table: write.table.clone(),
                table_id: write.table_id.clone(),
                filters: Vec::new(),
                order: None,
                start_sort_values: Vec::new(),
                start_doc_id: None,
                end_sort_values: Vec::new(),
                end_doc_id: None,
                result_count: rng.gen_range(0..4),
                page_size: 4,
            }),
            5 => dependencies.record_index_range(IndexRangeDependency {
                table: write.table.clone(),
                table_id: write.table_id.clone(),
                index_id: IndexId::new(),
                index_name: "by_rank".to_string(),
                field: "rank".to_string(),
                start: Some(json!(rng.gen_range(0..50))),
                end: Some(json!(rng.gen_range(50..100))),
                start_inclusive: rng.gen_bool(0.5),
                end_inclusive: rng.gen_bool(0.5),
            }),
            6 => dependencies.record_document(
                &write.table,
                &write.table_id,
                DocumentId::from_key(format!("absent-{}", rng.r#gen::<u64>()))
                    .expect("absent id should be valid"),
            ),
            _ => dependencies.record_table(&write.table, &TableId::new()),
        }
        dependencies
    }
}
