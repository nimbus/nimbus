use super::support::*;

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn libsql_document_versions_track_direct_write_history_and_snapshot_cache() {
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

        let replica_path = provider
            .refresh_tenant_snapshot(&tenant)
            .await
            .expect("tenant snapshot should refresh after version writes");
        let local = SqliteTenantStore::open(&replica_path)
            .expect("refreshed local replica cache should open");
        let local_at_update = local
            .get_document_version_at(&document.table, &table_id, &document.id, update.sequence)
            .expect("local cache update version should load")
            .expect("local cache update version should exist");
        assert_eq!(
            local_at_update.fields.get("title"),
            Some(&serde_json::json!("v2"))
        );
        assert!(
            local
                .get_document_version_at(&document.table, &table_id, &document.id, delete.sequence)
                .expect("local cache delete version should load")
                .is_none(),
            "local cache should copy delete tombstones"
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn libsql_document_versions_storage_diagnostic_reports_format_and_range() {
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
#[serial]
async fn libsql_document_versions_are_materialized_during_durable_recovery() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("document-version-recovery").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let fixture =
            crate::tests::provider_scenarios::exercise_document_versions_are_materialized_during_durable_recovery(
                opened.store.as_ref(),
            );

        // The replica cache must carry the replayed versions too, not just the
        // remote primary the shared body asserted against.
        let replica_path = provider
            .refresh_tenant_snapshot(&tenant)
            .await
            .expect("tenant snapshot should refresh after replayed version writes");
        let local = SqliteTenantStore::open(&replica_path)
            .expect("refreshed local replica cache should open");
        let local_at_insert = local
            .get_document_version_at(
                &fixture.table,
                &fixture.table_id,
                &fixture.document_id,
                SequenceNumber(1),
            )
            .expect("local cache insert version should load")
            .expect("local cache insert version should exist");
        assert_eq!(
            local_at_insert.fields.get("title"),
            Some(&serde_json::json!("v1"))
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn libsql_index_versions_track_direct_write_history_and_snapshot_cache() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("index-versions").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let table = TableName::new("indexed_versioned_tasks").expect("table name should be valid");
        let (schema, index) = indexed_rank_schema(&table);
        opened
            .store
            .replace_table_schema(&schema)
            .expect("schema should persist");
        let document = ranked_document(&table, "v1", 1);
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

        let replica_path = provider
            .refresh_tenant_snapshot(&tenant)
            .await
            .expect("tenant snapshot should refresh after index version writes");
        let local = SqliteTenantStore::open(&replica_path)
            .expect("refreshed local replica cache should open");
        let local_intervals = local
            .index_version_intervals_for_testing(&table_id, &index.id)
            .expect("local cache index versions should load");
        assert_eq!(local_intervals.len(), intervals.len());
        for (local_interval, remote_interval) in local_intervals.iter().zip(intervals.iter()) {
            assert_eq!(local_interval.document_id, remote_interval.document_id);
            assert_eq!(local_interval.visible_from, remote_interval.visible_from);
            assert_eq!(local_interval.visible_until, remote_interval.visible_until);
        }
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn libsql_index_versions_are_materialized_during_durable_recovery() {
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
#[serial]
async fn libsql_historical_index_scan_eq_and_range_use_versioned_visibility() {
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
#[serial]
async fn libsql_historical_index_prefix_composite_range_and_pagination_are_stable() {
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
