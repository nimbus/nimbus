use super::support::*;

#[tokio::test(flavor = "multi_thread")]
async fn mysql_document_versions_track_direct_write_history() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("document-versions").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let document = crate::tests::sample_document("versioned_tasks", "v1");
        let insert = opened
            .store
            .insert(&document)
            .expect("insert should succeed");
        let table_id = insert.writes[0].table_id.clone();
        let update = opened
            .store
            .update_validated(
                &document.table,
                &document.id,
                &serde_json::Map::from_iter([("title".to_string(), serde_json::json!("v2"))]),
                |_, _| Ok(()),
            )
            .expect("update should succeed");
        let (delete, _) = opened
            .store
            .delete_validated_returning_document(&document.table, &document.id, |_| Ok(()))
            .expect("delete should succeed");

        let at_insert = opened
            .store
            .get_document_version_at(&document.table, &table_id, &document.id, insert.sequence)
            .expect("insert version should load")
            .expect("insert version should exist");
        let at_update = opened
            .store
            .get_document_version_at(&document.table, &table_id, &document.id, update.sequence)
            .expect("update version should load")
            .expect("update version should exist");
        let at_delete = opened
            .store
            .get_document_version_at(&document.table, &table_id, &document.id, delete.sequence)
            .expect("delete version should load");

        assert_eq!(
            at_insert.fields.get("title"),
            Some(&serde_json::json!("v1"))
        );
        assert_eq!(
            at_update.fields.get("title"),
            Some(&serde_json::json!("v2"))
        );
        assert_eq!(at_delete, None);
        assert!(
            opened
                .store
                .get(&document.table, &document.id)
                .expect("current row get should succeed")
                .is_none(),
            "current row should still reflect latest delete"
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn mysql_document_versions_storage_diagnostic_reports_format_and_range() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("document-version-diagnostics").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let document = crate::tests::sample_document("versioned_diagnostic_tasks", "v1");
        let insert = opened
            .store
            .insert(&document)
            .expect("insert should succeed");
        let update = opened
            .store
            .update_validated(
                &document.table,
                &document.id,
                &serde_json::Map::from_iter([("title".to_string(), serde_json::json!("v2"))]),
                |_, _| Ok(()),
            )
            .expect("update should succeed");
        let (delete, _) = opened
            .store
            .delete_validated_returning_document(&document.table, &document.id, |_| Ok(()))
            .expect("delete should succeed");

        let health = opened
            .store
            .storage_health_diagnostic()
            .expect("health diagnostic should load");

        assert_eq!(
            health.document_versions.format_version,
            Some(crate::CURRENT_DOCUMENT_VERSION_STORAGE_FORMAT)
        );
        assert_eq!(health.document_versions.version_count, 3);
        assert_eq!(health.document_versions.min_sequence, Some(insert.sequence));
        assert_eq!(health.document_versions.max_sequence, Some(delete.sequence));
        assert!(update.sequence.0 > insert.sequence.0);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn mysql_document_versions_are_materialized_during_durable_recovery() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("document-version-recovery").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let table = TableName::new("versioned_replay_tasks").expect("table name should be valid");
        let table_id = TableId::new();
        let inserted = crate::tests::sample_document("versioned_replay_tasks", "v1");
        let mut updated = inserted.clone();
        updated
            .fields
            .insert("title".to_string(), serde_json::json!("v2"));
        updated.update_time = Timestamp(updated.update_time.0.saturating_add(1));
        let records = vec![
            TenantEventRecord::new(
                SequenceNumber(1),
                Timestamp(100),
                vec![WriteOp {
                    table: table.clone(),
                    table_id: table_id.clone(),
                    op_type: WriteOpType::Insert,
                    doc_id: inserted.id.clone(),
                    resource_path_binding: None,
                    trigger_write_origin: None,
                    previous: None,
                    current: Some(inserted.clone()),
                }],
                None,
            )
            .expect("insert durable record should build"),
            TenantEventRecord::new(
                SequenceNumber(2),
                Timestamp(101),
                vec![WriteOp {
                    table: table.clone(),
                    table_id: table_id.clone(),
                    op_type: WriteOpType::Update,
                    doc_id: inserted.id.clone(),
                    resource_path_binding: None,
                    trigger_write_origin: None,
                    previous: Some(inserted.clone()),
                    current: Some(updated.clone()),
                }],
                None,
            )
            .expect("update durable record should build"),
            TenantEventRecord::new(
                SequenceNumber(3),
                Timestamp(102),
                vec![WriteOp {
                    table: table.clone(),
                    table_id: table_id.clone(),
                    op_type: WriteOpType::Delete,
                    doc_id: inserted.id.clone(),
                    resource_path_binding: None,
                    trigger_write_origin: None,
                    previous: Some(updated.clone()),
                    current: None,
                }],
                None,
            )
            .expect("delete durable record should build"),
        ];

        opened
            .store
            .append_durable_records_batch(&records)
            .expect("durable append should succeed");
        assert!(
            opened
                .store
                .get_document_version_at(&table, &table_id, &inserted.id, SequenceNumber(3))
                .expect("unapplied version lookup should succeed")
                .is_none(),
            "durable-only records must not materialize historical versions before recovery"
        );

        opened
            .store
            .recover_durable_journal()
            .expect("durable recovery should succeed");

        let at_insert = opened
            .store
            .get_document_version_at(&table, &table_id, &inserted.id, SequenceNumber(1))
            .expect("insert replay version should load")
            .expect("insert replay version should exist");
        let at_update = opened
            .store
            .get_document_version_at(&table, &table_id, &inserted.id, SequenceNumber(2))
            .expect("update replay version should load")
            .expect("update replay version should exist");
        let at_delete = opened
            .store
            .get_document_version_at(&table, &table_id, &inserted.id, SequenceNumber(3))
            .expect("delete replay version should load");

        assert_eq!(
            at_insert.fields.get("title"),
            Some(&serde_json::json!("v1"))
        );
        assert_eq!(
            at_update.fields.get("title"),
            Some(&serde_json::json!("v2"))
        );
        assert_eq!(at_delete, None);
        assert!(
            opened
                .store
                .get(&table, &inserted.id)
                .expect("current row get should succeed")
                .is_none(),
            "replayed current row should still reflect latest delete"
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn mysql_index_versions_track_direct_write_history() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("index-versions").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let table = TableName::new("indexed_versioned_tasks").expect("table name should be valid");
        let (schema, index) = mysql_indexed_rank_schema(&table);
        opened
            .store
            .replace_table_schema(&schema)
            .expect("schema should persist");
        let document = mysql_ranked_document(&table, "v1", 1);
        let insert = opened
            .store
            .insert(&document)
            .expect("insert should succeed");
        let table_id = insert.writes[0].table_id.clone();
        let update = opened
            .store
            .update_validated(
                &document.table,
                &document.id,
                &serde_json::Map::from_iter([
                    ("title".to_string(), serde_json::json!("v2")),
                    ("rank".to_string(), serde_json::json!(2)),
                ]),
                |_, _| Ok(()),
            )
            .expect("update should succeed");
        let (delete, _) = opened
            .store
            .delete_validated_returning_document(&document.table, &document.id, |_| Ok(()))
            .expect("delete should succeed");

        let intervals = opened
            .store
            .index_version_intervals_for_testing(&table_id, &index.id)
            .expect("index versions should load");

        assert_eq!(intervals.len(), 2);
        assert!(
            intervals
                .iter()
                .all(|interval| interval.document_id == document.id)
        );
        assert_eq!(intervals[0].visible_from, insert.sequence);
        assert_eq!(intervals[0].visible_until, Some(update.sequence));
        assert_eq!(intervals[1].visible_from, update.sequence);
        assert_eq!(intervals[1].visible_until, Some(delete.sequence));
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn mysql_index_versions_are_materialized_during_durable_recovery() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("index-version-recovery").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let table = TableName::new("indexed_replay_tasks").expect("table name should be valid");
        let (schema, index) = mysql_indexed_rank_schema(&table);
        opened
            .store
            .replace_table_schema(&schema)
            .expect("schema should persist");
        let diagnostics = opened
            .store
            .table_identity_diagnostics()
            .expect("table identity diagnostics should load");
        let table_id = mysql_active_table_id_for_diagnostic(&diagnostics, &table);
        let inserted = mysql_ranked_document(&table, "v1", 1);
        let mut updated = inserted.clone();
        updated
            .fields
            .insert("title".to_string(), serde_json::json!("v2"));
        updated
            .fields
            .insert("rank".to_string(), serde_json::json!(2));
        updated.update_time = Timestamp(updated.update_time.0.saturating_add(1));
        let records = vec![
            mysql_durable_write_record(
                SequenceNumber(2),
                Timestamp(100),
                &table,
                &table_id,
                WriteOpType::Insert,
                inserted.id.clone(),
                None,
                Some(inserted.clone()),
            ),
            mysql_durable_write_record(
                SequenceNumber(3),
                Timestamp(101),
                &table,
                &table_id,
                WriteOpType::Update,
                inserted.id.clone(),
                Some(inserted.clone()),
                Some(updated.clone()),
            ),
            mysql_durable_write_record(
                SequenceNumber(4),
                Timestamp(102),
                &table,
                &table_id,
                WriteOpType::Delete,
                inserted.id.clone(),
                Some(updated),
                None,
            ),
        ];

        opened
            .store
            .append_durable_records_batch(&records)
            .expect("durable append should succeed");
        assert!(
            opened
                .store
                .index_version_intervals_for_testing(&table_id, &index.id)
                .expect("unapplied index versions should load")
                .is_empty(),
            "durable-only records must not materialize index versions before recovery"
        );

        opened
            .store
            .recover_durable_journal()
            .expect("durable recovery should succeed");

        let intervals = opened
            .store
            .index_version_intervals_for_testing(&table_id, &index.id)
            .expect("index versions should load after recovery");
        assert_eq!(intervals.len(), 2);
        assert_eq!(intervals[0].visible_from, SequenceNumber(2));
        assert_eq!(intervals[0].visible_until, Some(SequenceNumber(3)));
        assert_eq!(intervals[1].visible_from, SequenceNumber(3));
        assert_eq!(intervals[1].visible_until, Some(SequenceNumber(4)));
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn mysql_historical_index_scan_eq_and_range_use_versioned_visibility() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("historical-index").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let table = TableName::new("historical_indexed_tasks").expect("table name should be valid");
        let (schema, _) = mysql_indexed_rank_schema(&table);
        opened
            .store
            .replace_table_schema(&schema)
            .expect("schema should persist");
        let document = mysql_ranked_document(&table, "v1", 1);
        let insert = opened
            .store
            .insert(&document)
            .expect("insert should succeed");
        let table_id = insert.writes[0].table_id.clone();
        let update = opened
            .store
            .update_validated(
                &document.table,
                &document.id,
                &serde_json::Map::from_iter([
                    ("title".to_string(), serde_json::json!("v2")),
                    ("rank".to_string(), serde_json::json!(2)),
                ]),
                |_, _| Ok(()),
            )
            .expect("update should succeed");
        let (delete, _) = opened
            .store
            .delete_validated_returning_document(&document.table, &document.id, |_| Ok(()))
            .expect("delete should succeed");

        let at_insert = mysql_historical_read_shape(&table, &table_id, &schema, insert.sequence);
        let rank_one = opened
            .store
            .historical_index_scan_eq_cancellable(
                &at_insert,
                "by_rank",
                &serde_json::json!(1),
                &mut || Ok(()),
            )
            .expect("historical rank=1 scan should succeed");
        assert_eq!(mysql_document_titles(&rank_one), vec!["v1"]);
        assert_eq!(
            mysql_document_title_strings(&rank_one),
            mysql_rank_full_scan_oracle_titles(
                &opened.store,
                &table,
                &table_id,
                &[&document],
                insert.sequence,
                1
            )
        );
        assert!(
            opened
                .store
                .historical_index_scan_eq_cancellable(
                    &at_insert,
                    "by_rank",
                    &serde_json::json!(2),
                    &mut || Ok(())
                )
                .expect("historical rank=2 scan should succeed")
                .is_empty()
        );

        let at_update = mysql_historical_read_shape(&table, &table_id, &schema, update.sequence);
        let rank_two = opened
            .store
            .historical_index_scan_range_cancellable(
                &at_update,
                "by_rank",
                Bound::Included(&serde_json::json!(2)),
                Bound::Included(&serde_json::json!(2)),
                &mut || Ok(()),
            )
            .expect("historical rank range scan should succeed");
        assert_eq!(mysql_document_titles(&rank_two), vec!["v2"]);
        assert_eq!(
            mysql_document_title_strings(&rank_two),
            mysql_rank_full_scan_oracle_titles(
                &opened.store,
                &table,
                &table_id,
                &[&document],
                update.sequence,
                2
            )
        );

        let at_delete = mysql_historical_read_shape(&table, &table_id, &schema, delete.sequence);
        let deleted_rank_two = opened
            .store
            .historical_index_scan_eq_cancellable(
                &at_delete,
                "by_rank",
                &serde_json::json!(2),
                &mut || Ok(()),
            )
            .expect("historical deleted rank scan should succeed");
        assert_eq!(
            mysql_document_title_strings(&deleted_rank_two),
            mysql_rank_full_scan_oracle_titles(
                &opened.store,
                &table,
                &table_id,
                &[&document],
                delete.sequence,
                2
            )
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn mysql_historical_index_prefix_composite_range_and_pagination_are_stable() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("historical-composite").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let table =
            TableName::new("historical_composite_tasks").expect("table name should be valid");
        let schema = mysql_status_rank_schema(&table);
        opened
            .store
            .replace_table_schema(&schema)
            .expect("schema should persist");
        let first = mysql_status_rank_document(&table, "first", "open", 1);
        let second = mysql_status_rank_document(&table, "second", "open", 2);
        let third = mysql_status_rank_document(&table, "third", "closed", 3);
        let first_insert = opened
            .store
            .insert(&first)
            .expect("first insert should succeed");
        let table_id = first_insert.writes[0].table_id.clone();
        opened
            .store
            .insert(&second)
            .expect("second insert should succeed");
        let third_insert = opened
            .store
            .insert(&third)
            .expect("third insert should succeed");

        let read_shape =
            mysql_historical_read_shape(&table, &table_id, &schema, third_insert.sequence);
        let open_docs = opened
            .store
            .historical_index_scan_prefix_cancellable(
                &read_shape,
                "by_status_rank",
                &[serde_json::json!("open")],
                &mut || Ok(()),
            )
            .expect("historical prefix scan should succeed");
        assert_eq!(mysql_document_titles(&open_docs), vec!["first", "second"]);
        assert_eq!(
            mysql_document_title_strings(&open_docs),
            mysql_status_rank_full_scan_oracle_titles(
                &opened.store,
                &table_id,
                &[&first, &second, &third],
                third_insert.sequence,
                "open",
                None,
                None
            )
        );

        let exact_rank_two = opened
            .store
            .historical_index_scan_composite_range_cancellable(
                &read_shape,
                "by_status_rank",
                &[serde_json::json!("open")],
                Bound::Included(&serde_json::json!(2)),
                Bound::Included(&serde_json::json!(2)),
                &mut || Ok(()),
            )
            .expect("historical composite range scan should succeed");
        assert_eq!(mysql_document_titles(&exact_rank_two), vec!["second"]);
        assert_eq!(
            mysql_document_title_strings(&exact_rank_two),
            mysql_status_rank_full_scan_oracle_titles(
                &opened.store,
                &table_id,
                &[&first, &second, &third],
                third_insert.sequence,
                "open",
                Some(2),
                Some(2)
            )
        );

        let first_page = opened
            .store
            .historical_index_scan_prefix_page_cancellable(
                &read_shape,
                "by_status_rank",
                &[serde_json::json!("open")],
                None,
                1,
                &mut || Ok(()),
            )
            .expect("first historical page should succeed");
        assert_eq!(mysql_document_titles(&first_page.documents), vec!["first"]);
        let cursor = first_page
            .next_cursor
            .as_ref()
            .expect("first page should return a cursor");
        let second_page = opened
            .store
            .historical_index_scan_prefix_page_cancellable(
                &read_shape,
                "by_status_rank",
                &[serde_json::json!("open")],
                Some(cursor),
                1,
                &mut || Ok(()),
            )
            .expect("second historical page should succeed");
        assert_eq!(
            mysql_document_titles(&second_page.documents),
            vec!["second"]
        );

        let mismatch = opened
            .store
            .historical_index_scan_prefix_page_cancellable(
                &read_shape,
                "by_status_rank",
                &[serde_json::json!("closed")],
                Some(cursor),
                1,
                &mut || Ok(()),
            )
            .expect_err("cursor from a different prefix must fail closed");
        assert_eq!(
            mismatch.historical_read_kind(),
            Some(nimbus_core::HistoricalReadErrorKind::CursorMismatch)
        );
    })
    .await;
}
