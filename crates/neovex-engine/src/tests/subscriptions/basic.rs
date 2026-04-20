use super::*;

#[tokio::test]
async fn service_insert_drives_subscription_updates() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    let (tx, mut rx) = subscription_channel();
    let query = Query {
        table: TableName::new("tasks").expect("table name should be valid"),
        filters: Vec::new(),
        order: None,
        limit: None,
    };

    let subscription = service
        .subscribe(&tenant_id, query, "req-1".to_string(), tx)
        .expect("subscribe should succeed");
    let subscription_id = subscription.id();
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

    let (tx, mut rx) = subscription_channel();
    let query = Query {
        table: TableName::new("tasks").expect("table name should be valid"),
        filters: Vec::new(),
        order: None,
        limit: None,
    };

    let subscription = service
        .subscribe(&tenant_id, query, "req-2".to_string(), tx)
        .expect("subscribe should succeed");
    let subscription_id = subscription.id();
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
async fn slow_subscription_channels_are_dropped_instead_of_growing_unbounded() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    let document_id = service
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("Before"))]),
        )
        .expect("insert should succeed");

    let (tx, mut rx) = mpsc::channel::<SubscriptionUpdate>(1);
    let _subscription = service
        .subscribe(&tenant_id, query_for("tasks"), "slow-sub".to_string(), tx)
        .expect("subscribe should succeed");

    assert_eq!(
        service
            .active_subscription_count(&tenant_id)
            .expect("subscription count should load"),
        1
    );

    service
        .update_document(
            &tenant_id,
            tasks_table(),
            document_id,
            serde_json::Map::from_iter([("title".to_string(), json!("After"))]),
        )
        .expect("update should succeed");

    wait_for_active_subscription_count(
        &service,
        &tenant_id,
        "slow subscription should still be dropped after async delivery attempts",
        0,
    )
    .await;

    let initial = rx
        .recv()
        .await
        .expect("initial update should still be buffered");
    match initial {
        SubscriptionUpdate::Result { data, .. } => {
            assert_eq!(data.len(), 1);
            assert_eq!(data[0]["title"], json!("Before"));
        }
        other => panic!("unexpected initial subscription event: {other:?}"),
    }
    assert!(matches!(
        rx.try_recv(),
        Err(tokio::sync::mpsc::error::TryRecvError::Empty)
            | Err(tokio::sync::mpsc::error::TryRecvError::Disconnected)
    ));
}
