use super::*;

#[tokio::test]
async fn service_only_notifies_subscriptions_for_affected_tables() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    let (tasks_tx, mut tasks_rx) = subscription_channel();
    let (users_tx, mut users_rx) = subscription_channel();
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

    let _tasks_subscription = service
        .subscribe(&tenant_id, tasks_query, "tasks-1".to_string(), tasks_tx)
        .expect("tasks subscribe should succeed");
    let _users_subscription = service
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
        SubscriptionUpdate::Result { snapshot, .. } => {
            let data = snapshot.to_json_documents();
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
async fn service_insert_only_notifies_filtered_subscriptions_for_matching_documents() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    let (active_tx, mut active_rx) = subscription_channel();
    let (done_tx, mut done_rx) = subscription_channel();
    let active_query = Query {
        table: tasks_table(),
        filters: vec![filter("status", FilterOp::Eq, json!("active"))],
        order: None,
        limit: None,
    };
    let done_query = Query {
        table: tasks_table(),
        filters: vec![filter("status", FilterOp::Eq, json!("done"))],
        order: None,
        limit: None,
    };

    let _active_subscription = service
        .subscribe(&tenant_id, active_query, "active-1".to_string(), active_tx)
        .expect("active subscribe should succeed");
    let _done_subscription = service
        .subscribe(&tenant_id, done_query, "done-1".to_string(), done_tx)
        .expect("done subscribe should succeed");

    let _ = active_rx
        .recv()
        .await
        .expect("active initial update should arrive");
    let _ = done_rx
        .recv()
        .await
        .expect("done initial update should arrive");

    service
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([
                ("title".to_string(), json!("Ship it")),
                ("status".to_string(), json!("active")),
            ]),
        )
        .expect("insert should succeed");

    let active_update = active_rx.recv().await.expect("active update should arrive");
    match active_update {
        SubscriptionUpdate::Result { snapshot, .. } => {
            let data = snapshot.to_json_documents();
            assert_eq!(data.len(), 1);
            assert_eq!(data[0]["title"], json!("Ship it"));
        }
        other => panic!("unexpected active subscription event: {other:?}"),
    }

    let done_update = timeout(Duration::from_millis(100), done_rx.recv()).await;
    assert!(
        done_update.is_err(),
        "non-matching filtered subscription should not be invalidated"
    );
}

#[tokio::test]
async fn service_delete_only_notifies_filtered_subscriptions_for_matching_documents() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    let active_document_id = service
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([
                ("title".to_string(), json!("Keep moving")),
                ("status".to_string(), json!("active")),
            ]),
        )
        .expect("active seed insert should succeed");
    service
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([
                ("title".to_string(), json!("Archive")),
                ("status".to_string(), json!("done")),
            ]),
        )
        .expect("done seed insert should succeed");

    let (active_tx, mut active_rx) = subscription_channel();
    let (done_tx, mut done_rx) = subscription_channel();
    let active_query = Query {
        table: tasks_table(),
        filters: vec![filter("status", FilterOp::Eq, json!("active"))],
        order: None,
        limit: None,
    };
    let done_query = Query {
        table: tasks_table(),
        filters: vec![filter("status", FilterOp::Eq, json!("done"))],
        order: None,
        limit: None,
    };

    let _active_subscription = service
        .subscribe(
            &tenant_id,
            active_query,
            "active-delete".to_string(),
            active_tx,
        )
        .expect("active subscribe should succeed");
    let _done_subscription = service
        .subscribe(&tenant_id, done_query, "done-delete".to_string(), done_tx)
        .expect("done subscribe should succeed");

    let _ = active_rx
        .recv()
        .await
        .expect("active initial update should arrive");
    let _ = done_rx
        .recv()
        .await
        .expect("done initial update should arrive");

    service
        .delete_document(&tenant_id, tasks_table(), active_document_id)
        .expect("delete should succeed");

    let active_update = active_rx
        .recv()
        .await
        .expect("active delete update should arrive");
    match active_update {
        SubscriptionUpdate::Result { snapshot, .. } => {
            let data = snapshot.to_json_documents();
            assert!(data.is_empty());
            assert_eq!(snapshot.deleted_documents.len(), 1);
            assert_eq!(
                snapshot.deleted_documents[0].fields.get("status"),
                Some(&json!("active"))
            );
        }
        other => panic!("unexpected active delete subscription event: {other:?}"),
    }

    let done_update = timeout(Duration::from_millis(100), done_rx.recv()).await;
    assert!(
        done_update.is_err(),
        "deleting a non-matching document should not invalidate the other filter"
    );
}

#[tokio::test]
async fn service_updates_remain_conservative_for_filtered_subscriptions() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    let active_document_id = service
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([
                ("title".to_string(), json!("Before")),
                ("status".to_string(), json!("active")),
            ]),
        )
        .expect("seed insert should succeed");

    let (active_tx, mut active_rx) = subscription_channel();
    let (done_tx, mut done_rx) = subscription_channel();
    let active_query = Query {
        table: tasks_table(),
        filters: vec![filter("status", FilterOp::Eq, json!("active"))],
        order: None,
        limit: None,
    };
    let done_query = Query {
        table: tasks_table(),
        filters: vec![filter("status", FilterOp::Eq, json!("done"))],
        order: None,
        limit: None,
    };

    let _active_subscription = service
        .subscribe(
            &tenant_id,
            active_query,
            "active-update".to_string(),
            active_tx,
        )
        .expect("active subscribe should succeed");
    let _done_subscription = service
        .subscribe(&tenant_id, done_query, "done-update".to_string(), done_tx)
        .expect("done subscribe should succeed");

    let _ = active_rx
        .recv()
        .await
        .expect("active initial update should arrive");
    let _ = done_rx
        .recv()
        .await
        .expect("done initial update should arrive");

    service
        .update_document(
            &tenant_id,
            tasks_table(),
            active_document_id,
            serde_json::Map::from_iter([("title".to_string(), json!("After"))]),
        )
        .expect("update should succeed");

    let active_update = active_rx.recv().await.expect("active update should arrive");
    match active_update {
        SubscriptionUpdate::Result { snapshot, .. } => {
            let data = snapshot.to_json_documents();
            assert_eq!(data.len(), 1);
            assert_eq!(data[0]["title"], json!("After"));
        }
        other => panic!("unexpected active update subscription event: {other:?}"),
    }

    let done_update = done_rx
        .recv()
        .await
        .expect("done update should still arrive");
    match done_update {
        SubscriptionUpdate::Result { snapshot, .. } => {
            let data = snapshot.to_json_documents();
            assert!(data.is_empty());
        }
        other => panic!("unexpected done update subscription event: {other:?}"),
    }
}

#[tokio::test]
async fn service_limited_subscriptions_skip_out_of_window_ordered_writes() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    for rank in [1, 2, 3] {
        service
            .insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([
                    ("title".to_string(), json!(format!("Task {rank}"))),
                    ("rank".to_string(), json!(rank)),
                ]),
            )
            .expect("seed insert should succeed");
    }

    let (tx, mut rx) = subscription_channel();
    let query = Query {
        table: tasks_table(),
        filters: Vec::new(),
        order: Some(OrderBy {
            field: "rank".to_string(),
            direction: OrderDirection::Asc,
        }),
        limit: Some(2),
    };

    let _subscription = service
        .subscribe(&tenant_id, query, "ranked-limit".to_string(), tx)
        .expect("subscribe should succeed");

    let initial = rx.recv().await.expect("initial update should arrive");
    match initial {
        SubscriptionUpdate::Result { snapshot, .. } => {
            let data = snapshot.to_json_documents();
            assert_eq!(data.len(), 2);
            assert_eq!(data[0]["rank"], json!(1));
            assert_eq!(data[1]["rank"], json!(2));
        }
        other => panic!("unexpected initial subscription update: {other:?}"),
    }

    service
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([
                ("title".to_string(), json!("Task 99")),
                ("rank".to_string(), json!(99)),
            ]),
        )
        .expect("outside-window insert should succeed");
    assert!(
        timeout(Duration::from_millis(100), rx.recv())
            .await
            .is_err(),
        "writes beyond the visible ordered window should not invalidate the subscription"
    );

    let document_id = service
        .query_documents(
            &tenant_id,
            &Query {
                table: tasks_table(),
                filters: vec![filter("rank", FilterOp::Eq, json!(2))],
                order: None,
                limit: Some(1),
            },
        )
        .expect("rank lookup should succeed")
        .first()
        .map(|document| document.id.clone())
        .expect("rank-2 document should exist");
    service
        .update_document(
            &tenant_id,
            tasks_table(),
            document_id,
            serde_json::Map::from_iter([("rank".to_string(), json!(5))]),
        )
        .expect("window-shifting update should succeed");

    let shifted = rx.recv().await.expect("window shift update should arrive");
    match shifted {
        SubscriptionUpdate::Result { snapshot, .. } => {
            let data = snapshot.to_json_documents();
            assert_eq!(data.len(), 2);
            assert_eq!(data[0]["rank"], json!(1));
            assert_eq!(data[1]["rank"], json!(3));
        }
        other => panic!("unexpected shifted subscription update: {other:?}"),
    }

    service
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([
                ("title".to_string(), json!("Task 4")),
                ("rank".to_string(), json!(4)),
            ]),
        )
        .expect("second outside-window insert should succeed");
    assert!(
        timeout(Duration::from_millis(100), rx.recv())
            .await
            .is_err(),
        "dependency tracking should refresh after reevaluation and keep skipping later out-of-window writes"
    );

    service
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([
                ("title".to_string(), json!("Task 0")),
                ("rank".to_string(), json!(0)),
            ]),
        )
        .expect("inside-window insert should succeed");

    let refreshed = rx.recv().await.expect("inside-window update should arrive");
    match refreshed {
        SubscriptionUpdate::Result { snapshot, .. } => {
            let data = snapshot.to_json_documents();
            assert_eq!(data.len(), 2);
            assert_eq!(data[0]["rank"], json!(0));
            assert_eq!(data[1]["rank"], json!(1));
        }
        other => panic!("unexpected refreshed subscription update: {other:?}"),
    }
}
