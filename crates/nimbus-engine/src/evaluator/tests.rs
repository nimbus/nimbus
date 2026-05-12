use std::collections::BTreeSet;

use nimbus_core::{
    Error, Filter, FilterOp, OrderBy, OrderDirection, PaginatedQuery, Query, TableName,
};
use nimbus_storage::TenantStore;
use proptest::prelude::*;
use serde_json::json;

use super::{
    evaluate_paginated, evaluate_paginated_cancellable, evaluate_query, evaluate_query_cancellable,
};

fn tasks_table() -> TableName {
    TableName::new("tasks").expect("table name should be valid")
}

fn query_for(table: &str) -> Query {
    Query {
        table: TableName::new(table).expect("table name should be valid"),
        filters: Vec::new(),
        order: None,
        limit: None,
    }
}

fn filter(field: &str, op: FilterOp, value: serde_json::Value) -> Filter {
    Filter {
        field: field.to_string(),
        op,
        value,
    }
}

fn rank_document(rank: i64) -> nimbus_core::Document {
    nimbus_core::Document::new(
        tasks_table(),
        serde_json::Map::from_iter([("rank".to_string(), json!(rank))]),
    )
}

#[test]
fn evaluator_returns_ordered_and_limited_results() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let a = nimbus_core::Document::new(
        TableName::new("tasks").expect("table name should be valid"),
        serde_json::Map::from_iter([("title".to_string(), json!("B"))]),
    );
    let b = nimbus_core::Document::new(
        TableName::new("tasks").expect("table name should be valid"),
        serde_json::Map::from_iter([("title".to_string(), json!("A"))]),
    );
    store.insert(&a).expect("insert should succeed");
    store.insert(&b).expect("insert should succeed");

    let query = Query {
        table: TableName::new("tasks").expect("table name should be valid"),
        filters: Vec::new(),
        order: Some(OrderBy {
            field: "title".to_string(),
            direction: OrderDirection::Asc,
        }),
        limit: Some(1),
    };

    let documents = evaluate_query(&store, &query).expect("query should evaluate");
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].fields.get("title"), Some(&json!("A")));
}

#[test]
fn evaluator_applies_equality_filters() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let todo = nimbus_core::Document::new(
        TableName::new("tasks").expect("table name should be valid"),
        serde_json::Map::from_iter([("status".to_string(), json!("todo"))]),
    );
    let done = nimbus_core::Document::new(
        TableName::new("tasks").expect("table name should be valid"),
        serde_json::Map::from_iter([("status".to_string(), json!("done"))]),
    );
    store.insert(&todo).expect("insert should succeed");
    store.insert(&done).expect("insert should succeed");

    let query = Query {
        table: TableName::new("tasks").expect("table name should be valid"),
        filters: vec![Filter {
            field: "status".to_string(),
            op: FilterOp::Eq,
            value: json!("todo"),
        }],
        order: None,
        limit: None,
    };

    let documents = evaluate_query(&store, &query).expect("query should evaluate");
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].fields.get("status"), Some(&json!("todo")));
}

#[test]
fn evaluator_rejects_mixed_order_value_types() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let alpha = nimbus_core::Document::new(
        TableName::new("tasks").expect("table name should be valid"),
        serde_json::Map::from_iter([("rank".to_string(), json!("1"))]),
    );
    let beta = nimbus_core::Document::new(
        TableName::new("tasks").expect("table name should be valid"),
        serde_json::Map::from_iter([("rank".to_string(), json!(2))]),
    );
    store.insert(&alpha).expect("insert should succeed");
    store.insert(&beta).expect("insert should succeed");

    let query = Query {
        table: TableName::new("tasks").expect("table name should be valid"),
        filters: Vec::new(),
        order: Some(OrderBy {
            field: "rank".to_string(),
            direction: OrderDirection::Asc,
        }),
        limit: None,
    };

    let error = evaluate_query(&store, &query).expect_err("query should fail");
    assert!(
        error
            .to_string()
            .contains("ordering cannot mix string and number values"),
        "unexpected error: {error}"
    );
}

#[test]
fn evaluator_supports_neq_filter() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let todo = nimbus_core::Document::new(
        tasks_table(),
        serde_json::Map::from_iter([("status".to_string(), json!("todo"))]),
    );
    let done = nimbus_core::Document::new(
        tasks_table(),
        serde_json::Map::from_iter([("status".to_string(), json!("done"))]),
    );
    store.insert(&todo).expect("insert should succeed");
    store.insert(&done).expect("insert should succeed");

    let mut query = query_for("tasks");
    query.filters = vec![filter("status", FilterOp::Neq, json!("todo"))];

    let documents = evaluate_query(&store, &query).expect("query should evaluate");
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].fields.get("status"), Some(&json!("done")));
}

#[test]
fn evaluator_supports_range_filters_on_numbers() {
    let store = TenantStore::create_in_memory().expect("store should open");
    for rank in [1, 2, 3] {
        let document = nimbus_core::Document::new(
            tasks_table(),
            serde_json::Map::from_iter([("rank".to_string(), json!(rank))]),
        );
        store.insert(&document).expect("insert should succeed");
    }

    let mut gt_query = query_for("tasks");
    gt_query.filters = vec![filter("rank", FilterOp::Gt, json!(1))];
    assert_eq!(
        evaluate_query(&store, &gt_query)
            .expect("gt query should evaluate")
            .len(),
        2
    );

    let mut gte_query = query_for("tasks");
    gte_query.filters = vec![filter("rank", FilterOp::Gte, json!(2))];
    assert_eq!(
        evaluate_query(&store, &gte_query)
            .expect("gte query should evaluate")
            .len(),
        2
    );

    let mut lt_query = query_for("tasks");
    lt_query.filters = vec![filter("rank", FilterOp::Lt, json!(3))];
    assert_eq!(
        evaluate_query(&store, &lt_query)
            .expect("lt query should evaluate")
            .len(),
        2
    );

    let mut lte_query = query_for("tasks");
    lte_query.filters = vec![filter("rank", FilterOp::Lte, json!(2))];
    assert_eq!(
        evaluate_query(&store, &lte_query)
            .expect("lte query should evaluate")
            .len(),
        2
    );
}

#[test]
fn evaluator_range_filter_on_unsupported_field_type_still_errors_after_pushdown_defers() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let document = nimbus_core::Document::new(
        tasks_table(),
        serde_json::Map::from_iter([("rank".to_string(), json!({ "nested": 1 }))]),
    );
    store.insert(&document).expect("insert should succeed");

    let mut query = query_for("tasks");
    query.filters = vec![filter("rank", FilterOp::Gt, json!(0))];

    let error = evaluate_query(&store, &query).expect_err("query should fail");
    assert!(
        error
            .to_string()
            .contains("comparisons only support string and number fields"),
        "unexpected error: {error}"
    );
}

#[test]
fn evaluator_query_cancellable_stops_mid_scan() {
    let store = TenantStore::create_in_memory().expect("store should open");
    for rank in 0..32 {
        store
            .insert(&rank_document(rank))
            .expect("insert should succeed");
    }

    let mut checks = 0usize;
    let error = evaluate_query_cancellable(&store, &query_for("tasks"), &mut || {
        checks += 1;
        if checks > 8 {
            Err(Error::Cancelled)
        } else {
            Ok(())
        }
    })
    .expect_err("query should cancel");

    assert!(matches!(error, Error::Cancelled));
}

#[test]
fn evaluator_paginated_cancellable_stops_mid_scan() {
    let store = TenantStore::create_in_memory().expect("store should open");
    for rank in 0..32 {
        store
            .insert(&rank_document(rank))
            .expect("insert should succeed");
    }

    let query = PaginatedQuery {
        query: Query {
            table: tasks_table(),
            filters: Vec::new(),
            order: Some(OrderBy {
                field: "rank".to_string(),
                direction: OrderDirection::Asc,
            }),
            limit: None,
        },
        page_size: 5,
        after: None,
    };

    let mut checks = 0usize;
    let error = evaluate_paginated_cancellable(&store, &query, &mut || {
        checks += 1;
        if checks > 8 {
            Err(Error::Cancelled)
        } else {
            Ok(())
        }
    })
    .expect_err("pagination should cancel");

    assert!(matches!(error, Error::Cancelled));
}

#[test]
fn evaluator_supports_range_filters_on_strings() {
    let store = TenantStore::create_in_memory().expect("store should open");
    for title in ["alpha", "bravo", "charlie"] {
        let document = nimbus_core::Document::new(
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!(title))]),
        );
        store.insert(&document).expect("insert should succeed");
    }

    let mut query = query_for("tasks");
    query.filters = vec![filter("title", FilterOp::Gt, json!("alpha"))];

    let documents = evaluate_query(&store, &query).expect("query should evaluate");
    assert_eq!(documents.len(), 2);
    assert!(
        documents
            .iter()
            .all(|doc| doc.fields["title"] != json!("alpha"))
    );
}

#[test]
fn evaluator_filter_on_missing_field_excludes_document() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let titled = nimbus_core::Document::new(
        tasks_table(),
        serde_json::Map::from_iter([("title".to_string(), json!("Hello"))]),
    );
    let untitled = nimbus_core::Document::new(tasks_table(), serde_json::Map::new());
    store.insert(&titled).expect("insert should succeed");
    store.insert(&untitled).expect("insert should succeed");

    let mut query = query_for("tasks");
    query.filters = vec![filter("title", FilterOp::Eq, json!("Hello"))];

    let documents = evaluate_query(&store, &query).expect("query should evaluate");
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].fields.get("title"), Some(&json!("Hello")));
}

#[test]
fn evaluator_applies_multiple_filters_as_and() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let alpha = nimbus_core::Document::new(
        tasks_table(),
        serde_json::Map::from_iter([
            ("status".to_string(), json!("todo")),
            ("rank".to_string(), json!(1)),
        ]),
    );
    let beta = nimbus_core::Document::new(
        tasks_table(),
        serde_json::Map::from_iter([
            ("status".to_string(), json!("todo")),
            ("rank".to_string(), json!(2)),
        ]),
    );
    let gamma = nimbus_core::Document::new(
        tasks_table(),
        serde_json::Map::from_iter([
            ("status".to_string(), json!("done")),
            ("rank".to_string(), json!(1)),
        ]),
    );
    store.insert(&alpha).expect("insert should succeed");
    store.insert(&beta).expect("insert should succeed");
    store.insert(&gamma).expect("insert should succeed");

    let mut query = query_for("tasks");
    query.filters = vec![
        filter("status", FilterOp::Eq, json!("todo")),
        filter("rank", FilterOp::Eq, json!(1)),
    ];

    let documents = evaluate_query(&store, &query).expect("query should evaluate");
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].fields.get("status"), Some(&json!("todo")));
    assert_eq!(documents[0].fields.get("rank"), Some(&json!(1)));
}

#[test]
fn evaluator_orders_descending() {
    let store = TenantStore::create_in_memory().expect("store should open");
    for title in ["alpha", "bravo", "charlie"] {
        let document = nimbus_core::Document::new(
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!(title))]),
        );
        store.insert(&document).expect("insert should succeed");
    }

    let mut query = query_for("tasks");
    query.order = Some(OrderBy {
        field: "title".to_string(),
        direction: OrderDirection::Desc,
    });

    let documents = evaluate_query(&store, &query).expect("query should evaluate");
    assert_eq!(documents[0].fields.get("title"), Some(&json!("charlie")));
    assert_eq!(documents[2].fields.get("title"), Some(&json!("alpha")));
}

#[test]
fn evaluator_without_order_sorts_by_document_id() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let first = nimbus_core::Document::new(
        tasks_table(),
        serde_json::Map::from_iter([("title".to_string(), json!("second inserted"))]),
    );
    let second = nimbus_core::Document::new(
        tasks_table(),
        serde_json::Map::from_iter([("title".to_string(), json!("first inserted"))]),
    );
    store.insert(&second).expect("insert should succeed");
    store.insert(&first).expect("insert should succeed");

    let documents = evaluate_query(&store, &query_for("tasks")).expect("query should evaluate");
    let ids = documents
        .iter()
        .map(|document| document.id.clone())
        .collect::<Vec<_>>();
    let mut sorted_ids = ids.clone();
    sorted_ids.sort();
    assert_eq!(ids, sorted_ids);
}

#[test]
fn evaluator_honors_limit_zero_and_none() {
    let store = TenantStore::create_in_memory().expect("store should open");
    for title in ["alpha", "bravo"] {
        let document = nimbus_core::Document::new(
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!(title))]),
        );
        store.insert(&document).expect("insert should succeed");
    }

    let mut zero_limit_query = query_for("tasks");
    zero_limit_query.limit = Some(0);
    assert!(
        evaluate_query(&store, &zero_limit_query)
            .expect("query should evaluate")
            .is_empty()
    );

    let documents = evaluate_query(&store, &query_for("tasks")).expect("query should evaluate");
    assert_eq!(documents.len(), 2);
}

#[test]
fn evaluator_compares_integers_and_floats() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let low = nimbus_core::Document::new(
        tasks_table(),
        serde_json::Map::from_iter([("rank".to_string(), json!(1))]),
    );
    let high = nimbus_core::Document::new(
        tasks_table(),
        serde_json::Map::from_iter([("rank".to_string(), json!(2.5))]),
    );
    store.insert(&low).expect("insert should succeed");
    store.insert(&high).expect("insert should succeed");

    let mut query = query_for("tasks");
    query.filters = vec![filter("rank", FilterOp::Gt, json!(1.5))];

    let documents = evaluate_query(&store, &query).expect("query should evaluate");
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].fields.get("rank"), Some(&json!(2.5)));
}

#[test]
fn evaluator_rejects_ordering_on_boolean_fields() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let document = nimbus_core::Document::new(
        tasks_table(),
        serde_json::Map::from_iter([("active".to_string(), json!(true))]),
    );
    store.insert(&document).expect("insert should succeed");

    let mut query = query_for("tasks");
    query.order = Some(OrderBy {
        field: "active".to_string(),
        direction: OrderDirection::Asc,
    });

    let error = evaluate_query(&store, &query).expect_err("query should fail");
    assert!(
        error
            .to_string()
            .contains("ordering only supports string and number fields"),
        "unexpected error: {error}"
    );
}

#[test]
fn paginate_without_cursor_returns_first_page() {
    let store = TenantStore::create_in_memory().expect("store should open");
    for title in ["alpha", "bravo", "charlie", "delta", "echo"] {
        let document = nimbus_core::Document::new(
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!(title))]),
        );
        store.insert(&document).expect("insert should succeed");
    }

    let page = evaluate_paginated(
        &store,
        &PaginatedQuery {
            query: Query {
                table: tasks_table(),
                filters: Vec::new(),
                order: Some(OrderBy {
                    field: "title".to_string(),
                    direction: OrderDirection::Asc,
                }),
                limit: None,
            },
            page_size: 2,
            after: None,
        },
    )
    .expect("pagination should succeed");

    assert_eq!(page.data.len(), 2);
    assert_eq!(page.data[0]["title"], json!("alpha"));
    assert_eq!(page.data[1]["title"], json!("bravo"));
    assert!(page.has_more);
    assert!(page.next_cursor.is_some());
}

#[test]
fn paginate_with_cursor_returns_next_page() {
    let store = TenantStore::create_in_memory().expect("store should open");
    for title in ["alpha", "bravo", "charlie", "delta", "echo"] {
        let document = nimbus_core::Document::new(
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!(title))]),
        );
        store.insert(&document).expect("insert should succeed");
    }

    let first_page = evaluate_paginated(
        &store,
        &PaginatedQuery {
            query: Query {
                table: tasks_table(),
                filters: Vec::new(),
                order: Some(OrderBy {
                    field: "title".to_string(),
                    direction: OrderDirection::Asc,
                }),
                limit: None,
            },
            page_size: 2,
            after: None,
        },
    )
    .expect("pagination should succeed");

    let second_page = evaluate_paginated(
        &store,
        &PaginatedQuery {
            query: Query {
                table: tasks_table(),
                filters: Vec::new(),
                order: Some(OrderBy {
                    field: "title".to_string(),
                    direction: OrderDirection::Asc,
                }),
                limit: None,
            },
            page_size: 2,
            after: first_page.next_cursor.clone(),
        },
    )
    .expect("pagination should succeed");

    assert_eq!(second_page.data.len(), 2);
    assert_eq!(second_page.data[0]["title"], json!("charlie"));
    assert_eq!(second_page.data[1]["title"], json!("delta"));
    assert!(second_page.has_more);
    assert!(second_page.next_cursor.is_some());
}

#[test]
fn paginate_rejects_cursor_for_different_query_shape() {
    let store = TenantStore::create_in_memory().expect("store should open");
    for title in ["alpha", "bravo", "charlie"] {
        let document = nimbus_core::Document::new(
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!(title))]),
        );
        store.insert(&document).expect("insert should succeed");
    }

    let first_page = evaluate_paginated(
        &store,
        &PaginatedQuery {
            query: Query {
                table: tasks_table(),
                filters: Vec::new(),
                order: Some(OrderBy {
                    field: "title".to_string(),
                    direction: OrderDirection::Asc,
                }),
                limit: None,
            },
            page_size: 2,
            after: None,
        },
    )
    .expect("pagination should succeed");

    let error = evaluate_paginated(
        &store,
        &PaginatedQuery {
            query: Query {
                table: tasks_table(),
                filters: Vec::new(),
                order: Some(OrderBy {
                    field: "title".to_string(),
                    direction: OrderDirection::Desc,
                }),
                limit: None,
            },
            page_size: 2,
            after: first_page.next_cursor,
        },
    )
    .expect_err("cursor should be rejected");

    assert!(matches!(error, Error::InvalidInput(_)));
}

#[test]
fn paginate_last_page_has_no_cursor() {
    let store = TenantStore::create_in_memory().expect("store should open");
    for title in ["alpha", "bravo", "charlie"] {
        let document = nimbus_core::Document::new(
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!(title))]),
        );
        store.insert(&document).expect("insert should succeed");
    }

    let first_page = evaluate_paginated(
        &store,
        &PaginatedQuery {
            query: Query {
                table: tasks_table(),
                filters: Vec::new(),
                order: Some(OrderBy {
                    field: "title".to_string(),
                    direction: OrderDirection::Asc,
                }),
                limit: None,
            },
            page_size: 2,
            after: None,
        },
    )
    .expect("pagination should succeed");

    let last_page = evaluate_paginated(
        &store,
        &PaginatedQuery {
            query: Query {
                table: tasks_table(),
                filters: Vec::new(),
                order: Some(OrderBy {
                    field: "title".to_string(),
                    direction: OrderDirection::Asc,
                }),
                limit: None,
            },
            page_size: 2,
            after: first_page.next_cursor,
        },
    )
    .expect("pagination should succeed");

    assert_eq!(last_page.data.len(), 1);
    assert_eq!(last_page.data[0]["title"], json!("charlie"));
    assert!(!last_page.has_more);
    assert!(last_page.next_cursor.is_none());
}

#[test]
fn paginate_empty_table_returns_empty_page() {
    let store = TenantStore::create_in_memory().expect("store should open");

    let page = evaluate_paginated(
        &store,
        &PaginatedQuery {
            query: Query {
                table: tasks_table(),
                filters: Vec::new(),
                order: Some(OrderBy {
                    field: "title".to_string(),
                    direction: OrderDirection::Asc,
                }),
                limit: None,
            },
            page_size: 2,
            after: None,
        },
    )
    .expect("pagination should succeed");

    assert!(page.data.is_empty());
    assert!(!page.has_more);
    assert!(page.next_cursor.is_none());
}

#[test]
fn paginate_with_filters_and_ordering() {
    let store = TenantStore::create_in_memory().expect("store should open");
    for (title, status) in [
        ("a", "todo"),
        ("b", "done"),
        ("c", "todo"),
        ("d", "todo"),
        ("e", "done"),
        ("f", "todo"),
        ("g", "todo"),
    ] {
        let document = nimbus_core::Document::new(
            tasks_table(),
            serde_json::Map::from_iter([
                ("title".to_string(), json!(title)),
                ("status".to_string(), json!(status)),
            ]),
        );
        store.insert(&document).expect("insert should succeed");
    }

    let first_page = evaluate_paginated(
        &store,
        &PaginatedQuery {
            query: Query {
                table: tasks_table(),
                filters: vec![filter("status", FilterOp::Eq, json!("todo"))],
                order: Some(OrderBy {
                    field: "title".to_string(),
                    direction: OrderDirection::Desc,
                }),
                limit: None,
            },
            page_size: 3,
            after: None,
        },
    )
    .expect("pagination should succeed");

    assert_eq!(
        first_page
            .data
            .iter()
            .map(|document| {
                document["title"]
                    .as_str()
                    .expect("title should be a string")
            })
            .collect::<Vec<_>>(),
        vec!["g", "f", "d"]
    );
    assert!(first_page.has_more);

    let second_page = evaluate_paginated(
        &store,
        &PaginatedQuery {
            query: Query {
                table: tasks_table(),
                filters: vec![filter("status", FilterOp::Eq, json!("todo"))],
                order: Some(OrderBy {
                    field: "title".to_string(),
                    direction: OrderDirection::Desc,
                }),
                limit: None,
            },
            page_size: 3,
            after: first_page.next_cursor,
        },
    )
    .expect("pagination should succeed");

    assert_eq!(
        second_page
            .data
            .iter()
            .map(|document| {
                document["title"]
                    .as_str()
                    .expect("title should be a string")
            })
            .collect::<Vec<_>>(),
        vec!["c", "a"]
    );
    assert!(!second_page.has_more);
}

#[test]
fn fallback_query_filters_during_scan_for_selective_match() {
    let store = TenantStore::create_in_memory().expect("store should open");
    for rank in 0..512 {
        let status = if rank % 97 == 0 { "keep" } else { "skip" };
        let document = nimbus_core::Document::new(
            tasks_table(),
            serde_json::Map::from_iter([
                ("rank".to_string(), json!(rank)),
                ("status".to_string(), json!(status)),
            ]),
        );
        store.insert(&document).expect("insert should succeed");
    }

    let query = Query {
        table: tasks_table(),
        filters: vec![filter("status", FilterOp::Eq, json!("keep"))],
        order: Some(OrderBy {
            field: "rank".to_string(),
            direction: OrderDirection::Asc,
        }),
        limit: None,
    };

    let documents = evaluate_query(&store, &query).expect("fallback query should evaluate");
    let ranks = documents
        .into_iter()
        .map(|document| {
            document
                .fields
                .get("rank")
                .and_then(serde_json::Value::as_i64)
                .expect("rank should be present")
        })
        .collect::<Vec<_>>();
    assert_eq!(ranks, vec![0, 97, 194, 291, 388, 485]);
}

#[test]
fn paginated_fallback_scan_preserves_cursor_and_ordering() {
    let store = TenantStore::create_in_memory().expect("store should open");
    for rank in 0..12 {
        let status = if rank % 2 == 0 { "todo" } else { "done" };
        let document = nimbus_core::Document::new(
            tasks_table(),
            serde_json::Map::from_iter([
                ("rank".to_string(), json!(rank)),
                ("status".to_string(), json!(status)),
            ]),
        );
        store.insert(&document).expect("insert should succeed");
    }

    let paginated = PaginatedQuery {
        query: Query {
            table: tasks_table(),
            filters: vec![filter("status", FilterOp::Eq, json!("todo"))],
            order: Some(OrderBy {
                field: "rank".to_string(),
                direction: OrderDirection::Desc,
            }),
            limit: None,
        },
        page_size: 2,
        after: None,
    };

    let first_page =
        evaluate_paginated(&store, &paginated).expect("first fallback page should evaluate");
    assert_eq!(
        first_page
            .data
            .iter()
            .map(|document| {
                document
                    .get("rank")
                    .and_then(serde_json::Value::as_i64)
                    .expect("rank should be present")
            })
            .collect::<Vec<_>>(),
        vec![10, 8]
    );
    assert!(first_page.has_more);
    let second_page = evaluate_paginated(
        &store,
        &PaginatedQuery {
            after: first_page.next_cursor.clone(),
            ..paginated.clone()
        },
    )
    .expect("second fallback page should evaluate");
    assert_eq!(
        second_page
            .data
            .iter()
            .map(|document| {
                document
                    .get("rank")
                    .and_then(serde_json::Value::as_i64)
                    .expect("rank should be present")
            })
            .collect::<Vec<_>>(),
        vec![6, 4]
    );
    assert!(second_page.has_more);
    let third_page = evaluate_paginated(
        &store,
        &PaginatedQuery {
            after: second_page.next_cursor.clone(),
            ..paginated
        },
    )
    .expect("third fallback page should evaluate");
    assert_eq!(
        third_page
            .data
            .iter()
            .map(|document| {
                document
                    .get("rank")
                    .and_then(serde_json::Value::as_i64)
                    .expect("rank should be present")
            })
            .collect::<Vec<_>>(),
        vec![2, 0]
    );
    assert!(!third_page.has_more);
    assert!(third_page.next_cursor.is_none());
}

#[test]
fn paginate_rejects_zero_page_size() {
    let store = TenantStore::create_in_memory().expect("store should open");
    let error = evaluate_paginated(
        &store,
        &PaginatedQuery {
            query: Query {
                table: tasks_table(),
                filters: Vec::new(),
                order: None,
                limit: None,
            },
            page_size: 0,
            after: None,
        },
    )
    .expect_err("pagination should fail");

    assert!(matches!(error, Error::InvalidInput(_)));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn evaluator_gt_results_are_subset_of_gte(
        values in prop::collection::vec(-50i64..50, 0..20),
        threshold in -50i64..50,
    ) {
        let store = TenantStore::create_in_memory().expect("store should open");
        for value in values {
            store.insert(&rank_document(value)).expect("insert should succeed");
        }

        let mut gt_query = query_for("tasks");
        gt_query.filters = vec![filter("rank", FilterOp::Gt, json!(threshold))];
        let gt_documents = evaluate_query(&store, &gt_query).expect("gt query should evaluate");

        let mut gte_query = query_for("tasks");
        gte_query.filters = vec![filter("rank", FilterOp::Gte, json!(threshold))];
        let gte_documents = evaluate_query(&store, &gte_query).expect("gte query should evaluate");

        let gt_ids = gt_documents
            .iter()
            .map(|document| document.id.to_string())
            .collect::<BTreeSet<_>>();
        let gte_ids = gte_documents
            .iter()
            .map(|document| document.id.to_string())
            .collect::<BTreeSet<_>>();

        prop_assert!(gt_ids.is_subset(&gte_ids));
        for document in gt_documents {
            prop_assert!(
                document.fields["rank"]
                    .as_i64()
                    .expect("rank should be an i64")
                    > threshold
            );
        }
    }

    #[test]
    fn evaluator_descending_matches_reversed_ascending_for_unique_values(
        values in prop::collection::btree_set(-50i64..50, 0..20),
    ) {
        let store = TenantStore::create_in_memory().expect("store should open");
        for value in &values {
            store.insert(&rank_document(*value)).expect("insert should succeed");
        }

        let mut asc_query = query_for("tasks");
        asc_query.order = Some(OrderBy {
            field: "rank".to_string(),
            direction: OrderDirection::Asc,
        });
        let asc = evaluate_query(&store, &asc_query)
            .expect("ascending query should evaluate")
            .into_iter()
            .map(|document| {
                document.fields["rank"]
                    .as_i64()
                    .expect("rank should be an i64")
            })
            .collect::<Vec<_>>();

        let mut desc_query = query_for("tasks");
        desc_query.order = Some(OrderBy {
            field: "rank".to_string(),
            direction: OrderDirection::Desc,
        });
        let desc = evaluate_query(&store, &desc_query)
            .expect("descending query should evaluate")
            .into_iter()
            .map(|document| {
                document.fields["rank"]
                    .as_i64()
                    .expect("rank should be an i64")
            })
            .collect::<Vec<_>>();

        let mut expected = asc.clone();
        expected.reverse();
        prop_assert_eq!(desc, expected);
    }

    #[test]
    fn evaluator_limit_never_exceeds_requested_size(
        values in prop::collection::vec(-50i64..50, 0..20),
        limit in 0usize..30,
    ) {
        let store = TenantStore::create_in_memory().expect("store should open");
        for value in &values {
            store.insert(&rank_document(*value)).expect("insert should succeed");
        }

        let mut query = query_for("tasks");
        query.limit = Some(limit);
        let documents = evaluate_query(&store, &query).expect("query should evaluate");

        prop_assert!(documents.len() <= limit);
        prop_assert!(documents.len() <= values.len());
    }
}
