use std::collections::BTreeMap;
use std::str::FromStr;

use nimbus_core::{
    Document, DocumentId, Schema, SchemaChangeEvent, TableId, TableLifecycleEvent, TableName,
    TableSchema, TenantEventKind, Timestamp, TriggerDeliveryCursor, WriteOp, WriteOpType,
};
use serde_json::json;

use super::*;
use crate::MATERIALIZED_JOURNAL_SNAPSHOT_VERSION;

fn key(rank: usize) -> LogicalLeafKey {
    LogicalLeafKey::new(LogicalLeafKind::Document, &rank.to_be_bytes())
        .expect("test key should be valid")
}

fn leaves(count: usize) -> Vec<(LogicalLeafKey, Vec<u8>)> {
    (0..count)
        .map(|rank| (key(rank), format!("value-{rank}").into_bytes()))
        .collect()
}

fn empty_snapshot() -> MaterializedJournalSnapshot {
    MaterializedJournalSnapshot {
        version: MATERIALIZED_JOURNAL_SNAPSHOT_VERSION,
        applied_sequence: SequenceNumber(0),
        durable_head: SequenceNumber(0),
        table_identities: Vec::new(),
        schema: Schema::default(),
        documents: Vec::new(),
        resource_path_bindings: Vec::new(),
        scheduled_execution_ids: Vec::new(),
        trigger_delivery_cursor: nimbus_core::TriggerDeliveryCursor::default(),
    }
}

fn inserted_document_record(sequence: u64) -> (TenantEventRecord, TableId, Document) {
    let table = TableName::new("tasks").expect("table should be valid");
    let table_id = TableId::from_str("tasks-table").expect("table id should be valid");
    let document = Document::with_id_at(
        DocumentId::from_key("task-1").expect("document id should be valid"),
        table.clone(),
        serde_json::Map::from_iter([("title".to_string(), json!("one"))]),
        Timestamp(10),
    );
    let record = TenantEventRecord::new(
        SequenceNumber(sequence),
        Timestamp(11),
        vec![WriteOp {
            table,
            table_id: table_id.clone(),
            op_type: WriteOpType::Insert,
            doc_id: document.id.clone(),
            resource_path_binding: None,
            trigger_write_origin: None,
            previous: None,
            current: Some(document.clone()),
        }],
        None,
    )
    .expect("record should be valid");
    (record, table_id, document)
}

fn snapshot_after_insert(
    record: &TenantEventRecord,
    table_id: TableId,
    document: Document,
) -> MaterializedJournalSnapshot {
    MaterializedJournalSnapshot {
        version: MATERIALIZED_JOURNAL_SNAPSHOT_VERSION,
        applied_sequence: record.sequence,
        durable_head: record.sequence,
        table_identities: vec![TableIdentitySnapshotEntry::default_namespace(
            document.table.clone(),
            table_id,
        )],
        schema: Schema::default(),
        documents: vec![document],
        resource_path_bindings: Vec::new(),
        scheduled_execution_ids: Vec::new(),
        trigger_delivery_cursor: nimbus_core::TriggerDeliveryCursor::default(),
    }
}

fn assert_local_provider_applied_delta(
    export: impl Fn() -> Result<MaterializedJournalSnapshot>,
    append: impl Fn(&[TenantEventRecord]) -> Result<()>,
    apply: impl Fn(&[TenantEventRecord]) -> Result<()>,
) {
    let (record, _, _) = inserted_document_record(1);
    let mut tracker = MaterializedVerificationTracker::from_snapshot(
        &export().expect("baseline snapshot should export"),
    )
    .expect("baseline tracker should build");
    append(std::slice::from_ref(&record)).expect("durable append should succeed");
    apply(std::slice::from_ref(&record)).expect("materialized apply should succeed");
    let MaterializedDeltaApplyOutcome::Advanced(incremental) =
        tracker.apply_applied_record(&record)
    else {
        panic!("post-apply delta should advance the tracker");
    };
    let rebuilt = MaterializedVerificationTracker::from_snapshot(
        &export().expect("post-apply snapshot should export"),
    )
    .expect("post-apply tracker should rebuild")
    .position()
    .expect("post-apply tracker should be valid");
    assert_eq!(incremental, rebuilt);
}

#[test]
fn local_provider_apply_paths_publish_only_post_apply_deltas() {
    let redb = crate::TenantStore::create_in_memory().expect("redb store should open");
    assert_local_provider_applied_delta(
        || redb.export_materialized_journal_snapshot(),
        |records| redb.append_durable_records_batch(records),
        |records| redb.apply_durable_records_batch(records),
    );

    let memory = crate::MemoryTenantStore::new();
    assert_local_provider_applied_delta(
        || memory.export_materialized_journal_snapshot(),
        |records| memory.append_durable_records_batch(records),
        |records| memory.apply_durable_records_batch(records),
    );

    let directory = tempfile::tempdir().expect("sqlite tempdir should create");
    let sqlite = crate::SqliteTenantStore::open(directory.path().join("tenant.sqlite3"))
        .expect("sqlite store should open");
    assert_local_provider_applied_delta(
        || sqlite.export_materialized_journal_snapshot(),
        |records| sqlite.append_durable_records_batch(records),
        |records| sqlite.apply_durable_records_batch(records),
    );
}

#[test]
fn snapshot_restore_invalidates_local_verification_generations() {
    let snapshot = empty_snapshot();

    let redb = crate::TenantStore::create_in_memory().expect("redb store should open");
    let redb_generation = redb.materialized_verification_generation();
    redb.restore_materialized_journal_from_snapshot(&snapshot)
        .expect("redb snapshot should restore");
    assert!(!redb.materialized_verification_generation_is_current(redb_generation));

    let memory = crate::MemoryTenantStore::new();
    let memory_generation = memory.materialized_verification_generation();
    memory
        .restore_materialized_journal_from_snapshot(&snapshot)
        .expect("memory snapshot should restore");
    assert!(!memory.materialized_verification_generation_is_current(memory_generation));

    let directory = tempfile::tempdir().expect("sqlite tempdir should create");
    let sqlite = crate::SqliteTenantStore::open(directory.path().join("tenant.sqlite3"))
        .expect("sqlite store should open");
    let sqlite_generation = sqlite.materialized_verification_generation();
    sqlite
        .restore_materialized_journal_from_snapshot(&snapshot)
        .expect("sqlite snapshot should restore");
    assert!(!sqlite.materialized_verification_generation_is_current(sqlite_generation));
}

#[test]
fn overlapping_replacements_share_one_non_current_generation() {
    let invalidator = MaterializedVerificationInvalidator::default();
    let before = invalidator.generation();
    assert!(invalidator.is_current(before));

    let update = invalidator
        .begin_update()
        .expect("first replacement should begin");
    let during = invalidator.generation();
    assert!(!invalidator.is_current(before));
    assert!(!invalidator.is_current(during));
    let overlapping = invalidator
        .begin_update()
        .expect("an overlapping replacement should join the invalidation epoch");

    drop(update);
    assert_eq!(invalidator.generation(), during);
    assert!(!invalidator.is_current(during));

    drop(overlapping);
    let after = invalidator.generation();
    assert!(invalidator.is_current(after));
    assert!(!invalidator.is_current(before));
    assert!(!invalidator.is_current(during));
}

#[test]
fn schema_scheduler_and_lifecycle_records_have_safe_verification_effects() {
    let store = crate::TenantStore::create_in_memory().expect("store should open");
    let mut tracker = MaterializedVerificationTracker::from_snapshot(
        &store
            .export_materialized_journal_snapshot()
            .expect("baseline snapshot should export"),
    )
    .expect("baseline tracker should build");
    let table = TableName::new("tasks").expect("table should be valid");
    let table_id = TableId::from_str("tasks-table").expect("table id should be valid");
    let schema = TableSchema {
        table: table.clone(),
        fields: Vec::new(),
        indexes: Vec::new(),
        access_policy: None,
    };
    let deleted_schema = schema.clone();
    let schema_record = TenantEventRecord::schema_change(
        SequenceNumber(1),
        Timestamp(1),
        SchemaChangeEvent::SetTable {
            table: table.clone(),
            table_id: table_id.clone(),
            previous: None,
            current: schema,
        },
    )
    .expect("schema record should build");
    let scheduled_record = TenantEventRecord::from_events(
        SequenceNumber(2),
        Timestamp(2),
        vec![TenantEventKind::ScheduledExecution {
            execution_id: "execution-1".to_string(),
        }],
    )
    .expect("scheduled record should build");
    let schema_delete_record = TenantEventRecord::schema_change(
        SequenceNumber(3),
        Timestamp(3),
        SchemaChangeEvent::DeleteTable {
            table: table.clone(),
            table_id: Some(table_id),
            previous: Some(deleted_schema),
        },
    )
    .expect("schema delete record should build");

    for record in [&schema_record, &scheduled_record, &schema_delete_record] {
        store
            .append_durable_records_batch(std::slice::from_ref(record))
            .expect("record should append");
        store
            .apply_durable_records_batch(std::slice::from_ref(record))
            .expect("record should apply");
        let MaterializedDeltaApplyOutcome::Advanced(incremental) =
            tracker.apply_applied_record(record)
        else {
            panic!("exact record should advance");
        };
        let rebuilt = MaterializedVerificationTracker::from_snapshot(
            &store
                .export_materialized_journal_snapshot()
                .expect("snapshot should export"),
        )
        .expect("snapshot tracker should rebuild")
        .position()
        .expect("rebuilt tracker should be valid");
        assert_eq!(incremental, rebuilt);
    }

    let lifecycle_record = TenantEventRecord::table_lifecycle(
        SequenceNumber(4),
        Timestamp(4),
        TableLifecycleEvent::StageHidden {
            table: TableName::new("replacement").expect("table should be valid"),
            table_id: TableId::from_str("replacement-table").expect("table id should be valid"),
        },
    )
    .expect("lifecycle record should build");
    store
        .append_durable_records_batch(std::slice::from_ref(&lifecycle_record))
        .expect("lifecycle record should append");
    store
        .apply_durable_records_batch(std::slice::from_ref(&lifecycle_record))
        .expect("lifecycle record should apply");
    assert_eq!(
        tracker.apply_applied_record(&lifecycle_record),
        MaterializedDeltaApplyOutcome::Invalidated
    );
    assert!(!tracker.is_valid());
}

#[test]
fn document_insert_update_delete_deltas_match_full_rebuilds() {
    let store = crate::TenantStore::create_in_memory().expect("store should open");
    let mut tracker = MaterializedVerificationTracker::from_snapshot(
        &store
            .export_materialized_journal_snapshot()
            .expect("baseline snapshot should export"),
    )
    .expect("baseline tracker should build");
    let (insert, table_id, original) = inserted_document_record(1);
    let mut updated = original.clone();
    updated.update_time = Timestamp(20);
    updated.fields.insert("title".to_string(), json!("two"));
    let update = TenantEventRecord::new(
        SequenceNumber(2),
        Timestamp(21),
        vec![WriteOp {
            table: original.table.clone(),
            table_id: table_id.clone(),
            op_type: WriteOpType::Update,
            doc_id: original.id.clone(),
            resource_path_binding: None,
            trigger_write_origin: None,
            previous: Some(original),
            current: Some(updated.clone()),
        }],
        None,
    )
    .expect("update record should build");
    let delete = TenantEventRecord::new(
        SequenceNumber(3),
        Timestamp(22),
        vec![WriteOp {
            table: updated.table.clone(),
            table_id,
            op_type: WriteOpType::Delete,
            doc_id: updated.id.clone(),
            resource_path_binding: None,
            trigger_write_origin: None,
            previous: Some(updated),
            current: None,
        }],
        None,
    )
    .expect("delete record should build");

    for record in [insert, update, delete] {
        store
            .append_durable_records_batch(std::slice::from_ref(&record))
            .expect("record should append");
        store
            .apply_durable_records_batch(std::slice::from_ref(&record))
            .expect("record should apply");
        assert!(matches!(
            tracker.apply_applied_record(&record),
            MaterializedDeltaApplyOutcome::Advanced(_)
        ));
        let rebuilt = MaterializedVerificationTracker::from_snapshot(
            &store
                .export_materialized_journal_snapshot()
                .expect("snapshot should export"),
        )
        .expect("snapshot tracker should rebuild");
        assert_eq!(tracker.position(), rebuilt.position());
    }
}

#[test]
fn resource_binding_and_trigger_cursor_deltas_match_full_rebuilds() {
    let store = crate::MemoryTenantStore::new();
    let mut tracker = MaterializedVerificationTracker::from_snapshot(
        &store
            .export_materialized_journal_snapshot()
            .expect("baseline snapshot should export"),
    )
    .expect("baseline tracker should build");
    let table = TableName::new("tasks").expect("table should be valid");
    let table_id = TableId::from_str("tasks-table").expect("table id should be valid");
    let document = Document::with_id_at(
        DocumentId::from_key("task-1").expect("document id should be valid"),
        table.clone(),
        serde_json::Map::from_iter([("title".to_string(), json!("one"))]),
        Timestamp(10),
    );
    let binding = nimbus_core::ResourcePathBinding::new(
        nimbus_core::DocumentLocator::new(table.clone(), document.id.clone()),
        nimbus_core::DocumentPath::from_segments(["tasks", document.id.as_str()])
            .expect("resource path should be valid"),
    );
    let insert = TenantEventRecord::new(
        SequenceNumber(1),
        Timestamp(11),
        vec![WriteOp {
            table: table.clone(),
            table_id: table_id.clone(),
            op_type: WriteOpType::Insert,
            doc_id: document.id.clone(),
            resource_path_binding: Some(binding.clone()),
            trigger_write_origin: None,
            previous: None,
            current: Some(document.clone()),
        }],
        None,
    )
    .expect("insert record should build");
    let trigger = TenantEventRecord::from_events(
        SequenceNumber(2),
        Timestamp(12),
        vec![TenantEventKind::TriggerDelivery {
            cursor: TriggerDeliveryCursor::new(SequenceNumber(1)),
        }],
    )
    .expect("trigger record should build");
    let delete = TenantEventRecord::new(
        SequenceNumber(3),
        Timestamp(13),
        vec![WriteOp {
            table,
            table_id,
            op_type: WriteOpType::Delete,
            doc_id: document.id.clone(),
            resource_path_binding: Some(binding),
            trigger_write_origin: None,
            previous: Some(document),
            current: None,
        }],
        None,
    )
    .expect("delete record should build");

    for record in [insert, trigger, delete] {
        store
            .append_durable_records_batch(std::slice::from_ref(&record))
            .expect("record should append");
        store
            .apply_durable_records_batch(std::slice::from_ref(&record))
            .expect("record should apply");
        assert!(matches!(
            tracker.apply_applied_record(&record),
            MaterializedDeltaApplyOutcome::Advanced(_)
        ));
        let rebuilt = MaterializedVerificationTracker::from_snapshot(
            &store
                .export_materialized_journal_snapshot()
                .expect("snapshot should export"),
        )
        .expect("snapshot tracker should rebuild");
        assert_eq!(tracker.position(), rebuilt.position());
    }
}

#[test]
fn hidden_lineage_document_write_matches_provider_activation() {
    let store = crate::TenantStore::create_in_memory().expect("store should open");
    let (initial_record, initial_table_id, _) = inserted_document_record(1);
    store
        .append_durable_records_batch(std::slice::from_ref(&initial_record))
        .expect("initial write should append");
    store
        .apply_durable_records_batch(std::slice::from_ref(&initial_record))
        .expect("initial write should apply");
    let table = TableName::new("tasks").expect("table should be valid");
    let hidden_id = TableId::from_str("hidden-table").expect("table id should be valid");
    store
        .stage_hidden_table_identity(&table, &hidden_id)
        .expect("hidden identity should stage");
    let checkpoint = store
        .export_materialized_journal_snapshot()
        .expect("checkpoint should export");
    let hidden_document = Document::with_id_at(
        DocumentId::from_key("hidden-task").expect("document id should be valid"),
        table.clone(),
        serde_json::Map::from_iter([("title".to_string(), json!("hidden"))]),
        Timestamp(10),
    );
    let record = TenantEventRecord::new(
        SequenceNumber(checkpoint.applied_sequence.0 + 1),
        Timestamp(11),
        vec![WriteOp {
            table,
            table_id: hidden_id.clone(),
            op_type: WriteOpType::Insert,
            doc_id: hidden_document.id.clone(),
            resource_path_binding: None,
            trigger_write_origin: None,
            previous: None,
            current: Some(hidden_document),
        }],
        None,
    )
    .expect("hidden write record should build");
    store
        .append_durable_records_batch(std::slice::from_ref(&record))
        .expect("hidden write should append");
    store
        .apply_durable_records_batch(std::slice::from_ref(&record))
        .expect("hidden write should apply");
    let expected = store
        .export_materialized_journal_snapshot()
        .expect("applied snapshot should export");
    assert!(
        expected
            .table_identities
            .iter()
            .any(|identity| { identity.table_id == hidden_id && identity.is_active() })
    );
    assert!(expected.table_identities.iter().any(|identity| {
        identity.table_id == initial_table_id && identity.state == nimbus_core::TableState::Deleting
    }));
    assert!(!expected.table_identities.iter().any(|identity| {
        identity.table_id == hidden_id && identity.state == nimbus_core::TableState::Hidden
    }));
    assert_eq!(expected.documents.len(), 2);

    let mut tracker = MaterializedVerificationTracker::from_snapshot(&checkpoint)
        .expect("checkpoint tracker should build");
    assert!(matches!(
        tracker.apply_applied_record(&record),
        MaterializedDeltaApplyOutcome::Advanced(_)
    ));
    let rebuilt = MaterializedVerificationTracker::from_snapshot(&expected)
        .expect("expected tracker should build");
    assert_eq!(tracker.position(), rebuilt.position());
}

#[test]
fn root_advances_with_applied_sequence() {
    let (record, table_id, document) = inserted_document_record(1);
    let expected = MaterializedVerificationTracker::from_snapshot(&snapshot_after_insert(
        &record, table_id, document,
    ))
    .expect("post-apply snapshot should build")
    .position()
    .expect("post-apply tracker should be valid");
    let mut tracker = MaterializedVerificationTracker::from_snapshot(&empty_snapshot())
        .expect("empty tracker should build");

    let MaterializedDeltaApplyOutcome::Advanced(actual) = tracker.apply_applied_record(&record)
    else {
        panic!("contiguous applied record should advance the tracker");
    };
    assert_eq!(actual.applied_sequence(), SequenceNumber(1));
    assert_eq!(actual.root_hash(), expected.root_hash());
}

#[test]
fn failed_apply_does_not_advance_root() {
    let (record, _, _) = inserted_document_record(2);
    let store = crate::TenantStore::create_in_memory().expect("store should open");
    let tracker = MaterializedVerificationTracker::from_snapshot(
        &store
            .export_materialized_journal_snapshot()
            .expect("snapshot should export"),
    )
    .expect("tracker should build");
    let before = tracker.position().expect("tracker should be valid");

    assert!(store.apply_durable_records_batch(&[record]).is_err());
    assert_eq!(tracker.position(), Some(before));
}

#[test]
fn replay_duplicate_keeps_root() {
    let (record, _, _) = inserted_document_record(1);
    let mut tracker = MaterializedVerificationTracker::from_snapshot(&empty_snapshot())
        .expect("tracker should build");
    let MaterializedDeltaApplyOutcome::Advanced(first) = tracker.apply_applied_record(&record)
    else {
        panic!("first record should advance");
    };
    assert_eq!(
        tracker.apply_applied_record(&record),
        MaterializedDeltaApplyOutcome::Duplicate(first)
    );
    assert_eq!(tracker.position(), Some(first));
}

#[test]
fn corrupt_index_never_reports_success() {
    let (record, _, _) = inserted_document_record(1);
    let mut tracker = MaterializedVerificationTracker::from_snapshot(&empty_snapshot())
        .expect("tracker should build");
    assert!(matches!(
        tracker.apply_applied_record(&record),
        MaterializedDeltaApplyOutcome::Advanced(_)
    ));
    let mut corrupt = record;
    corrupt.integrity_sha256[0] ^= 0xff;

    assert_eq!(
        tracker.apply_applied_record(&corrupt),
        MaterializedDeltaApplyOutcome::Invalidated
    );
    assert!(!tracker.is_valid());
    assert_eq!(tracker.position(), None);
}

#[test]
fn full_scrub_detects_state_tamper_at_same_sequence() {
    let (record, _, document) = inserted_document_record(1);
    let store = crate::MemoryTenantStore::new();
    store
        .append_durable_records_batch(std::slice::from_ref(&record))
        .expect("record should append");
    store
        .apply_durable_records_batch(std::slice::from_ref(&record))
        .expect("record should apply");

    let expected_snapshot = store
        .export_materialized_journal_snapshot()
        .expect("expected snapshot should export");
    let expected = MaterializedVerificationTracker::from_snapshot(&expected_snapshot)
        .expect("expected tracker should build")
        .position()
        .expect("expected tracker should be valid");

    let mut tampered = document;
    tampered
        .fields
        .insert("title".to_string(), serde_json::json!("tampered"));
    store
        .tamper_document_for_testing(tampered)
        .expect("test state should tamper");

    let actual_snapshot = store
        .export_materialized_journal_snapshot()
        .expect("tampered snapshot should export");
    let actual = MaterializedVerificationTracker::from_snapshot(&actual_snapshot)
        .expect("tampered tracker should build")
        .position()
        .expect("tampered tracker should be valid");

    assert_eq!(actual.applied_sequence(), expected.applied_sequence());
    assert_ne!(actual.root_hash(), expected.root_hash());
}

#[test]
fn full_verification_root_covers_bindings_and_trigger_cursor() {
    let baseline = empty_snapshot();
    let baseline_position = MaterializedVerificationTracker::from_snapshot(&baseline)
        .expect("baseline tracker should build")
        .position()
        .expect("baseline tracker should have a position");

    let table = TableName::new("tasks").expect("table should be valid");
    let id = DocumentId::from_key("task-1").expect("document id should be valid");
    let mut with_binding = baseline.clone();
    with_binding.resource_path_bindings = vec![nimbus_core::ResourcePathBinding::new(
        nimbus_core::DocumentLocator::new(table, id.clone()),
        nimbus_core::DocumentPath::from_segments(["tasks", id.as_str()])
            .expect("resource path should be valid"),
    )];
    assert_ne!(
        MaterializedVerificationTracker::from_snapshot(&with_binding)
            .expect("binding tracker should build")
            .position()
            .expect("binding tracker should have a position"),
        baseline_position,
        "resource bindings must contribute verification leaves"
    );

    let mut with_cursor = baseline;
    with_cursor.trigger_delivery_cursor =
        nimbus_core::TriggerDeliveryCursor::new(SequenceNumber(1));
    assert_ne!(
        MaterializedVerificationTracker::from_snapshot(&with_cursor)
            .expect("cursor tracker should build")
            .position()
            .expect("cursor tracker should have a position"),
        baseline_position,
        "the trigger cursor must contribute a verification leaf"
    );
}

#[test]
fn apply_gap_invalidates_verification_index() {
    let (record, _, _) = inserted_document_record(2);
    let mut tracker = MaterializedVerificationTracker::from_snapshot(&empty_snapshot())
        .expect("tracker should build");

    assert_eq!(
        tracker.apply_applied_record(&record),
        MaterializedDeltaApplyOutcome::Invalidated
    );
    assert!(!tracker.is_valid());
    assert_eq!(tracker.position(), None);
}

#[test]
fn durable_head_ahead_of_apply_does_not_advance_verification_root() {
    let (record, _, _) = inserted_document_record(1);
    let store = crate::TenantStore::create_in_memory().expect("store should open");
    let tracker = MaterializedVerificationTracker::from_snapshot(
        &store
            .export_materialized_journal_snapshot()
            .expect("snapshot should export"),
    )
    .expect("tracker should build");
    let before = tracker.position().expect("tracker should be valid");

    store
        .append_durable_records_batch(&[record])
        .expect("durable append should succeed");
    let progress = store.journal_progress().expect("progress should load");
    assert_eq!(progress.durable_head, SequenceNumber(1));
    assert_eq!(progress.applied_head, SequenceNumber(0));
    assert_eq!(tracker.position(), Some(before));
}

fn adversarial_chain_keys(required: usize) -> Vec<LogicalLeafKey> {
    let candidates = (0..20_000_u64)
        .map(|rank| {
            let mut raw = [0; HASH_BYTES];
            raw[HASH_BYTES - 8..].copy_from_slice(&rank.to_be_bytes());
            LogicalLeafKey(raw)
        })
        .collect::<Vec<_>>();
    let priorities = candidates
        .iter()
        .map(|key| {
            hash_parts(
                VerificationRootVersion::current(),
                PRIORITY_DOMAIN,
                &[key.as_bytes()],
            )
        })
        .collect::<Vec<_>>();
    let mut tails = Vec::<Hash>::new();
    let mut tail_indices = Vec::<usize>::new();
    let mut previous = vec![None; candidates.len()];
    for (index, priority) in priorities.iter().copied().enumerate() {
        let position = tails.partition_point(|tail| tail < &priority);
        if position > 0 {
            previous[index] = Some(tail_indices[position - 1]);
        }
        if position == tails.len() {
            tails.push(priority);
            tail_indices.push(index);
        } else {
            tails[position] = priority;
            tail_indices[position] = index;
        }
    }
    assert!(
        tails.len() >= required,
        "candidate corpus must contain a chain"
    );
    let mut cursor = Some(tail_indices[required - 1]);
    let mut selected = Vec::with_capacity(required);
    while let Some(index) = cursor {
        selected.push(candidates[index]);
        cursor = previous[index];
    }
    selected.reverse();
    selected
}

#[test]
fn batch_and_incremental_verification_roots_match() {
    let leaves = leaves(256);
    let batch = MaterializedVerificationIndex::from_leaves(leaves.clone())
        .expect("batch root should build");
    let mut incremental = MaterializedVerificationIndex::new();
    for (key, value) in &leaves {
        assert!(
            incremental
                .upsert(*key, value)
                .expect("incremental insert should succeed")
        );
    }
    assert_eq!(batch.root_hash(), incremental.root_hash());

    for rank in (0..256).step_by(5) {
        incremental
            .upsert(key(rank), format!("updated-{rank}").as_bytes())
            .expect("incremental update should succeed");
    }
    for rank in (0..256).step_by(7) {
        incremental.remove(&key(rank));
    }
    let rebuilt = MaterializedVerificationIndex::from_leaves(
        (0..256).filter(|rank| rank % 7 != 0).map(|rank| {
            let value = if rank % 5 == 0 {
                format!("updated-{rank}")
            } else {
                format!("value-{rank}")
            };
            (key(rank), value.into_bytes())
        }),
    )
    .expect("post-change root should rebuild");
    assert_eq!(rebuilt.root_hash(), incremental.root_hash());
}

#[test]
fn verification_root_is_independent_of_update_order() {
    let leaves = leaves(512);
    let expected = MaterializedVerificationIndex::from_leaves(leaves.clone())
        .expect("reference root should build")
        .root_hash();
    let mut order = (0..leaves.len()).collect::<Vec<_>>();
    let mut seed = 0x71d4_60a5_d9c8_2e13_u64;
    for index in (1..order.len()).rev() {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        order.swap(index, seed as usize % (index + 1));
    }
    let mut random = MaterializedVerificationIndex::new();
    for index in order {
        random
            .upsert(leaves[index].0, &leaves[index].1)
            .expect("random insert should succeed");
    }
    let reverse = MaterializedVerificationIndex::from_leaves(leaves.into_iter().rev())
        .expect("reverse root should build");
    assert_eq!(random.root_hash(), expected);
    assert_eq!(reverse.root_hash(), expected);
}

#[test]
fn delete_then_reinsert_restores_root() {
    let mut index =
        MaterializedVerificationIndex::from_leaves(leaves(128)).expect("root should build");
    let before = index.root_hash();
    let removed_key = key(47);
    assert!(index.remove(&removed_key));
    assert_ne!(index.root_hash(), before);
    assert!(
        index
            .upsert(removed_key, b"value-47")
            .expect("reinsert should succeed")
    );
    assert_eq!(index.root_hash(), before);
}

#[test]
fn verification_root_version_separates_formats() {
    let leaves = leaves(32);
    let current = MaterializedVerificationIndex::from_leaves_with_version(
        VerificationRootVersion::current(),
        leaves.clone(),
    )
    .expect("current root should build");
    let future = MaterializedVerificationIndex::from_leaves_with_version(
        VerificationRootVersion(MATERIALIZED_VERIFICATION_ROOT_VERSION + 1),
        leaves,
    )
    .expect("test-only future root should build");
    assert_ne!(current.root_hash(), future.root_hash());
    assert!(
        VerificationPosition::from_parts(
            MATERIALIZED_VERIFICATION_ROOT_VERSION + 1,
            SequenceNumber(1),
            future.root_hash(),
        )
        .is_err()
    );
}

#[test]
fn duplicate_batch_keys_are_rejected() {
    let duplicate = key(1);
    assert!(
        MaterializedVerificationIndex::from_leaves([
            (duplicate, b"first".as_slice()),
            (duplicate, b"second".as_slice()),
        ])
        .is_err()
    );
}

#[test]
fn memory_derivation_stays_inside_the_approved_limit() {
    assert_eq!(VERIFICATION_INDEX_NODE_BYTES, 148);
    assert_eq!(VERIFICATION_INDEX_BUDGETED_BYTES_PER_LEAF, 164);
}

#[test]
fn generated_operation_histories_match_full_rebuilds() {
    for history_seed in 1..=16_u64 {
        let mut seed = history_seed;
        let mut model = BTreeMap::<usize, Vec<u8>>::new();
        let mut index = MaterializedVerificationIndex::new();
        for step in 0..500 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let rank = (seed as usize) % 256;
            if seed.rotate_left(17) % 4 == 0 {
                assert_eq!(index.remove(&key(rank)), model.remove(&rank).is_some());
            } else {
                let value = format!("seed-{history_seed}-step-{step}").into_bytes();
                let inserted = index
                    .upsert(key(rank), &value)
                    .expect("generated upsert should succeed");
                assert_eq!(inserted, model.insert(rank, value).is_none());
            }

            if step % 50 == 0 {
                let rebuilt = MaterializedVerificationIndex::from_leaves(
                    model.iter().map(|(rank, value)| (key(*rank), value)),
                )
                .expect("generated model should rebuild");
                assert_eq!(
                    index.root_hash(),
                    rebuilt.root_hash(),
                    "history seed {history_seed} diverged at step {step}"
                );
                assert_eq!(index.len(), model.len());
            }
        }
    }
}

#[test]
fn logical_leaf_families_and_positions_are_opaque() {
    let identity = b"tasks/example";
    let document = LogicalLeafKey::new(LogicalLeafKind::Document, identity)
        .expect("document identity should be valid");
    let schema = LogicalLeafKey::new(LogicalLeafKind::Schema, identity)
        .expect("schema identity should be valid");
    assert_ne!(document, schema);
    assert!(LogicalLeafKey::new(LogicalLeafKind::Document, b"").is_err());

    let mut index = MaterializedVerificationIndex::new();
    index
        .upsert(document, b"canonical value")
        .expect("position leaf should insert");
    let position = index.position(SequenceNumber(41));
    assert_eq!(position.version(), VerificationRootVersion::current());
    assert_eq!(position.applied_sequence(), SequenceNumber(41));
    assert_eq!(position.root_hash(), &index.root_hash());
}

#[test]
fn adversarial_depth_is_rejected_and_incremental_insert_rolls_back() {
    let keys = adversarial_chain_keys(VERIFICATION_INDEX_MAX_DEPTH + 1);
    assert!(matches!(
        MaterializedVerificationIndex::from_leaves(keys.iter().copied().map(|key| (key, b"value"))),
        Err(Error::ResourceExhausted(_))
    ));

    let mut index = MaterializedVerificationIndex::new();
    for key in keys.iter().take(VERIFICATION_INDEX_MAX_DEPTH) {
        index
            .upsert(*key, b"value")
            .expect("a tree at the safety limit should build");
    }
    assert_eq!(index.max_depth(), VERIFICATION_INDEX_MAX_DEPTH);
    let root_before = index.root_hash();
    let len_before = index.len();
    assert!(matches!(
        index.upsert(keys[VERIFICATION_INDEX_MAX_DEPTH], b"value"),
        Err(Error::ResourceExhausted(_))
    ));
    assert_eq!(index.root_hash(), root_before);
    assert_eq!(index.len(), len_before);
    assert_eq!(index.max_depth(), VERIFICATION_INDEX_MAX_DEPTH);
}
