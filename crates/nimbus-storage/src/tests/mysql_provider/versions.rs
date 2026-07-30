use super::support::*;

#[tokio::test(flavor = "multi_thread")]
async fn mysql_document_versions_track_direct_write_history() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("document-versions").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        crate::tests::sql_pair_scenarios::exercise_document_versions_track_direct_write_history(
            opened.store.as_ref(),
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
        crate::tests::provider_scenarios::exercise_document_versions_storage_diagnostic_reports_format_and_range(opened.store.as_ref());
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
        crate::tests::provider_scenarios::exercise_document_versions_are_materialized_during_durable_recovery(
            opened.store.as_ref(),
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
        crate::tests::sql_pair_scenarios::exercise_index_versions_track_direct_write_history(
            opened.store.as_ref(),
        );
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
        let (schema, index) = indexed_rank_schema(&table);
        opened
            .store
            .replace_table_schema(&schema)
            .expect("schema should persist");
        let diagnostics = opened
            .store
            .table_identity_diagnostics()
            .expect("table identity diagnostics should load");
        let table_id = active_table_id_for_diagnostic(&diagnostics, &table);
        let inserted = ranked_document(&table, "v1", 1);
        let mut updated = inserted.clone();
        updated
            .fields
            .insert("title".to_string(), serde_json::json!("v2"));
        updated
            .fields
            .insert("rank".to_string(), serde_json::json!(2));
        updated.update_time = Timestamp(updated.update_time.0.saturating_add(1));
        let records = vec![
            durable_write_record(
                SequenceNumber(2),
                Timestamp(100),
                &table,
                &table_id,
                WriteOpType::Insert,
                inserted.id.clone(),
                None,
                Some(inserted.clone()),
            ),
            durable_write_record(
                SequenceNumber(3),
                Timestamp(101),
                &table,
                &table_id,
                WriteOpType::Update,
                inserted.id.clone(),
                Some(inserted.clone()),
                Some(updated.clone()),
            ),
            durable_write_record(
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
        crate::tests::provider_scenarios::exercise_historical_index_scan_eq_and_range_use_versioned_visibility(
            opened.store.as_ref(),
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
        crate::tests::provider_scenarios::exercise_historical_index_prefix_composite_range_and_pagination_are_stable(
            opened.store.as_ref(),
        );
    })
    .await;
}
