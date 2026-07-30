use super::support::*;

#[tokio::test(flavor = "multi_thread")]
async fn postgres_index_reads_round_trip_after_schema_write() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("indexed-reads").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        crate::tests::sql_pair_scenarios::exercise_index_reads_round_trip_after_schema_write(
            opened.store.as_ref(),
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn postgres_table_lifecycle_activates_hidden_identity_and_diagnostics_track_layout() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("table-lifecycle").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let table = TableName::new("tasks_lifecycle").expect("table name should build");
        let schema = TableSchema {
            table: table.clone(),
            fields: Vec::new(),
            indexes: vec![IndexDefinition {
                id: nimbus_core::IndexId::new(),
                state: nimbus_core::IndexState::Enabled,
                name: "by_title".to_string(),
                fields: vec!["title".to_string()],
            }],
            access_policy: None,
        };
        opened
            .store
            .replace_table_schema(&schema)
            .expect("schema write should succeed");

        let old_document = Document::new(
            table.clone(),
            serde_json::Map::from_iter([("title".to_string(), serde_json::json!("old"))]),
        );
        let old_commit = opened
            .store
            .insert(&old_document)
            .expect("old document should insert");
        let old_table_id = old_commit.writes[0].table_id.clone();
        let replacement_table_id = TableId::new();

        opened
            .store
            .stage_hidden_table_identity(&table, &replacement_table_id)
            .expect("hidden replacement identity should stage");
        let staged = opened
            .store
            .table_identity_diagnostics()
            .expect("diagnostics should load after staging");
        assert!(staged.iter().any(|diagnostic| {
            diagnostic.table_name == table
                && diagnostic.table_id == replacement_table_id
                && diagnostic.state == TableState::Hidden
                && diagnostic.backend_layout == crate::TableBackendLayout::SharedDocumentsByTableId
                && diagnostic.summary_status == crate::TableSummaryStatus::Unsupported
                && diagnostic.document_count.is_none()
        }));

        let retired = opened
            .store
            .activate_hidden_table_identity(&table, &replacement_table_id)
            .expect("hidden identity should activate");
        assert_eq!(retired.as_ref(), Some(&old_table_id));
        assert_eq!(
            opened.store.table_id(&table).expect("table id should load"),
            Some(replacement_table_id.clone())
        );
        assert!(
            opened
                .store
                .get(&table, &old_document.id)
                .expect("logical get should use active replacement")
                .is_none()
        );

        let new_document = Document::new(
            table.clone(),
            serde_json::Map::from_iter([("title".to_string(), serde_json::json!("new"))]),
        );
        let new_commit = opened
            .store
            .insert(&new_document)
            .expect("new document should insert under replacement identity");
        assert_eq!(new_commit.writes[0].table_id, replacement_table_id);

        let diagnostics = opened
            .store
            .table_identity_diagnostics()
            .expect("diagnostics should load after activation");
        let active = diagnostics
            .iter()
            .find(|diagnostic| {
                diagnostic.table_name == table && diagnostic.table_id == replacement_table_id
            })
            .expect("active replacement diagnostic should exist");
        assert_eq!(active.state, TableState::Active);
        assert_eq!(active.document_count, Some(1));
        assert_eq!(
            active.summary_status,
            crate::TableSummaryStatus::ExactDocumentCount
        );
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.table_name == table
                && diagnostic.table_id == old_table_id
                && diagnostic.state == TableState::Deleting
                && diagnostic.document_count.is_none()
        }));

        assert!(
            opened
                .store
                .hard_delete_table_identity(&old_table_id)
                .expect("hard delete should succeed")
        );
        let diagnostics = opened
            .store
            .table_identity_diagnostics()
            .expect("diagnostics should load after hard delete");
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic.table_id != old_table_id),
            "hard delete should remove retired catalog identity: {diagnostics:?}"
        );
        let mut check_cancel = || Ok(());
        assert_eq!(
            opened
                .store
                .index_scan_eq_cancellable(
                    &table,
                    "by_title",
                    &serde_json::json!("new"),
                    &mut check_cancel,
                )
                .expect("active replacement index scan should succeed"),
            vec![new_document]
        );
    })
    .await;
}
