use super::support::*;

#[tokio::test]
async fn sqlite_async_write_schema_change_persists_after_reopen() {
    let dir = tempdir().expect("temporary directory should create");
    let path = dir.path().join("tenant.sqlite3");
    let store = Arc::new(SqliteTenantStore::open(&path).expect("sqlite tenant store should open"));
    let first = Document::new(
        TableName::new("tasks").expect("table name should build"),
        serde_json::Map::from_iter([("rank".to_string(), serde_json::json!(7))]),
    );
    let second = Document::new(
        TableName::new("tasks").expect("table name should build"),
        serde_json::Map::from_iter([("rank".to_string(), serde_json::json!(9))]),
    );
    store
        .insert(&first)
        .expect("seed insert before async schema write should succeed");
    store
        .insert(&second)
        .expect("second seed insert before async schema write should succeed");
    let storage =
        SqliteTenantStorage::with_max_concurrent_reads(store, tokio::runtime::Handle::current(), 1);
    let schema = TableSchema {
        table: TableName::new("tasks").expect("table name should build"),
        fields: vec![FieldSchema {
            name: "rank".to_string(),
            field_type: FieldType::Number,
            required: false,
        }],
        indexes: vec![IndexDefinition {
            name: "by_rank".to_string(),
            fields: vec!["rank".to_string()],
        }],
        access_policy: None,
    };

    let schema_for_task = schema.clone();
    storage
        .execute_write(move |transaction| transaction.replace_table_schema(&schema_for_task))
        .await
        .expect("async schema write should succeed");

    let reopened = SqliteTenantStore::open(&path).expect("sqlite tenant store should reopen");
    let persisted = reopened
        .load_schema()
        .expect("schema should read after reopen");
    assert!(
        persisted.get_table(&schema.table).is_some(),
        "async sqlite schema writes should persist schema rows before the store reopens"
    );
    assert_eq!(
        reopened
            .index_scan_eq(&schema.table, "by_rank", &serde_json::json!(7))
            .expect("index scan should succeed after reopen")
            .len(),
        1,
        "async sqlite schema writes should also rebuild durable index entries for existing rows"
    );
}

#[tokio::test]
async fn sqlite_async_write_schema_change_updates_live_schema_cache() {
    let dir = tempdir().expect("temporary directory should create");
    let store = Arc::new(
        SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
            .expect("sqlite tenant store should open"),
    );
    let document = Document::new(
        TableName::new("tasks").expect("table name should build"),
        serde_json::Map::from_iter([
            ("rank".to_string(), serde_json::json!(7)),
            ("title".to_string(), serde_json::json!("alpha")),
        ]),
    );
    store
        .insert(&document)
        .expect("seed insert before async schema write should succeed");
    let storage = SqliteTenantStorage::with_max_concurrent_reads(
        store.clone(),
        tokio::runtime::Handle::current(),
        1,
    );
    let rank_schema = TableSchema {
        table: TableName::new("tasks").expect("table name should build"),
        fields: vec![
            FieldSchema {
                name: "rank".to_string(),
                field_type: FieldType::Number,
                required: false,
            },
            FieldSchema {
                name: "title".to_string(),
                field_type: FieldType::String,
                required: false,
            },
        ],
        indexes: vec![IndexDefinition {
            name: "by_rank".to_string(),
            fields: vec!["rank".to_string()],
        }],
        access_policy: None,
    };

    let rank_schema_for_task = rank_schema.clone();
    storage
        .execute_write(move |transaction| transaction.replace_table_schema(&rank_schema_for_task))
        .await
        .expect("async schema write should succeed");

    assert_eq!(
        store
            .load_schema()
            .expect("live schema cache should read")
            .get_table(&rank_schema.table),
        Some(&rank_schema)
    );
    assert_eq!(
        store
            .index_scan_eq(&rank_schema.table, "by_rank", &serde_json::json!(7))
            .expect("rank index scan should succeed after live cache refresh"),
        vec![document.clone()]
    );

    let title_schema = TableSchema {
        table: rank_schema.table.clone(),
        fields: rank_schema.fields.clone(),
        indexes: vec![IndexDefinition {
            name: "by_title".to_string(),
            fields: vec!["title".to_string()],
        }],
        access_policy: None,
    };
    let title_schema_for_task = title_schema.clone();
    storage
        .execute_write(move |transaction| transaction.replace_table_schema(&title_schema_for_task))
        .await
        .expect("second async schema write should succeed");

    assert_eq!(
        store
            .load_schema()
            .expect("live schema cache should refresh after second write")
            .get_table(&title_schema.table),
        Some(&title_schema)
    );
    assert_eq!(
        store
            .index_scan_eq(&title_schema.table, "by_title", &serde_json::json!("alpha"))
            .expect("new title index scan should succeed"),
        vec![document.clone()]
    );
    let error = store
        .index_scan_eq(&title_schema.table, "by_rank", &serde_json::json!(7))
        .expect_err("old index lookup should fail after schema replacement");
    assert!(
        matches!(error, Error::InvalidInput(_)),
        "old index lookups should fail once the live schema cache refreshes: {error:?}"
    );
}

#[test]
fn sqlite_store_round_trips_schema_get_and_index_scans() {
    let dir = tempdir().expect("temporary directory should create");
    let store = SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
        .expect("sqlite tenant store should open");
    let table = TableName::new("tasks").expect("table should build");
    let schema = TableSchema {
        table: table.clone(),
        fields: Vec::new(),
        indexes: vec![IndexDefinition {
            name: "by_status_rank".to_string(),
            fields: vec!["status".to_string(), "rank".to_string()],
        }],
        access_policy: None,
    };
    store
        .replace_table_schema(&schema)
        .expect("sqlite schema should save");
    assert_eq!(
        store
            .load_schema()
            .expect("schema should load")
            .get_table(&table),
        Some(&schema)
    );

    let open_one = Document {
        id: DocumentId::new(),
        table: table.clone(),
        creation_time: Timestamp(1),
        fields: serde_json::Map::from_iter([
            ("status".to_string(), json!("open")),
            ("rank".to_string(), json!(1)),
        ]),
    };
    let open_three = Document {
        id: DocumentId::new(),
        table: table.clone(),
        creation_time: Timestamp(2),
        fields: serde_json::Map::from_iter([
            ("status".to_string(), json!("open")),
            ("rank".to_string(), json!(3)),
        ]),
    };
    let closed_two = Document {
        id: DocumentId::new(),
        table: table.clone(),
        creation_time: Timestamp(3),
        fields: serde_json::Map::from_iter([
            ("status".to_string(), json!("closed")),
            ("rank".to_string(), json!(2)),
        ]),
    };
    for document in [&open_one, &open_three, &closed_two] {
        store
            .insert_document_for_testing(document)
            .expect("document should insert");
    }

    assert_eq!(
        store
            .get(&table, &open_one.id)
            .expect("get should succeed")
            .as_ref(),
        Some(&open_one)
    );

    let exact = store
        .index_scan_eq(&table, "by_status_rank", &json!("open"))
        .expect("exact scan should succeed");
    assert_eq!(
        exact
            .iter()
            .map(|document| {
                document
                    .get_field("rank")
                    .cloned()
                    .expect("rank should exist")
            })
            .collect::<Vec<_>>(),
        vec![json!(1), json!(3)]
    );

    let prefix = store
        .index_scan_prefix(&table, "by_status_rank", &[json!("open"), json!(3)])
        .expect("prefix scan should succeed");
    assert_eq!(prefix, vec![open_three.clone()]);

    let composite = store
        .index_scan_composite_range_cancellable(
            &table,
            "by_status_rank",
            &[json!("open")],
            Some(&json!(2)),
            Some(&json!(4)),
            true,
            true,
            &mut || Ok(()),
        )
        .expect("composite range scan should succeed");
    assert_eq!(composite, vec![open_three.clone()]);
}

#[test]
fn sqlite_index_query_plan_builders_match_runtime_sql_shape() {
    let exact = crate::sqlite_index_scan_prefix_query_sql(&["status"], 1)
        .expect("single-field indexed query SQL should build");
    assert_eq!(
        exact,
        "SELECT id, creation_time, data_json
         FROM documents
         WHERE table_name = ?1 AND json_extract(data_json, '$.\"status\"') = ?2
         ORDER BY id"
    );

    let composite = crate::sqlite_index_scan_composite_range_query_sql(
        &["team", "status", "rank"],
        2,
        true,
        true,
        true,
        false,
    )
    .expect("composite indexed query SQL should build");
    assert_eq!(
        composite,
        "SELECT id, creation_time, data_json
         FROM documents
         WHERE table_name = ?1 AND json_extract(data_json, '$.\"team\"') = ?2 AND json_extract(data_json, '$.\"status\"') = ?3 AND json_extract(data_json, '$.\"rank\"') >= ?4 AND json_extract(data_json, '$.\"rank\"') < ?5
         ORDER BY json_extract(data_json, '$.\"rank\"'), id"
    );
}

#[test]
fn sqlite_index_query_plans_elide_temp_btree_for_equality_prefixes() {
    let dir = tempdir().expect("temporary directory should create");
    let path = dir.path().join("tenant.sqlite3");
    let store = SqliteTenantStore::open(&path).expect("sqlite tenant store should open");
    let schema = TableSchema {
        table: TableName::new("tasks").expect("table name should build"),
        fields: vec![
            FieldSchema {
                name: "team".to_string(),
                field_type: FieldType::String,
                required: false,
            },
            FieldSchema {
                name: "status".to_string(),
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
            IndexDefinition {
                name: "by_status".to_string(),
                fields: vec!["status".to_string()],
            },
            IndexDefinition {
                name: "by_team_status_rank".to_string(),
                fields: vec!["team".to_string(), "status".to_string(), "rank".to_string()],
            },
        ],
        access_policy: None,
    };
    store
        .replace_table_schema(&schema)
        .expect("sqlite schema should save");

    let conn = rusqlite::Connection::open(&path).expect("raw sqlite connection should open");
    let exact_plan = explain_query_plan(
        &conn,
        &crate::sqlite_index_scan_prefix_query_sql(&["status"], 1)
            .expect("single-field indexed query SQL should build"),
        rusqlite::params!["tasks", "open"],
    );
    assert!(
        exact_plan
            .iter()
            .any(|detail| detail.contains("USING INDEX idx_tasks_by_status")),
        "single-field scan should use the intended index: {exact_plan:?}"
    );
    assert!(
        exact_plan
            .iter()
            .all(|detail| !detail.contains("USE TEMP B-TREE")),
        "single-field scan should avoid a temp B-tree once equality-constrained order fields are elided: {exact_plan:?}"
    );

    let composite_plan = explain_query_plan(
        &conn,
        &crate::sqlite_index_scan_composite_range_query_sql(
            &["team", "status", "rank"],
            2,
            true,
            true,
            true,
            false,
        )
        .expect("composite indexed query SQL should build"),
        rusqlite::params!["tasks", "alpha", "open", 500_i64, 2_500_i64],
    );
    assert!(
        composite_plan
            .iter()
            .any(|detail| detail.contains("USING INDEX idx_tasks_by_team_status_rank")),
        "composite scan should use the intended index: {composite_plan:?}"
    );
    assert!(
        composite_plan
            .iter()
            .all(|detail| !detail.contains("USE TEMP B-TREE")),
        "composite scan should avoid a temp B-tree once equality-constrained order fields are elided: {composite_plan:?}"
    );
}
