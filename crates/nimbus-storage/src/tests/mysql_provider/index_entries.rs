use super::support::*;

fn tasks_schema(table: &TableName) -> (TableSchema, nimbus_core::IndexId, nimbus_core::IndexId) {
    let by_team = nimbus_core::IndexId::new();
    let by_rank = nimbus_core::IndexId::new();
    let table_schema = TableSchema {
        table: table.clone(),
        fields: vec![
            FieldSchema {
                name: "team".to_string(),
                field_type: FieldType::String,
                required: false,
            },
            FieldSchema {
                name: "rank".to_string(),
                field_type: FieldType::Number,
                required: false,
            },
        ],
        indexes: vec![
            nimbus_core::IndexDefinition {
                id: by_team.clone(),
                state: nimbus_core::IndexState::Enabled,
                name: "by_team".to_string(),
                fields: vec!["team".to_string()],
            },
            nimbus_core::IndexDefinition {
                id: by_rank.clone(),
                state: nimbus_core::IndexState::Enabled,
                name: "by_rank".to_string(),
                fields: vec!["rank".to_string()],
            },
        ],
        access_policy: None,
    };
    (table_schema, by_team, by_rank)
}

fn rows_for<'a>(
    rows: &'a [IndexEntryRow],
    index_id: &nimbus_core::IndexId,
    document: &Document,
) -> Vec<&'a IndexEntryRow> {
    rows.iter()
        .filter(|(_, row_index, row_document, _)| {
            row_index == index_id.as_str() && row_document == &document.id.to_string()
        })
        .collect()
}

/// Every document write keeps the current-state index keyspace in step: one
/// row per maintained index that the document carries a tuple for, updated in
/// place on a change and removed on a delete.
#[tokio::test(flavor = "multi_thread")]
async fn mysql_writes_maintain_index_entries() {
    with_test_provider(|provider, config| async move {
        let tenant = TenantId::new("index-entries").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let table = TableName::new("tasks").expect("table name should build");
        let (table_schema, by_team, by_rank) = tasks_schema(&table);
        opened
            .store
            .replace_table_schema(&table_schema)
            .expect("schema write should succeed");
        let table_id = opened
            .store
            .table_identity_diagnostics()
            .expect("table identity diagnostics should load")
            .into_iter()
            .find(|entry| entry.table_name == table)
            .expect("table identity should exist")
            .table_id;
        let database_name = opened.store.database_name().to_string();

        let first = Document::new(
            table.clone(),
            serde_json::json!({"team": "core", "rank": 1})
                .as_object()
                .cloned()
                .expect("document body should be an object"),
        );
        let second = Document::new(
            table.clone(),
            serde_json::json!({"team": "infra", "rank": 2})
                .as_object()
                .cloned()
                .expect("document body should be an object"),
        );
        opened
            .store
            .insert(&first)
            .expect("first insert should succeed");
        opened
            .store
            .insert(&second)
            .expect("second insert should succeed");

        let rows = index_entry_rows(&config.connection_string, &database_name).await;
        assert_eq!(rows.len(), 4, "two documents times two indexes: {rows:?}");
        assert!(
            rows.iter()
                .all(|(row_table, _, _, _)| row_table == table_id.as_str()),
            "every row belongs to the table identity: {rows:?}"
        );
        let first_rank_before = rows_for(&rows, &by_rank, &first)[0].3.clone();
        let first_team_before = rows_for(&rows, &by_team, &first)[0].3.clone();

        let mut patch = serde_json::Map::new();
        patch.insert("rank".to_string(), serde_json::json!(9));
        opened
            .store
            .update_validated(&table, &first.id, &patch, |_, _| Ok(()))
            .expect("update should succeed");
        let rows = index_entry_rows(&config.connection_string, &database_name).await;
        assert_eq!(rows.len(), 4, "an update replaces rows in place: {rows:?}");
        let first_rank_after = rows_for(&rows, &by_rank, &first);
        assert_eq!(first_rank_after.len(), 1);
        assert_ne!(
            first_rank_after[0].3, first_rank_before,
            "the by_rank row carries the new tuple"
        );
        let first_team_after = rows_for(&rows, &by_team, &first);
        assert_eq!(first_team_after.len(), 1);
        assert_eq!(
            first_team_after[0].3, first_team_before,
            "the by_team row is unchanged"
        );

        opened
            .store
            .delete_validated_returning_document(&table, &first.id, |_| Ok(()))
            .expect("delete should succeed");
        let rows = index_entry_rows(&config.connection_string, &database_name).await;
        assert_eq!(rows.len(), 2, "a delete removes every row of the document");
        assert!(rows_for(&rows, &by_rank, &first).is_empty());
        assert!(rows_for(&rows, &by_team, &first).is_empty());

        let team_only = Document::new(
            table.clone(),
            serde_json::json!({"team": "ops"})
                .as_object()
                .cloned()
                .expect("document body should be an object"),
        );
        opened
            .store
            .insert(&team_only)
            .expect("team-only insert should succeed");
        let rows = index_entry_rows(&config.connection_string, &database_name).await;
        assert_eq!(
            rows.len(),
            3,
            "a document without the indexed field has no row"
        );
        assert_eq!(rows_for(&rows, &by_team, &team_only).len(), 1);
        assert!(rows_for(&rows, &by_rank, &team_only).is_empty());
    })
    .await;
}
