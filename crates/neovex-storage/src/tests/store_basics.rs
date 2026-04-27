use super::*;

#[test]
fn update_applies_patch_and_appends_commit() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let document = sample_document("tasks", "Before");

    store.insert(&document).expect("insert should succeed");
    let commit = store
        .update(
            &document.table,
            &document.id,
            &serde_json::Map::from_iter([("title".to_string(), json!("After"))]),
        )
        .expect("update should succeed");
    let fetched = store
        .get(&document.table, &document.id)
        .expect("get should succeed")
        .expect("document should exist");

    assert_eq!(commit.sequence, SequenceNumber(2));
    assert_eq!(fetched.fields.get("title"), Some(&json!("After")));
}

#[test]
fn delete_removes_document_and_appends_commit() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let document = sample_document("tasks", "Disposable");

    store.insert(&document).expect("insert should succeed");
    let commit = store
        .delete(&document.table, &document.id)
        .expect("delete should succeed");
    let fetched = store
        .get(&document.table, &document.id)
        .expect("get should succeed");

    assert_eq!(commit.sequence, SequenceNumber(2));
    assert!(fetched.is_none());
}

#[test]
fn store_reopens_from_disk() {
    let dir = tempdir().expect("tempdir should create");
    let path = dir.path().join("tenant.redb");
    let document = sample_document("tasks", "Persisted");

    {
        let store = TenantStore::open(&path).expect("store should open");
        store.insert(&document).expect("insert should succeed");
    }

    let reopened = TenantStore::open(&path).expect("store should reopen");
    let documents = reopened
        .scan_table(&TableName::new("tasks").expect("table name should be valid"))
        .expect("scan should succeed");

    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].fields.get("title"), Some(&json!("Persisted")));
}

#[test]
fn store_reopens_with_typed_scalar_metadata_intact() {
    let dir = tempdir().expect("tempdir should create");
    let path = dir.path().join("tenant.redb");
    let mut document = sample_document("tasks", "Typed");
    document.set_typed_field(
        "updatedAt",
        neovex_core::TypedScalarValue::Timestamp {
            value: Timestamp(1_234),
        },
    );
    document.set_typed_field(
        "ceiling",
        neovex_core::TypedScalarValue::SpecialDouble {
            value: neovex_core::SpecialDouble::PositiveInfinity,
        },
    );

    {
        let store = TenantStore::open(&path).expect("store should open");
        store.insert(&document).expect("insert should succeed");
    }

    let reopened = TenantStore::open(&path).expect("store should reopen");
    let fetched = reopened
        .get(&document.table, &document.id)
        .expect("get should succeed")
        .expect("document should exist");

    assert_eq!(
        fetched.typed_field("updatedAt"),
        Some(&neovex_core::TypedScalarValue::Timestamp {
            value: Timestamp(1_234),
        })
    );
    assert_eq!(fetched.get_field("updatedAt"), Some(&json!(1234_u64)));
    assert_eq!(
        fetched.typed_field("ceiling"),
        Some(&neovex_core::TypedScalarValue::SpecialDouble {
            value: neovex_core::SpecialDouble::PositiveInfinity,
        })
    );
    assert_eq!(fetched.get_field("ceiling"), Some(&json!("Infinity")));
}

#[test]
fn store_get_nonexistent_document_returns_none() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let result = store
        .get(
            &TableName::new("tasks").expect("table name should be valid"),
            &DocumentId::new(),
        )
        .expect("get should succeed");

    assert!(result.is_none());
}

#[test]
fn store_scan_empty_table_returns_empty_vec() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let documents = store
        .scan_table(&TableName::new("tasks").expect("table name should be valid"))
        .expect("scan should succeed");

    assert!(documents.is_empty());
}

#[test]
fn store_latest_sequence_on_fresh_store_returns_zero() {
    let store = TenantStore::create_in_memory().expect("store should open");
    assert_eq!(
        store
            .latest_sequence()
            .expect("latest sequence should succeed"),
        SequenceNumber(0)
    );
}

#[test]
fn store_update_nonexistent_document_returns_error() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let error = store
        .update(
            &TableName::new("tasks").expect("table name should be valid"),
            &DocumentId::new(),
            &serde_json::Map::from_iter([("title".to_string(), json!("After"))]),
        )
        .expect_err("update should fail");

    assert!(matches!(error, Error::DocumentNotFound(_)));
}

#[test]
fn store_delete_nonexistent_document_returns_error() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let error = store
        .delete(
            &TableName::new("tasks").expect("table name should be valid"),
            &DocumentId::new(),
        )
        .expect_err("delete should fail");

    assert!(matches!(error, Error::DocumentNotFound(_)));
}

#[test]
fn schema_roundtrip_through_redb() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let table_schema = TableSchema {
        table: TableName::new("users").expect("table name should be valid"),
        fields: vec![
            FieldSchema {
                name: "name".to_string(),
                field_type: FieldType::String,
                required: true,
            },
            FieldSchema {
                name: "age".to_string(),
                field_type: FieldType::Number,
                required: false,
            },
        ],
        indexes: Vec::new(),
        access_policy: None,
    };

    store
        .save_table_schema(&table_schema)
        .expect("schema should save");
    let schema = store.load_schema().expect("schema should load");

    assert_eq!(schema.get_table(&table_schema.table), Some(&table_schema));

    store
        .delete_table_schema_entry(&table_schema.table)
        .expect("schema entry should delete");
    let schema = store.load_schema().expect("schema should load");
    assert!(schema.get_table(&table_schema.table).is_none());
}
