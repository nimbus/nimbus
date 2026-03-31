use neovex_core::{
    CreateCronRequest, Error, FieldSchema, FieldType, Filter, FilterOp, IndexDefinition, Mutation,
    OrderBy, OrderDirection, PaginatedQuery, Query, ScheduleRequest, ScheduledJobOutcome,
    TableName, TableSchema, TenantId, Timestamp,
};
use neovex_test_support::ServiceFixture;
use proptest::prelude::*;
use serde_json::json;
use std::collections::BTreeSet;
use std::sync::Arc;
use tempfile::tempdir;
use tokio::sync::{mpsc, watch};
use tokio::time::{Duration, timeout};

use crate::evaluator::{
    evaluate_paginated, evaluate_paginated_cancellable, evaluate_query, evaluate_query_cancellable,
};
use crate::{Service, SubscriptionUpdate};

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

fn rank_document(rank: i64) -> neovex_core::Document {
    neovex_core::Document::new(
        tasks_table(),
        serde_json::Map::from_iter([("rank".to_string(), json!(rank))]),
    )
}

fn users_schema() -> TableSchema {
    TableSchema {
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
    }
}

fn insert_task_mutation(title: &str) -> Mutation {
    Mutation::Insert {
        table: tasks_table(),
        fields: serde_json::Map::from_iter([("title".to_string(), json!(title))]),
    }
}

async fn spawn_scheduler(
    service: Arc<Service>,
    interval: Duration,
) -> (watch::Sender<bool>, tokio::task::JoinHandle<()>) {
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let handle = tokio::spawn(async move {
        crate::scheduler::run_scheduler_with_interval(service, shutdown_rx, interval).await;
    });
    (shutdown_tx, handle)
}

#[test]
fn evaluator_returns_ordered_and_limited_results() {
    let store = neovex_storage::TenantStore::create_in_memory().expect("store should open");
    let a = neovex_core::Document::new(
        TableName::new("tasks").expect("table name should be valid"),
        serde_json::Map::from_iter([("title".to_string(), json!("B"))]),
    );
    let b = neovex_core::Document::new(
        TableName::new("tasks").expect("table name should be valid"),
        serde_json::Map::from_iter([("title".to_string(), json!("A"))]),
    );
    store.insert(&a).expect("insert should succeed");
    store.insert(&b).expect("insert should succeed");

    let query = Query {
        table: TableName::new("tasks").expect("table name should be valid"),
        filters: Vec::new(),
        order: Some(neovex_core::OrderBy {
            field: "title".to_string(),
            direction: neovex_core::OrderDirection::Asc,
        }),
        limit: Some(1),
    };

    let documents = evaluate_query(&store, &query).expect("query should evaluate");
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].fields.get("title"), Some(&json!("A")));
}

#[test]
fn evaluator_applies_equality_filters() {
    let store = neovex_storage::TenantStore::create_in_memory().expect("store should open");
    let todo = neovex_core::Document::new(
        TableName::new("tasks").expect("table name should be valid"),
        serde_json::Map::from_iter([("status".to_string(), json!("todo"))]),
    );
    let done = neovex_core::Document::new(
        TableName::new("tasks").expect("table name should be valid"),
        serde_json::Map::from_iter([("status".to_string(), json!("done"))]),
    );
    store.insert(&todo).expect("insert should succeed");
    store.insert(&done).expect("insert should succeed");

    let query = Query {
        table: TableName::new("tasks").expect("table name should be valid"),
        filters: vec![neovex_core::Filter {
            field: "status".to_string(),
            op: neovex_core::FilterOp::Eq,
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
    let store = neovex_storage::TenantStore::create_in_memory().expect("store should open");
    let alpha = neovex_core::Document::new(
        TableName::new("tasks").expect("table name should be valid"),
        serde_json::Map::from_iter([("rank".to_string(), json!("1"))]),
    );
    let beta = neovex_core::Document::new(
        TableName::new("tasks").expect("table name should be valid"),
        serde_json::Map::from_iter([("rank".to_string(), json!(2))]),
    );
    store.insert(&alpha).expect("insert should succeed");
    store.insert(&beta).expect("insert should succeed");

    let query = Query {
        table: TableName::new("tasks").expect("table name should be valid"),
        filters: Vec::new(),
        order: Some(neovex_core::OrderBy {
            field: "rank".to_string(),
            direction: neovex_core::OrderDirection::Asc,
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
    let store = neovex_storage::TenantStore::create_in_memory().expect("store should open");
    let todo = neovex_core::Document::new(
        tasks_table(),
        serde_json::Map::from_iter([("status".to_string(), json!("todo"))]),
    );
    let done = neovex_core::Document::new(
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
    let store = neovex_storage::TenantStore::create_in_memory().expect("store should open");
    for rank in [1, 2, 3] {
        let document = neovex_core::Document::new(
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
fn evaluator_query_cancellable_stops_mid_scan() {
    let store = neovex_storage::TenantStore::create_in_memory().expect("store should open");
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
    let store = neovex_storage::TenantStore::create_in_memory().expect("store should open");
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
    let store = neovex_storage::TenantStore::create_in_memory().expect("store should open");
    for title in ["alpha", "bravo", "charlie"] {
        let document = neovex_core::Document::new(
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
    let store = neovex_storage::TenantStore::create_in_memory().expect("store should open");
    let titled = neovex_core::Document::new(
        tasks_table(),
        serde_json::Map::from_iter([("title".to_string(), json!("Hello"))]),
    );
    let untitled = neovex_core::Document::new(tasks_table(), serde_json::Map::new());
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
    let store = neovex_storage::TenantStore::create_in_memory().expect("store should open");
    let alpha = neovex_core::Document::new(
        tasks_table(),
        serde_json::Map::from_iter([
            ("status".to_string(), json!("todo")),
            ("rank".to_string(), json!(1)),
        ]),
    );
    let beta = neovex_core::Document::new(
        tasks_table(),
        serde_json::Map::from_iter([
            ("status".to_string(), json!("todo")),
            ("rank".to_string(), json!(2)),
        ]),
    );
    let gamma = neovex_core::Document::new(
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
    let store = neovex_storage::TenantStore::create_in_memory().expect("store should open");
    for title in ["alpha", "bravo", "charlie"] {
        let document = neovex_core::Document::new(
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
    let store = neovex_storage::TenantStore::create_in_memory().expect("store should open");
    let first = neovex_core::Document::new(
        tasks_table(),
        serde_json::Map::from_iter([("title".to_string(), json!("second inserted"))]),
    );
    let second = neovex_core::Document::new(
        tasks_table(),
        serde_json::Map::from_iter([("title".to_string(), json!("first inserted"))]),
    );
    store.insert(&second).expect("insert should succeed");
    store.insert(&first).expect("insert should succeed");

    let documents = evaluate_query(&store, &query_for("tasks")).expect("query should evaluate");
    let ids = documents
        .iter()
        .map(|document| document.id)
        .collect::<Vec<_>>();
    let mut sorted_ids = ids.clone();
    sorted_ids.sort();
    assert_eq!(ids, sorted_ids);
}

#[test]
fn evaluator_honors_limit_zero_and_none() {
    let store = neovex_storage::TenantStore::create_in_memory().expect("store should open");
    for title in ["alpha", "bravo"] {
        let document = neovex_core::Document::new(
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
    let store = neovex_storage::TenantStore::create_in_memory().expect("store should open");
    let low = neovex_core::Document::new(
        tasks_table(),
        serde_json::Map::from_iter([("rank".to_string(), json!(1))]),
    );
    let high = neovex_core::Document::new(
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
    let store = neovex_storage::TenantStore::create_in_memory().expect("store should open");
    let document = neovex_core::Document::new(
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

#[tokio::test]
async fn service_insert_drives_subscription_updates() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    let (tx, mut rx) = mpsc::unbounded_channel::<SubscriptionUpdate>();
    let query = Query {
        table: TableName::new("tasks").expect("table name should be valid"),
        filters: Vec::new(),
        order: None,
        limit: None,
    };

    let subscription_id = service
        .subscribe(&tenant_id, query, "req-1".to_string(), tx)
        .expect("subscribe should succeed");
    let initial = rx.recv().await.expect("initial update should arrive");
    match initial {
        SubscriptionUpdate::Result {
            subscription_id: actual_id,
            request_id,
            data,
            ..
        } => {
            assert_eq!(actual_id, subscription_id);
            assert_eq!(request_id.as_deref(), Some("req-1"));
            assert!(data.is_empty());
        }
        other => panic!("unexpected initial subscription event: {other:?}"),
    }

    service
        .insert_document(
            &tenant_id,
            TableName::new("tasks").expect("table name should be valid"),
            serde_json::Map::from_iter([("title".to_string(), json!("Hello"))]),
        )
        .expect("insert should succeed");

    let update = rx.recv().await.expect("reactive update should arrive");
    match update {
        SubscriptionUpdate::Result {
            subscription_id: actual_id,
            request_id,
            data,
            ..
        } => {
            assert_eq!(actual_id, subscription_id);
            assert!(request_id.is_none());
            assert_eq!(data.len(), 1);
            assert_eq!(data[0]["title"], json!("Hello"));
        }
        other => panic!("unexpected reactive update: {other:?}"),
    }
}

#[tokio::test]
async fn service_update_and_delete_drive_subscription_updates() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    let document_id = service
        .insert_document(
            &tenant_id,
            TableName::new("tasks").expect("table name should be valid"),
            serde_json::Map::from_iter([("title".to_string(), json!("Before"))]),
        )
        .expect("insert should succeed");

    let (tx, mut rx) = mpsc::unbounded_channel::<SubscriptionUpdate>();
    let query = Query {
        table: TableName::new("tasks").expect("table name should be valid"),
        filters: Vec::new(),
        order: None,
        limit: None,
    };

    let subscription_id = service
        .subscribe(&tenant_id, query, "req-2".to_string(), tx)
        .expect("subscribe should succeed");
    let initial = rx.recv().await.expect("initial update should arrive");
    match initial {
        SubscriptionUpdate::Result {
            subscription_id: actual_id,
            data,
            ..
        } => {
            assert_eq!(actual_id, subscription_id);
            assert_eq!(data.len(), 1);
            assert_eq!(data[0]["title"], json!("Before"));
        }
        other => panic!("unexpected initial subscription event: {other:?}"),
    }

    service
        .update_document(
            &tenant_id,
            TableName::new("tasks").expect("table name should be valid"),
            document_id,
            serde_json::Map::from_iter([("title".to_string(), json!("After"))]),
        )
        .expect("update should succeed");

    let updated = rx.recv().await.expect("update should arrive");
    match updated {
        SubscriptionUpdate::Result {
            subscription_id: actual_id,
            request_id,
            data,
            ..
        } => {
            assert_eq!(actual_id, subscription_id);
            assert!(request_id.is_none());
            assert_eq!(data[0]["title"], json!("After"));
        }
        other => panic!("unexpected update subscription event: {other:?}"),
    }

    service
        .delete_document(
            &tenant_id,
            TableName::new("tasks").expect("table name should be valid"),
            document_id,
        )
        .expect("delete should succeed");

    let deleted = rx.recv().await.expect("delete should arrive");
    match deleted {
        SubscriptionUpdate::Result {
            subscription_id: actual_id,
            data,
            ..
        } => {
            assert_eq!(actual_id, subscription_id);
            assert_eq!(data, Vec::<serde_json::Value>::new());
        }
        other => panic!("unexpected delete subscription event: {other:?}"),
    }
}

#[tokio::test]
async fn service_only_notifies_subscriptions_for_affected_tables() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    let (tasks_tx, mut tasks_rx) = mpsc::unbounded_channel::<SubscriptionUpdate>();
    let (users_tx, mut users_rx) = mpsc::unbounded_channel::<SubscriptionUpdate>();
    let tasks_query = Query {
        table: TableName::new("tasks").expect("table name should be valid"),
        filters: Vec::new(),
        order: None,
        limit: None,
    };
    let users_query = Query {
        table: TableName::new("users").expect("table name should be valid"),
        filters: Vec::new(),
        order: None,
        limit: None,
    };

    service
        .subscribe(&tenant_id, tasks_query, "tasks-1".to_string(), tasks_tx)
        .expect("tasks subscribe should succeed");
    service
        .subscribe(&tenant_id, users_query, "users-1".to_string(), users_tx)
        .expect("users subscribe should succeed");

    let _ = tasks_rx
        .recv()
        .await
        .expect("tasks initial update should arrive");
    let _ = users_rx
        .recv()
        .await
        .expect("users initial update should arrive");

    service
        .insert_document(
            &tenant_id,
            TableName::new("tasks").expect("table name should be valid"),
            serde_json::Map::from_iter([("title".to_string(), json!("Hello"))]),
        )
        .expect("insert should succeed");

    let tasks_update = tasks_rx.recv().await.expect("tasks update should arrive");
    match tasks_update {
        SubscriptionUpdate::Result { data, .. } => {
            assert_eq!(data.len(), 1);
            assert_eq!(data[0]["title"], json!("Hello"));
        }
        other => panic!("unexpected tasks subscription event: {other:?}"),
    }

    let users_update = timeout(Duration::from_millis(100), users_rx.recv()).await;
    assert!(
        users_update.is_err(),
        "users subscription should not be invalidated"
    );
}

#[tokio::test]
async fn service_does_not_fail_committed_mutation_when_subscription_re_evaluation_errors() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    service
        .insert_document(
            &tenant_id,
            TableName::new("tasks").expect("table name should be valid"),
            serde_json::Map::from_iter([("rank".to_string(), json!("1"))]),
        )
        .expect("seed insert should succeed");

    let (tx, mut rx) = mpsc::unbounded_channel::<SubscriptionUpdate>();
    let query = Query {
        table: TableName::new("tasks").expect("table name should be valid"),
        filters: Vec::new(),
        order: Some(neovex_core::OrderBy {
            field: "rank".to_string(),
            direction: neovex_core::OrderDirection::Asc,
        }),
        limit: None,
    };

    service
        .subscribe(&tenant_id, query, "req-3".to_string(), tx)
        .expect("subscribe should succeed");
    let _ = rx
        .recv()
        .await
        .expect("initial subscription result should arrive");

    let result = service.insert_document(
        &tenant_id,
        TableName::new("tasks").expect("table name should be valid"),
        serde_json::Map::from_iter([("rank".to_string(), json!(2))]),
    );
    assert!(result.is_ok(), "committed mutation should still succeed");

    let event = rx
        .recv()
        .await
        .expect("subscription error event should arrive");
    match event {
        SubscriptionUpdate::Error { message, .. } => {
            assert!(message.contains("ordering cannot mix string and number values"));
        }
        other => panic!("unexpected subscription event: {other:?}"),
    }

    service
        .update_document(
            &tenant_id,
            TableName::new("tasks").expect("table name should be valid"),
            result.expect("insert should return document id"),
            serde_json::Map::from_iter([("rank".to_string(), json!("2"))]),
        )
        .expect("repair update should succeed");

    let recovered = rx
        .recv()
        .await
        .expect("recovered subscription result should arrive");
    match recovered {
        SubscriptionUpdate::Result { data, .. } => {
            assert_eq!(data.len(), 2);
        }
        other => panic!("unexpected recovered subscription event: {other:?}"),
    }
}

#[tokio::test]
async fn service_delete_tenant_tears_down_active_subscriptions() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    let (tx, mut rx) = mpsc::unbounded_channel::<SubscriptionUpdate>();
    let query = Query {
        table: TableName::new("tasks").expect("table name should be valid"),
        filters: Vec::new(),
        order: None,
        limit: None,
    };

    let subscription_id = service
        .subscribe(&tenant_id, query, "req-delete".to_string(), tx)
        .expect("subscribe should succeed");
    let _ = rx.recv().await.expect("initial update should arrive");

    service
        .delete_tenant(&tenant_id)
        .expect("tenant delete should succeed");

    let teardown = rx.recv().await.expect("teardown error should arrive");
    match teardown {
        SubscriptionUpdate::Error {
            subscription_id: actual_id,
            request_id,
            message,
        } => {
            assert_eq!(actual_id, subscription_id);
            assert!(request_id.is_none());
            assert!(message.contains("tenant deleted: demo"));
        }
        other => panic!("unexpected teardown event: {other:?}"),
    }
}

#[tokio::test]
async fn service_create_duplicate_tenant_returns_already_exists() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    let error = service
        .create_tenant(tenant_id)
        .expect_err("duplicate tenant should fail");
    assert!(matches!(error, Error::AlreadyExists(_)));
}

#[tokio::test]
async fn service_delete_nonexistent_tenant_returns_not_found() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = TenantId::new("demo").expect("tenant id should be valid");

    let error = service
        .delete_tenant(&tenant_id)
        .expect_err("missing tenant should fail");
    assert!(matches!(error, Error::TenantNotFound(_)));
}

#[tokio::test]
async fn service_missing_document_operations_return_not_found() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let missing_id = neovex_core::DocumentId::new();

    let get_error = service
        .get_document(&tenant_id, &tasks_table(), missing_id)
        .expect_err("missing get should fail");
    assert!(matches!(get_error, Error::DocumentNotFound(_)));

    let update_error = service
        .update_document(
            &tenant_id,
            tasks_table(),
            missing_id,
            serde_json::Map::from_iter([("title".to_string(), json!("After"))]),
        )
        .expect_err("missing update should fail");
    assert!(matches!(update_error, Error::DocumentNotFound(_)));

    let delete_error = service
        .delete_document(&tenant_id, tasks_table(), missing_id)
        .expect_err("missing delete should fail");
    assert!(matches!(delete_error, Error::DocumentNotFound(_)));
}

#[tokio::test]
async fn service_tenant_data_is_isolated_across_tenants() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let alpha_tenant = fixture.create_tenant("alpha", Service::create_tenant);
    let beta_tenant = fixture.create_tenant("beta", Service::create_tenant);

    service
        .insert_document(
            &alpha_tenant,
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("Alpha"))]),
        )
        .expect("insert should succeed");
    service
        .insert_document(
            &beta_tenant,
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("Beta"))]),
        )
        .expect("insert should succeed");

    let alpha_docs = service
        .list_documents(&alpha_tenant, &tasks_table())
        .expect("list should succeed");
    let beta_docs = service
        .list_documents(&beta_tenant, &tasks_table())
        .expect("list should succeed");

    assert_eq!(alpha_docs.len(), 1);
    assert_eq!(beta_docs.len(), 1);
    assert_eq!(alpha_docs[0].fields.get("title"), Some(&json!("Alpha")));
    assert_eq!(beta_docs[0].fields.get("title"), Some(&json!("Beta")));
}

#[tokio::test]
async fn service_lazy_loads_tenant_from_disk() {
    let data_dir = tempdir().expect("tempdir should create");
    let service = Service::new(data_dir.path()).expect("service should create");
    let tenant_id = TenantId::new("demo").expect("tenant id should be valid");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");
    service
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("Persisted"))]),
        )
        .expect("insert should succeed");

    drop(service);

    let reloaded = Service::new(data_dir.path()).expect("service should reopen");
    let documents = reloaded
        .list_documents(&tenant_id, &tasks_table())
        .expect("list should succeed");

    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].fields.get("title"), Some(&json!("Persisted")));
}

#[tokio::test]
async fn service_unsubscribe_stops_notifications() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    let (tx, mut rx) = mpsc::unbounded_channel::<SubscriptionUpdate>();
    let subscription_id = service
        .subscribe(&tenant_id, query_for("tasks"), "req-unsub".to_string(), tx)
        .expect("subscribe should succeed");
    let _ = rx.recv().await.expect("initial update should arrive");

    service
        .unsubscribe(&tenant_id, subscription_id)
        .expect("unsubscribe should succeed");
    service
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("Hello"))]),
        )
        .expect("insert should succeed");

    let result = timeout(Duration::from_millis(100), rx.recv()).await;
    assert!(
        !matches!(result, Ok(Some(_))),
        "unsubscribe should stop notifications"
    );
}

#[tokio::test]
async fn service_validates_insert_against_schema() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    service
        .set_table_schema(&tenant_id, users_schema())
        .expect("schema should save");

    let missing_name = service
        .insert_document(
            &tenant_id,
            TableName::new("users").expect("table name should be valid"),
            serde_json::Map::from_iter([("age".to_string(), json!(30))]),
        )
        .expect_err("insert should fail");
    assert!(matches!(missing_name, Error::SchemaValidation(_)));

    let wrong_type = service
        .insert_document(
            &tenant_id,
            TableName::new("users").expect("table name should be valid"),
            serde_json::Map::from_iter([("name".to_string(), json!(123))]),
        )
        .expect_err("insert should fail");
    assert!(matches!(wrong_type, Error::SchemaValidation(_)));

    service
        .insert_document(
            &tenant_id,
            TableName::new("users").expect("table name should be valid"),
            serde_json::Map::from_iter([
                ("name".to_string(), json!("Alice")),
                ("age".to_string(), json!(30)),
            ]),
        )
        .expect("insert should succeed");
}

#[tokio::test]
async fn service_validates_update_against_full_document() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    service
        .set_table_schema(&tenant_id, users_schema())
        .expect("schema should save");
    let document_id = service
        .insert_document(
            &tenant_id,
            TableName::new("users").expect("table name should be valid"),
            serde_json::Map::from_iter([
                ("name".to_string(), json!("Alice")),
                ("age".to_string(), json!(30)),
            ]),
        )
        .expect("insert should succeed");

    let wrong_type = service
        .update_document(
            &tenant_id,
            TableName::new("users").expect("table name should be valid"),
            document_id,
            serde_json::Map::from_iter([("age".to_string(), json!("not a number"))]),
        )
        .expect_err("update should fail");
    assert!(matches!(wrong_type, Error::SchemaValidation(_)));

    service
        .update_document(
            &tenant_id,
            TableName::new("users").expect("table name should be valid"),
            document_id,
            serde_json::Map::from_iter([("age".to_string(), json!(31))]),
        )
        .expect("update should succeed");
}

#[tokio::test]
async fn no_schema_allows_anything() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    service
        .insert_document(
            &tenant_id,
            TableName::new("events").expect("table name should be valid"),
            serde_json::Map::from_iter([
                ("payload".to_string(), json!({ "kind": "anything" })),
                ("count".to_string(), json!(7)),
            ]),
        )
        .expect("insert should succeed");
}

#[tokio::test]
async fn query_uses_index_for_equality_filter() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let schema = TableSchema {
        table: tasks_table(),
        fields: vec![FieldSchema {
            name: "status".to_string(),
            field_type: FieldType::String,
            required: false,
        }],
        indexes: vec![neovex_core::IndexDefinition {
            name: "by_status".to_string(),
            field: "status".to_string(),
        }],
    };
    service
        .set_table_schema(&tenant_id, schema)
        .expect("schema should save");

    for index in 0..100 {
        let status = if index < 10 { "active" } else { "inactive" };
        service
            .insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([("status".to_string(), json!(status))]),
            )
            .expect("insert should succeed");
    }

    let documents = service
        .query_documents(
            &tenant_id,
            &Query {
                table: tasks_table(),
                filters: vec![filter("status", FilterOp::Eq, json!("active"))],
                order: None,
                limit: None,
            },
        )
        .expect("query should succeed");
    assert_eq!(documents.len(), 10);
}

#[tokio::test]
async fn subscription_initial_evaluation_uses_indexed_query_path() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let schema = TableSchema {
        table: tasks_table(),
        fields: vec![FieldSchema {
            name: "status".to_string(),
            field_type: FieldType::String,
            required: false,
        }],
        indexes: vec![neovex_core::IndexDefinition {
            name: "by_status".to_string(),
            field: "status".to_string(),
        }],
    };
    service
        .set_table_schema(&tenant_id, schema)
        .expect("schema should save");

    for status in ["active", "inactive", "active"] {
        service
            .insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([("status".to_string(), json!(status))]),
            )
            .expect("insert should succeed");
    }

    let (tx, mut rx) = mpsc::unbounded_channel::<SubscriptionUpdate>();
    let subscription_id = service
        .subscribe(
            &tenant_id,
            Query {
                table: tasks_table(),
                filters: vec![filter("status", FilterOp::Eq, json!("active"))],
                order: None,
                limit: None,
            },
            "sub-index-1".to_string(),
            tx,
        )
        .expect("subscribe should succeed");

    let initial = rx.recv().await.expect("initial update should arrive");
    match initial {
        SubscriptionUpdate::Result {
            subscription_id: actual_id,
            request_id,
            data,
            ..
        } => {
            assert_eq!(actual_id, subscription_id);
            assert_eq!(request_id.as_deref(), Some("sub-index-1"));
            assert_eq!(data.len(), 2);
            assert!(
                data.iter()
                    .all(|document| document["status"] == json!("active"))
            );
        }
        other => panic!("unexpected initial subscription event: {other:?}"),
    }
}

#[tokio::test]
async fn setting_schema_backfills_indexes_for_existing_documents() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    for status in ["active", "inactive", "active"] {
        service
            .insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([("status".to_string(), json!(status))]),
            )
            .expect("insert should succeed");
    }

    let schema = TableSchema {
        table: tasks_table(),
        fields: vec![FieldSchema {
            name: "status".to_string(),
            field_type: FieldType::String,
            required: false,
        }],
        indexes: vec![neovex_core::IndexDefinition {
            name: "by_status".to_string(),
            field: "status".to_string(),
        }],
    };
    service
        .set_table_schema(&tenant_id, schema)
        .expect("schema should save");

    let documents = service
        .query_documents(
            &tenant_id,
            &Query {
                table: tasks_table(),
                filters: vec![filter("status", FilterOp::Eq, json!("active"))],
                order: None,
                limit: None,
            },
        )
        .expect("query should succeed");
    assert_eq!(documents.len(), 2);
}

#[tokio::test]
async fn query_uses_index_for_range_filter() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let schema = TableSchema {
        table: tasks_table(),
        fields: vec![FieldSchema {
            name: "rank".to_string(),
            field_type: FieldType::Number,
            required: false,
        }],
        indexes: vec![neovex_core::IndexDefinition {
            name: "by_rank".to_string(),
            field: "rank".to_string(),
        }],
    };
    service
        .set_table_schema(&tenant_id, schema)
        .expect("schema should save");

    for rank in 0..100 {
        service
            .insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([("rank".to_string(), json!(rank))]),
            )
            .expect("insert should succeed");
    }

    let documents = service
        .query_documents(
            &tenant_id,
            &Query {
                table: tasks_table(),
                filters: vec![filter("rank", FilterOp::Gte, json!(90))],
                order: Some(OrderBy {
                    field: "rank".to_string(),
                    direction: OrderDirection::Asc,
                }),
                limit: None,
            },
        )
        .expect("query should succeed");
    assert_eq!(documents.len(), 10);
    assert_eq!(documents[0].fields.get("rank"), Some(&json!(90)));
    assert_eq!(documents[9].fields.get("rank"), Some(&json!(99)));
}

#[tokio::test]
async fn query_uses_index_for_eq_filter_and_still_applies_remaining_filters() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let schema = TableSchema {
        table: tasks_table(),
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
        indexes: vec![neovex_core::IndexDefinition {
            name: "by_status".to_string(),
            field: "status".to_string(),
        }],
    };
    service
        .set_table_schema(&tenant_id, schema)
        .expect("schema should save");

    for (status, rank) in [("active", 1), ("active", 2), ("inactive", 2)] {
        service
            .insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([
                    ("status".to_string(), json!(status)),
                    ("rank".to_string(), json!(rank)),
                ]),
            )
            .expect("insert should succeed");
    }

    let documents = service
        .query_documents(
            &tenant_id,
            &Query {
                table: tasks_table(),
                filters: vec![
                    filter("status", FilterOp::Eq, json!("active")),
                    filter("rank", FilterOp::Gte, json!(2)),
                ],
                order: Some(OrderBy {
                    field: "rank".to_string(),
                    direction: OrderDirection::Asc,
                }),
                limit: None,
            },
        )
        .expect("query should succeed");

    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].fields.get("status"), Some(&json!("active")));
    assert_eq!(documents[0].fields.get("rank"), Some(&json!(2)));
}

#[tokio::test]
async fn subscription_re_evaluation_uses_indexed_query_path() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let schema = TableSchema {
        table: tasks_table(),
        fields: vec![FieldSchema {
            name: "status".to_string(),
            field_type: FieldType::String,
            required: false,
        }],
        indexes: vec![neovex_core::IndexDefinition {
            name: "by_status".to_string(),
            field: "status".to_string(),
        }],
    };
    service
        .set_table_schema(&tenant_id, schema)
        .expect("schema should save");

    service
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([("status".to_string(), json!("active"))]),
        )
        .expect("seed insert should succeed");

    let (tx, mut rx) = mpsc::unbounded_channel::<SubscriptionUpdate>();
    service
        .subscribe(
            &tenant_id,
            Query {
                table: tasks_table(),
                filters: vec![filter("status", FilterOp::Eq, json!("active"))],
                order: None,
                limit: None,
            },
            "sub-index-2".to_string(),
            tx,
        )
        .expect("subscribe should succeed");
    let _ = rx.recv().await.expect("initial update should arrive");

    service
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([("status".to_string(), json!("active"))]),
        )
        .expect("active insert should succeed");
    let active_update = rx.recv().await.expect("active update should arrive");
    match active_update {
        SubscriptionUpdate::Result { data, .. } => {
            assert_eq!(data.len(), 2);
            assert!(
                data.iter()
                    .all(|document| document["status"] == json!("active"))
            );
        }
        other => panic!("unexpected active update: {other:?}"),
    }

    service
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([("status".to_string(), json!("inactive"))]),
        )
        .expect("inactive insert should succeed");
    let inactive_update = rx.recv().await.expect("inactive update should arrive");
    match inactive_update {
        SubscriptionUpdate::Result { data, .. } => {
            assert_eq!(data.len(), 2);
            assert!(
                data.iter()
                    .all(|document| document["status"] == json!("active"))
            );
        }
        other => panic!("unexpected inactive update: {other:?}"),
    }
}

#[tokio::test]
async fn query_uses_index_for_bounded_range_filter() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let schema = TableSchema {
        table: tasks_table(),
        fields: vec![FieldSchema {
            name: "rank".to_string(),
            field_type: FieldType::Number,
            required: false,
        }],
        indexes: vec![neovex_core::IndexDefinition {
            name: "by_rank".to_string(),
            field: "rank".to_string(),
        }],
    };
    service
        .set_table_schema(&tenant_id, schema)
        .expect("schema should save");

    for rank in 0..50 {
        service
            .insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([("rank".to_string(), json!(rank))]),
            )
            .expect("insert should succeed");
    }

    let documents = service
        .query_documents(
            &tenant_id,
            &Query {
                table: tasks_table(),
                filters: vec![
                    filter("rank", FilterOp::Gte, json!(20)),
                    filter("rank", FilterOp::Lt, json!(25)),
                ],
                order: Some(OrderBy {
                    field: "rank".to_string(),
                    direction: OrderDirection::Asc,
                }),
                limit: None,
            },
        )
        .expect("query should succeed");
    assert_eq!(documents.len(), 5);
    assert_eq!(documents[0].fields.get("rank"), Some(&json!(20)));
    assert_eq!(documents[4].fields.get("rank"), Some(&json!(24)));
}

#[tokio::test]
async fn query_documents_cancellable_stops_during_index_scan() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let schema = TableSchema {
        table: tasks_table(),
        fields: vec![FieldSchema {
            name: "rank".to_string(),
            field_type: FieldType::Number,
            required: false,
        }],
        indexes: vec![IndexDefinition {
            name: "by_rank".to_string(),
            field: "rank".to_string(),
        }],
    };
    service
        .set_table_schema(&tenant_id, schema)
        .expect("schema should save");

    for rank in 0..64 {
        service
            .insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([("rank".to_string(), json!(rank))]),
            )
            .expect("insert should succeed");
    }

    let mut checks = 0usize;
    let error = service
        .query_documents_cancellable(
            &tenant_id,
            &Query {
                table: tasks_table(),
                filters: vec![filter("rank", FilterOp::Gte, json!(0))],
                order: Some(OrderBy {
                    field: "rank".to_string(),
                    direction: OrderDirection::Asc,
                }),
                limit: None,
            },
            &mut || {
                checks += 1;
                if checks > 8 {
                    Err(Error::Cancelled)
                } else {
                    Ok(())
                }
            },
        )
        .expect_err("query should cancel");

    assert!(matches!(error, Error::Cancelled));
}

#[tokio::test]
async fn paginated_query_uses_index_for_range_filter() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let schema = TableSchema {
        table: tasks_table(),
        fields: vec![FieldSchema {
            name: "rank".to_string(),
            field_type: FieldType::Number,
            required: false,
        }],
        indexes: vec![neovex_core::IndexDefinition {
            name: "by_rank".to_string(),
            field: "rank".to_string(),
        }],
    };
    service
        .set_table_schema(&tenant_id, schema)
        .expect("schema should save");

    for rank in 0..10 {
        service
            .insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([("rank".to_string(), json!(rank))]),
            )
            .expect("insert should succeed");
    }

    let first_page = service
        .paginate_documents(
            &tenant_id,
            &PaginatedQuery {
                query: Query {
                    table: tasks_table(),
                    filters: vec![filter("rank", FilterOp::Gte, json!(5))],
                    order: Some(OrderBy {
                        field: "rank".to_string(),
                        direction: OrderDirection::Asc,
                    }),
                    limit: None,
                },
                page_size: 2,
                after: None,
            },
        )
        .expect("first page should succeed");
    assert_eq!(first_page.data.len(), 2);
    assert_eq!(first_page.data[0]["rank"], json!(5));
    assert_eq!(first_page.data[1]["rank"], json!(6));
    assert!(first_page.has_more);

    let second_page = service
        .paginate_documents(
            &tenant_id,
            &PaginatedQuery {
                query: Query {
                    table: tasks_table(),
                    filters: vec![filter("rank", FilterOp::Gte, json!(5))],
                    order: Some(OrderBy {
                        field: "rank".to_string(),
                        direction: OrderDirection::Asc,
                    }),
                    limit: None,
                },
                page_size: 2,
                after: first_page.next_cursor.clone(),
            },
        )
        .expect("second page should succeed");
    assert_eq!(second_page.data.len(), 2);
    assert_eq!(second_page.data[0]["rank"], json!(7));
    assert_eq!(second_page.data[1]["rank"], json!(8));
}

#[tokio::test]
async fn scheduled_mutation_executes_and_triggers_reactive_update() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let (shutdown_tx, scheduler_handle) =
        spawn_scheduler(service.clone(), Duration::from_millis(25)).await;

    let (tx, mut rx) = mpsc::unbounded_channel::<SubscriptionUpdate>();
    let subscription_id = service
        .subscribe(&tenant_id, query_for("tasks"), "sched-1".to_string(), tx)
        .expect("subscribe should succeed");
    let initial = rx.recv().await.expect("initial update should arrive");
    match initial {
        SubscriptionUpdate::Result {
            subscription_id: actual_id,
            data,
            ..
        } => {
            assert_eq!(actual_id, subscription_id);
            assert!(data.is_empty());
        }
        other => panic!("unexpected initial update: {other:?}"),
    }

    let job_id = service
        .schedule_mutation(
            &tenant_id,
            ScheduleRequest {
                run_after_ms: 50,
                mutation: insert_task_mutation("Scheduled task"),
            },
        )
        .expect("schedule should succeed");

    let update = timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("reactive update should arrive before timeout")
        .expect("reactive update channel should stay open");
    match update {
        SubscriptionUpdate::Result {
            subscription_id: actual_id,
            request_id,
            data,
            ..
        } => {
            assert_eq!(actual_id, subscription_id);
            assert!(request_id.is_none());
            assert_eq!(data.len(), 1);
            assert_eq!(data[0]["title"], json!("Scheduled task"));
        }
        other => panic!("unexpected scheduled update: {other:?}"),
    }

    assert!(
        service
            .list_scheduled_jobs(&tenant_id)
            .expect("list should succeed")
            .is_empty()
    );
    assert!(
        service.complete_scheduled_job(&tenant_id, &job_id).is_ok(),
        "completing an already-finished job should be harmless"
    );

    let _ = shutdown_tx.send(true);
    scheduler_handle.await.expect("scheduler should shut down");
}

#[tokio::test]
async fn scheduled_mutation_validates_against_schema() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    service
        .set_table_schema(&tenant_id, users_schema())
        .expect("schema should save");
    let job_id = service
        .schedule_mutation(
            &tenant_id,
            ScheduleRequest {
                run_after_ms: 0,
                mutation: Mutation::Insert {
                    table: TableName::new("users").expect("table name should be valid"),
                    fields: serde_json::Map::from_iter([("age".to_string(), json!(42))]),
                },
            },
        )
        .expect("schedule should succeed");

    crate::scheduler::tick_at(service.as_ref(), Timestamp::now()).expect("tick should succeed");

    assert!(
        service
            .list_scheduled_jobs(&tenant_id)
            .expect("list should succeed")
            .is_empty()
    );
    assert!(
        service
            .list_documents(
                &tenant_id,
                &TableName::new("users").expect("table name should be valid"),
            )
            .expect("list should succeed")
            .is_empty()
    );

    let result = service
        .get_scheduled_job_result(&tenant_id, &job_id)
        .expect("job result should exist");
    assert_eq!(result.outcome, ScheduledJobOutcome::Failed);
    assert!(
        result
            .error
            .as_deref()
            .expect("failed result should include an error")
            .contains("schema validation error")
    );
}

#[tokio::test]
async fn cron_job_executes_repeatedly() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    service
        .create_cron_job(
            &tenant_id,
            CreateCronRequest {
                name: "heartbeat".to_string(),
                schedule: neovex_core::CronSchedule::Interval { seconds: 1 },
                mutation: insert_task_mutation("heartbeat"),
            },
        )
        .expect("cron should create");

    for _ in 0..3 {
        let cron = service
            .load_cron_jobs(&tenant_id)
            .expect("load should succeed")
            .into_iter()
            .next()
            .expect("cron should exist");
        crate::scheduler::tick_at(service.as_ref(), cron.next_run).expect("tick should succeed");
    }

    let documents = service
        .list_documents(&tenant_id, &tasks_table())
        .expect("list should succeed");
    assert_eq!(documents.len(), 3);
}

#[tokio::test]
async fn cron_missed_ticks_execute_once() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    service
        .create_cron_job(
            &tenant_id,
            CreateCronRequest {
                name: "catchup".to_string(),
                schedule: neovex_core::CronSchedule::Interval { seconds: 1 },
                mutation: insert_task_mutation("catchup"),
            },
        )
        .expect("cron should create");

    let mut cron = service
        .load_cron_jobs(&tenant_id)
        .expect("load should succeed")
        .into_iter()
        .next()
        .expect("cron should exist");
    cron.next_run = Timestamp(1_000);
    cron.last_run = None;
    service
        .update_cron_job(&tenant_id, &cron)
        .expect("cron should update");

    crate::scheduler::tick_at(service.as_ref(), Timestamp(10_000)).expect("tick should succeed");

    let documents = service
        .list_documents(&tenant_id, &tasks_table())
        .expect("list should succeed");
    assert_eq!(documents.len(), 1);

    let updated = service
        .load_cron_jobs(&tenant_id)
        .expect("load should succeed")
        .into_iter()
        .next()
        .expect("cron should exist");
    assert_eq!(updated.last_run, Some(Timestamp(10_000)));
    assert!(updated.next_run.0 > 10_000);
}

#[tokio::test]
async fn load_tenants_with_scheduled_work_recovers_running_jobs() {
    let data_dir = tempdir().expect("tempdir should create");
    let tenant_id = TenantId::new("demo").expect("tenant id should be valid");

    {
        let service = Service::new(data_dir.path()).expect("service should create");
        service
            .create_tenant(tenant_id.clone())
            .expect("tenant should create");
        service
            .schedule_mutation(
                &tenant_id,
                ScheduleRequest {
                    run_after_ms: 0,
                    mutation: insert_task_mutation("Recovered task"),
                },
            )
            .expect("schedule should succeed");
        let claimed = service
            .claim_due_jobs(&tenant_id, Timestamp::now())
            .expect("claim should succeed");
        assert_eq!(claimed.len(), 1);
    }

    let reloaded = Service::new(data_dir.path()).expect("service should reopen");
    reloaded
        .load_tenants_with_scheduled_work()
        .expect("scheduled tenants should load");

    assert_eq!(reloaded.loaded_tenant_ids(), vec![tenant_id.clone()]);
    assert_eq!(
        reloaded
            .list_scheduled_jobs(&tenant_id)
            .expect("list should succeed")
            .len(),
        1
    );

    crate::scheduler::tick_at(&reloaded, Timestamp::now()).expect("tick should succeed");
    let documents = reloaded
        .list_documents(&tenant_id, &tasks_table())
        .expect("list should succeed");
    assert_eq!(documents.len(), 1);
    assert_eq!(
        documents[0].fields.get("title"),
        Some(&json!("Recovered task"))
    );
}

#[tokio::test]
async fn recovered_scheduled_job_does_not_double_apply_after_replay() {
    let data_dir = tempdir().expect("tempdir should create");
    let tenant_id = TenantId::new("demo").expect("tenant id should be valid");

    let job_id = {
        let service = Service::new(data_dir.path()).expect("service should create");
        service
            .create_tenant(tenant_id.clone())
            .expect("tenant should create");
        let job_id = service
            .schedule_mutation(
                &tenant_id,
                ScheduleRequest {
                    run_after_ms: 0,
                    mutation: insert_task_mutation("Only once"),
                },
            )
            .expect("schedule should succeed");
        let claimed = service
            .claim_due_jobs(&tenant_id, Timestamp::now())
            .expect("claim should succeed");
        assert_eq!(claimed.len(), 1);
        let execution_id = format!("scheduled:{job_id}");
        assert!(
            service
                .execute_scheduled_mutation(&tenant_id, &execution_id, claimed[0].mutation.clone(),)
                .expect("first scheduled execution should succeed")
        );
        job_id
    };

    let reloaded = Service::new(data_dir.path()).expect("service should reopen");
    reloaded
        .load_tenants_with_scheduled_work()
        .expect("scheduled tenants should load");

    crate::scheduler::tick_at(&reloaded, Timestamp::now()).expect("tick should succeed");
    let documents = reloaded
        .list_documents(&tenant_id, &tasks_table())
        .expect("list should succeed");
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].fields.get("title"), Some(&json!("Only once")));

    let result = reloaded
        .get_scheduled_job_result(&tenant_id, &job_id)
        .expect("job result should exist");
    assert_eq!(result.outcome, ScheduledJobOutcome::Completed);
    assert!(result.error.is_none());
}

#[test]
fn paginate_without_cursor_returns_first_page() {
    let store = neovex_storage::TenantStore::create_in_memory().expect("store should open");
    for title in ["alpha", "bravo", "charlie", "delta", "echo"] {
        let document = neovex_core::Document::new(
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
    let store = neovex_storage::TenantStore::create_in_memory().expect("store should open");
    for title in ["alpha", "bravo", "charlie", "delta", "echo"] {
        let document = neovex_core::Document::new(
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
    let store = neovex_storage::TenantStore::create_in_memory().expect("store should open");
    for title in ["alpha", "bravo", "charlie"] {
        let document = neovex_core::Document::new(
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
    let store = neovex_storage::TenantStore::create_in_memory().expect("store should open");
    for title in ["alpha", "bravo", "charlie"] {
        let document = neovex_core::Document::new(
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
    let store = neovex_storage::TenantStore::create_in_memory().expect("store should open");

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
    let store = neovex_storage::TenantStore::create_in_memory().expect("store should open");
    for (title, status) in [
        ("a", "todo"),
        ("b", "done"),
        ("c", "todo"),
        ("d", "todo"),
        ("e", "done"),
        ("f", "todo"),
        ("g", "todo"),
    ] {
        let document = neovex_core::Document::new(
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
            .map(|document| document["title"]
                .as_str()
                .expect("title should be a string"))
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
            .map(|document| document["title"]
                .as_str()
                .expect("title should be a string"))
            .collect::<Vec<_>>(),
        vec!["c", "a"]
    );
    assert!(!second_page.has_more);
}

#[test]
fn paginate_rejects_zero_page_size() {
    let store = neovex_storage::TenantStore::create_in_memory().expect("store should open");
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
        let store = neovex_storage::TenantStore::create_in_memory().expect("store should open");
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
        let store = neovex_storage::TenantStore::create_in_memory().expect("store should open");
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
        let store = neovex_storage::TenantStore::create_in_memory().expect("store should open");
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
