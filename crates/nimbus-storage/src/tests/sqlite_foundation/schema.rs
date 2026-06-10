use std::ops::Bound;

use super::support::*;

#[test]
fn sqlite_table_identity_diagnostics_report_layout_state_and_counts() {
    let dir = tempdir().expect("temporary directory should create");
    let path = dir.path().join("tenant.sqlite3");
    let store = SqliteTenantStore::open(&path).expect("sqlite tenant store should open");
    let table = TableName::new("diagnostic_tasks").expect("table name should build");
    let first = Document::new(table.clone(), serde_json::Map::new());
    let second = Document::new(table.clone(), serde_json::Map::new());
    store.insert(&first).expect("first insert should succeed");
    store.insert(&second).expect("second insert should succeed");

    let diagnostics = store
        .table_identity_diagnostics()
        .expect("diagnostics should load");

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].table_name, table);
    assert_eq!(diagnostics[0].state, nimbus_core::TableState::Active);
    assert_eq!(
        diagnostics[0].backend_layout,
        crate::TableBackendLayout::SharedDocumentsByTableId
    );
    assert_eq!(diagnostics[0].document_count, Some(2));
    assert_eq!(
        diagnostics[0].summary_status,
        crate::TableSummaryStatus::ExactDocumentCount
    );
}

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
            id: nimbus_core::IndexId::new(),
            state: nimbus_core::IndexState::Enabled,
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
            id: nimbus_core::IndexId::new(),
            state: nimbus_core::IndexState::Enabled,
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
            id: nimbus_core::IndexId::new(),
            state: nimbus_core::IndexState::Enabled,
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
            id: nimbus_core::IndexId::new(),
            state: nimbus_core::IndexState::Enabled,
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
        update_time: Timestamp(1),
        fields: serde_json::Map::from_iter([
            ("status".to_string(), json!("open")),
            ("rank".to_string(), json!(1)),
        ]),
        typed_fields: Default::default(),
    };
    let open_three = Document {
        id: DocumentId::new(),
        table: table.clone(),
        creation_time: Timestamp(2),
        update_time: Timestamp(2),
        fields: serde_json::Map::from_iter([
            ("status".to_string(), json!("open")),
            ("rank".to_string(), json!(3)),
        ]),
        typed_fields: Default::default(),
    };
    let closed_two = Document {
        id: DocumentId::new(),
        table: table.clone(),
        creation_time: Timestamp(3),
        update_time: Timestamp(3),
        fields: serde_json::Map::from_iter([
            ("status".to_string(), json!("closed")),
            ("rank".to_string(), json!(2)),
        ]),
        typed_fields: Default::default(),
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
            Bound::Included(&json!(2)),
            Bound::Included(&json!(4)),
            &mut || Ok(()),
        )
        .expect("composite range scan should succeed");
    assert_eq!(composite, vec![open_three.clone()]);
}

#[test]
fn sqlite_documents_are_physically_keyed_by_table_id() {
    let dir = tempdir().expect("temporary directory should create");
    let path = dir.path().join("tenant.sqlite3");
    let store = SqliteTenantStore::open(&path).expect("sqlite tenant store should open");
    let table = TableName::new("tasks").expect("table should build");
    let document = Document::new(
        table.clone(),
        serde_json::Map::from_iter([("body".to_string(), json!("ship table ids"))]),
    );

    store.insert(&document).expect("insert should succeed");

    let conn = rusqlite::Connection::open(&path).expect("raw sqlite connection should open");
    let document_columns = table_columns(&conn, "documents");
    assert!(
        document_columns.iter().any(|column| column == "table_id"),
        "documents table must carry the stable physical table id: {document_columns:?}"
    );
    assert!(
        !document_columns.iter().any(|column| column == "table_name"),
        "documents table must not be physically keyed by logical table name: {document_columns:?}"
    );

    let (catalog_table_name, catalog_table_id, catalog_state): (String, String, String) = conn
        .query_row(
            "SELECT table_name, table_id, state FROM table_catalog WHERE namespace = 'default'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("catalog row should exist");
    let (stored_table_id, stored_document_id): (String, String) = conn
        .query_row("SELECT table_id, id FROM documents", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .expect("document row should exist");

    assert_eq!(catalog_table_name, table.as_str());
    assert_eq!(catalog_state, "active");
    assert_eq!(stored_table_id, catalog_table_id);
    assert_eq!(stored_document_id, document.id.to_string());
    assert_eq!(
        store
            .get(&table, &document.id)
            .expect("logical get should succeed"),
        Some(document)
    );
}

#[test]
fn sqlite_writes_reject_deleting_table_identity() {
    let dir = tempdir().expect("temporary directory should create");
    let path = dir.path().join("tenant.sqlite3");
    let store = SqliteTenantStore::open(&path).expect("sqlite tenant store should open");
    let table = TableName::new("tasks_deleting").expect("table should build");
    let table_id = TableId::new();
    {
        let conn = rusqlite::Connection::open(&path).expect("raw sqlite connection should open");
        conn.execute(
            "INSERT INTO table_catalog (namespace, table_name, table_id, state)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params!["default", table.as_str(), table_id.as_str(), "deleting"],
        )
        .expect("deleting catalog state should insert");
    }

    let document = Document::new(
        table,
        serde_json::Map::from_iter([("body".to_string(), json!("blocked"))]),
    );
    let error = store
        .insert(&document)
        .expect_err("writes to deleting tables should fail");

    assert!(
        error.to_string().contains("deleting lifecycle state"),
        "deleting table rejection should be explicit: {error:?}"
    );
}

#[test]
fn sqlite_table_lifecycle_activates_hidden_identity_and_hard_deletes_old_data() {
    let dir = tempdir().expect("temporary directory should create");
    let path = dir.path().join("tenant.sqlite3");
    let store = SqliteTenantStore::open(&path).expect("sqlite tenant store should open");
    let table = TableName::new("tasks_lifecycle").expect("table should build");
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
    store
        .replace_table_schema(&schema)
        .expect("schema should persist");
    let old_document = Document::new(
        table.clone(),
        serde_json::Map::from_iter([("title".to_string(), json!("old"))]),
    );
    let old_commit = store
        .insert(&old_document)
        .expect("old document should insert");
    let old_table_id = old_commit.writes[0].table_id.clone();
    let replacement_table_id = TableId::new();

    store
        .stage_hidden_table_identity(&table, &replacement_table_id)
        .expect("hidden replacement identity should stage");
    let staged = store
        .read_snapshot()
        .expect("snapshot should open")
        .table_identities()
        .expect("table identities should export");
    assert!(
        staged.iter().any(|identity| {
            identity.namespace
                == crate::table_identity::hidden_table_namespace(&replacement_table_id)
                && identity.table == table
                && identity.table_id == replacement_table_id
                && identity.state == nimbus_core::TableState::Hidden
        }),
        "hidden replacement identity should be visible in catalog snapshots: {staged:?}"
    );

    let retired = store
        .activate_hidden_table_identity(&table, &replacement_table_id)
        .expect("hidden identity should activate");
    assert_eq!(retired.as_ref(), Some(&old_table_id));
    assert_eq!(
        store.table_id(&table).expect("table id should resolve"),
        Some(replacement_table_id.clone())
    );
    assert!(
        store
            .get(&table, &old_document.id)
            .expect("logical get should resolve against replacement identity")
            .is_none(),
        "old rows must not be reachable through the recreated logical table name"
    );

    let new_document = Document::new(
        table.clone(),
        serde_json::Map::from_iter([("title".to_string(), json!("new"))]),
    );
    let new_commit = store
        .insert(&new_document)
        .expect("new document should insert under replacement identity");
    assert_eq!(new_commit.writes[0].table_id, replacement_table_id);

    let conn = rusqlite::Connection::open(&path).expect("raw sqlite connection should open");
    let old_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM documents WHERE table_id = ?1",
            rusqlite::params![old_table_id.as_str()],
            |row| row.get(0),
        )
        .expect("old physical row count should read");
    assert_eq!(
        old_count, 1,
        "retired table data should remain until hard delete"
    );
    drop(conn);

    assert!(
        store
            .hard_delete_table_identity(&old_table_id)
            .expect("hard delete should succeed"),
        "hard delete should report that it removed the retiring table"
    );

    let conn = rusqlite::Connection::open(&path).expect("raw sqlite connection should reopen");
    let old_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM documents WHERE table_id = ?1",
            rusqlite::params![old_table_id.as_str()],
            |row| row.get(0),
        )
        .expect("old physical row count should read");
    let old_catalog_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM table_catalog WHERE table_id = ?1",
            rusqlite::params![old_table_id.as_str()],
            |row| row.get(0),
        )
        .expect("old catalog row count should read");
    assert_eq!(old_count, 0, "hard delete should remove retired rows");
    assert_eq!(
        old_catalog_count, 0,
        "hard delete should remove the retired catalog identity"
    );
    assert_eq!(
        store
            .index_scan_eq(&table, "by_title", &json!("new"))
            .expect("active replacement index scan should succeed"),
        vec![new_document]
    );
}

#[test]
fn sqlite_index_query_plan_builders_match_runtime_sql_shape() {
    let exact = crate::sqlite_index_scan_prefix_query_sql(&["status"], 1)
        .expect("single-field indexed query SQL should build");
    assert_eq!(
        exact,
        "SELECT id, creation_time, update_time, data_json, typed_fields_json
         FROM documents
         WHERE table_id = ?1 AND json_extract(data_json, '$.\"status\"') = ?2
         ORDER BY id"
    );

    let composite = crate::sqlite_index_scan_composite_range_query_sql(
        &["team", "status", "rank"],
        2,
        Bound::Included(()),
        Bound::Excluded(()),
    )
    .expect("composite indexed query SQL should build");
    assert_eq!(
        composite,
        "SELECT id, creation_time, update_time, data_json, typed_fields_json
         FROM documents
         WHERE table_id = ?1 AND json_extract(data_json, '$.\"team\"') = ?2 AND json_extract(data_json, '$.\"status\"') = ?3 AND json_extract(data_json, '$.\"rank\"') >= ?4 AND json_extract(data_json, '$.\"rank\"') < ?5
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
                id: nimbus_core::IndexId::new(),
                state: nimbus_core::IndexState::Enabled,
                name: "by_status".to_string(),
                fields: vec!["status".to_string()],
            },
            IndexDefinition {
                id: nimbus_core::IndexId::new(),
                state: nimbus_core::IndexState::Enabled,
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
        rusqlite::params![
            sqlite_table_id_for_test(&conn, schema.table.as_str()),
            "open"
        ],
    );
    assert!(
        exact_plan
            .iter()
            .any(|detail| detail.contains(&format!("USING INDEX idx_{}", schema.indexes[0].id))),
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
            Bound::Included(()),
            Bound::Excluded(()),
        )
        .expect("composite indexed query SQL should build"),
        rusqlite::params![
            sqlite_table_id_for_test(&conn, schema.table.as_str()),
            "alpha",
            "open",
            500_i64,
            2_500_i64
        ],
    );
    assert!(
        composite_plan
            .iter()
            .any(|detail| detail.contains(&format!("USING INDEX idx_{}", schema.indexes[1].id))),
        "composite scan should use the intended index: {composite_plan:?}"
    );
    assert!(
        composite_plan
            .iter()
            .all(|detail| !detail.contains("USE TEMP B-TREE")),
        "composite scan should avoid a temp B-tree once equality-constrained order fields are elided: {composite_plan:?}"
    );
}

fn table_columns(conn: &rusqlite::Connection, table: &str) -> Vec<String> {
    let mut statement = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .expect("table_info query should prepare");
    statement
        .query_map([], |row| row.get::<_, String>(1))
        .expect("table_info rows should query")
        .collect::<Result<Vec<_>, _>>()
        .expect("table_info rows should decode")
}

fn sqlite_table_id_for_test(conn: &rusqlite::Connection, table: &str) -> String {
    conn.query_row(
        "SELECT table_id FROM table_catalog WHERE namespace = 'default' AND table_name = ?1",
        rusqlite::params![table],
        |row| row.get::<_, String>(0),
    )
    .expect("test table id should exist")
}
