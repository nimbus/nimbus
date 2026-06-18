use nimbus_core::{
    DocumentId, Error, FieldSchema, FieldType, IndexDefinition, IndexLifecycleEvent,
    SchemaChangeEvent, SequenceNumber, TableId, TableName, TableSchema, TenantEventKind,
    TenantEventRecord, Timestamp, TriggerDeliveryCursor, WriteOp, WriteOpType,
};
use serde_json::json;

use crate::TenantStore;

fn sample_document(table: &str, title: &str) -> nimbus_core::Document {
    nimbus_core::Document::new(
        TableName::new(table).expect("table name should be valid"),
        serde_json::Map::from_iter([("title".to_string(), json!(title))]),
    )
}

fn ranked_tasks_schema(table: &TableName) -> TableSchema {
    TableSchema {
        table: table.clone(),
        fields: vec![FieldSchema {
            name: "rank".to_string(),
            field_type: FieldType::Number,
            required: false,
        }],
        indexes: vec![IndexDefinition {
            id: nimbus_core::IndexId::new(),
            name: "by_rank".to_string(),
            fields: vec!["rank".to_string()],
            state: nimbus_core::IndexState::Enabled,
        }],
        access_policy: None,
    }
}

fn ranked_document(table: &TableName, title: &str, rank: i64) -> nimbus_core::Document {
    nimbus_core::Document::new(
        table.clone(),
        serde_json::Map::from_iter([
            ("title".to_string(), json!(title)),
            ("rank".to_string(), json!(rank)),
        ]),
    )
}

#[test]
fn durable_journal_batch_append_enforces_no_holes() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let table_id = TableId::new();
    let first = TenantEventRecord::new(
        SequenceNumber(1),
        Timestamp(10),
        vec![WriteOp {
            table: TableName::new("tasks").expect("table name should be valid"),
            table_id: table_id.clone(),
            op_type: WriteOpType::Insert,
            doc_id: DocumentId::new(),
            resource_path_binding: None,
            trigger_write_origin: None,
            previous: None,
            current: Some(sample_document("tasks", "First")),
        }],
        None,
    )
    .expect("first durable record should build");
    let second = TenantEventRecord::new(
        SequenceNumber(2),
        Timestamp(11),
        vec![WriteOp {
            table: TableName::new("tasks").expect("table name should be valid"),
            table_id: table_id.clone(),
            op_type: WriteOpType::Insert,
            doc_id: DocumentId::new(),
            resource_path_binding: None,
            trigger_write_origin: None,
            previous: None,
            current: Some(sample_document("tasks", "Second")),
        }],
        None,
    )
    .expect("second durable record should build");

    store
        .append_durable_records_batch(&[first.clone(), second.clone()])
        .expect("initial batch append should succeed");
    assert_eq!(
        store
            .journal_progress()
            .expect("journal progress should read"),
        super::super::JournalProgress {
            durable_head: SequenceNumber(2),
            applied_head: SequenceNumber(0),
        }
    );

    let error = store
        .append_durable_records_batch(&[TenantEventRecord::new(
            SequenceNumber(4),
            Timestamp(12),
            vec![WriteOp {
                table: TableName::new("tasks").expect("table name should be valid"),
                table_id: table_id.clone(),
                op_type: WriteOpType::Insert,
                doc_id: DocumentId::new(),
                resource_path_binding: None,
                trigger_write_origin: None,
                previous: None,
                current: Some(sample_document("tasks", "Gap")),
            }],
            None,
        )
        .expect("gap record should build")])
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
fn recovery_replays_durable_but_unapplied_records() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let first = sample_document("tasks", "First");
    let second = sample_document("tasks", "Second");
    let table_id = TableId::new();
    let records = vec![
        TenantEventRecord::new(
            SequenceNumber(1),
            Timestamp(100),
            vec![WriteOp {
                table: first.table.clone(),
                table_id: table_id.clone(),
                op_type: WriteOpType::Insert,
                doc_id: first.id.clone(),
                resource_path_binding: None,
                trigger_write_origin: None,
                previous: None,
                current: Some(first.clone()),
            }],
            None,
        )
        .expect("first durable record should build"),
        TenantEventRecord::new(
            SequenceNumber(2),
            Timestamp(101),
            vec![WriteOp {
                table: second.table.clone(),
                table_id: table_id.clone(),
                op_type: WriteOpType::Insert,
                doc_id: second.id.clone(),
                resource_path_binding: None,
                trigger_write_origin: None,
                previous: None,
                current: Some(second.clone()),
            }],
            None,
        )
        .expect("second durable record should build"),
    ];

    store
        .append_durable_records_batch(&records)
        .expect("durable append should succeed");
    assert_eq!(
        store
            .journal_progress()
            .expect("journal progress should read"),
        super::super::JournalProgress {
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
        super::super::JournalProgress {
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
fn tenant_event_journal_appends_schema_table_index_scheduler_and_trigger_events_atomically() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let table = TableName::new("tasks").expect("table name should be valid");
    let schema = ranked_tasks_schema(&table);

    let committed = store
        .execute_write(|transaction| {
            transaction.save_table_schema(&schema)?;
            assert!(transaction.begin_scheduled_execution(Some("scheduled:tenant-event"))?);
            transaction
                .set_trigger_delivery_cursor(TriggerDeliveryCursor::new(SequenceNumber(7)))?;
            Ok(())
        })
        .expect("metadata transaction should commit");

    let commit = committed
        .commit
        .expect("metadata-only transaction must append a tenant event");
    assert_eq!(commit.sequence, SequenceNumber(1));
    assert!(commit.writes.is_empty());

    let records = store
        .read_durable_journal_from(SequenceNumber(1))
        .expect("tenant event journal should read");
    assert_eq!(records.len(), 1);
    assert!(
        records[0]
            .events()
            .iter()
            .any(|event| matches!(event, TenantEventKind::SchemaChange { .. }))
    );
    assert!(
        records[0]
            .events()
            .iter()
            .any(|event| matches!(event, TenantEventKind::IndexLifecycle { .. }))
    );
    assert!(
        records[0]
            .events()
            .iter()
            .any(|event| matches!(event, TenantEventKind::ScheduledExecution { .. }))
    );
    assert!(
        records[0]
            .events()
            .iter()
            .any(|event| matches!(event, TenantEventKind::TriggerDelivery { .. }))
    );
    assert_eq!(
        store
            .trigger_delivery_cursor()
            .expect("trigger cursor should read"),
        TriggerDeliveryCursor::new(SequenceNumber(7))
    );
}

#[test]
fn tenant_event_journal_advances_next_sequence_once_per_commit() {
    let store = TenantStore::create_in_memory().expect("store should open");

    for (execution_id, expected_sequence) in [
        ("scheduled:first", SequenceNumber(1)),
        ("scheduled:second", SequenceNumber(2)),
    ] {
        let committed = store
            .execute_write(|transaction| {
                assert!(transaction.begin_scheduled_execution(Some(execution_id))?);
                Ok(())
            })
            .expect("scheduled execution transaction should commit");
        let commit = committed
            .commit
            .expect("metadata-only transaction should append a tenant event");

        assert_eq!(commit.sequence, expected_sequence);
        assert_eq!(
            store
                .latest_sequence()
                .expect("latest sequence should reflect committed event"),
            expected_sequence
        );
    }

    let sequences = store
        .read_durable_journal_from(SequenceNumber(1))
        .expect("tenant event journal should read")
        .into_iter()
        .map(|record| record.sequence)
        .collect::<Vec<_>>();
    assert_eq!(sequences, vec![SequenceNumber(1), SequenceNumber(2)]);
}

#[test]
fn redb_tenant_event_journal_replays_mixed_history() {
    let table = TableName::new("tasks").expect("table name should be valid");
    let table_id = TableId::new();
    let schema = ranked_tasks_schema(&table);
    let document = ranked_document(&table, "First", 1);
    let record_schema = TenantEventRecord::from_events(
        SequenceNumber(1),
        Timestamp(10),
        vec![
            TenantEventKind::SchemaChange {
                change: Box::new(SchemaChangeEvent::SetTable {
                    table: table.clone(),
                    table_id: table_id.clone(),
                    previous: None,
                    current: schema.clone(),
                }),
            },
            TenantEventKind::IndexLifecycle {
                index: IndexLifecycleEvent {
                    table: table.clone(),
                    table_id: table_id.clone(),
                    index_id: schema.indexes[0].id.clone(),
                    state: schema.indexes[0].state,
                    definition: schema.indexes[0].clone(),
                },
            },
        ],
    )
    .expect("schema tenant event should build");
    let record_document = TenantEventRecord::new(
        SequenceNumber(2),
        Timestamp(11),
        vec![WriteOp {
            table: table.clone(),
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
    .expect("document tenant event should build");
    let record_trigger = TenantEventRecord::trigger_delivery(
        SequenceNumber(3),
        Timestamp(12),
        TriggerDeliveryCursor::new(SequenceNumber(2)),
    )
    .expect("trigger tenant event should build");
    let store = TenantStore::create_in_memory().expect("store should open");

    store
        .append_durable_records_batch(&[record_schema, record_document, record_trigger])
        .expect("mixed tenant events should append");
    let progress = store
        .recover_durable_journal()
        .expect("mixed tenant events should recover");

    assert_eq!(progress.applied_head, SequenceNumber(3));
    assert_eq!(
        store.load_schema().expect("schema should replay"),
        nimbus_core::Schema {
            tables: std::collections::HashMap::from_iter([(table.clone(), schema.clone())]),
        }
    );
    assert_eq!(
        store
            .scan_table(&table)
            .expect("documents should replay through tenant event"),
        vec![document]
    );
    assert_eq!(
        store
            .index_scan_eq(&table, "by_rank", &json!(1))
            .expect("index should replay"),
        store.scan_table(&table).expect("scan should replay")
    );
    assert_eq!(
        store
            .trigger_delivery_cursor()
            .expect("trigger cursor should replay"),
        TriggerDeliveryCursor::new(SequenceNumber(2))
    );
}
