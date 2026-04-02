use std::num::NonZeroU64;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex};

use neovex_core::{
    CronJob, CronSchedule, DependencySet, Document, DocumentId, DurableMutationRecord, Error,
    FieldSchema, FieldType, IndexDefinition, IndexRangeDependency, Mutation, ScheduledJob,
    ScheduledJobOutcome, ScheduledJobResult, SequenceNumber, TableName, TableSchema, Timestamp,
    WriteOp, WriteOpType, durable_record_intersects_dependency_set,
};
use serde_json::json;
use tempfile::tempdir;
use time::{Date, Month, PrimitiveDateTime, Time};
use tokio::sync::Notify;
use tokio::time::{Duration, timeout};

use crate::index::encode_index_value;
use crate::keys::{document_key, prefix_end, table_prefix};
use crate::{
    FaultInjector, FaultOccurrence, FaultPoint, ManualClock, RedbTenantStorage,
    SeededFaultInjector, ShadowMaterializer, ShadowMaterializerConfig, ShadowMaterializerManifest,
    TenantReadStorage, TenantStore, TenantWriteOutcome, TenantWriteStorage, UsageStore,
};

fn sample_document(table: &str, title: &str) -> Document {
    Document::new(
        TableName::new(table).expect("table name should be valid"),
        serde_json::Map::from_iter([("title".to_string(), json!(title))]),
    )
}

fn scheduled_insert_job(run_at: Timestamp, title: &str) -> ScheduledJob {
    ScheduledJob {
        id: DocumentId::new(),
        run_at,
        mutation: Mutation::Insert {
            table: TableName::new("tasks").expect("table name should be valid"),
            fields: serde_json::Map::from_iter([("title".to_string(), json!(title))]),
        },
        created_at: Timestamp(1_000),
    }
}

fn next_seeded_u64(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
    *seed
}

struct BlockingReadGate {
    entered: Notify,
    release_gate: (Mutex<bool>, Condvar),
}

struct BlockingFaultInjector {
    point: FaultPoint,
    entered: Notify,
    release_gate: (Mutex<bool>, Condvar),
}

impl BlockingFaultInjector {
    fn new(point: FaultPoint) -> Arc<Self> {
        Arc::new(Self {
            point,
            entered: Notify::new(),
            release_gate: (Mutex::new(false), Condvar::new()),
        })
    }

    async fn wait_until_entered(&self) {
        self.entered.notified().await;
    }

    fn release(&self) {
        let (lock, cvar) = &self.release_gate;
        let mut released = lock
            .lock()
            .expect("blocking fault injector should acquire release lock");
        *released = true;
        cvar.notify_all();
    }
}

impl FaultInjector for BlockingFaultInjector {
    fn check(&self, point: FaultPoint) -> neovex_core::Result<()> {
        if point != self.point {
            return Ok(());
        }
        self.entered.notify_one();
        let (lock, cvar) = &self.release_gate;
        let mut released = lock
            .lock()
            .expect("blocking fault injector should acquire release lock");
        while !*released {
            released = cvar
                .wait(released)
                .expect("blocking fault injector should wait for release");
        }
        Ok(())
    }
}

impl BlockingReadGate {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            entered: Notify::new(),
            release_gate: (Mutex::new(false), Condvar::new()),
        })
    }

    async fn wait_until_entered(&self) {
        self.entered.notified().await;
    }

    fn block(&self) {
        self.entered.notify_one();
        let (lock, cvar) = &self.release_gate;
        let mut released = lock
            .lock()
            .expect("blocking read gate should acquire release lock");
        while !*released {
            released = cvar
                .wait(released)
                .expect("blocking read gate should wait for release");
        }
    }

    fn release(&self) {
        let (lock, cvar) = &self.release_gate;
        let mut released = lock
            .lock()
            .expect("blocking read gate should acquire release lock");
        *released = true;
        cvar.notify_all();
    }
}

#[test]
fn key_helpers_create_prefix_scannable_ranges() {
    let table = TableName::new("tasks").expect("table name should be valid");
    let id = DocumentId::new();
    let key = document_key(&table, &id);
    let prefix = table_prefix(&table);
    let end = prefix_end(&prefix).expect("prefix end should exist");

    assert!(key.starts_with(&prefix));
    assert!(key.as_slice() < end.as_slice());
}

#[test]
fn insert_then_get_roundtrip() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let document = sample_document("tasks", "Hello");

    let commit = store.insert(&document).expect("insert should succeed");
    let fetched = store
        .get(&document.table, &document.id)
        .expect("get should succeed")
        .expect("document should exist");

    assert_eq!(commit.sequence, SequenceNumber(1));
    assert_eq!(fetched.fields.get("title"), Some(&json!("Hello")));
}

#[test]
fn seeded_fault_injector_reproduces_the_same_schedule_for_the_same_seed() {
    let left = SeededFaultInjector::new(7, NonZeroU64::new(3).expect("period should be non-zero"));
    let right = SeededFaultInjector::new(7, NonZeroU64::new(3).expect("period should be non-zero"));

    let left_results = [
        FaultPoint::StorageCommitBeforeVisibility,
        FaultPoint::JournalAppendBeforeDurableFlush,
        FaultPoint::StorageCommitBeforeVisibility,
        FaultPoint::CheckpointPublishBeforeManifestUpdate,
        FaultPoint::StorageCommitBeforeVisibility,
        FaultPoint::CompactionStartBeforePublish,
    ]
    .into_iter()
    .map(|point| left.check(point).is_err())
    .collect::<Vec<_>>();
    let right_results = [
        FaultPoint::StorageCommitBeforeVisibility,
        FaultPoint::JournalAppendBeforeDurableFlush,
        FaultPoint::StorageCommitBeforeVisibility,
        FaultPoint::CheckpointPublishBeforeManifestUpdate,
        FaultPoint::StorageCommitBeforeVisibility,
        FaultPoint::CompactionStartBeforePublish,
    ]
    .into_iter()
    .map(|point| right.check(point).is_err())
    .collect::<Vec<_>>();

    assert_eq!(left_results, right_results);
}

#[test]
fn injected_fault_before_visibility_rolls_back_the_write_deterministically() {
    let clock = Arc::new(ManualClock::new(Timestamp(10_000)));
    let faults = Arc::new(crate::ScriptedFaultInjector::new([FaultOccurrence {
        point: FaultPoint::StorageCommitBeforeVisibility,
        visit: 1,
    }]));
    let store = TenantStore::create_in_memory_with_simulation(clock, faults)
        .expect("store should open with simulation seams");
    let document = sample_document("tasks", "Hello");

    let error = store
        .insert(&document)
        .expect_err("first insert should fail before visibility");
    assert!(
        matches!(error, Error::Internal(message) if message.contains("storage_commit_before_visibility"))
    );
    assert!(
        store
            .get(&document.table, &document.id)
            .expect("get should succeed after injected failure")
            .is_none()
    );
    assert_eq!(
        store
            .latest_sequence()
            .expect("latest sequence should remain unchanged"),
        SequenceNumber(0)
    );

    let commit = store
        .insert(&document)
        .expect("second insert should commit");
    assert_eq!(commit.timestamp, Timestamp(10_000));
}

#[test]
fn scheduled_execution_marker_deduplicates_insert_commit() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let document = sample_document("tasks", "Hello once");

    let first = store
        .insert_once(&document, Some("scheduled:test-job"))
        .expect("first insert should succeed");
    let second = store
        .insert_once(&document, Some("scheduled:test-job"))
        .expect("second insert should succeed");

    assert!(first.is_some(), "first scheduled execution should commit");
    assert!(
        second.is_none(),
        "second scheduled execution should be skipped"
    );
    assert_eq!(
        store.latest_sequence().expect("latest sequence"),
        SequenceNumber(1)
    );
    let tasks = store
        .scan_table(&TableName::new("tasks").expect("table name should be valid"))
        .expect("scan should succeed");
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].fields.get("title"), Some(&json!("Hello once")));
}

#[test]
fn scan_table_is_logically_isolated() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let task = sample_document("tasks", "Task");
    let user = sample_document("users", "User");

    store.insert(&task).expect("task insert should succeed");
    store.insert(&user).expect("user insert should succeed");

    let tasks = store
        .scan_table(&TableName::new("tasks").expect("table name should be valid"))
        .expect("scan should succeed");
    let users = store
        .scan_table(&TableName::new("users").expect("table name should be valid"))
        .expect("scan should succeed");

    assert_eq!(tasks.len(), 1);
    assert_eq!(users.len(), 1);
    assert_eq!(tasks[0].fields.get("title"), Some(&json!("Task")));
    assert_eq!(users[0].fields.get("title"), Some(&json!("User")));
}

#[test]
fn commit_log_sequences_increment() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let first = sample_document("tasks", "First");
    let second = sample_document("tasks", "Second");

    let first_commit = store.insert(&first).expect("first insert should succeed");
    let second_commit = store.insert(&second).expect("second insert should succeed");
    let entries = store
        .read_commit_log_from(SequenceNumber(1))
        .expect("commit log read should succeed");

    assert_eq!(first_commit.sequence, SequenceNumber(1));
    assert_eq!(second_commit.sequence, SequenceNumber(2));
    assert_eq!(entries.len(), 2);
    assert_eq!(
        store.latest_sequence().expect("latest sequence"),
        SequenceNumber(2)
    );
}

#[test]
fn durable_journal_serialization_preserves_payload_and_metadata() {
    let table = TableName::new("tasks").expect("table name should be valid");
    let before = Document::new(
        table.clone(),
        serde_json::Map::from_iter([("title".to_string(), json!("Before"))]),
    );
    let mut after = before.clone();
    after.fields.insert("title".to_string(), json!("After"));

    let record = DurableMutationRecord::new(
        SequenceNumber(7),
        Timestamp(42),
        vec![WriteOp {
            table: table.clone(),
            op_type: WriteOpType::Update,
            doc_id: before.id,
            previous: Some(before.clone()),
            current: Some(after.clone()),
        }],
        Some("scheduled:job-7".to_string()),
    )
    .expect("durable record should build");

    let encoded =
        crate::commit_log::serialize_durable_record(&record).expect("record should serialize");
    let decoded =
        crate::commit_log::deserialize_durable_record(&encoded).expect("record should deserialize");

    assert_eq!(decoded, record);
    assert_eq!(decoded.writes[0].table, table);
    assert_eq!(decoded.writes[0].doc_id, before.id);
    assert_eq!(
        decoded.writes[0]
            .current
            .as_ref()
            .and_then(|document| document.fields.get("title")),
        Some(&json!("After"))
    );
    assert_eq!(
        decoded.scheduled_execution_id.as_deref(),
        Some("scheduled:job-7")
    );
}

#[test]
fn durable_journal_metadata_supports_dependency_intersection_checks() {
    let table = TableName::new("tasks").expect("table name should be valid");
    let before = Document::new(
        table.clone(),
        serde_json::Map::from_iter([
            ("rank".to_string(), json!(3)),
            ("status".to_string(), json!("open")),
        ]),
    );
    let mut after = before.clone();
    after.fields.insert("rank".to_string(), json!(8));

    let record = DurableMutationRecord::new(
        SequenceNumber(3),
        Timestamp(12),
        vec![WriteOp {
            table: table.clone(),
            op_type: WriteOpType::Update,
            doc_id: before.id,
            previous: Some(before.clone()),
            current: Some(after.clone()),
        }],
        None,
    )
    .expect("durable record should build");
    let mut document_dependency = DependencySet::default();
    document_dependency.record_document(&table, before.id);
    assert!(durable_record_intersects_dependency_set(
        &record,
        &document_dependency,
        &[],
        |_, _| Ok(None)
    ));

    let mut table_dependency = DependencySet::default();
    table_dependency.record_table(&table);
    assert!(durable_record_intersects_dependency_set(
        &record,
        &table_dependency,
        &[],
        |_, _| Ok(None)
    ));

    let mut index_range_dependency = DependencySet::default();
    index_range_dependency.record_index_range(IndexRangeDependency {
        table: table.clone(),
        index_name: "by_rank".to_string(),
        field: "rank".to_string(),
        start: Some(json!(5)),
        end: Some(json!(10)),
        start_inclusive: true,
        end_inclusive: true,
    });
    assert!(durable_record_intersects_dependency_set(
        &record,
        &index_range_dependency,
        &[],
        |_, _| Ok(None)
    ));

    let mut unrelated = DependencySet::default();
    unrelated.record_table(&TableName::new("users").expect("table name should be valid"));
    assert!(!durable_record_intersects_dependency_set(
        &record,
        &unrelated,
        &[],
        |_, _| Ok(None)
    ));
}

#[test]
fn durable_journal_batch_append_enforces_no_holes() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let first = DurableMutationRecord::new(
        SequenceNumber(1),
        Timestamp(10),
        vec![WriteOp {
            table: TableName::new("tasks").expect("table name should be valid"),
            op_type: WriteOpType::Insert,
            doc_id: DocumentId::new(),
            previous: None,
            current: Some(sample_document("tasks", "First")),
        }],
        None,
    )
    .expect("first durable record should build");
    let second = DurableMutationRecord::new(
        SequenceNumber(2),
        Timestamp(11),
        vec![WriteOp {
            table: TableName::new("tasks").expect("table name should be valid"),
            op_type: WriteOpType::Insert,
            doc_id: DocumentId::new(),
            previous: None,
            current: Some(sample_document("tasks", "Second")),
        }],
        None,
    )
    .expect("second durable record should build");

    store
        .append_durable_records_batch(vec![first.clone(), second.clone()])
        .expect("initial batch append should succeed");
    assert_eq!(
        store
            .journal_progress()
            .expect("journal progress should read"),
        crate::JournalProgress {
            durable_head: SequenceNumber(2),
            applied_head: SequenceNumber(0),
        }
    );

    let error = store
        .append_durable_records_batch(vec![
            DurableMutationRecord::new(
                SequenceNumber(4),
                Timestamp(12),
                vec![WriteOp {
                    table: TableName::new("tasks").expect("table name should be valid"),
                    op_type: WriteOpType::Insert,
                    doc_id: DocumentId::new(),
                    previous: None,
                    current: Some(sample_document("tasks", "Gap")),
                }],
                None,
            )
            .expect("gap record should build"),
        ])
        .expect_err("batch append should reject sequence holes");
    assert!(
        matches!(error, Error::Internal(message) if message.contains("expected sequence 3, got 4"))
    );
    assert_eq!(
        store
            .latest_sequence()
            .expect("latest sequence should stay stable"),
        SequenceNumber(2)
    );
    assert_eq!(
        store
            .read_durable_journal_from(SequenceNumber(1))
            .expect("durable journal should read")
            .into_iter()
            .map(|record| record.sequence)
            .collect::<Vec<_>>(),
        vec![SequenceNumber(1), SequenceNumber(2)]
    );
}

#[test]
fn durable_journal_stream_uses_cursor_floor_after_retention_cut() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let first = sample_document("tasks", "first");
    let second = sample_document("tasks", "second");
    store.insert(&first).expect("first insert should succeed");
    store.insert(&second).expect("second insert should succeed");

    let write_txn = store.db.begin_write().expect("write txn should open");
    {
        let mut journal = write_txn
            .open_table(crate::store::COMMIT_LOG)
            .expect("commit log table should open");
        journal
            .remove(1)
            .expect("first durable journal entry should be removable");
    }
    store
        .commit_write_txn(write_txn)
        .expect("retention-cut transaction should commit");

    let error = store
        .stream_durable_journal(SequenceNumber(0), 10)
        .expect_err("cursor behind the retained floor should fail");
    assert!(matches!(
        error,
        Error::InvalidInput(message)
            if message.contains("behind the retention floor 1")
    ));

    let page = store
        .stream_durable_journal(SequenceNumber(1), 10)
        .expect("cursor at the retained floor should succeed");
    assert_eq!(page.cursor_floor, SequenceNumber(1));
    assert_eq!(page.latest_sequence, SequenceNumber(2));
    assert_eq!(page.next_cursor, SequenceNumber(2));
    assert_eq!(page.records.len(), 1);
    assert_eq!(page.records[0].sequence, SequenceNumber(2));
}

#[test]
fn recovery_replays_durable_but_unapplied_records() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let first = sample_document("tasks", "First");
    let second = sample_document("tasks", "Second");
    let records = vec![
        DurableMutationRecord::new(
            SequenceNumber(1),
            Timestamp(100),
            vec![WriteOp {
                table: first.table.clone(),
                op_type: WriteOpType::Insert,
                doc_id: first.id,
                previous: None,
                current: Some(first.clone()),
            }],
            None,
        )
        .expect("first durable record should build"),
        DurableMutationRecord::new(
            SequenceNumber(2),
            Timestamp(101),
            vec![WriteOp {
                table: second.table.clone(),
                op_type: WriteOpType::Insert,
                doc_id: second.id,
                previous: None,
                current: Some(second.clone()),
            }],
            None,
        )
        .expect("second durable record should build"),
    ];

    store
        .append_durable_records_batch(records)
        .expect("durable append should succeed");
    assert_eq!(
        store
            .journal_progress()
            .expect("journal progress should read"),
        crate::JournalProgress {
            durable_head: SequenceNumber(2),
            applied_head: SequenceNumber(0),
        }
    );
    assert!(
        store
            .scan_table(&TableName::new("tasks").expect("table name should be valid"))
            .expect("scan should succeed")
            .is_empty(),
        "unapplied durable records must not become visible through table scans"
    );

    let progress = store
        .recover_durable_journal()
        .expect("recovery should apply pending durable records");
    assert_eq!(
        progress,
        crate::JournalProgress {
            durable_head: SequenceNumber(2),
            applied_head: SequenceNumber(2),
        }
    );

    let documents = store
        .scan_table(&TableName::new("tasks").expect("table name should be valid"))
        .expect("scan should succeed after recovery");
    assert_eq!(documents.len(), 2);
    let mut titles = documents
        .iter()
        .map(|document| {
            document
                .fields
                .get("title")
                .and_then(|value| value.as_str())
                .expect("recovered document title should exist")
        })
        .collect::<Vec<_>>();
    titles.sort_unstable();
    assert_eq!(titles, vec!["First", "Second"]);
}

#[test]
fn materialized_snapshot_plus_journal_tail_rebuild_matches_live_state() {
    let live = TenantStore::create_in_memory().expect("store should open");
    let table = TableName::new("tasks").expect("table name should be valid");
    let table_schema = TableSchema {
        table: table.clone(),
        fields: vec![FieldSchema {
            name: "rank".to_string(),
            field_type: FieldType::Number,
            required: true,
        }],
        indexes: vec![IndexDefinition {
            name: "by_rank".to_string(),
            field: "rank".to_string(),
        }],
        access_policy: None,
    };
    live.replace_table_schema(&table_schema)
        .expect("table schema should persist");

    let first = Document::new(
        table.clone(),
        serde_json::Map::from_iter([
            ("title".to_string(), json!("First")),
            ("rank".to_string(), json!(1)),
        ]),
    );
    live.insert_with_indexes(&first, &table_schema.indexes)
        .expect("first insert should succeed");
    let snapshot = live
        .export_materialized_journal_snapshot()
        .expect("snapshot export should succeed");
    assert_eq!(snapshot.version, 1);
    assert_eq!(snapshot.applied_sequence, SequenceNumber(1));
    assert_eq!(snapshot.durable_head, SequenceNumber(1));

    let second = Document::new(
        table.clone(),
        serde_json::Map::from_iter([
            ("title".to_string(), json!("Second")),
            ("rank".to_string(), json!(3)),
        ]),
    );
    live.insert_with_indexes(&second, &table_schema.indexes)
        .expect("second insert should succeed");
    live.update_with_indexes(
        &table,
        &first.id,
        &serde_json::Map::from_iter([("rank".to_string(), json!(2))]),
        &table_schema.indexes,
    )
    .expect("update should succeed");

    let tail = live
        .read_durable_journal_from(SequenceNumber(snapshot.applied_sequence.0 + 1))
        .expect("journal tail should read");
    let rebuilt = TenantStore::create_in_memory().expect("rebuilt store should open");
    let progress = rebuilt
        .rebuild_materialized_journal_from_snapshot(&snapshot, &tail, None)
        .expect("snapshot plus tail rebuild should succeed");

    assert_eq!(
        progress,
        live.journal_progress()
            .expect("live journal progress should read")
    );
    assert_eq!(
        rebuilt.load_schema().expect("rebuilt schema should load"),
        live.load_schema().expect("live schema should load")
    );
    assert_eq!(
        rebuilt
            .scan_table(&table)
            .expect("rebuilt scan should succeed"),
        live.scan_table(&table).expect("live scan should succeed")
    );
    assert_eq!(
        rebuilt
            .index_scan_eq(&table, "by_rank", &json!(2))
            .expect("rebuilt index scan should succeed"),
        live.index_scan_eq(&table, "by_rank", &json!(2))
            .expect("live index scan should succeed")
    );
    assert_eq!(
        rebuilt
            .index_scan_eq(&table, "by_rank", &json!(3))
            .expect("rebuilt index scan should succeed"),
        live.index_scan_eq(&table, "by_rank", &json!(3))
            .expect("live index scan should succeed")
    );
}

#[test]
fn materialized_snapshot_rebuild_can_stop_at_a_point_in_time_sequence() {
    let live = TenantStore::create_in_memory().expect("store should open");
    let table = TableName::new("tasks").expect("table name should be valid");
    let table_schema = TableSchema {
        table: table.clone(),
        fields: vec![FieldSchema {
            name: "rank".to_string(),
            field_type: FieldType::Number,
            required: true,
        }],
        indexes: vec![IndexDefinition {
            name: "by_rank".to_string(),
            field: "rank".to_string(),
        }],
        access_policy: None,
    };
    live.replace_table_schema(&table_schema)
        .expect("table schema should persist");

    let first = Document::new(
        table.clone(),
        serde_json::Map::from_iter([
            ("title".to_string(), json!("First")),
            ("rank".to_string(), json!(1)),
        ]),
    );
    live.insert_with_indexes(&first, &table_schema.indexes)
        .expect("first insert should succeed");
    let snapshot = live
        .export_materialized_journal_snapshot()
        .expect("snapshot export should succeed");
    assert_eq!(snapshot.version, 1);
    assert_eq!(snapshot.durable_head, SequenceNumber(1));

    let second = Document::new(
        table.clone(),
        serde_json::Map::from_iter([
            ("title".to_string(), json!("Second")),
            ("rank".to_string(), json!(3)),
        ]),
    );
    live.insert_with_indexes(&second, &table_schema.indexes)
        .expect("second insert should succeed");
    live.update_with_indexes(
        &table,
        &first.id,
        &serde_json::Map::from_iter([("rank".to_string(), json!(2))]),
        &table_schema.indexes,
    )
    .expect("update should succeed");

    let tail = live
        .read_durable_journal_from(SequenceNumber(snapshot.applied_sequence.0 + 1))
        .expect("journal tail should read");
    let rebuilt = TenantStore::create_in_memory().expect("rebuilt store should open");
    let progress = rebuilt
        .rebuild_materialized_journal_from_snapshot(&snapshot, &tail, Some(SequenceNumber(2)))
        .expect("point-in-time rebuild should succeed");

    assert_eq!(
        progress,
        crate::JournalProgress {
            durable_head: SequenceNumber(2),
            applied_head: SequenceNumber(2),
        }
    );
    let documents = rebuilt
        .scan_table(&table)
        .expect("rebuilt point-in-time scan should succeed");
    assert_eq!(documents.len(), 2);
    let rebuilt_first = documents
        .iter()
        .find(|document| document.id == first.id)
        .expect("first document should exist at point-in-time rebuild");
    assert_eq!(rebuilt_first.fields.get("rank"), Some(&json!(1)));
    assert_eq!(
        rebuilt
            .index_scan_eq(&table, "by_rank", &json!(1))
            .expect("rank 1 index scan should succeed")
            .len(),
        1
    );
    assert_eq!(
        rebuilt
            .index_scan_eq(&table, "by_rank", &json!(2))
            .expect("rank 2 index scan should succeed")
            .len(),
        0
    );
}

#[test]
fn shadow_materializer_rebuild_from_checkpoint_and_journal_matches_live_state() {
    let live = TenantStore::create_in_memory().expect("store should open");
    let table = TableName::new("tasks").expect("table name should be valid");

    let first = sample_document("tasks", "first");
    live.insert(&first).expect("first insert should succeed");
    let checkpoint = live
        .export_materialized_journal_snapshot()
        .expect("checkpoint snapshot should export");

    let second = sample_document("tasks", "second");
    live.insert(&second).expect("second insert should succeed");
    live.update(
        &table,
        &first.id,
        &serde_json::Map::from_iter([("title".to_string(), json!("first-updated"))]),
    )
    .expect("update should succeed");

    let journal_tail = live
        .read_durable_journal_from(SequenceNumber(checkpoint.applied_sequence.0 + 1))
        .expect("journal tail should read");
    let materializer = ShadowMaterializer::from_checkpoint_and_journal(
        checkpoint,
        journal_tail,
        ShadowMaterializerConfig {
            compaction_threshold_records: 10,
        },
    )
    .expect("shadow materializer should rebuild");

    let live_snapshot = live
        .export_materialized_journal_snapshot()
        .expect("live snapshot should export");
    assert_eq!(materializer.current_snapshot(), live_snapshot);
}

#[test]
fn shadow_materializer_compaction_is_deterministic_for_same_input() {
    let live = TenantStore::create_in_memory().expect("store should open");
    let checkpoint = live
        .export_materialized_journal_snapshot()
        .expect("checkpoint snapshot should export");
    for title in ["alpha", "beta", "gamma"] {
        live.insert(&sample_document("tasks", title))
            .expect("insert should succeed");
    }
    let journal_tail = live
        .read_durable_journal_from(SequenceNumber(1))
        .expect("journal tail should read");
    let config = ShadowMaterializerConfig {
        compaction_threshold_records: 2,
    };

    let left = ShadowMaterializer::from_checkpoint_and_journal(
        checkpoint.clone(),
        journal_tail.clone(),
        config,
    )
    .expect("left materializer should build");
    let right = ShadowMaterializer::from_checkpoint_and_journal(checkpoint, journal_tail, config)
        .expect("right materializer should build");

    assert_eq!(left.current_snapshot(), right.current_snapshot());
    assert_eq!(left.manifest(), right.manifest());
}

#[test]
fn shadow_materializer_recovery_from_checkpoint_and_pending_journal_restores_same_state() {
    let live = TenantStore::create_in_memory().expect("store should open");
    let checkpoint = live
        .export_materialized_journal_snapshot()
        .expect("checkpoint snapshot should export");
    for title in ["alpha", "beta"] {
        live.insert(&sample_document("tasks", title))
            .expect("insert should succeed");
    }
    let journal_tail = live
        .read_durable_journal_from(SequenceNumber(1))
        .expect("journal tail should read");
    let config = ShadowMaterializerConfig {
        compaction_threshold_records: 10,
    };
    let materializer =
        ShadowMaterializer::from_checkpoint_and_journal(checkpoint.clone(), journal_tail, config)
            .expect("materializer should build");

    let recovered = ShadowMaterializer::recover(
        checkpoint,
        materializer.pending_records().to_vec(),
        materializer.manifest().clone(),
        config,
    )
    .expect("materializer should recover");

    assert_eq!(
        recovered.current_snapshot(),
        materializer.current_snapshot()
    );
    assert_eq!(recovered.manifest(), materializer.manifest());
}

#[test]
fn shadow_materializer_recovery_after_interrupted_compaction_converges_to_clean_rebuild() {
    let live = TenantStore::create_in_memory().expect("store should open");
    let first = sample_document("tasks", "alpha");
    live.insert(&first).expect("first insert should succeed");
    let checkpoint = live
        .export_materialized_journal_snapshot()
        .expect("checkpoint snapshot should export");

    for title in ["beta", "gamma", "delta"] {
        live.insert(&sample_document("tasks", title))
            .expect("insert should succeed");
    }

    let journal_tail = live
        .read_durable_journal_from(SequenceNumber(checkpoint.applied_sequence.0 + 1))
        .expect("journal tail should read");
    let config = ShadowMaterializerConfig {
        compaction_threshold_records: 2,
    };

    let clean = ShadowMaterializer::from_checkpoint_and_journal(
        checkpoint.clone(),
        journal_tail.clone(),
        config,
    )
    .expect("clean shadow materializer should rebuild");

    let interrupted_manifest = ShadowMaterializerManifest {
        version: 1,
        checkpoint_sequence: checkpoint.applied_sequence,
        current_sequence: SequenceNumber(4),
        pending_record_count: journal_tail.len(),
        compaction_runs: 0,
        compaction_threshold_records: config.compaction_threshold_records,
    };
    let recovered =
        ShadowMaterializer::recover(checkpoint, journal_tail, interrupted_manifest, config)
            .expect("recovery after interrupted compaction should succeed");

    assert_eq!(recovered.current_snapshot(), clean.current_snapshot());
    assert_eq!(recovered.manifest(), clean.manifest());
}

#[test]
fn shadow_materializer_rejects_corrupted_journal_input() {
    let live = TenantStore::create_in_memory().expect("store should open");
    let first = sample_document("tasks", "alpha");
    live.insert(&first).expect("first insert should succeed");
    let checkpoint = live
        .export_materialized_journal_snapshot()
        .expect("checkpoint snapshot should export");
    live.insert(&sample_document("tasks", "beta"))
        .expect("second insert should succeed");

    let mut journal_tail = live
        .read_durable_journal_from(SequenceNumber(checkpoint.applied_sequence.0 + 1))
        .expect("journal tail should read");
    journal_tail[0].integrity_sha256[0] ^= 0xff;

    let error = ShadowMaterializer::from_checkpoint_and_journal(
        checkpoint,
        journal_tail,
        ShadowMaterializerConfig {
            compaction_threshold_records: 4,
        },
    )
    .expect_err("corrupted journal input should be rejected");
    assert!(
        matches!(error, Error::Internal(message) if message.contains("failed integrity verification"))
    );
}

#[test]
fn shadow_materializer_rejects_corrupted_manifest_recovery_input() {
    let live = TenantStore::create_in_memory().expect("store should open");
    let checkpoint = live
        .export_materialized_journal_snapshot()
        .expect("checkpoint snapshot should export");
    live.insert(&sample_document("tasks", "alpha"))
        .expect("insert should succeed");

    let journal_tail = live
        .read_durable_journal_from(SequenceNumber(1))
        .expect("journal tail should read");
    let config = ShadowMaterializerConfig {
        compaction_threshold_records: 8,
    };
    let materializer = ShadowMaterializer::from_checkpoint_and_journal(
        checkpoint.clone(),
        journal_tail.clone(),
        config,
    )
    .expect("materializer should rebuild");

    let mut corrupted_manifest = materializer.manifest().clone();
    corrupted_manifest.pending_record_count += 1;
    let error = ShadowMaterializer::recover(checkpoint, journal_tail, corrupted_manifest, config)
        .expect_err("corrupted manifest should be rejected");
    assert!(matches!(error, Error::InvalidInput(message) if message.contains("pending count")));
}

#[test]
fn shadow_materializer_seeded_rebuild_matches_live_state_across_operation_sequences() {
    let table = TableName::new("tasks").expect("table name should be valid");

    for initial_seed in [1_u64, 7, 13, 42] {
        let live = TenantStore::create_in_memory().expect("store should open");
        let mut seed = initial_seed;
        let mut live_ids = Vec::new();
        let snapshot_step = (next_seeded_u64(&mut seed) % 12 + 4) as usize;
        let mut checkpoint = live
            .export_materialized_journal_snapshot()
            .expect("initial checkpoint should export");

        for step in 0..24 {
            let draw = next_seeded_u64(&mut seed);
            let choice = if live_ids.is_empty() { 0 } else { draw % 3 };
            match choice {
                0 => {
                    let document = Document::new(
                        table.clone(),
                        serde_json::Map::from_iter([
                            (
                                "title".to_string(),
                                json!(format!("seed-{initial_seed}-insert-{step}")),
                            ),
                            ("rank".to_string(), json!((draw % 100) as i64)),
                        ]),
                    );
                    live.insert(&document).expect("insert should succeed");
                    live_ids.push(document.id);
                }
                1 => {
                    let index = (draw as usize) % live_ids.len();
                    let document_id = live_ids[index];
                    live.update(
                        &table,
                        &document_id,
                        &serde_json::Map::from_iter([
                            (
                                "title".to_string(),
                                json!(format!("seed-{initial_seed}-update-{step}")),
                            ),
                            ("rank".to_string(), json!(((draw >> 8) % 100) as i64)),
                        ]),
                    )
                    .expect("update should succeed");
                }
                _ => {
                    let index = (draw as usize) % live_ids.len();
                    let document_id = live_ids.swap_remove(index);
                    live.delete(&table, &document_id)
                        .expect("delete should succeed");
                }
            }

            if step == snapshot_step {
                checkpoint = live
                    .export_materialized_journal_snapshot()
                    .expect("mid-run checkpoint should export");
            }
        }

        let journal_tail = live
            .read_durable_journal_from(SequenceNumber(checkpoint.applied_sequence.0 + 1))
            .expect("journal tail should read");
        let config = ShadowMaterializerConfig {
            compaction_threshold_records: ((initial_seed % 4) + 2) as usize,
        };

        let left = ShadowMaterializer::from_checkpoint_and_journal(
            checkpoint.clone(),
            journal_tail.clone(),
            config,
        )
        .expect("left shadow materializer should rebuild");
        let right =
            ShadowMaterializer::from_checkpoint_and_journal(checkpoint, journal_tail, config)
                .expect("right shadow materializer should rebuild");
        let live_snapshot = live
            .export_materialized_journal_snapshot()
            .expect("live snapshot should export");

        assert_eq!(
            left.current_snapshot(),
            live_snapshot,
            "seed {initial_seed}"
        );
        assert_eq!(
            left.current_snapshot(),
            right.current_snapshot(),
            "rebuild should be deterministic for seed {initial_seed}"
        );
        assert_eq!(
            left.manifest(),
            right.manifest(),
            "manifest should be deterministic for seed {initial_seed}"
        );
    }
}

#[test]
fn materialized_snapshot_records_durable_boundary_and_rejects_incomplete_tail() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let document = Document::new(
        TableName::new("tasks").expect("table name should be valid"),
        serde_json::Map::from_iter([("title".to_string(), json!("durable-only"))]),
    );
    let record = DurableMutationRecord::new(
        SequenceNumber(1),
        Timestamp(100),
        vec![WriteOp {
            table: document.table.clone(),
            op_type: WriteOpType::Insert,
            doc_id: document.id,
            previous: None,
            current: Some(document.clone()),
        }],
        None,
    )
    .expect("durable record should build");
    store
        .append_durable_records_batch(vec![record.clone()])
        .expect("durable append should succeed");

    let snapshot = store
        .export_materialized_journal_snapshot()
        .expect("snapshot export should succeed");
    assert_eq!(snapshot.version, 1);
    assert_eq!(snapshot.applied_sequence, SequenceNumber(0));
    assert_eq!(snapshot.durable_head, SequenceNumber(1));

    let rebuilt = TenantStore::create_in_memory().expect("rebuilt store should open");
    let error = rebuilt
        .rebuild_materialized_journal_from_snapshot(&snapshot, &[], None)
        .expect_err("rebuild should reject a missing journal tail when the snapshot saw apply lag");
    assert!(matches!(
        error,
        Error::InvalidInput(message)
            if message.contains("available head 0 is behind snapshot durable head 1")
    ));

    let rebuilt = TenantStore::create_in_memory().expect("rebuilt store should open");
    let progress = rebuilt
        .rebuild_materialized_journal_from_snapshot(&snapshot, &[record], None)
        .expect("rebuild should succeed once the required tail is present");
    assert_eq!(
        progress,
        crate::JournalProgress {
            durable_head: SequenceNumber(1),
            applied_head: SequenceNumber(1),
        }
    );
    let documents = rebuilt
        .scan_table(&document.table)
        .expect("rebuilt scan should succeed");
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].id, document.id);
}

#[test]
fn update_applies_patch_and_appends_commit() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let document = sample_document("tasks", "Before");

    store.insert(&document).expect("insert should succeed");
    let commit = store
        .update(
            &document.table,
            &document.id,
            &serde_json::Map::from_iter([("title".to_string(), json!("After"))]),
        )
        .expect("update should succeed");
    let fetched = store
        .get(&document.table, &document.id)
        .expect("get should succeed")
        .expect("document should exist");

    assert_eq!(commit.sequence, SequenceNumber(2));
    assert_eq!(fetched.fields.get("title"), Some(&json!("After")));
}

#[test]
fn delete_removes_document_and_appends_commit() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let document = sample_document("tasks", "Disposable");

    store.insert(&document).expect("insert should succeed");
    let commit = store
        .delete(&document.table, &document.id)
        .expect("delete should succeed");
    let fetched = store
        .get(&document.table, &document.id)
        .expect("get should succeed");

    assert_eq!(commit.sequence, SequenceNumber(2));
    assert!(fetched.is_none());
}

#[test]
fn store_reopens_from_disk() {
    let dir = tempdir().expect("tempdir should create");
    let path = dir.path().join("tenant.redb");
    let document = sample_document("tasks", "Persisted");

    {
        let store = TenantStore::open(&path).expect("store should open");
        store.insert(&document).expect("insert should succeed");
    }

    let reopened = TenantStore::open(&path).expect("store should reopen");
    let documents = reopened
        .scan_table(&TableName::new("tasks").expect("table name should be valid"))
        .expect("scan should succeed");

    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].fields.get("title"), Some(&json!("Persisted")));
}

#[test]
fn store_get_nonexistent_document_returns_none() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let result = store
        .get(
            &TableName::new("tasks").expect("table name should be valid"),
            &DocumentId::new(),
        )
        .expect("get should succeed");

    assert!(result.is_none());
}

#[test]
fn store_scan_empty_table_returns_empty_vec() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let documents = store
        .scan_table(&TableName::new("tasks").expect("table name should be valid"))
        .expect("scan should succeed");

    assert!(documents.is_empty());
}

#[test]
fn store_latest_sequence_on_fresh_store_returns_zero() {
    let store = TenantStore::create_in_memory().expect("store should open");
    assert_eq!(
        store
            .latest_sequence()
            .expect("latest sequence should succeed"),
        SequenceNumber(0)
    );
}

#[test]
fn store_update_nonexistent_document_returns_error() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let error = store
        .update(
            &TableName::new("tasks").expect("table name should be valid"),
            &DocumentId::new(),
            &serde_json::Map::from_iter([("title".to_string(), json!("After"))]),
        )
        .expect_err("update should fail");

    assert!(matches!(error, Error::DocumentNotFound(_)));
}

#[test]
fn store_delete_nonexistent_document_returns_error() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let error = store
        .delete(
            &TableName::new("tasks").expect("table name should be valid"),
            &DocumentId::new(),
        )
        .expect_err("delete should fail");

    assert!(matches!(error, Error::DocumentNotFound(_)));
}

#[test]
fn schema_roundtrip_through_redb() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let table_schema = TableSchema {
        table: TableName::new("users").expect("table name should be valid"),
        fields: vec![
            FieldSchema {
                name: "name".to_string(),
                field_type: FieldType::String,
                required: true,
            },
            FieldSchema {
                name: "age".to_string(),
                field_type: FieldType::Number,
                required: false,
            },
        ],
        indexes: Vec::new(),
        access_policy: None,
    };

    store
        .save_table_schema(&table_schema)
        .expect("schema should save");
    let schema = store.load_schema().expect("schema should load");

    assert_eq!(schema.get_table(&table_schema.table), Some(&table_schema));

    store
        .delete_table_schema_entry(&table_schema.table)
        .expect("schema entry should delete");
    let schema = store.load_schema().expect("schema should load");
    assert!(schema.get_table(&table_schema.table).is_none());
}

#[test]
fn replace_table_schema_rebuilds_indexes_and_persists_schema() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let table = TableName::new("users").expect("table name should be valid");
    for email in ["a@test.com", "b@test.com", "a@test.com"] {
        let document = Document::new(
            table.clone(),
            serde_json::Map::from_iter([("email".to_string(), json!(email))]),
        );
        store.insert(&document).expect("insert should succeed");
    }

    let table_schema = TableSchema {
        table: table.clone(),
        fields: vec![FieldSchema {
            name: "email".to_string(),
            field_type: FieldType::String,
            required: false,
        }],
        indexes: vec![IndexDefinition {
            name: "by_email".to_string(),
            field: "email".to_string(),
        }],
        access_policy: None,
    };

    store
        .replace_table_schema(&table_schema)
        .expect("schema replacement should succeed");

    let schema = store.load_schema().expect("schema should load");
    assert_eq!(schema.get_table(&table), Some(&table_schema));

    let docs = store
        .index_scan_eq(&table, "by_email", &json!("a@test.com"))
        .expect("index scan should succeed");
    assert_eq!(docs.len(), 2);
}

#[test]
fn delete_table_schema_clears_schema_and_indexes() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let table = TableName::new("users").expect("table name should be valid");
    let document = Document::new(
        table.clone(),
        serde_json::Map::from_iter([("email".to_string(), json!("gone@test.com"))]),
    );
    store.insert(&document).expect("insert should succeed");

    let table_schema = TableSchema {
        table: table.clone(),
        fields: vec![FieldSchema {
            name: "email".to_string(),
            field_type: FieldType::String,
            required: false,
        }],
        indexes: vec![IndexDefinition {
            name: "by_email".to_string(),
            field: "email".to_string(),
        }],
        access_policy: None,
    };
    store
        .replace_table_schema(&table_schema)
        .expect("schema replacement should succeed");

    store
        .delete_table_schema(&table)
        .expect("schema deletion should succeed");

    let schema = store.load_schema().expect("schema should load");
    assert!(schema.get_table(&table).is_none());
    let docs = store
        .index_scan_eq(&table, "by_email", &json!("gone@test.com"))
        .expect("index scan should succeed");
    assert!(docs.is_empty());
}

#[test]
fn update_with_indexes_validated_maintains_entries() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let table = TableName::new("users").expect("table name should be valid");
    let index = IndexDefinition {
        name: "by_email".to_string(),
        field: "email".to_string(),
    };
    let document = Document::new(
        table.clone(),
        serde_json::Map::from_iter([("email".to_string(), json!("old@test.com"))]),
    );
    store
        .insert_with_indexes(&document, std::slice::from_ref(&index))
        .expect("insert should succeed");

    store
        .update_with_indexes_validated(
            &table,
            &document.id,
            &serde_json::Map::from_iter([("email".to_string(), json!("new@test.com"))]),
            std::slice::from_ref(&index),
            |_existing, updated| {
                assert_eq!(updated.fields.get("email"), Some(&json!("new@test.com")));
                Ok(())
            },
        )
        .expect("validated update should succeed");

    let old_docs = store
        .index_scan_eq(&table, "by_email", &json!("old@test.com"))
        .expect("old index scan should succeed");
    let new_docs = store
        .index_scan_eq(&table, "by_email", &json!("new@test.com"))
        .expect("new index scan should succeed");

    assert!(old_docs.is_empty());
    assert_eq!(new_docs.len(), 1);
    assert_eq!(
        new_docs[0].fields.get("email"),
        Some(&json!("new@test.com"))
    );
}

#[test]
fn index_key_encoding_preserves_number_sort_order() {
    let mut encoded = [
        encode_index_value(&json!(-1.5)).expect("value should encode"),
        encode_index_value(&json!(0)).expect("value should encode"),
        encode_index_value(&json!(1)).expect("value should encode"),
        encode_index_value(&json!(100)).expect("value should encode"),
    ];
    encoded.sort();

    assert_eq!(
        encoded[0],
        encode_index_value(&json!(-1.5)).expect("value should encode")
    );
    assert_eq!(
        encoded[1],
        encode_index_value(&json!(0)).expect("value should encode")
    );
    assert_eq!(
        encoded[2],
        encode_index_value(&json!(1)).expect("value should encode")
    );
    assert_eq!(
        encoded[3],
        encode_index_value(&json!(100)).expect("value should encode")
    );
}

#[test]
fn index_key_encoding_preserves_string_sort_order() {
    let mut encoded = [
        encode_index_value(&json!("charlie")).expect("value should encode"),
        encode_index_value(&json!("alpha")).expect("value should encode"),
        encode_index_value(&json!("bravo")).expect("value should encode"),
    ];
    encoded.sort();

    assert_eq!(
        encoded[0],
        encode_index_value(&json!("alpha")).expect("value should encode")
    );
    assert_eq!(
        encoded[1],
        encode_index_value(&json!("bravo")).expect("value should encode")
    );
    assert_eq!(
        encoded[2],
        encode_index_value(&json!("charlie")).expect("value should encode")
    );
}

#[test]
fn index_insert_and_eq_scan() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let index = IndexDefinition {
        name: "by_email".to_string(),
        field: "email".to_string(),
    };
    for email in ["a@test.com", "b@test.com", "c@test.com"] {
        let document = Document::new(
            TableName::new("users").expect("table name should be valid"),
            serde_json::Map::from_iter([("email".to_string(), json!(email))]),
        );
        store
            .insert_with_indexes(&document, std::slice::from_ref(&index))
            .expect("insert should succeed");
    }

    let match_docs = store
        .index_scan_eq(
            &TableName::new("users").expect("table name should be valid"),
            "by_email",
            &json!("b@test.com"),
        )
        .expect("index scan should succeed");
    assert_eq!(match_docs.len(), 1);
    assert_eq!(
        match_docs[0].fields.get("email"),
        Some(&json!("b@test.com"))
    );

    let missing_docs = store
        .index_scan_eq(
            &TableName::new("users").expect("table name should be valid"),
            "by_email",
            &json!("missing@test.com"),
        )
        .expect("index scan should succeed");
    assert!(missing_docs.is_empty());
}

#[test]
fn index_update_maintains_entries() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let index = IndexDefinition {
        name: "by_email".to_string(),
        field: "email".to_string(),
    };
    let document = Document::new(
        TableName::new("users").expect("table name should be valid"),
        serde_json::Map::from_iter([("email".to_string(), json!("old@test.com"))]),
    );
    store
        .insert_with_indexes(&document, std::slice::from_ref(&index))
        .expect("insert should succeed");

    store
        .update_with_indexes(
            &document.table,
            &document.id,
            &serde_json::Map::from_iter([("email".to_string(), json!("new@test.com"))]),
            std::slice::from_ref(&index),
        )
        .expect("update should succeed");

    let old_docs = store
        .index_scan_eq(&document.table, "by_email", &json!("old@test.com"))
        .expect("index scan should succeed");
    assert!(old_docs.is_empty());

    let new_docs = store
        .index_scan_eq(&document.table, "by_email", &json!("new@test.com"))
        .expect("index scan should succeed");
    assert_eq!(new_docs.len(), 1);
    assert_eq!(
        new_docs[0].fields.get("email"),
        Some(&json!("new@test.com"))
    );
}

#[test]
fn index_delete_removes_entries() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let index = IndexDefinition {
        name: "by_email".to_string(),
        field: "email".to_string(),
    };
    let document = Document::new(
        TableName::new("users").expect("table name should be valid"),
        serde_json::Map::from_iter([("email".to_string(), json!("gone@test.com"))]),
    );
    store
        .insert_with_indexes(&document, std::slice::from_ref(&index))
        .expect("insert should succeed");

    store
        .delete_with_indexes(&document.table, &document.id, std::slice::from_ref(&index))
        .expect("delete should succeed");

    let docs = store
        .index_scan_eq(&document.table, "by_email", &json!("gone@test.com"))
        .expect("index scan should succeed");
    assert!(docs.is_empty());
}

#[test]
fn index_scan_range_on_numbers() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let index = IndexDefinition {
        name: "by_age".to_string(),
        field: "age".to_string(),
    };
    for age in [20, 30, 40, 50] {
        let document = Document::new(
            TableName::new("users").expect("table name should be valid"),
            serde_json::Map::from_iter([("age".to_string(), json!(age))]),
        );
        store
            .insert_with_indexes(&document, std::slice::from_ref(&index))
            .expect("insert should succeed");
    }

    let over_25 = store
        .index_scan_range(
            &TableName::new("users").expect("table name should be valid"),
            "by_age",
            Some(&json!(25)),
            None,
            false,
            true,
        )
        .expect("range scan should succeed");
    assert_eq!(over_25.len(), 3);

    let between = store
        .index_scan_range(
            &TableName::new("users").expect("table name should be valid"),
            "by_age",
            Some(&json!(25)),
            Some(&json!(35)),
            true,
            true,
        )
        .expect("range scan should succeed");
    assert_eq!(between.len(), 1);
    assert_eq!(between[0].fields.get("age"), Some(&json!(30)));
}

#[test]
fn scheduled_job_insert_and_claim_due_removes_pending_entry() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let job = scheduled_insert_job(Timestamp(1_000), "due");

    store
        .insert_scheduled_job(&job)
        .expect("job insert should succeed");
    let due = store
        .claim_due_jobs(Timestamp(1_000))
        .expect("claim should succeed");

    assert_eq!(due, vec![job.clone()]);
    assert!(
        store
            .list_scheduled_jobs()
            .expect("list should succeed")
            .is_empty()
    );
    assert!(
        store
            .claim_due_jobs(Timestamp(1_000))
            .expect("second claim should succeed")
            .is_empty()
    );

    store
        .complete_scheduled_job(&job.id)
        .expect("complete should succeed");
}

#[test]
fn scheduled_job_future_not_due() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let job = scheduled_insert_job(Timestamp(5_000), "later");

    store
        .insert_scheduled_job(&job)
        .expect("job insert should succeed");

    assert!(
        store
            .claim_due_jobs(Timestamp(4_999))
            .expect("claim should succeed")
            .is_empty()
    );
    assert_eq!(
        store
            .list_scheduled_jobs()
            .expect("list should succeed")
            .len(),
        1
    );
}

#[test]
fn cancel_scheduled_job_removes_pending_entry() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let job = scheduled_insert_job(Timestamp(5_000), "cancel me");

    store
        .insert_scheduled_job(&job)
        .expect("job insert should succeed");

    assert!(
        store
            .cancel_scheduled_job(&job.id)
            .expect("cancel should succeed")
    );
    assert!(
        store
            .list_scheduled_jobs()
            .expect("list should succeed")
            .is_empty()
    );
    assert!(
        !store
            .cancel_scheduled_job(&job.id)
            .expect("second cancel should succeed")
    );
}

#[test]
fn recover_running_jobs_moves_orphaned_work_back_to_pending() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let job = scheduled_insert_job(Timestamp(1_000), "recover");

    store
        .insert_scheduled_job(&job)
        .expect("job insert should succeed");
    let claimed = store
        .claim_due_jobs(Timestamp(1_000))
        .expect("claim should succeed");
    assert_eq!(claimed.len(), 1);
    assert!(
        store
            .list_scheduled_jobs()
            .expect("list should succeed")
            .is_empty()
    );

    store
        .recover_running_jobs(Timestamp(2_000))
        .expect("recovery should succeed");

    let pending = store.list_scheduled_jobs().expect("list should succeed");
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, job.id);
    assert_eq!(pending[0].run_at, Timestamp(2_000));
}

#[test]
fn cron_job_crud_and_restart_roundtrip() {
    let dir = tempdir().expect("tempdir should create");
    let path = dir.path().join("tenant.redb");
    let cron = CronJob {
        name: "heartbeat".to_string(),
        schedule: CronSchedule::Interval { seconds: 10 },
        mutation: Mutation::Insert {
            table: TableName::new("tasks").expect("table name should be valid"),
            fields: serde_json::Map::from_iter([("title".to_string(), json!("heartbeat"))]),
        },
        enabled: true,
        last_run: None,
        next_run: Timestamp(10_000),
        created_at: Timestamp(1_000),
    };

    {
        let store = TenantStore::open(&path).expect("store should open");
        store.save_cron_job(&cron).expect("save should succeed");
        let crons = store.load_cron_jobs().expect("load should succeed");
        assert_eq!(crons, vec![cron.clone()]);
        assert!(store.has_scheduled_work().expect("has work should succeed"));
        store
            .delete_cron_job("heartbeat")
            .expect("delete should succeed");
        assert!(
            store
                .load_cron_jobs()
                .expect("load after delete should succeed")
                .is_empty()
        );
        store.save_cron_job(&cron).expect("re-save should succeed");
    }

    let reopened = TenantStore::open(&path).expect("store should reopen");
    let crons = reopened.load_cron_jobs().expect("load should succeed");
    assert_eq!(crons, vec![cron]);
}

#[test]
fn has_scheduled_work_detects_pending_or_running_jobs() {
    let store = TenantStore::create_in_memory().expect("store should open");
    assert!(!store.has_scheduled_work().expect("has work should succeed"));

    let job = scheduled_insert_job(Timestamp(1_000), "pending");
    store
        .insert_scheduled_job(&job)
        .expect("job insert should succeed");
    assert!(store.has_scheduled_work().expect("has work should succeed"));

    let _ = store
        .claim_due_jobs(Timestamp(1_000))
        .expect("claim should succeed");
    assert!(store.has_scheduled_work().expect("has work should succeed"));

    store
        .complete_scheduled_job(&job.id)
        .expect("complete should succeed");
    assert!(!store.has_scheduled_work().expect("has work should succeed"));
}

#[test]
fn next_scheduled_work_at_prefers_earliest_pending_or_enabled_cron() {
    let store = TenantStore::create_in_memory().expect("store should open");
    assert_eq!(
        store
            .next_scheduled_work_at()
            .expect("next due lookup should succeed"),
        None
    );

    let future_job = scheduled_insert_job(Timestamp(5_000), "later");
    let earlier_job = scheduled_insert_job(Timestamp(2_000), "earlier");
    store
        .insert_scheduled_job(&future_job)
        .expect("future job insert should succeed");
    store
        .insert_scheduled_job(&earlier_job)
        .expect("earlier job insert should succeed");
    store
        .save_cron_job(&CronJob {
            name: "disabled".to_string(),
            schedule: CronSchedule::Interval { seconds: 10 },
            mutation: Mutation::Insert {
                table: TableName::new("tasks").expect("table name should be valid"),
                fields: serde_json::Map::from_iter([("title".to_string(), json!("disabled"))]),
            },
            enabled: false,
            last_run: None,
            next_run: Timestamp(1_000),
            created_at: Timestamp(500),
        })
        .expect("disabled cron should save");
    store
        .save_cron_job(&CronJob {
            name: "heartbeat".to_string(),
            schedule: CronSchedule::Interval { seconds: 10 },
            mutation: Mutation::Insert {
                table: TableName::new("tasks").expect("table name should be valid"),
                fields: serde_json::Map::from_iter([("title".to_string(), json!("heartbeat"))]),
            },
            enabled: true,
            last_run: None,
            next_run: Timestamp(3_000),
            created_at: Timestamp(500),
        })
        .expect("enabled cron should save");

    assert_eq!(
        store
            .next_scheduled_work_at()
            .expect("next due lookup should succeed"),
        Some(Timestamp(2_000))
    );

    let claimed = store
        .claim_due_jobs(Timestamp(2_000))
        .expect("claim should succeed");
    assert_eq!(claimed, vec![earlier_job]);

    assert_eq!(
        store
            .next_scheduled_work_at()
            .expect("next due lookup should succeed"),
        Some(Timestamp(3_000))
    );
}

#[test]
fn scheduled_job_result_roundtrip_and_lookup() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let job = scheduled_insert_job(Timestamp(1_000), "done");
    let result = ScheduledJobResult {
        id: job.id,
        run_at: job.run_at,
        finished_at: Timestamp(2_000),
        mutation: job.mutation,
        outcome: ScheduledJobOutcome::Failed,
        error: Some("boom".to_string()),
    };

    store
        .record_scheduled_job_result(&result)
        .expect("result record should succeed");
    let loaded = store
        .get_scheduled_job_result(&result.id)
        .expect("result lookup should succeed")
        .expect("result should exist");

    assert_eq!(loaded, result);
}

#[test]
fn claim_due_jobs_includes_u64_max_boundary() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let job = ScheduledJob {
        id: DocumentId::from_bytes([0; 16]),
        run_at: Timestamp(u64::MAX),
        mutation: Mutation::Insert {
            table: TableName::new("tasks").expect("table name should be valid"),
            fields: serde_json::Map::from_iter([("title".to_string(), json!("edge"))]),
        },
        created_at: Timestamp(1_000),
    };

    store
        .insert_scheduled_job(&job)
        .expect("job insert should succeed");
    let due = store
        .claim_due_jobs(Timestamp(u64::MAX))
        .expect("claim should succeed");

    assert_eq!(due, vec![job]);
}

#[test]
fn usage_store_counts_unique_monthly_active_users_per_month() {
    let store = UsageStore::create_in_memory().expect("usage store should open");
    let march_10 = utc_unix_ms(2026, Month::March, 10);
    let march_20 = utc_unix_ms(2026, Month::March, 20);
    let april_2 = utc_unix_ms(2026, Month::April, 2);

    assert!(
        store
            .record_monthly_active_user("https://issuer.example.com|ada", march_10)
            .expect("first monthly active user should record"),
    );
    assert!(
        !store
            .record_monthly_active_user("https://issuer.example.com|ada", march_20)
            .expect("same user in same month should dedupe"),
    );
    assert!(
        store
            .record_monthly_active_user("https://issuer.example.com|grace", march_20)
            .expect("second user in same month should record"),
    );

    let march = store
        .monthly_active_users_for(march_20)
        .expect("march usage should load");
    assert_eq!(march.month, "2026-03");
    assert_eq!(march.monthly_active_users, 2);
    assert_eq!(march.last_recorded_at_unix_ms, Some(march_20));
    assert_eq!(
        store
            .distinct_identities_for_month(march_20)
            .expect("march identities should load"),
        vec![
            "https://issuer.example.com|ada".to_string(),
            "https://issuer.example.com|grace".to_string()
        ]
    );

    assert!(
        store
            .record_monthly_active_user("https://issuer.example.com|ada", april_2)
            .expect("same user in next month should count again"),
    );
    let april = store
        .monthly_active_users_for(april_2)
        .expect("april usage should load");
    assert_eq!(april.month, "2026-04");
    assert_eq!(april.monthly_active_users, 1);
}

#[tokio::test]
async fn queued_canceled_async_read_never_begins_real_storage_execution() {
    let store = Arc::new(TenantStore::create_in_memory().expect("store should open"));
    let read_storage = RedbTenantStorage::with_max_concurrent_reads(store, 1);
    let first_gate = BlockingReadGate::new();
    let first_gate_for_task = first_gate.clone();
    let first_storage = read_storage.clone();
    let first = tokio::spawn(async move {
        first_storage
            .execute(move |_store| {
                first_gate_for_task.block();
                Ok(())
            })
            .await
    });

    timeout(Duration::from_secs(1), first_gate.wait_until_entered())
        .await
        .expect("first read should acquire the only permit");

    let started = Arc::new(AtomicBool::new(false));
    let cancel = Arc::new(Notify::new());
    let started_for_task = started.clone();
    let cancel_for_wait = cancel.clone();
    let queued_read_storage = read_storage.clone();
    let second = tokio::spawn(async move {
        queued_read_storage
            .execute_cancellable(
                async move {
                    cancel_for_wait.notified().await;
                },
                || Ok(()),
                move |_store, _check_cancel| {
                    started_for_task.store(true, Ordering::SeqCst);
                    Ok(())
                },
            )
            .await
    });

    cancel.notify_one();
    let error = timeout(Duration::from_secs(1), second)
        .await
        .expect("queued read should resolve after cancellation")
        .expect("queued read task should join successfully")
        .expect_err("queued read should cancel");
    assert!(matches!(error, Error::Cancelled));
    assert!(
        !started.load(Ordering::SeqCst),
        "queued read should not begin executing once canceled"
    );

    first_gate.release();
    first
        .await
        .expect("first read task should join successfully")
        .expect("first read should complete");
}

#[tokio::test]
async fn same_tenant_async_reads_can_progress_concurrently() {
    let store = Arc::new(TenantStore::create_in_memory().expect("store should open"));
    let read_storage = RedbTenantStorage::with_max_concurrent_reads(store, 2);
    let first_gate = BlockingReadGate::new();
    let first_gate_for_task = first_gate.clone();
    let first_storage = read_storage.clone();
    let first = tokio::spawn(async move {
        first_storage
            .execute(move |_store| {
                first_gate_for_task.block();
                Ok(1usize)
            })
            .await
    });

    timeout(Duration::from_secs(1), first_gate.wait_until_entered())
        .await
        .expect("first read should start");

    let second_started = Arc::new(AtomicBool::new(false));
    let second_started_for_task = second_started.clone();
    let second_storage = read_storage.clone();
    let second = tokio::spawn(async move {
        second_storage
            .execute(move |_store| {
                second_started_for_task.store(true, Ordering::SeqCst);
                Ok(2usize)
            })
            .await
    });

    let second_result = timeout(Duration::from_secs(1), second)
        .await
        .expect("second read should not wait behind the blocked first read")
        .expect("second read task should join successfully")
        .expect("second read should complete");
    assert_eq!(second_result, 2);
    assert!(
        second_started.load(Ordering::SeqCst),
        "second read should begin while the first read is still blocked"
    );

    first_gate.release();
    let first_result = first
        .await
        .expect("first read task should join successfully")
        .expect("first read should complete");
    assert_eq!(first_result, 1);
}

#[tokio::test]
async fn canceled_async_write_before_commit_leaves_no_durable_state() {
    let store = Arc::new(TenantStore::create_in_memory().expect("store should open"));
    let storage = RedbTenantStorage::with_max_concurrent_reads(store.clone(), 2);
    let document = Document::new(
        TableName::new("tasks").expect("table should build"),
        serde_json::Map::from_iter([("rank".to_string(), json!(7))]),
    );
    let indexes = vec![IndexDefinition {
        name: "by_rank".to_string(),
        field: "rank".to_string(),
    }];
    let gate = BlockingReadGate::new();
    let gate_for_task = gate.clone();
    let cancel = Arc::new(Notify::new());
    let cancel_for_wait = cancel.clone();
    let handle = tokio::spawn({
        let storage = storage.clone();
        let document = document.clone();
        async move {
            storage
                .execute_write_cancellable(
                    async move {
                        cancel_for_wait.notified().await;
                    },
                    || Ok(()),
                    move |transaction| {
                        transaction.insert_document_with_indexes(&document, &indexes)?;
                        gate_for_task.block();
                        Ok(document.id)
                    },
                )
                .await
        }
    });

    timeout(Duration::from_secs(1), gate.wait_until_entered())
        .await
        .expect("write should reach the pre-commit gate");
    cancel.notify_one();
    tokio::time::sleep(Duration::from_millis(25)).await;
    gate.release();

    let outcome = timeout(Duration::from_secs(1), handle)
        .await
        .expect("async write should resolve after cancellation")
        .expect("write task should join successfully")
        .expect("write executor should return an outcome");
    assert!(matches!(outcome, TenantWriteOutcome::CancelledBeforeCommit));
    assert!(
        store
            .get(&document.table, &document.id)
            .expect("get should succeed")
            .is_none(),
        "document should not become visible after pre-commit cancellation"
    );
    assert!(
        store
            .index_scan_eq(&document.table, "by_rank", &json!(7))
            .expect("index scan should succeed")
            .is_empty(),
        "index entries should roll back with the canceled write"
    );
    assert!(
        store
            .read_commit_log_from(SequenceNumber(1))
            .expect("commit log should read")
            .is_empty(),
        "commit log should stay empty after pre-commit cancellation"
    );
}

#[tokio::test]
async fn canceled_async_write_after_commit_still_reports_committed() {
    let clock = Arc::new(ManualClock::new(Timestamp(10_000)));
    let faults = BlockingFaultInjector::new(FaultPoint::StorageCommitAfterVisibilityBeforeReturn);
    let store = Arc::new(
        TenantStore::create_in_memory_with_simulation(clock, faults.clone())
            .expect("store should open with simulation seams"),
    );
    let storage = RedbTenantStorage::with_max_concurrent_reads(store.clone(), 2);
    let document = Document::new(
        TableName::new("tasks").expect("table should build"),
        serde_json::Map::from_iter([("rank".to_string(), json!(11))]),
    );
    let indexes = vec![IndexDefinition {
        name: "by_rank".to_string(),
        field: "rank".to_string(),
    }];
    let cancel = Arc::new(Notify::new());
    let cancel_for_wait = cancel.clone();
    let handle = tokio::spawn({
        let storage = storage.clone();
        let document = document.clone();
        async move {
            storage
                .execute_write_cancellable(
                    async move {
                        cancel_for_wait.notified().await;
                    },
                    || Ok(()),
                    move |transaction| {
                        transaction.insert_document_with_indexes(&document, &indexes)?;
                        Ok(document.id)
                    },
                )
                .await
        }
    });

    timeout(Duration::from_secs(1), faults.wait_until_entered())
        .await
        .expect("write should block after the durable commit point");
    cancel.notify_one();
    faults.release();

    let outcome = timeout(Duration::from_secs(1), handle)
        .await
        .expect("async write should resolve after post-commit cancellation")
        .expect("write task should join successfully")
        .expect("write executor should return an outcome");
    match outcome {
        TenantWriteOutcome::Committed(committed) => {
            assert_eq!(committed.value, document.id);
            assert_eq!(
                committed
                    .commit
                    .expect("committed write should append a commit entry")
                    .sequence,
                SequenceNumber(1)
            );
        }
        TenantWriteOutcome::CancelledBeforeCommit => {
            panic!("post-commit cancellation must not downgrade a committed write")
        }
    }
    assert!(
        store
            .get(&document.table, &document.id)
            .expect("get should succeed")
            .is_some(),
        "document should stay visible after post-commit cancellation"
    );
    assert_eq!(
        store
            .index_scan_eq(&document.table, "by_rank", &json!(11))
            .expect("index scan should succeed")
            .len(),
        1
    );
    assert_eq!(
        store
            .read_commit_log_from(SequenceNumber(1))
            .expect("commit log should read")
            .len(),
        1
    );
}

fn utc_unix_ms(year: i32, month: Month, day: u8) -> u64 {
    let date = Date::from_calendar_date(year, month, day).expect("calendar date should build");
    let datetime = PrimitiveDateTime::new(date, Time::MIDNIGHT).assume_utc();
    u64::try_from(datetime.unix_timestamp_nanos() / 1_000_000)
        .expect("unix milliseconds should fit in u64")
}
