use super::*;
use neovex_core::{Filter, FilterOp, Query, TableName, commit_intersects_dependency_set};
use neovex_engine::encode_cursor;
use serde_json::{Value, json};

#[test]
fn synthesize_runtime_subscription_base_queries_keeps_disjoint_same_table_predicates() {
    let table = TableName::new("messages").expect("table should be valid");
    let mut read_set = RuntimeReadSet::default();
    read_set.record_predicate(
        &table,
        &[Filter {
            field: "author".to_string(),
            op: FilterOp::Eq,
            value: Value::String("Ada".to_string()),
        }],
    );
    read_set.record_predicate(
        &table,
        &[Filter {
            field: "author".to_string(),
            op: FilterOp::Eq,
            value: Value::String("Bob".to_string()),
        }],
    );

    let queries =
        synthesize_runtime_subscription_base_queries(&read_set).expect("queries should synthesize");

    assert_eq!(queries.len(), 2);
    assert!(queries.iter().all(|query| query.table == table));
    assert!(queries.iter().any(|query| query.filters
        == vec![Filter {
            field: "author".to_string(),
            op: FilterOp::Eq,
            value: Value::String("Ada".to_string()),
        }]));
    assert!(queries.iter().any(|query| query.filters
        == vec![Filter {
            field: "author".to_string(),
            op: FilterOp::Eq,
            value: Value::String("Bob".to_string()),
        }]));
}

#[test]
fn synthesize_runtime_subscription_base_queries_prefers_broad_query_for_full_table_reads() {
    let table = TableName::new("messages").expect("table should be valid");
    let mut read_set = RuntimeReadSet::default();
    read_set.record_table(&table);
    read_set.record_predicate(
        &table,
        &[Filter {
            field: "author".to_string(),
            op: FilterOp::Eq,
            value: Value::String("Ada".to_string()),
        }],
    );

    let queries =
        synthesize_runtime_subscription_base_queries(&read_set).expect("queries should synthesize");

    assert_eq!(
        queries,
        vec![Query {
            table,
            filters: Vec::new(),
            order: None,
            limit: None,
        }]
    );
}

#[test]
fn runtime_read_set_converts_to_shared_dependency_set_without_losing_skip_behavior() {
    let table = TableName::new("messages").expect("table should be valid");
    let mut read_set = RuntimeReadSet::default();
    read_set.record_predicate(
        &table,
        &[Filter {
            field: "author".to_string(),
            op: FilterOp::Eq,
            value: Value::String("Ada".to_string()),
        }],
    );

    let dependencies = read_set.dependency_set();
    assert_eq!(dependencies.predicates.len(), 1);
    assert!(dependencies.tables.is_empty());

    let document_id = neovex_core::DocumentId::new();
    let matching_document = neovex_core::Document {
        id: document_id.clone(),
        table: table.clone(),
        creation_time: neovex_core::Timestamp::now(),
        update_time: neovex_core::Timestamp::now(),
        fields: serde_json::Map::from_iter([("author".to_string(), json!("Ada"))]),
        typed_fields: Default::default(),
    };
    let commit = neovex_core::CommitEntry {
        sequence: neovex_core::SequenceNumber(1),
        timestamp: neovex_core::Timestamp::now(),
        writes: vec![neovex_core::WriteOp {
            table: table.clone(),
            op_type: neovex_core::WriteOpType::Insert,
            doc_id: document_id.clone(),
            resource_path_binding: None,
            trigger_write_origin: None,
            previous: None,
            current: Some(matching_document.clone()),
        }],
    };

    assert!(commit_intersects_dependency_set(
        &commit,
        &dependencies,
        &[matching_document],
        |_, _| Ok(None),
    ));
}

#[test]
fn shared_dependency_matching_uses_previous_document_snapshots_for_updates() {
    let table = TableName::new("messages").expect("table should be valid");
    let mut read_set = RuntimeReadSet::default();
    read_set.record_predicate(
        &table,
        &[Filter {
            field: "author".to_string(),
            op: FilterOp::Eq,
            value: Value::String("Ada".to_string()),
        }],
    );

    let document_id = neovex_core::DocumentId::new();
    let previous = neovex_core::Document {
        id: document_id.clone(),
        table: table.clone(),
        creation_time: neovex_core::Timestamp::now(),
        update_time: neovex_core::Timestamp::now(),
        fields: serde_json::Map::from_iter([("author".to_string(), json!("Ada"))]),
        typed_fields: Default::default(),
    };
    let current = neovex_core::Document {
        id: document_id.clone(),
        table: table.clone(),
        creation_time: previous.creation_time,
        update_time: neovex_core::Timestamp::now(),
        fields: serde_json::Map::from_iter([("author".to_string(), json!("Grace"))]),
        typed_fields: Default::default(),
    };

    let commit = neovex_core::CommitEntry {
        sequence: neovex_core::SequenceNumber(2),
        timestamp: neovex_core::Timestamp::now(),
        writes: vec![neovex_core::WriteOp {
            table,
            op_type: neovex_core::WriteOpType::Update,
            doc_id: document_id.clone(),
            resource_path_binding: None,
            trigger_write_origin: None,
            previous: Some(previous),
            current: Some(current),
        }],
    };

    assert!(commit_intersects_dependency_set(
        &commit,
        &read_set.dependency_set(),
        &[],
        |_, _| Ok(None),
    ));
}

#[test]
fn runtime_read_set_preserves_tuple_capable_paginated_window_boundaries() {
    let table = TableName::new("messages").expect("table should be valid");
    let query = Query {
        table: table.clone(),
        filters: vec![Filter {
            field: "author".to_string(),
            op: FilterOp::Eq,
            value: json!("Ada"),
        }],
        order: Some(neovex_core::OrderBy {
            field: "rank".to_string(),
            direction: neovex_core::OrderDirection::Asc,
        }),
        limit: None,
    };
    let start_doc_id = neovex_core::DocumentId::new();
    let end_doc_id = neovex_core::DocumentId::new();
    let after =
        encode_cursor(&[Some(json!(1))], &start_doc_id, &query).expect("cursor should encode");
    let page = neovex_core::Page {
        data: vec![json!({
            "_id": end_doc_id.to_string(),
            "author": "Ada",
            "rank": 2,
        })],
        next_cursor: None,
        has_more: false,
    };

    let mut read_set = RuntimeReadSet::default();
    read_set.record_paginated_window(&query, 1, Some(&after), &page);

    let dependencies = read_set.dependency_set();
    assert_eq!(dependencies.paginated_windows.len(), 1);
    let dependency = &dependencies.paginated_windows[0];
    assert_eq!(dependency.start_sort_values, vec![Some(json!(1))]);
    assert_eq!(dependency.start_doc_id, Some(start_doc_id));
    assert_eq!(dependency.end_sort_values, vec![Some(json!(2))]);
    assert_eq!(dependency.end_doc_id, Some(end_doc_id));
}
