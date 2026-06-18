use super::support::*;

#[tokio::test(flavor = "multi_thread")]
async fn mysql_direct_writes_dedupe_and_journal_progress_round_trip() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("writes").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let document = crate::tests::sample_document("tasks", "First");

        let first_commit = opened
            .store
            .insert_once(&document, Some("exec-1"))
            .expect("first deduplicated insert should succeed")
            .expect("first deduplicated insert should commit");
        assert_eq!(first_commit.sequence, SequenceNumber(1));
        assert!(
            opened
                .store
                .insert_once(&document, Some("exec-1"))
                .expect("duplicate deduplicated insert should succeed")
                .is_none()
        );

        let second_commit = opened
            .store
            .update_validated(
                &document.table,
                &document.id,
                &serde_json::Map::from_iter([("title".to_string(), serde_json::json!("Renamed"))]),
                |_, _| Ok(()),
            )
            .expect("update should succeed");
        assert_eq!(second_commit.sequence, SequenceNumber(2));

        let updated = opened
            .store
            .get(&document.table, &document.id)
            .expect("document lookup should succeed")
            .expect("updated document should exist");
        assert_eq!(
            updated.fields.get("title").and_then(|value| value.as_str()),
            Some("Renamed")
        );

        let (third_commit, removed) = opened
            .store
            .delete_validated_returning_document(&document.table, &document.id, |_| Ok(()))
            .expect("delete should succeed");
        assert_eq!(third_commit.sequence, SequenceNumber(3));
        assert_eq!(removed.id, document.id);
        assert_eq!(
            opened
                .store
                .journal_progress()
                .expect("journal progress should read"),
            crate::store::JournalProgress {
                durable_head: SequenceNumber(3),
                applied_head: SequenceNumber(3),
            }
        );

        let commits = opened
            .store
            .read_commit_log_from(SequenceNumber(1))
            .expect("commit log should read");
        assert_eq!(commits.len(), 3);
        assert_eq!(commits[0].writes[0].op_type, WriteOpType::Insert);
        assert_eq!(commits[1].writes[0].op_type, WriteOpType::Update);
        assert_eq!(commits[2].writes[0].op_type, WriteOpType::Delete);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn mysql_durable_journal_recovery_applies_pending_records() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("recovery").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let first = crate::tests::sample_document("tasks", "First");
        let second = crate::tests::sample_document("tasks", "Second");
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
                Timestamp(200),
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

        opened
            .store
            .append_durable_records_batch(&records)
            .expect("durable append should succeed");
        assert_eq!(
            opened
                .store
                .journal_progress()
                .expect("journal progress should read"),
            crate::store::JournalProgress {
                durable_head: SequenceNumber(2),
                applied_head: SequenceNumber(0),
            }
        );
        assert!(
            opened
                .store
                .get(&first.table, &first.id)
                .expect("first lookup should succeed")
                .is_none()
        );

        let progress = opened
            .store
            .recover_durable_journal()
            .expect("recovery should apply pending durable records");
        assert_eq!(
            progress,
            crate::store::JournalProgress {
                durable_head: SequenceNumber(2),
                applied_head: SequenceNumber(2),
            }
        );
        assert_eq!(
            opened
                .store
                .get(&first.table, &first.id)
                .expect("first lookup should succeed")
                .as_ref(),
            Some(&first)
        );
        assert_eq!(
            opened
                .store
                .get(&second.table, &second.id)
                .expect("second lookup should succeed")
                .as_ref(),
            Some(&second)
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn mysql_tenant_event_journal_replays_mixed_history() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("tenant-event-mixed").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let table = TableName::new("tasks_tenant_event").expect("table name should build");
        let table_id = TableId::new();
        let schema = TableSchema {
            table: table.clone(),
            fields: vec![FieldSchema {
                name: "rank".to_string(),
                field_type: FieldType::Number,
                required: false,
            }],
            indexes: vec![nimbus_core::IndexDefinition {
                id: nimbus_core::IndexId::new(),
                state: nimbus_core::IndexState::Enabled,
                name: "by_rank".to_string(),
                fields: vec!["rank".to_string()],
            }],
            access_policy: None,
        };
        let document = Document::new(
            table.clone(),
            serde_json::Map::from_iter([
                ("title".to_string(), serde_json::json!("evented")),
                ("rank".to_string(), serde_json::json!(7)),
            ]),
        );
        let records = vec![
            TenantEventRecord::from_events(
                SequenceNumber(1),
                Timestamp(100),
                vec![TenantEventKind::TableLifecycle {
                    lifecycle: nimbus_core::TableLifecycleEvent::StageHidden {
                        table: table.clone(),
                        table_id: table_id.clone(),
                    },
                }],
            )
            .expect("stage-hidden event should build"),
            TenantEventRecord::from_events(
                SequenceNumber(2),
                Timestamp(200),
                vec![TenantEventKind::TableLifecycle {
                    lifecycle: nimbus_core::TableLifecycleEvent::ActivateHidden {
                        table: table.clone(),
                        table_id: table_id.clone(),
                        replaced_table_id: None,
                    },
                }],
            )
            .expect("activate-hidden event should build"),
            TenantEventRecord::from_events(
                SequenceNumber(3),
                Timestamp(300),
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
                        index: nimbus_core::IndexLifecycleEvent {
                            table: table.clone(),
                            table_id: table_id.clone(),
                            index_id: schema.indexes[0].id.clone(),
                            state: schema.indexes[0].state,
                            definition: schema.indexes[0].clone(),
                        },
                    },
                ],
            )
            .expect("schema event should build"),
            TenantEventRecord::from_events(
                SequenceNumber(4),
                Timestamp(400),
                vec![TenantEventKind::DocumentWrite {
                    writes: vec![WriteOp {
                        table: table.clone(),
                        table_id: table_id.clone(),
                        op_type: WriteOpType::Insert,
                        doc_id: document.id.clone(),
                        resource_path_binding: None,
                        trigger_write_origin: None,
                        previous: None,
                        current: Some(document.clone()),
                    }],
                }],
            )
            .expect("document event should build"),
            TenantEventRecord::from_events(
                SequenceNumber(5),
                Timestamp(500),
                vec![TenantEventKind::TriggerDelivery {
                    cursor: TriggerDeliveryCursor::new(SequenceNumber(4)),
                }],
            )
            .expect("trigger cursor event should build"),
        ];

        opened
            .store
            .apply_durable_records_batch(&records)
            .expect("mixed tenant event replay should apply");

        assert_eq!(
            opened.store.table_id(&table).expect("table id should load"),
            Some(table_id)
        );
        let loaded_schema = opened.store.load_schema().expect("schema should load");
        assert_eq!(loaded_schema.get_table(&table), Some(&schema));
        assert_eq!(
            opened
                .store
                .get(&table, &document.id)
                .expect("document lookup should succeed")
                .as_ref(),
            Some(&document)
        );
        assert_eq!(
            opened
                .store
                .trigger_delivery_cursor()
                .expect("trigger cursor should load"),
            TriggerDeliveryCursor::new(SequenceNumber(4))
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn mysql_durable_replay_retires_recreated_table_identity() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("durable-recreate").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let table = TableName::new("tasks_durable_recreate").expect("table name should build");
        let old_table_id = TableId::new();
        let new_table_id = TableId::new();
        let old_document = Document::new(
            table.clone(),
            serde_json::Map::from_iter([("title".to_string(), serde_json::json!("old"))]),
        );
        let new_document = Document::new(
            table.clone(),
            serde_json::Map::from_iter([("title".to_string(), serde_json::json!("new"))]),
        );
        let records = vec![
            TenantEventRecord::new(
                SequenceNumber(1),
                Timestamp(100),
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
            TenantEventRecord::new(
                SequenceNumber(2),
                Timestamp(200),
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

        opened
            .store
            .apply_durable_records_batch(&records)
            .expect("durable replay should infer table recreation");

        assert_eq!(
            opened.store.table_id(&table).expect("table id should load"),
            Some(new_table_id.clone())
        );
        assert!(
            opened
                .store
                .get(&table, &old_document.id)
                .expect("old logical lookup should succeed")
                .is_none()
        );
        assert_eq!(
            opened
                .store
                .get(&table, &new_document.id)
                .expect("new logical lookup should succeed")
                .as_ref(),
            Some(&new_document)
        );
        let mut check_cancel = || Ok(());
        assert_eq!(
            opened
                .store
                .scan_table_matching_cancellable(&table, &mut check_cancel, |_| Ok(true))
                .expect("active table scan should succeed"),
            vec![new_document]
        );
        let diagnostics = opened
            .store
            .table_identity_diagnostics()
            .expect("diagnostics should load after replay");
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.table_name == table
                && diagnostic.table_id == new_table_id
                && diagnostic.state == TableState::Active
                && diagnostic.document_count == Some(1)
        }));
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.table_name == table
                && diagnostic.table_id == old_table_id
                && diagnostic.state == TableState::Deleting
                && diagnostic.document_count.is_none()
        }));
    })
    .await;
}
