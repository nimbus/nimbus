use std::ops::Bound;

use nimbus_core::{
    Document, DocumentId, FieldSchema, FieldType, IndexDefinition, IndexState, TableName,
    TableSchema, order_preserving_number_bits,
};
use serde_json::json;

use crate::TenantStore;

use super::encode_index_value;

fn save_schema_for_indexes(store: &TenantStore, table: &TableName, indexes: &[IndexDefinition]) {
    let mut fields = Vec::new();
    for index in indexes {
        for field in &index.fields {
            if !fields
                .iter()
                .any(|existing: &FieldSchema| existing.name == *field)
            {
                fields.push(FieldSchema {
                    name: field.clone(),
                    field_type: FieldType::Any,
                    required: false,
                });
            }
        }
    }
    store
        .save_table_schema(&TableSchema {
            table: table.clone(),
            fields,
            indexes: indexes.to_vec(),
            access_policy: None,
        })
        .expect("index schema should save");
}

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
            id: nimbus_core::IndexId::new(),
            state: nimbus_core::IndexState::Enabled,
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
fn replace_table_schema_preserves_index_id_for_unchanged_definition() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let table = TableName::new("users").expect("table name should be valid");
    let first_schema = TableSchema {
        table: table.clone(),
        fields: vec![FieldSchema {
            name: "email".to_string(),
            field_type: FieldType::String,
            required: false,
        }],
        indexes: vec![IndexDefinition::new("by_email", ["email"])],
        access_policy: None,
    };
    let first_generated_id = first_schema.indexes[0].id.clone();
    store
        .replace_table_schema(&first_schema)
        .expect("first schema replacement should succeed");

    let second_schema = TableSchema {
        table: table.clone(),
        fields: first_schema.fields.clone(),
        indexes: vec![IndexDefinition::new("by_email", ["email"])],
        access_policy: None,
    };
    let second_generated_id = second_schema.indexes[0].id.clone();
    store
        .replace_table_schema(&second_schema)
        .expect("second schema replacement should succeed");

    let stored = store
        .load_schema()
        .expect("schema should load")
        .get_table(&table)
        .expect("table schema should exist")
        .clone();
    assert_ne!(first_generated_id, second_generated_id);
    assert_eq!(stored.indexes[0].id, first_generated_id);
}

#[test]
fn backfilling_index_is_maintained_but_not_queryable_until_enabled() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let table = TableName::new("users").expect("table name should be valid");
    let document = Document::new(
        table.clone(),
        serde_json::Map::from_iter([("email".to_string(), json!("a@test.com"))]),
    );
    store.insert(&document).expect("insert should succeed");

    let mut backfilling_index =
        IndexDefinition::with_state("by_email", ["email"], IndexState::Backfilling);
    let index_id = backfilling_index.id.clone();
    let backfilling_schema = TableSchema {
        table: table.clone(),
        fields: vec![FieldSchema {
            name: "email".to_string(),
            field_type: FieldType::String,
            required: false,
        }],
        indexes: vec![backfilling_index.clone()],
        access_policy: None,
    };
    store
        .replace_table_schema(&backfilling_schema)
        .expect("backfilling schema replacement should succeed");

    let error = store
        .index_scan_eq(&table, "by_email", &json!("a@test.com"))
        .expect_err("backfilling indexes should not be queryable");
    assert!(error.to_string().contains("enabled index not found"));

    backfilling_index.state = IndexState::Enabled;
    let enabled_schema = TableSchema {
        indexes: vec![backfilling_index],
        ..backfilling_schema
    };
    store
        .replace_table_schema(&enabled_schema)
        .expect("enabled schema replacement should succeed");

    let docs = store
        .index_scan_eq(&table, "by_email", &json!("a@test.com"))
        .expect("enabled index scan should succeed");
    assert_eq!(docs.len(), 1);
    let stored = store
        .load_schema()
        .expect("schema should load")
        .get_table(&table)
        .expect("table schema should exist")
        .clone();
    assert_eq!(stored.indexes[0].id, index_id);
    assert_eq!(stored.indexes[0].state, IndexState::Enabled);
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
            id: nimbus_core::IndexId::new(),
            state: nimbus_core::IndexState::Enabled,
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
    let error = store
        .index_scan_eq(&table, "by_email", &json!("gone@test.com"))
        .expect_err("deleted schema should make the index non-queryable");
    assert!(matches!(error, nimbus_core::Error::SchemaNotFound(_)));
}

#[test]
fn update_with_indexes_validated_maintains_entries() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let table = TableName::new("users").expect("table name should be valid");
    let index = IndexDefinition {
        id: nimbus_core::IndexId::new(),
        state: nimbus_core::IndexState::Enabled,
        name: "by_email".to_string(),
        fields: vec!["email".to_string()],
    };
    save_schema_for_indexes(&store, &table, std::slice::from_ref(&index));
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
    fn expected_number_encoding(value: f64) -> Vec<u8> {
        let mut encoded = vec![0x02];
        encoded.extend_from_slice(&order_preserving_number_bits(value).to_be_bytes());
        encoded
    }

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
    assert_eq!(
        encode_index_value(&json!(-1.5)).expect("value should encode"),
        expected_number_encoding(-1.5)
    );
    assert_eq!(
        encode_index_value(&json!(0)).expect("value should encode"),
        expected_number_encoding(0.0)
    );
    assert_eq!(
        encode_index_value(&json!(100)).expect("value should encode"),
        expected_number_encoding(100.0)
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
    let table = TableName::new("users").expect("table name should be valid");
    let index = IndexDefinition {
        id: nimbus_core::IndexId::new(),
        state: nimbus_core::IndexState::Enabled,
        name: "by_email".to_string(),
        fields: vec!["email".to_string()],
    };
    save_schema_for_indexes(&store, &table, std::slice::from_ref(&index));
    for email in ["a@test.com", "b@test.com", "c@test.com"] {
        let document = Document::new(
            table.clone(),
            serde_json::Map::from_iter([("email".to_string(), json!(email))]),
        );
        store
            .insert_with_indexes(&document, std::slice::from_ref(&index))
            .expect("insert should succeed");
    }

    let match_docs = store
        .index_scan_eq(&table, "by_email", &json!("b@test.com"))
        .expect("index scan should succeed");
    assert_eq!(match_docs.len(), 1);
    assert_eq!(
        match_docs[0].fields.get("email"),
        Some(&json!("b@test.com"))
    );

    let missing_docs = store
        .index_scan_eq(&table, "by_email", &json!("missing@test.com"))
        .expect("index scan should succeed");
    assert!(missing_docs.is_empty());
}

#[test]
fn index_scan_roundtrips_firestore_style_document_id() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let table = TableName::new("users").expect("table name should be valid");
    let index = IndexDefinition {
        id: nimbus_core::IndexId::new(),
        state: nimbus_core::IndexState::Enabled,
        name: "by_email".to_string(),
        fields: vec!["email".to_string()],
    };
    save_schema_for_indexes(&store, &table, std::slice::from_ref(&index));
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
    let table = TableName::new("users").expect("table name should be valid");
    let index = IndexDefinition {
        id: nimbus_core::IndexId::new(),
        state: nimbus_core::IndexState::Enabled,
        name: "by_email".to_string(),
        fields: vec!["email".to_string()],
    };
    save_schema_for_indexes(&store, &table, std::slice::from_ref(&index));
    let document = Document::new(
        table,
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
    let table = TableName::new("users").expect("table name should be valid");
    let index = IndexDefinition {
        id: nimbus_core::IndexId::new(),
        state: nimbus_core::IndexState::Enabled,
        name: "by_email".to_string(),
        fields: vec!["email".to_string()],
    };
    save_schema_for_indexes(&store, &table, std::slice::from_ref(&index));
    let document = Document::new(
        table,
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
    let table = TableName::new("users").expect("table name should be valid");
    let index = IndexDefinition {
        id: nimbus_core::IndexId::new(),
        state: nimbus_core::IndexState::Enabled,
        name: "by_age".to_string(),
        fields: vec!["age".to_string()],
    };
    save_schema_for_indexes(&store, &table, std::slice::from_ref(&index));
    for age in [20, 30, 40, 50] {
        let document = Document::new(
            table.clone(),
            serde_json::Map::from_iter([("age".to_string(), json!(age))]),
        );
        store
            .insert_with_indexes(&document, std::slice::from_ref(&index))
            .expect("insert should succeed");
    }

    let over_25 = store
        .index_scan_range(
            &table,
            "by_age",
            Bound::Excluded(&json!(25)),
            Bound::Unbounded,
        )
        .expect("range scan should succeed");
    assert_eq!(over_25.len(), 3);

    let between = store
        .index_scan_range(
            &table,
            "by_age",
            Bound::Included(&json!(25)),
            Bound::Included(&json!(35)),
        )
        .expect("range scan should succeed");
    assert_eq!(between.len(), 1);
    assert_eq!(between[0].fields.get("age"), Some(&json!(30)));
}

#[test]
fn index_scan_open_ended_range_excludes_other_json_types() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let table = TableName::new("users").expect("table name should be valid");
    let index = IndexDefinition {
        id: nimbus_core::IndexId::new(),
        state: nimbus_core::IndexState::Enabled,
        name: "by_age".to_string(),
        fields: vec!["age".to_string()],
    };
    save_schema_for_indexes(&store, &table, std::slice::from_ref(&index));

    for (label, age) in [
        ("number-low", json!(20)),
        ("number-high", json!(30)),
        ("string", json!("zzz")),
        ("bool", json!(true)),
        ("null", json!(null)),
    ] {
        let document = Document::new(
            table.clone(),
            serde_json::Map::from_iter([
                ("label".to_string(), json!(label)),
                ("age".to_string(), age),
            ]),
        );
        store
            .insert_with_indexes(&document, std::slice::from_ref(&index))
            .expect("insert should succeed");
    }

    let labels_for = |documents: Vec<Document>| {
        documents
            .iter()
            .map(|document| {
                document
                    .fields
                    .get("label")
                    .and_then(serde_json::Value::as_str)
                    .expect("label should be present")
                    .to_owned()
            })
            .collect::<Vec<_>>()
    };

    let at_least_25 = store
        .index_scan_range(
            &table,
            "by_age",
            Bound::Included(&json!(25)),
            Bound::Unbounded,
        )
        .expect("range scan should succeed");
    assert_eq!(labels_for(at_least_25), vec!["number-high".to_owned()]);

    let at_most_25 = store
        .index_scan_range(
            &table,
            "by_age",
            Bound::Unbounded,
            Bound::Included(&json!(25)),
        )
        .expect("range scan should succeed");
    assert_eq!(labels_for(at_most_25), vec!["number-low".to_owned()]);
}

#[test]
fn index_scan_range_orders_negative_and_positive_numbers() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let table = TableName::new("scores").expect("table name should be valid");
    let index = IndexDefinition {
        id: nimbus_core::IndexId::new(),
        state: nimbus_core::IndexState::Enabled,
        name: "by_score".to_string(),
        fields: vec!["score".to_string()],
    };
    save_schema_for_indexes(&store, &table, std::slice::from_ref(&index));

    for score in [-10, -1, 0, 1, 10] {
        let document = Document::new(
            table.clone(),
            serde_json::Map::from_iter([("score".to_string(), json!(score))]),
        );
        store
            .insert_with_indexes(&document, std::slice::from_ref(&index))
            .expect("insert should succeed");
    }

    let scores_for = |documents: Vec<Document>| {
        documents
            .iter()
            .map(|document| {
                document
                    .fields
                    .get("score")
                    .and_then(serde_json::Value::as_i64)
                    .expect("score should be present")
            })
            .collect::<Vec<_>>()
    };

    let spanning_zero = store
        .index_scan_range(
            &table,
            "by_score",
            Bound::Included(&json!(-2)),
            Bound::Included(&json!(2)),
        )
        .expect("range scan should succeed");
    assert_eq!(scores_for(spanning_zero), vec![-1, 0, 1]);

    let greater_than_minimum = store
        .index_scan_range(
            &table,
            "by_score",
            Bound::Excluded(&json!(-10)),
            Bound::Unbounded,
        )
        .expect("range scan should succeed");
    assert_eq!(scores_for(greater_than_minimum), vec![-1, 0, 1, 10]);
}

#[test]
fn composite_index_entries_appear_only_after_all_fields_exist_and_delete_cleanly() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let index = IndexDefinition {
        id: nimbus_core::IndexId::new(),
        state: nimbus_core::IndexState::Enabled,
        name: "by_status_rank".to_string(),
        fields: vec!["status".to_string(), "rank".to_string()],
    };
    let table = TableName::new("tasks").expect("table name should be valid");
    save_schema_for_indexes(&store, &table, std::slice::from_ref(&index));
    let document = Document::new(
        table,
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
            id: nimbus_core::IndexId::new(),
            state: nimbus_core::IndexState::Enabled,
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
        id: nimbus_core::IndexId::new(),
        state: nimbus_core::IndexState::Enabled,
        name: "by_status_rank".to_string(),
        fields: vec!["status".to_string(), "rank".to_string()],
    };
    save_schema_for_indexes(&store, &table, std::slice::from_ref(&index));

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
        id: nimbus_core::IndexId::new(),
        state: nimbus_core::IndexState::Enabled,
        name: "by_status_rank".to_string(),
        fields: vec!["status".to_string(), "rank".to_string()],
    };
    save_schema_for_indexes(&store, &table, std::slice::from_ref(&index));

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
            Bound::Included(&json!(2)),
            Bound::Excluded(&json!(4)),
        )
        .expect("composite range scan should succeed");
    assert_eq!(indexed.len(), 1);
    assert_eq!(indexed[0].fields.get("status"), Some(&json!("open")));
    assert_eq!(indexed[0].fields.get("rank"), Some(&json!(2)));
}

#[test]
fn composite_index_range_scan_excludes_other_json_types_on_range_field() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let table = TableName::new("tasks").expect("table name should be valid");
    let index = IndexDefinition {
        id: nimbus_core::IndexId::new(),
        state: nimbus_core::IndexState::Enabled,
        name: "by_status_rank".to_string(),
        fields: vec!["status".to_string(), "rank".to_string()],
    };
    save_schema_for_indexes(&store, &table, std::slice::from_ref(&index));

    for (label, rank) in [
        ("number-low", json!(1)),
        ("number-high", json!(3)),
        ("string", json!("zzz")),
        ("bool", json!(true)),
        ("null", json!(null)),
    ] {
        let document = Document::new(
            table.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("open")),
                ("label".to_string(), json!(label)),
                ("rank".to_string(), rank),
            ]),
        );
        store
            .insert_with_indexes(&document, std::slice::from_ref(&index))
            .expect("insert should succeed");
    }

    let labels_for = |documents: Vec<Document>| {
        documents
            .iter()
            .map(|document| {
                document
                    .fields
                    .get("label")
                    .and_then(serde_json::Value::as_str)
                    .expect("label should be present")
                    .to_owned()
            })
            .collect::<Vec<_>>()
    };

    let at_least_two = store
        .index_scan_composite_range(
            &table,
            "by_status_rank",
            &[json!("open")],
            Bound::Included(&json!(2)),
            Bound::Unbounded,
        )
        .expect("composite range scan should succeed");
    assert_eq!(labels_for(at_least_two), vec!["number-high".to_owned()]);

    let at_most_two = store
        .index_scan_composite_range(
            &table,
            "by_status_rank",
            &[json!("open")],
            Bound::Unbounded,
            Bound::Included(&json!(2)),
        )
        .expect("composite range scan should succeed");
    assert_eq!(labels_for(at_most_two), vec!["number-low".to_owned()]);
}

#[test]
fn composite_index_three_field_range_scan_respects_two_field_prefix() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let table = TableName::new("tasks").expect("table name should be valid");
    let index = IndexDefinition {
        id: nimbus_core::IndexId::new(),
        state: nimbus_core::IndexState::Enabled,
        name: "by_team_status_rank".to_string(),
        fields: vec!["team".to_string(), "status".to_string(), "rank".to_string()],
    };
    save_schema_for_indexes(&store, &table, std::slice::from_ref(&index));

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
            Bound::Included(&json!(2)),
            Bound::Excluded(&json!(4)),
        )
        .expect("three-field composite range scan should succeed");
    assert_eq!(ranged.len(), 2);
    assert_eq!(ranged[0].fields.get("rank"), Some(&json!(2)));
    assert_eq!(ranged[1].fields.get("rank"), Some(&json!(3)));
}
