use nimbus_core::{
    Document, IndexDefinition, IndexLifecycleEvent, IndexState, SchemaChangeEvent, SequenceNumber,
    TableId, TableLifecycleEvent, TableName, TableSchema, TenantEventKind, TenantEventRecord,
    Timestamp, TriggerDeliveryCursor, WriteOp, WriteOpType,
};
use serde_json::json;

use crate::{ChangefeedHandle, TenantStore};

fn table_schema(table: &TableName) -> TableSchema {
    TableSchema {
        table: table.clone(),
        fields: Vec::new(),
        indexes: vec![IndexDefinition {
            id: nimbus_core::IndexId::new(),
            state: IndexState::Enabled,
            name: "by_rank".to_string(),
            fields: vec!["rank".to_string()],
        }],
        access_policy: None,
    }
}

#[test]
fn changefeed_bootstrap_pages_events_without_missing_or_duplicating_handoff_records() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let bootstrap = store
        .export_changefeed_bootstrap()
        .expect("changefeed bootstrap should export");
    assert_eq!(bootstrap.cursor.after, SequenceNumber(0));
    assert_eq!(bootstrap.latest_sequence, SequenceNumber(0));
    assert_eq!(bootstrap.snapshot.applied_sequence, SequenceNumber(0));

    let table = TableName::new("tasks").expect("table should parse");
    let table_id = TableId::new();
    let schema = table_schema(&table);
    let index = schema.indexes[0].clone();
    let document = Document::new(
        table.clone(),
        serde_json::Map::from_iter([
            ("title".to_string(), json!("first")),
            ("rank".to_string(), json!(1)),
        ]),
    );
    let lifecycle_record = TenantEventRecord::from_events(
        SequenceNumber(1),
        Timestamp(100),
        vec![
            TenantEventKind::TableLifecycle {
                lifecycle: TableLifecycleEvent::StageHidden {
                    table: table.clone(),
                    table_id: table_id.clone(),
                },
            },
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
                    index_id: index.id.clone(),
                    state: index.state,
                    definition: index,
                },
            },
        ],
    )
    .expect("lifecycle record should build");
    let write_record = TenantEventRecord::from_events(
        SequenceNumber(2),
        Timestamp(200),
        vec![
            TenantEventKind::DocumentWrite {
                writes: vec![WriteOp {
                    table: table.clone(),
                    table_id,
                    op_type: WriteOpType::Insert,
                    doc_id: document.id.clone(),
                    resource_path_binding: None,
                    trigger_write_origin: None,
                    previous: None,
                    current: Some(document),
                }],
            },
            TenantEventKind::TriggerDelivery {
                cursor: TriggerDeliveryCursor::new(SequenceNumber(2)),
            },
        ],
    )
    .expect("write record should build");
    store
        .append_durable_records_batch(&[lifecycle_record, write_record])
        .expect("records should append");

    let first_page = store
        .stream_changefeed(&bootstrap.cursor, 1)
        .expect("first changefeed page should stream");
    assert!(first_page.has_more);
    assert_eq!(first_page.events.len(), 1);
    assert_eq!(first_page.events[0].sequence, SequenceNumber(1));
    assert!(
        first_page.events[0]
            .events
            .iter()
            .any(|event| matches!(event, TenantEventKind::TableLifecycle { .. }))
    );
    assert!(
        first_page.events[0]
            .events
            .iter()
            .any(|event| matches!(event, TenantEventKind::SchemaChange { .. }))
    );
    assert!(
        first_page.events[0]
            .events
            .iter()
            .any(|event| matches!(event, TenantEventKind::IndexLifecycle { .. }))
    );

    let rotated_cursor = first_page
        .next_cursor
        .rotate_handle(ChangefeedHandle::new(SequenceNumber(2), SequenceNumber(0)))
        .expect("handle rotation at retained floor should succeed");
    let second_page = store
        .stream_changefeed(&rotated_cursor, 10)
        .expect("second changefeed page should stream");
    assert!(!second_page.has_more);
    assert_eq!(second_page.events.len(), 1);
    assert_eq!(second_page.events[0].sequence, SequenceNumber(2));
    assert!(
        second_page.events[0]
            .events
            .iter()
            .any(|event| matches!(event, TenantEventKind::DocumentWrite { .. }))
    );
    assert!(
        second_page.events[0]
            .events
            .iter()
            .any(|event| matches!(event, TenantEventKind::TriggerDelivery { .. }))
    );

    let empty_page = store
        .stream_changefeed(&second_page.next_cursor, 10)
        .expect("fully consumed cursor should stream an empty page");
    assert!(empty_page.events.is_empty());
    assert_eq!(empty_page.next_cursor.after, SequenceNumber(2));
}
