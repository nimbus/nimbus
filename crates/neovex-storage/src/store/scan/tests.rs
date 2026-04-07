use neovex_core::{Document, Filter, FilterOp, TableName};
use serde_json::json;

use crate::TenantStore;

fn filter(field: &str, op: FilterOp, value: serde_json::Value) -> Filter {
    Filter {
        field: field.to_string(),
        op,
        value,
    }
}

fn sample_document(table: &str, title: &str) -> Document {
    Document::new(
        TableName::new(table).expect("table name should be valid"),
        serde_json::Map::from_iter([("title".to_string(), json!(title))]),
    )
}

#[test]
fn scan_table_is_logically_isolated() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let task = sample_document("tasks", "Task");
    let user = sample_document("users", "User");

    store.insert(&task).expect("task insert should succeed");
    store.insert(&user).expect("user insert should succeed");

    let tasks = store
        .scan_table(&TableName::new("tasks").expect("table name should be valid"))
        .expect("scan should succeed");
    let users = store
        .scan_table(&TableName::new("users").expect("table name should be valid"))
        .expect("scan should succeed");

    assert_eq!(tasks.len(), 1);
    assert_eq!(users.len(), 1);
    assert_eq!(tasks[0].fields.get("title"), Some(&json!("Task")));
    assert_eq!(users[0].fields.get("title"), Some(&json!("User")));
}

#[test]
fn scan_pushdown_rejects_selective_rows_before_full_decode() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let table = TableName::new("tasks").expect("table name should be valid");
    for rank in 0..512 {
        let status = if rank % 97 == 0 { "keep" } else { "skip" };
        let document = Document::new(
            table.clone(),
            serde_json::Map::from_iter([
                ("rank".to_string(), json!(rank)),
                ("status".to_string(), json!(status)),
            ]),
        );
        store.insert(&document).expect("insert should succeed");
    }

    let documents = store
        .scan_table_matching_with_filters_cancellable(
            &table,
            &[filter("status", FilterOp::Eq, json!("keep"))],
            &mut || Ok(()),
            |_document| Ok(true),
        )
        .expect("pushdown scan should succeed");
    let stats = store.scan_stats();

    assert_eq!(documents.len(), 6);
    assert_eq!(stats.scanned_rows, 512);
    assert_eq!(stats.decoded_rows, 6);
    assert_eq!(stats.pushdown_rejected_rows, 506);
}

#[test]
fn unsupported_scan_filters_fall_back_to_full_decode() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let table = TableName::new("tasks").expect("table name should be valid");
    for title in ["a", "b", "c"] {
        let document = Document::new(
            table.clone(),
            serde_json::Map::from_iter([("title".to_string(), json!(title))]),
        );
        store.insert(&document).expect("insert should succeed");
    }

    let documents = store
        .scan_table_matching_with_filters_cancellable(
            &table,
            &[filter("title", FilterOp::Neq, json!("b"))],
            &mut || Ok(()),
            |document| Ok(document.get_field("title") != Some(&json!("b"))),
        )
        .expect("fallback scan should succeed");
    let stats = store.scan_stats();

    assert_eq!(documents.len(), 2);
    assert_eq!(stats.scanned_rows, 3);
    assert_eq!(stats.decoded_rows, 3);
    assert_eq!(stats.pushdown_rejected_rows, 0);
}

#[test]
fn range_scan_pushdown_rejects_out_of_range_rows_before_full_decode() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let table = TableName::new("tasks").expect("table name should be valid");
    for rank in 0..100 {
        let document = Document::new(
            table.clone(),
            serde_json::Map::from_iter([("rank".to_string(), json!(rank))]),
        );
        store.insert(&document).expect("insert should succeed");
    }

    let documents = store
        .scan_table_matching_with_filters_cancellable(
            &table,
            &[filter("rank", FilterOp::Gte, json!(90))],
            &mut || Ok(()),
            |_document| Ok(true),
        )
        .expect("range pushdown scan should succeed");
    let stats = store.scan_stats();

    assert_eq!(documents.len(), 10);
    assert_eq!(stats.scanned_rows, 100);
    assert_eq!(stats.decoded_rows, 10);
    assert_eq!(stats.pushdown_rejected_rows, 90);
}

#[test]
fn multiple_pushdown_filters_reject_rows_before_full_decode() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let table = TableName::new("tasks").expect("table name should be valid");
    for rank in 0..100 {
        let status = if rank % 25 == 0 { "keep" } else { "skip" };
        let document = Document::new(
            table.clone(),
            serde_json::Map::from_iter([
                ("rank".to_string(), json!(rank)),
                ("status".to_string(), json!(status)),
            ]),
        );
        store.insert(&document).expect("insert should succeed");
    }

    let documents = store
        .scan_table_matching_with_filters_cancellable(
            &table,
            &[
                filter("status", FilterOp::Eq, json!("keep")),
                filter("rank", FilterOp::Gte, json!(50)),
                filter("rank", FilterOp::Lt, json!(80)),
            ],
            &mut || Ok(()),
            |_document| Ok(true),
        )
        .expect("multi-filter pushdown scan should succeed");
    let stats = store.scan_stats();

    assert_eq!(documents.len(), 2);
    assert_eq!(stats.scanned_rows, 100);
    assert_eq!(stats.decoded_rows, 2);
    assert_eq!(stats.pushdown_rejected_rows, 98);
}
