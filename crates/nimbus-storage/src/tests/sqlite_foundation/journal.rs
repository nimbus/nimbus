use super::support::*;

#[test]
fn sqlite_direct_writes_emit_commit_entries_and_round_trip_journal_reads() {
    let dir = tempdir().expect("temporary directory should create");
    let store = SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
        .expect("sqlite tenant store should open");
    let document = sample_document("tasks", "Hello");

    let insert_commit = store.insert(&document).expect("insert should succeed");
    let patch = serde_json::Map::from_iter([("title".to_string(), json!("Updated"))]);
    let update_commit = store
        .update(&document.table, &document.id, &patch)
        .expect("update should succeed");
    let (delete_commit, removed_document) = store
        .delete_returning_document(&document.table, &document.id)
        .expect("delete should succeed");

    assert_eq!(insert_commit.sequence, SequenceNumber(1));
    assert_eq!(update_commit.sequence, SequenceNumber(2));
    assert_eq!(delete_commit.sequence, SequenceNumber(3));
    assert_eq!(
        removed_document.fields.get("title"),
        Some(&json!("Updated"))
    );
    assert!(
        store
            .get(&document.table, &document.id)
            .expect("get should succeed after delete")
            .is_none()
    );
    assert_eq!(
        store
            .journal_progress()
            .expect("journal progress should read"),
        crate::store::JournalProgress {
            durable_head: SequenceNumber(3),
            applied_head: SequenceNumber(3),
        }
    );

    let entries = store
        .read_commit_log_from(SequenceNumber(1))
        .expect("commit log should read");
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].writes[0].op_type, WriteOpType::Insert);
    assert_eq!(entries[1].writes[0].op_type, WriteOpType::Update);
    assert_eq!(entries[2].writes[0].op_type, WriteOpType::Delete);
}

#[test]
fn sqlite_durable_journal_batch_append_enforces_no_holes() {
    let dir = tempdir().expect("temporary directory should create");
    let store = SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
        .expect("sqlite tenant store should open");
    let table_id = TableId::new();
    let first = DurableMutationRecord::new(
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
    let second = DurableMutationRecord::new(
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
        crate::store::JournalProgress {
            durable_head: SequenceNumber(2),
            applied_head: SequenceNumber(0),
        }
    );

    let error = store
        .append_durable_records_batch(&[DurableMutationRecord::new(
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
fn sqlite_recovery_replays_durable_but_unapplied_records() {
    let dir = tempdir().expect("temporary directory should create");
    let store = SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
        .expect("sqlite tenant store should open");
    let first = sample_document("tasks", "First");
    let second = sample_document("tasks", "Second");
    let table_id = TableId::new();
    let records = vec![
        DurableMutationRecord::new(
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
        DurableMutationRecord::new(
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
        crate::store::JournalProgress {
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
        crate::store::JournalProgress {
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
fn sqlite_tenant_event_journal_replays_mixed_history() {
    let dir = tempdir().expect("temporary directory should create");
    let store = SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
        .expect("sqlite tenant store should open");
    let table_schema = ranked_tasks_schema();
    let table = table_schema.table.clone();
    let table_id = TableId::new();
    let document = ranked_document(&table, "First", 1);
    let record_schema = TenantEventRecord::from_events(
        SequenceNumber(1),
        Timestamp(10),
        vec![
            TenantEventKind::SchemaChange {
                change: SchemaChangeEvent::SetTable {
                    table: table.clone(),
                    table_id: table_id.clone(),
                    previous: None,
                    current: table_schema.clone(),
                },
            },
            TenantEventKind::IndexLifecycle {
                index: IndexLifecycleEvent {
                    table: table.clone(),
                    table_id: table_id.clone(),
                    index_id: table_schema.indexes[0].id.clone(),
                    state: table_schema.indexes[0].state,
                    definition: table_schema.indexes[0].clone(),
                },
            },
        ],
    )
    .expect("schema tenant event should build");
    let record_document = DurableMutationRecord::new(
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

    store
        .append_durable_records_batch(&[record_schema, record_document, record_trigger])
        .expect("mixed tenant events should append");
    let progress = store
        .recover_durable_journal()
        .expect("mixed tenant events should recover");

    assert_eq!(progress.applied_head, SequenceNumber(3));
    assert_eq!(
        store.load_schema().expect("schema should replay"),
        Schema {
            tables: std::collections::HashMap::from_iter([(table.clone(), table_schema.clone())]),
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

#[test]
fn sqlite_durable_replay_retires_recreated_table_identity() {
    let dir = tempdir().expect("temporary directory should create");
    let store = SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
        .expect("sqlite tenant store should open");
    let table = TableName::new("tasks_replayed_lifecycle").expect("table should parse");
    let old_table_id = TableId::new();
    let new_table_id = TableId::new();
    let old_document = nimbus_core::Document::new(
        table.clone(),
        serde_json::Map::from_iter([("title".to_string(), json!("old"))]),
    );
    let new_document = nimbus_core::Document::new(
        table.clone(),
        serde_json::Map::from_iter([("title".to_string(), json!("new"))]),
    );
    let records = vec![
        DurableMutationRecord::new(
            SequenceNumber(1),
            Timestamp(1),
            vec![WriteOp {
                table: table.clone(),
                table_id: old_table_id.clone(),
                op_type: WriteOpType::Insert,
                doc_id: old_document.id.clone(),
                resource_path_binding: None,
                trigger_write_origin: None,
                previous: None,
                current: Some(old_document.clone()),
            }],
            None,
        )
        .expect("old durable record should build"),
        DurableMutationRecord::new(
            SequenceNumber(2),
            Timestamp(2),
            vec![WriteOp {
                table: table.clone(),
                table_id: new_table_id.clone(),
                op_type: WriteOpType::Insert,
                doc_id: new_document.id.clone(),
                resource_path_binding: None,
                trigger_write_origin: None,
                previous: None,
                current: Some(new_document.clone()),
            }],
            None,
        )
        .expect("new durable record should build"),
    ];

    store
        .apply_durable_records_batch(&records)
        .expect("durable replay should infer table recreation");

    assert_eq!(
        store
            .table_id(&table)
            .expect("active table id should resolve"),
        Some(new_table_id.clone())
    );
    assert!(
        store
            .get(&table, &old_document.id)
            .expect("logical get should use active replacement")
            .is_none()
    );
    assert_eq!(
        store
            .scan_table(&table)
            .expect("scan should use active replacement"),
        vec![new_document]
    );
    let identities = store
        .read_snapshot()
        .expect("snapshot should open")
        .table_identities()
        .expect("table identities should export");
    assert!(identities.iter().any(|identity| {
        identity.namespace == crate::table_identity::DEFAULT_TABLE_NAMESPACE
            && identity.table == table
            && identity.table_id == new_table_id
            && identity.state == nimbus_core::TableState::Active
    }));
    assert!(identities.iter().any(|identity| {
        identity.namespace == crate::table_identity::deleting_table_namespace(&old_table_id)
            && identity.table == table
            && identity.table_id == old_table_id
            && identity.state == nimbus_core::TableState::Deleting
    }));
}

#[test]
fn sqlite_durable_journal_stream_uses_cursor_floor_after_retention_cut() {
    let dir = tempdir().expect("temporary directory should create");
    let path = dir.path().join("tenant.sqlite3");
    let store = SqliteTenantStore::open(&path).expect("sqlite tenant store should open");
    let first = sample_document("tasks", "first");
    let second = sample_document("tasks", "second");
    store.insert(&first).expect("first insert should succeed");
    store.insert(&second).expect("second insert should succeed");

    rusqlite::Connection::open(&path)
        .expect("raw sqlite connection should open")
        .execute("DELETE FROM commit_log WHERE sequence = 1", [])
        .expect("first journal row should delete");

    let error = store
        .stream_durable_journal(SequenceNumber(0), 10)
        .expect_err("cursor behind the retained floor should fail");
    assert!(matches!(
        error,
        Error::InvalidInput(message) if message.contains("behind the retention floor 1")
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
