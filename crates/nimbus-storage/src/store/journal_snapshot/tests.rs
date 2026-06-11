use std::sync::Arc;

use nimbus_core::{
    DurableMutationRecord, Error, FieldSchema, FieldType, IndexDefinition, SequenceNumber, TableId,
    TableName, TableSchema, TableState, Timestamp, WriteOp, WriteOpType,
};
use serde_json::json;

use crate::{
    ManualClock, MaterializedJournalSnapshot, NoopFaultInjector, PointInTimeRestoreTarget,
    RetentionGcConfig, TableIdentitySnapshotEntry, TenantStore,
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
fn point_in_time_archive_restores_sequence_and_timestamp_to_matching_fingerprints() {
    let clock = Arc::new(ManualClock::new(Timestamp(1_000)));
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
        sequence_archive.target_fingerprint,
        timestamp_archive.target_fingerprint
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
            .canonical_fingerprint()
            .expect("sequence restored fingerprint should compute"),
        sequence_archive.target_fingerprint
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
            .canonical_fingerprint()
            .expect("timestamp restored fingerprint should compute"),
        sequence_archive.target_fingerprint
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
    let record = DurableMutationRecord::new(
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
