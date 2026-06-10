use std::cmp::Ordering as CompareOrdering;

use nimbus_core::{Document, Filter, FilterOp, TableName, Timestamp, TypedScalarValue};
use serde_json::{Value, json};

use super::{ScanPushdown, compare_pushdown_values};
use crate::TenantStore;
use crate::document_codec::{decode_document_msgpack, encode_document_msgpack};

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

fn finite_number(value: f64) -> Value {
    serde_json::Number::from_f64(value)
        .map(Value::Number)
        .expect("test value should be a finite JSON number")
}

fn assert_pushdown_matches_full_decode(
    document: &Document,
    filters: Vec<Filter>,
    expected_kept_by_full_decode: bool,
) {
    let bytes = encode_document_msgpack(document).expect("document should serialize");
    let pushdown = ScanPushdown::compile(&filters).expect("filters should compile to pushdown");
    let pushdown_keeps_document = !pushdown.rejects_document_bytes(&bytes);
    let decoded = decode_document_msgpack(&bytes).expect("document should deserialize");
    let full_decode_keeps_document = filters
        .iter()
        .all(|filter| full_decode_matches_filter(&decoded, filter));

    assert_eq!(full_decode_keeps_document, expected_kept_by_full_decode);
    assert_eq!(
        pushdown_keeps_document, full_decode_keeps_document,
        "pushdown decision diverged from full decode for filters {filters:?} and fields {:?}",
        decoded.fields
    );
}

fn full_decode_matches_filter(document: &Document, filter: &Filter) -> bool {
    let Some(field_value) = document.get_field(&filter.field) else {
        return false;
    };

    match filter.op {
        FilterOp::Eq => field_value == &filter.value,
        FilterOp::Neq => field_value != &filter.value,
        FilterOp::Gt => {
            matches!(
                compare_pushdown_values(field_value, &filter.value),
                Some(CompareOrdering::Greater)
            )
        }
        FilterOp::Gte => {
            matches!(
                compare_pushdown_values(field_value, &filter.value),
                Some(CompareOrdering::Greater | CompareOrdering::Equal)
            )
        }
        FilterOp::Lt => {
            matches!(
                compare_pushdown_values(field_value, &filter.value),
                Some(CompareOrdering::Less)
            )
        }
        FilterOp::Lte => {
            matches!(
                compare_pushdown_values(field_value, &filter.value),
                Some(CompareOrdering::Less | CompareOrdering::Equal)
            )
        }
    }
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

#[test]
fn scan_pushdown_probe_matches_full_decode_for_document_msgpack_layouts() {
    let table = TableName::new("tasks").expect("table name should be valid");

    let plain_with_extra_fields = Document::new(
        table.clone(),
        serde_json::Map::from_iter([
            ("rank".to_string(), json!(10)),
            ("status".to_string(), json!("keep")),
            (
                "payload".to_string(),
                json!({"nested": [1, 2, {"skip": true}]}),
            ),
        ]),
    );
    assert_pushdown_matches_full_decode(
        &plain_with_extra_fields,
        vec![
            filter("status", FilterOp::Eq, json!("keep")),
            filter("rank", FilterOp::Gte, json!(10)),
            filter("rank", FilterOp::Lt, json!(11)),
        ],
        true,
    );

    let missing_target_field = Document::new(
        table.clone(),
        serde_json::Map::from_iter([("payload".to_string(), json!({"status": "keep"}))]),
    );
    assert_pushdown_matches_full_decode(
        &missing_target_field,
        vec![filter("status", FilterOp::Eq, json!("keep"))],
        false,
    );

    let mut typed_fields = Document::new(
        table.clone(),
        serde_json::Map::from_iter([("status".to_string(), json!("keep"))]),
    );
    typed_fields.set_typed_field(
        "updatedAt",
        TypedScalarValue::Timestamp {
            value: Timestamp(42),
        },
    );
    assert_pushdown_matches_full_decode(
        &typed_fields,
        vec![
            filter("status", FilterOp::Eq, json!("keep")),
            filter("updatedAt", FilterOp::Eq, json!(42_u64)),
        ],
        true,
    );

    let negative_zero = Document::new(
        table.clone(),
        serde_json::Map::from_iter([("rank".to_string(), finite_number(-0.0))]),
    );
    assert_pushdown_matches_full_decode(
        &negative_zero,
        vec![
            filter("rank", FilterOp::Gte, finite_number(-0.0)),
            filter("rank", FilterOp::Lte, finite_number(0.0)),
        ],
        true,
    );

    let fractional_out_of_range = Document::new(
        table,
        serde_json::Map::from_iter([("rank".to_string(), finite_number(-1.5))]),
    );
    assert_pushdown_matches_full_decode(
        &fractional_out_of_range,
        vec![filter("rank", FilterOp::Gt, finite_number(-1.0))],
        false,
    );
}
