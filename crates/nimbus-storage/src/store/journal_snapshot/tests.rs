use nimbus_core::{
    DurableMutationRecord, Error, FieldSchema, FieldType, IndexDefinition, SequenceNumber, TableId,
    TableName, TableSchema, TableState, Timestamp, WriteOp, WriteOpType,
};
use serde_json::json;

use crate::{MaterializedJournalSnapshot, TableIdentitySnapshotEntry, TenantStore};

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
