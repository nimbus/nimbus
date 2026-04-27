use neovex_core::{
    Document, DocumentId, FieldSchema, FieldType, IndexDefinition, TableName, TableSchema,
};
use serde_json::json;

use crate::TenantStore;

use super::encode_index_value;

#[test]
fn replace_table_schema_rebuilds_indexes_and_persists_schema() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let table = TableName::new("users").expect("table name should be valid");
    for email in ["a@test.com", "b@test.com", "a@test.com"] {
        let document = Document::new(
            table.clone(),
            serde_json::Map::from_iter([("email".to_string(), json!(email))]),
        );
        store.insert(&document).expect("insert should succeed");
    }

    let table_schema = TableSchema {
        table: table.clone(),
        fields: vec![FieldSchema {
            name: "email".to_string(),
            field_type: FieldType::String,
            required: false,
        }],
        indexes: vec![IndexDefinition {
            name: "by_email".to_string(),
            fields: vec!["email".to_string()],
        }],
        access_policy: None,
    };

    store
        .replace_table_schema(&table_schema)
        .expect("schema replacement should succeed");

    let schema = store.load_schema().expect("schema should load");
    assert_eq!(schema.get_table(&table), Some(&table_schema));

    let docs = store
        .index_scan_eq(&table, "by_email", &json!("a@test.com"))
        .expect("index scan should succeed");
    assert_eq!(docs.len(), 2);
}

#[test]
fn delete_table_schema_clears_schema_and_indexes() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let table = TableName::new("users").expect("table name should be valid");
    let document = Document::new(
        table.clone(),
        serde_json::Map::from_iter([("email".to_string(), json!("gone@test.com"))]),
    );
    store.insert(&document).expect("insert should succeed");

    let table_schema = TableSchema {
        table: table.clone(),
        fields: vec![FieldSchema {
            name: "email".to_string(),
            field_type: FieldType::String,
            required: false,
        }],
        indexes: vec![IndexDefinition {
            name: "by_email".to_string(),
            fields: vec!["email".to_string()],
        }],
        access_policy: None,
    };
    store
        .replace_table_schema(&table_schema)
        .expect("schema replacement should succeed");

    store
        .delete_table_schema(&table)
        .expect("schema deletion should succeed");

    let schema = store.load_schema().expect("schema should load");
    assert!(schema.get_table(&table).is_none());
    let docs = store
        .index_scan_eq(&table, "by_email", &json!("gone@test.com"))
        .expect("index scan should succeed");
    assert!(docs.is_empty());
}

#[test]
fn update_with_indexes_validated_maintains_entries() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let table = TableName::new("users").expect("table name should be valid");
    let index = IndexDefinition {
        name: "by_email".to_string(),
        fields: vec!["email".to_string()],
    };
    let document = Document::new(
        table.clone(),
        serde_json::Map::from_iter([("email".to_string(), json!("old@test.com"))]),
    );
    store
        .insert_with_indexes(&document, std::slice::from_ref(&index))
        .expect("insert should succeed");

    store
        .update_with_indexes_validated(
            &table,
            &document.id,
            &serde_json::Map::from_iter([("email".to_string(), json!("new@test.com"))]),
            std::slice::from_ref(&index),
            |_existing, updated| {
                assert_eq!(updated.fields.get("email"), Some(&json!("new@test.com")));
                Ok(())
            },
        )
        .expect("validated update should succeed");

    let old_docs = store
        .index_scan_eq(&table, "by_email", &json!("old@test.com"))
        .expect("old index scan should succeed");
    let new_docs = store
        .index_scan_eq(&table, "by_email", &json!("new@test.com"))
        .expect("new index scan should succeed");

    assert!(old_docs.is_empty());
    assert_eq!(new_docs.len(), 1);
    assert_eq!(
        new_docs[0].fields.get("email"),
        Some(&json!("new@test.com"))
    );
}

#[test]
fn index_key_encoding_preserves_number_sort_order() {
    let mut encoded = [
        encode_index_value(&json!(-1.5)).expect("value should encode"),
        encode_index_value(&json!(0)).expect("value should encode"),
        encode_index_value(&json!(1)).expect("value should encode"),
        encode_index_value(&json!(100)).expect("value should encode"),
    ];
    encoded.sort();

    assert_eq!(
        encoded[0],
        encode_index_value(&json!(-1.5)).expect("value should encode")
    );
    assert_eq!(
        encoded[1],
        encode_index_value(&json!(0)).expect("value should encode")
    );
    assert_eq!(
        encoded[2],
        encode_index_value(&json!(1)).expect("value should encode")
    );
    assert_eq!(
        encoded[3],
        encode_index_value(&json!(100)).expect("value should encode")
    );
}

#[test]
fn index_key_encoding_preserves_string_sort_order() {
    let mut encoded = [
        encode_index_value(&json!("charlie")).expect("value should encode"),
        encode_index_value(&json!("alpha")).expect("value should encode"),
        encode_index_value(&json!("bravo")).expect("value should encode"),
    ];
    encoded.sort();

    assert_eq!(
        encoded[0],
        encode_index_value(&json!("alpha")).expect("value should encode")
    );
    assert_eq!(
        encoded[1],
        encode_index_value(&json!("bravo")).expect("value should encode")
    );
    assert_eq!(
        encoded[2],
        encode_index_value(&json!("charlie")).expect("value should encode")
    );
}

#[test]
fn index_insert_and_eq_scan() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let index = IndexDefinition {
        name: "by_email".to_string(),
        fields: vec!["email".to_string()],
    };
    for email in ["a@test.com", "b@test.com", "c@test.com"] {
        let document = Document::new(
            TableName::new("users").expect("table name should be valid"),
            serde_json::Map::from_iter([("email".to_string(), json!(email))]),
        );
        store
            .insert_with_indexes(&document, std::slice::from_ref(&index))
            .expect("insert should succeed");
    }

    let match_docs = store
        .index_scan_eq(
            &TableName::new("users").expect("table name should be valid"),
            "by_email",
            &json!("b@test.com"),
        )
        .expect("index scan should succeed");
    assert_eq!(match_docs.len(), 1);
    assert_eq!(
        match_docs[0].fields.get("email"),
        Some(&json!("b@test.com"))
    );

    let missing_docs = store
        .index_scan_eq(
            &TableName::new("users").expect("table name should be valid"),
            "by_email",
            &json!("missing@test.com"),
        )
        .expect("index scan should succeed");
    assert!(missing_docs.is_empty());
}

#[test]
fn index_scan_roundtrips_firestore_style_document_id() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let table = TableName::new("users").expect("table name should be valid");
    let index = IndexDefinition {
        name: "by_email".to_string(),
        fields: vec!["email".to_string()],
    };
    let explicit_id =
        DocumentId::from_key("users.alice-1".to_string()).expect("document id should be valid");
    let document = Document::with_id(
        explicit_id.clone(),
        table.clone(),
        serde_json::Map::from_iter([("email".to_string(), json!("alice@test.com"))]),
    );

    store
        .insert_with_indexes(&document, std::slice::from_ref(&index))
        .expect("insert should succeed");

    let docs = store
        .index_scan_eq(&table, "by_email", &json!("alice@test.com"))
        .expect("index scan should succeed");

    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0].id, explicit_id);
}

#[test]
fn index_update_maintains_entries() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let index = IndexDefinition {
        name: "by_email".to_string(),
        fields: vec!["email".to_string()],
    };
    let document = Document::new(
        TableName::new("users").expect("table name should be valid"),
        serde_json::Map::from_iter([("email".to_string(), json!("old@test.com"))]),
    );
    store
        .insert_with_indexes(&document, std::slice::from_ref(&index))
        .expect("insert should succeed");

    store
        .update_with_indexes(
            &document.table,
            &document.id,
            &serde_json::Map::from_iter([("email".to_string(), json!("new@test.com"))]),
            std::slice::from_ref(&index),
        )
        .expect("update should succeed");

    let old_docs = store
        .index_scan_eq(&document.table, "by_email", &json!("old@test.com"))
        .expect("index scan should succeed");
    assert!(old_docs.is_empty());

    let new_docs = store
        .index_scan_eq(&document.table, "by_email", &json!("new@test.com"))
        .expect("index scan should succeed");
    assert_eq!(new_docs.len(), 1);
    assert_eq!(
        new_docs[0].fields.get("email"),
        Some(&json!("new@test.com"))
    );
}

#[test]
fn index_delete_removes_entries() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let index = IndexDefinition {
        name: "by_email".to_string(),
        fields: vec!["email".to_string()],
    };
    let document = Document::new(
        TableName::new("users").expect("table name should be valid"),
        serde_json::Map::from_iter([("email".to_string(), json!("gone@test.com"))]),
    );
    store
        .insert_with_indexes(&document, std::slice::from_ref(&index))
        .expect("insert should succeed");

    store
        .delete_with_indexes(&document.table, &document.id, std::slice::from_ref(&index))
        .expect("delete should succeed");

    let docs = store
        .index_scan_eq(&document.table, "by_email", &json!("gone@test.com"))
        .expect("index scan should succeed");
    assert!(docs.is_empty());
}

#[test]
fn index_scan_range_on_numbers() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let index = IndexDefinition {
        name: "by_age".to_string(),
        fields: vec!["age".to_string()],
    };
    for age in [20, 30, 40, 50] {
        let document = Document::new(
            TableName::new("users").expect("table name should be valid"),
            serde_json::Map::from_iter([("age".to_string(), json!(age))]),
        );
        store
            .insert_with_indexes(&document, std::slice::from_ref(&index))
            .expect("insert should succeed");
    }

    let over_25 = store
        .index_scan_range(
            &TableName::new("users").expect("table name should be valid"),
            "by_age",
            Some(&json!(25)),
            None,
            false,
            true,
        )
        .expect("range scan should succeed");
    assert_eq!(over_25.len(), 3);

    let between = store
        .index_scan_range(
            &TableName::new("users").expect("table name should be valid"),
            "by_age",
            Some(&json!(25)),
            Some(&json!(35)),
            true,
            true,
        )
        .expect("range scan should succeed");
    assert_eq!(between.len(), 1);
    assert_eq!(between[0].fields.get("age"), Some(&json!(30)));
}

#[test]
fn composite_index_entries_appear_only_after_all_fields_exist_and_delete_cleanly() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let index = IndexDefinition {
        name: "by_status_rank".to_string(),
        fields: vec!["status".to_string(), "rank".to_string()],
    };
    let document = Document::new(
        TableName::new("tasks").expect("table name should be valid"),
        serde_json::Map::from_iter([("status".to_string(), json!("open"))]),
    );
    store
        .insert_with_indexes(&document, std::slice::from_ref(&index))
        .expect("insert should succeed");

    assert!(
        store
            .index_scan_eq(&document.table, "by_status_rank", &json!("open"))
            .expect("composite prefix scan should succeed")
            .is_empty(),
        "documents missing any indexed field should not get a composite index entry"
    );

    store
        .update_with_indexes(
            &document.table,
            &document.id,
            &serde_json::Map::from_iter([("rank".to_string(), json!(1))]),
            std::slice::from_ref(&index),
        )
        .expect("update should succeed");

    let indexed = store
        .index_scan_eq(&document.table, "by_status_rank", &json!("open"))
        .expect("composite prefix scan should succeed");
    assert_eq!(indexed.len(), 1);
    assert_eq!(indexed[0].id, document.id);

    store
        .update_with_indexes(
            &document.table,
            &document.id,
            &serde_json::Map::from_iter([("rank".to_string(), json!(null))]),
            std::slice::from_ref(&index),
        )
        .expect("update should succeed");

    let indexed = store
        .index_scan_eq(&document.table, "by_status_rank", &json!("open"))
        .expect("composite prefix scan should succeed");
    assert_eq!(indexed.len(), 1, "explicit null should stay indexable");
    assert_eq!(indexed[0].id, document.id);

    store
        .delete_with_indexes(&document.table, &document.id, std::slice::from_ref(&index))
        .expect("delete should succeed");
    assert!(
        store
            .index_scan_eq(&document.table, "by_status_rank", &json!("open"))
            .expect("composite prefix scan should succeed")
            .is_empty(),
        "delete should remove the composite index entry"
    );
}

#[test]
fn composite_index_backfill_indexes_only_documents_with_all_indexed_fields() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let table = TableName::new("tasks").expect("table name should be valid");

    let complete = Document::new(
        table.clone(),
        serde_json::Map::from_iter([
            ("status".to_string(), json!("open")),
            ("rank".to_string(), json!(1)),
        ]),
    );
    let missing_rank = Document::new(
        table.clone(),
        serde_json::Map::from_iter([("status".to_string(), json!("open"))]),
    );
    store.insert(&complete).expect("insert should succeed");
    store.insert(&missing_rank).expect("insert should succeed");

    let table_schema = TableSchema {
        table: table.clone(),
        fields: vec![
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
        indexes: vec![IndexDefinition {
            name: "by_status_rank".to_string(),
            fields: vec!["status".to_string(), "rank".to_string()],
        }],
        access_policy: None,
    };
    store
        .replace_table_schema(&table_schema)
        .expect("schema replacement should rebuild indexes");

    let indexed = store
        .index_scan_eq(&table, "by_status_rank", &json!("open"))
        .expect("composite prefix scan should succeed");
    assert_eq!(indexed.len(), 1);
    assert_eq!(indexed[0].id, complete.id);
}

#[test]
fn composite_index_prefix_scan_matches_all_leading_fields() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let table = TableName::new("tasks").expect("table name should be valid");
    let index = IndexDefinition {
        name: "by_status_rank".to_string(),
        fields: vec!["status".to_string(), "rank".to_string()],
    };

    for (status, rank) in [("open", 1), ("open", 2), ("done", 2)] {
        let document = Document::new(
            table.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!(status)),
                ("rank".to_string(), json!(rank)),
            ]),
        );
        store
            .insert_with_indexes(&document, std::slice::from_ref(&index))
            .expect("insert should succeed");
    }

    let indexed = store
        .index_scan_prefix(&table, "by_status_rank", &[json!("open"), json!(2)])
        .expect("composite prefix scan should succeed");
    assert_eq!(indexed.len(), 1);
    assert_eq!(indexed[0].fields.get("status"), Some(&json!("open")));
    assert_eq!(indexed[0].fields.get("rank"), Some(&json!(2)));
}

#[test]
fn composite_index_range_scan_respects_exact_prefix_on_leading_fields() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let table = TableName::new("tasks").expect("table name should be valid");
    let index = IndexDefinition {
        name: "by_status_rank".to_string(),
        fields: vec!["status".to_string(), "rank".to_string()],
    };

    for (status, rank) in [("open", 1), ("open", 2), ("open", 4), ("done", 2)] {
        let document = Document::new(
            table.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!(status)),
                ("rank".to_string(), json!(rank)),
            ]),
        );
        store
            .insert_with_indexes(&document, std::slice::from_ref(&index))
            .expect("insert should succeed");
    }

    let indexed = store
        .index_scan_composite_range(
            &table,
            "by_status_rank",
            &[json!("open")],
            Some(&json!(2)),
            Some(&json!(4)),
            true,
            false,
        )
        .expect("composite range scan should succeed");
    assert_eq!(indexed.len(), 1);
    assert_eq!(indexed[0].fields.get("status"), Some(&json!("open")));
    assert_eq!(indexed[0].fields.get("rank"), Some(&json!(2)));
}

#[test]
fn composite_index_three_field_range_scan_respects_two_field_prefix() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let table = TableName::new("tasks").expect("table name should be valid");
    let index = IndexDefinition {
        name: "by_team_status_rank".to_string(),
        fields: vec!["team".to_string(), "status".to_string(), "rank".to_string()],
    };

    for (team, status, rank) in [
        ("alpha", "open", 1),
        ("alpha", "open", 2),
        ("alpha", "open", 3),
        ("alpha", "done", 2),
        ("beta", "open", 2),
    ] {
        let document = Document::new(
            table.clone(),
            serde_json::Map::from_iter([
                ("team".to_string(), json!(team)),
                ("status".to_string(), json!(status)),
                ("rank".to_string(), json!(rank)),
            ]),
        );
        store
            .insert_with_indexes(&document, std::slice::from_ref(&index))
            .expect("insert should succeed");
    }

    let prefixed = store
        .index_scan_prefix(
            &table,
            "by_team_status_rank",
            &[json!("alpha"), json!("open")],
        )
        .expect("three-field prefix scan should succeed");
    assert_eq!(prefixed.len(), 3);

    let ranged = store
        .index_scan_composite_range(
            &table,
            "by_team_status_rank",
            &[json!("alpha"), json!("open")],
            Some(&json!(2)),
            Some(&json!(4)),
            true,
            false,
        )
        .expect("three-field composite range scan should succeed");
    assert_eq!(ranged.len(), 2);
    assert_eq!(ranged[0].fields.get("rank"), Some(&json!(2)));
    assert_eq!(ranged[1].fields.get("rank"), Some(&json!(3)));
}
