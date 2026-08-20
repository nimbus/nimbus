use std::sync::Arc;

use nimbus_core::{
    Error, FieldSchema, FieldType, IndexDefinition, ManualWallClock, SequenceNumber, TableId,
    TableName, TableSchema, TableState, TenantEventRecord, Timestamp, WriteOp, WriteOpType,
};
use serde_json::json;

use crate::{
    MaterializedJournalSnapshot, NoopFaultInjector, PointInTimeRestoreTarget, RetentionGcConfig,
    TableIdentitySnapshotEntry, TenantStore,
};

fn tasks_schema() -> TableSchema {
    TableSchema {
        table: TableName::new("tasks").expect("table name should be valid"),
        fields: vec![FieldSchema {
            name: "rank".to_string(),
            field_type: FieldType::Number,
            required: true,
        }],
        indexes: vec![IndexDefinition {
            id: nimbus_core::IndexId::new(),
            state: nimbus_core::IndexState::Enabled,
            name: "by_rank".to_string(),
            fields: vec!["rank".to_string()],
        }],
        access_policy: None,
    }
}

#[test]
fn materialized_snapshot_rejects_lifecycle_namespace_state_mismatch() {
    let table = TableName::new("tasks").expect("table name should parse");
    let table_id = TableId::new();
    let snapshot = MaterializedJournalSnapshot {
        version: crate::store::MATERIALIZED_JOURNAL_SNAPSHOT_VERSION,
        applied_sequence: SequenceNumber(0),
        durable_head: SequenceNumber(0),
        table_identities: vec![TableIdentitySnapshotEntry {
            namespace: crate::table_identity::hidden_table_namespace(&table_id),
            table,
            table_id,
            state: TableState::Active,
        }],
        schema: nimbus_core::Schema::default(),
        documents: Vec::new(),
        scheduled_execution_ids: Vec::new(),
    };

    let error = snapshot
        .validate()
        .expect_err("active identity in hidden namespace should be rejected");
    assert!(matches!(
        error,
        Error::InvalidInput(message)
            if message.contains("active state requires default")
    ));
}

#[test]
fn materialized_snapshot_plus_journal_tail_rebuild_matches_live_state() {
    let live = TenantStore::create_in_memory().expect("store should open");
    let table_schema = tasks_schema();
    let table = table_schema.table.clone();
    live.replace_table_schema(&table_schema)
        .expect("table schema should persist");

    let first = nimbus_core::Document::new(
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
    assert_eq!(
        snapshot.version,
        crate::store::MATERIALIZED_JOURNAL_SNAPSHOT_VERSION
    );
    assert_eq!(snapshot.applied_sequence, SequenceNumber(2));
    assert_eq!(snapshot.durable_head, SequenceNumber(2));

    let second = nimbus_core::Document::new(
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
            .export_materialized_journal_snapshot()
            .expect("rebuilt snapshot should export")
            .table_identities,
        live.export_materialized_journal_snapshot()
            .expect("live snapshot should export")
            .table_identities,
        "snapshot restore/rebuild must preserve stable table identities"
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
    let table_schema = tasks_schema();
    let table = table_schema.table.clone();
    live.replace_table_schema(&table_schema)
        .expect("table schema should persist");

    let first = nimbus_core::Document::new(
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
    assert_eq!(
        snapshot.version,
        crate::store::MATERIALIZED_JOURNAL_SNAPSHOT_VERSION
    );
    assert_eq!(snapshot.durable_head, SequenceNumber(2));

    let second = nimbus_core::Document::new(
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
        .rebuild_materialized_journal_from_snapshot(&snapshot, &tail, Some(SequenceNumber(3)))
        .expect("point-in-time rebuild should succeed");

    assert_eq!(
        progress,
        super::super::JournalProgress {
            durable_head: SequenceNumber(3),
            applied_head: SequenceNumber(3),
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
fn point_in_time_archive_restores_sequence_and_timestamp_to_matching_positions() {
    let clock = Arc::new(ManualWallClock::new(Timestamp(1_000)));
    let live =
        TenantStore::create_in_memory_with_simulation(clock.clone(), Arc::new(NoopFaultInjector))
            .expect("store should open");
    let table_schema = tasks_schema();
    let table = table_schema.table.clone();
    live.replace_table_schema(&table_schema)
        .expect("table schema should persist");

    clock.set(Timestamp(1_100));
    let first = nimbus_core::Document::new(
        table.clone(),
        serde_json::Map::from_iter([
            ("title".to_string(), json!("First")),
            ("rank".to_string(), json!(1)),
        ]),
    );
    live.insert_with_indexes(&first, &table_schema.indexes)
        .expect("first insert should succeed");

    clock.set(Timestamp(1_200));
    let second = nimbus_core::Document::new(
        table.clone(),
        serde_json::Map::from_iter([
            ("title".to_string(), json!("Second")),
            ("rank".to_string(), json!(3)),
        ]),
    );
    let second_commit = live
        .insert_with_indexes(&second, &table_schema.indexes)
        .expect("second insert should succeed");

    clock.set(Timestamp(1_300));
    live.update_with_indexes(
        &table,
        &first.id,
        &serde_json::Map::from_iter([("rank".to_string(), json!(2))]),
        &table_schema.indexes,
    )
    .expect("update should succeed");

    let sequence_archive = live
        .export_point_in_time_restore_archive(
            PointInTimeRestoreTarget::Sequence(second_commit.sequence),
            RetentionGcConfig::retain_all(),
        )
        .expect("sequence archive should export");
    let timestamp_archive = live
        .export_point_in_time_restore_archive(
            PointInTimeRestoreTarget::Timestamp(Timestamp(1_250)),
            RetentionGcConfig::retain_all(),
        )
        .expect("timestamp archive should export");

    assert_eq!(sequence_archive.target_sequence, second_commit.sequence);
    assert_eq!(timestamp_archive.target_sequence, second_commit.sequence);
    assert_eq!(
        sequence_archive.target_position,
        timestamp_archive.target_position
    );
    assert_eq!(
        sequence_archive.storage_format_version,
        crate::CURRENT_STORAGE_FORMAT_VERSION
    );
    assert_eq!(
        sequence_archive.document_version_storage_format,
        crate::CURRENT_DOCUMENT_VERSION_STORAGE_FORMAT
    );
    assert_eq!(
        sequence_archive.index_version_storage_format,
        crate::CURRENT_INDEX_VERSION_STORAGE_FORMAT
    );

    let restored_from_sequence =
        TenantStore::create_in_memory().expect("sequence restore store should open");
    let sequence_progress = restored_from_sequence
        .import_point_in_time_restore_archive(&sequence_archive)
        .expect("sequence point-in-time archive should import");
    assert_eq!(sequence_progress.durable_head, second_commit.sequence);
    assert_eq!(sequence_progress.applied_head, second_commit.sequence);
    let sequence_snapshot = restored_from_sequence
        .export_materialized_journal_snapshot()
        .expect("sequence restored snapshot should export");
    assert_eq!(
        sequence_snapshot
            .materialized_position()
            .expect("sequence restored position should compute"),
        sequence_archive.target_position
    );

    let restored_from_timestamp =
        TenantStore::create_in_memory().expect("timestamp restore store should open");
    let timestamp_progress = restored_from_timestamp
        .import_point_in_time_restore_archive(&timestamp_archive)
        .expect("timestamp point-in-time archive should import");
    assert_eq!(timestamp_progress, sequence_progress);
    let timestamp_snapshot = restored_from_timestamp
        .export_materialized_journal_snapshot()
        .expect("timestamp restored snapshot should export");
    assert_eq!(
        timestamp_snapshot
            .materialized_position()
            .expect("timestamp restored position should compute"),
        sequence_archive.target_position
    );
    assert_eq!(
        timestamp_snapshot.table_identities,
        sequence_snapshot.table_identities
    );

    let documents = restored_from_sequence
        .scan_table(&table)
        .expect("restored scan should succeed");
    assert_eq!(documents.len(), 2);
    let restored_first = documents
        .iter()
        .find(|document| document.id == first.id)
        .expect("first document should be present at target");
    assert_eq!(restored_first.fields.get("rank"), Some(&json!(1)));
    assert_eq!(
        restored_from_sequence
            .index_scan_eq(&table, "by_rank", &json!(2))
            .expect("rank 2 index scan should succeed")
            .len(),
        0
    );
    assert_eq!(
        restored_from_sequence
            .index_scan_eq(&table, "by_rank", &json!(3))
            .expect("rank 3 index scan should succeed")
            .len(),
        1
    );
}

#[test]
fn point_in_time_archive_rejects_expired_retention_target() {
    let live = TenantStore::create_in_memory().expect("store should open");
    let table_schema = tasks_schema();
    let table = table_schema.table.clone();
    live.replace_table_schema(&table_schema)
        .expect("table schema should persist");
    let first = nimbus_core::Document::new(
        table.clone(),
        serde_json::Map::from_iter([
            ("title".to_string(), json!("First")),
            ("rank".to_string(), json!(1)),
        ]),
    );
    live.insert_with_indexes(&first, &table_schema.indexes)
        .expect("first insert should succeed");
    let second = nimbus_core::Document::new(
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

    let error = live
        .export_point_in_time_restore_archive(
            PointInTimeRestoreTarget::Sequence(SequenceNumber(1)),
            RetentionGcConfig::new(1).expect("config should build"),
        )
        .expect_err("expired point-in-time target should be rejected");
    assert!(matches!(
        error,
        Error::HistoricalRead {
            kind: nimbus_core::HistoricalReadErrorKind::RetentionExpired,
            ..
        }
    ));
}

#[test]
fn materialized_snapshot_records_durable_boundary_and_rejects_incomplete_tail() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let document = nimbus_core::Document::new(
        TableName::new("tasks").expect("table name should be valid"),
        serde_json::Map::from_iter([("title".to_string(), json!("durable-only"))]),
    );
    let record = TenantEventRecord::new(
        SequenceNumber(1),
        Timestamp(100),
        vec![WriteOp {
            table: document.table.clone(),
            table_id: TableId::new(),
            op_type: WriteOpType::Insert,
            doc_id: document.id.clone(),
            resource_path_binding: None,
            trigger_write_origin: None,
            previous: None,
            current: Some(document.clone()),
        }],
        None,
    )
    .expect("durable record should build");
    store
        .append_durable_records_batch(std::slice::from_ref(&record))
        .expect("durable append should succeed");

    let snapshot = store
        .export_materialized_journal_snapshot()
        .expect("snapshot export should succeed");
    assert_eq!(
        snapshot.version,
        crate::store::MATERIALIZED_JOURNAL_SNAPSHOT_VERSION
    );
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
        super::super::JournalProgress {
            durable_head: SequenceNumber(1),
            applied_head: SequenceNumber(1),
        }
    );
    let documents = rebuilt
        .scan_table(&document.table)
        .expect("rebuilt scan should succeed");
    assert_eq!(documents.len(), 1);
}

#[test]
fn journal_replay_base_validator_accepts_empty_and_rejects_each_populated_field() {
    // The all-empty sequence-0 base is the valid journal-replay import target.
    // Inverting the documents clause rejects this empty base, so this assertion
    // directly pins that mutation.
    let empty = MaterializedJournalSnapshot::empty_for_point_in_time_base();
    super::validate_materialized_journal_replay_base_is_empty(&empty)
        .expect("an all-empty sequence-0 base must be accepted for journal-replay import");

    let table = TableName::new("tasks").expect("table name should be valid");
    let table_id = TableId::new();

    // Each field, populated in isolation from the empty base, must be rejected.
    // This pins every clause of the validator individually.
    let mut nonzero_applied = empty.clone();
    nonzero_applied.applied_sequence = SequenceNumber(1);
    assert!(
        matches!(
            super::validate_materialized_journal_replay_base_is_empty(&nonzero_applied),
            Err(Error::InvalidInput(_))
        ),
        "a nonzero applied_sequence must be rejected"
    );

    let mut nonzero_durable = empty.clone();
    nonzero_durable.durable_head = SequenceNumber(1);
    assert!(
        matches!(
            super::validate_materialized_journal_replay_base_is_empty(&nonzero_durable),
            Err(Error::InvalidInput(_))
        ),
        "a nonzero durable_head must be rejected"
    );

    let mut with_identity = empty.clone();
    with_identity.table_identities = vec![TableIdentitySnapshotEntry {
        namespace: crate::table_identity::hidden_table_namespace(&table_id),
        table: table.clone(),
        table_id: table_id.clone(),
        state: TableState::Active,
    }];
    assert!(
        matches!(
            super::validate_materialized_journal_replay_base_is_empty(&with_identity),
            Err(Error::InvalidInput(_))
        ),
        "a non-empty table_identities set must be rejected"
    );

    let mut with_schema = empty.clone();
    with_schema
        .schema
        .tables
        .insert(table.clone(), tasks_schema());
    assert!(
        matches!(
            super::validate_materialized_journal_replay_base_is_empty(&with_schema),
            Err(Error::InvalidInput(_))
        ),
        "a non-empty schema must be rejected"
    );

    // Documents-only base: all other fields empty. If the documents clause is
    // inverted, this base slips through as valid, so requiring rejection here
    // pins the mutation from the other direction.
    let mut with_documents = empty.clone();
    with_documents.documents = vec![nimbus_core::Document::new(
        table.clone(),
        serde_json::Map::from_iter([("rank".to_string(), json!(1))]),
    )];
    assert!(
        matches!(
            super::validate_materialized_journal_replay_base_is_empty(&with_documents),
            Err(Error::InvalidInput(_))
        ),
        "a non-empty documents set must be rejected (a documents-only base must not pass)"
    );

    let mut with_scheduled = empty.clone();
    with_scheduled.scheduled_execution_ids = vec!["exec-1".to_string()];
    assert!(
        matches!(
            super::validate_materialized_journal_replay_base_is_empty(&with_scheduled),
            Err(Error::InvalidInput(_))
        ),
        "a non-empty scheduled_execution_ids set must be rejected"
    );
}

/// One table's contribution to a snapshot, built once so that every assembled
/// snapshot carries byte-identical parts and only their ordering varies.
struct SnapshotPart {
    schema: TableSchema,
    identity: TableIdentitySnapshotEntry,
    document: nimbus_core::Document,
}

fn snapshot_parts(tables: &[(&str, i64)]) -> Vec<SnapshotPart> {
    tables
        .iter()
        .map(|(name, rank)| {
            let table = TableName::new(*name).expect("table name should parse");
            let mut schema = tasks_schema();
            schema.table = table.clone();
            SnapshotPart {
                schema,
                identity: TableIdentitySnapshotEntry::default_namespace(
                    table.clone(),
                    TableId::new(),
                ),
                document: nimbus_core::Document::new(
                    table,
                    serde_json::Map::from_iter([("rank".to_string(), json!(rank))]),
                ),
            }
        })
        .collect()
}

/// Assemble a snapshot from `parts` in `order`. The order reaches the schema
/// `HashMap` as insertion order and the identity and document vectors as
/// element order, which is exactly what the canonical state must normalize.
fn assemble_snapshot(
    applied_sequence: SequenceNumber,
    parts: &[SnapshotPart],
    order: &[usize],
    scheduled_execution_ids: Vec<String>,
) -> MaterializedJournalSnapshot {
    let mut schema = nimbus_core::Schema::default();
    let mut table_identities = Vec::new();
    let mut documents = Vec::new();
    for index in order {
        let part = &parts[*index];
        schema
            .tables
            .insert(part.schema.table.clone(), part.schema.clone());
        table_identities.push(part.identity.clone());
        documents.push(part.document.clone());
    }

    MaterializedJournalSnapshot {
        version: crate::store::MATERIALIZED_JOURNAL_SNAPSHOT_VERSION,
        applied_sequence,
        durable_head: applied_sequence,
        table_identities,
        schema,
        documents,
        scheduled_execution_ids,
    }
}

#[test]
fn same_sequence_different_state_has_different_materialized_position() {
    let sequence = SequenceNumber(7);
    let parts = snapshot_parts(&[("alpha", 1), ("beta", 2)]);
    let baseline = assemble_snapshot(sequence, &parts, &[0, 1], vec!["exec-a".to_string()]);

    let mut drifted = baseline.clone();
    drifted
        .documents
        .first_mut()
        .expect("baseline snapshot should carry a document")
        .fields
        .insert("rank".to_string(), json!(99));

    let baseline_position = baseline
        .materialized_position()
        .expect("baseline position should compute");
    let drifted_position = drifted
        .materialized_position()
        .expect("drifted position should compute");

    // The sequence alone cannot tell these apart. That is exactly the silence
    // this contract closes: equal sequence, unequal state, unequal position.
    assert_eq!(
        baseline_position.applied_sequence, drifted_position.applied_sequence,
        "the drift must not move the applied sequence, or the test proves nothing"
    );
    assert_ne!(
        baseline_position.state_digest, drifted_position.state_digest,
        "a document change at an unchanged sequence must change the state digest"
    );
    assert_ne!(
        baseline_position, drifted_position,
        "positions must differ when the materialized state differs"
    );
}

#[test]
fn logical_order_does_not_change_materialized_position() {
    let sequence = SequenceNumber(7);
    let parts = snapshot_parts(&[("alpha", 1), ("beta", 2), ("gamma", 3), ("delta", 4)]);
    let scheduled = vec!["exec-b".to_string(), "exec-a".to_string()];

    // Same logical state, opposite insertion order. `Schema::tables` is a
    // `HashMap`, so serializing it in iteration order gives a different digest
    // per instance; the canonical state has to sort that out.
    let forward = assemble_snapshot(sequence, &parts, &[0, 1, 2, 3], scheduled.clone());
    let reversed = assemble_snapshot(
        sequence,
        &parts,
        &[3, 2, 1, 0],
        scheduled.iter().rev().cloned().collect(),
    );

    let forward_position = forward
        .materialized_position()
        .expect("forward position should compute");
    let reversed_position = reversed
        .materialized_position()
        .expect("reversed position should compute");
    assert_eq!(
        forward_position, reversed_position,
        "logically equal snapshots must share one position regardless of ordering"
    );

    // Repeat over fresh snapshot instances: each `HashMap` carries its own
    // `RandomState`, so an unsorted digest drifts across instances even when
    // the insertion order never changes.
    let repeats = (0..32)
        .map(|_| {
            assemble_snapshot(sequence, &parts, &[0, 1, 2, 3], scheduled.clone())
                .canonical_state()
                .expect("canonical state should build")
                .digest()
                .expect("digest should compute")
        })
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        repeats.len(),
        1,
        "the canonical digest must be stable across snapshot instances, saw {} distinct values",
        repeats.len()
    );
}

#[test]
fn pitr_import_rejects_wrong_target_digest() {
    let clock = Arc::new(ManualWallClock::new(Timestamp(1_000)));
    let live =
        TenantStore::create_in_memory_with_simulation(clock.clone(), Arc::new(NoopFaultInjector))
            .expect("live store should open");
    let table_schema = tasks_schema();
    let table = table_schema.table.clone();
    live.replace_table_schema(&table_schema)
        .expect("table schema should persist");

    clock.set(Timestamp(1_100));
    let document = nimbus_core::Document::new(
        table.clone(),
        serde_json::Map::from_iter([("rank".to_string(), json!(1))]),
    );
    let commit = live
        .insert_with_indexes(&document, &table_schema.indexes)
        .expect("insert should succeed");

    let mut archive = live
        .export_point_in_time_restore_archive(
            PointInTimeRestoreTarget::Sequence(commit.sequence),
            RetentionGcConfig::retain_all(),
        )
        .expect("archive should export");

    let honest = TenantStore::create_in_memory().expect("honest restore store should open");
    honest
        .import_point_in_time_restore_archive(&archive)
        .expect("an untampered archive must restore");

    // Only the digest is wrong: the target sequence, the base snapshot, and the
    // journal tail all still agree, so a sequence-only target would accept this
    // restore without noticing that the state it produced is not the state the
    // archive promised.
    archive.target_position.state_digest =
        "0000000000000000000000000000000000000000000000000000000000000000".to_string();
    let restored = TenantStore::create_in_memory().expect("restore store should open");
    let error = restored
        .import_point_in_time_restore_archive(&archive)
        .expect_err("a restore whose target digest does not match must fail");
    let message = error.to_string();
    assert!(
        message.contains("point-in-time restore position mismatch"),
        "expected a position mismatch, saw {message}"
    );
}
