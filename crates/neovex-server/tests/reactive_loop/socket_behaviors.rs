use super::*;

#[tokio::test]
async fn websocket_invalid_message_returns_error_event() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    assert!(api.create_tenant("demo").await.status().is_success());

    let mut socket = WebSocketFixture::connect(&api.ws_url("/ws"), "demo").await;
    socket.send_text("{not json").await;

    let message = socket.next_json().await;
    assert_eq!(message["type"], json!("error"));
    assert!(message["request_id"].is_null());
    assert!(
        message["message"]
            .as_str()
            .expect("message should be a string")
            .contains("invalid websocket message")
    );
}

#[tokio::test]
async fn websocket_unsubscribe_stops_receiving_updates() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    assert!(api.create_tenant("demo").await.status().is_success());

    let mut socket = WebSocketFixture::connect(&api.ws_url("/ws"), "demo").await;
    socket.subscribe_all("4", "tasks").await;

    let initial = socket.next_json().await;
    let subscription_id = initial["subscription_id"]
        .as_u64()
        .expect("subscription id should be present");

    socket.unsubscribe(subscription_id).await;
    assert!(
        api.insert_document("demo", "tasks", json!({ "title": "Hello" }))
            .await
            .status()
            .is_success()
    );

    let next = socket
        .next_json_with_timeout(Duration::from_millis(150))
        .await;
    assert!(next.is_none(), "unsubscribe should stop reactive pushes");
}

#[tokio::test]
async fn websocket_multiple_subscriptions_share_a_connection() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    assert!(api.create_tenant("demo").await.status().is_success());

    let mut socket = WebSocketFixture::connect(&api.ws_url("/ws"), "demo").await;
    socket.subscribe_all("tasks", "tasks").await;
    socket.subscribe_all("users", "users").await;

    let first = socket.next_json().await;
    let second = socket.next_json().await;
    assert_eq!(first["type"], json!("subscription_result"));
    assert_eq!(second["type"], json!("subscription_result"));

    assert!(
        api.insert_document("demo", "tasks", json!({ "title": "Hello" }))
            .await
            .status()
            .is_success()
    );

    let update = socket.next_json().await;
    assert_eq!(update["type"], json!("subscription_result"));
    assert_eq!(update["data"][0]["title"], json!("Hello"));

    let maybe_extra = socket
        .next_json_with_timeout(Duration::from_millis(150))
        .await;
    assert!(
        maybe_extra.is_none(),
        "unrelated subscription should stay idle"
    );
}

#[tokio::test]
async fn scheduled_mutation_over_http_drives_websocket_push() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let scheduler_handle = tokio::spawn(run_scheduler(service.clone(), shutdown_rx));
    let server = ServerFixture::start(build_router(service)).await;
    let api = HttpApiFixture::new(&server);

    assert!(api.create_tenant("demo").await.status().is_success());
    assert_eq!(
        api.set_table_schema(
            "demo",
            "tasks",
            json!({
                "table": "tasks",
                "fields": [
                    { "name": "priority", "field_type": "number", "required": false },
                    { "name": "title", "field_type": "string", "required": false }
                ],
                "indexes": [
                    { "name": "by_priority", "field": "priority" }
                ]
            }),
        )
        .await
        .status(),
        reqwest::StatusCode::NO_CONTENT
    );

    let mut socket = WebSocketFixture::connect(&api.ws_url("/ws"), "demo").await;
    socket
        .send_text(
            json!({
                "type": "subscribe",
                "request_id": "sched-http",
                "query": {
                    "table": "tasks",
                    "filters": [
                        { "field": "priority", "op": "lte", "value": 5 }
                    ],
                    "order": { "field": "priority", "direction": "asc" },
                    "limit": null
                }
            })
            .to_string(),
        )
        .await;

    let initial = socket.next_json().await;
    assert_eq!(initial["type"], json!("subscription_result"));
    assert_eq!(initial["request_id"], json!("sched-http"));
    assert_eq!(initial["data"], json!([]));

    let schedule = api
        .schedule_mutation(
            "demo",
            json!({
                "run_after_ms": 0,
                "mutation": {
                    "type": "insert",
                    "table": "tasks",
                    "fields": { "title": "Scheduled task", "priority": 1 }
                }
            }),
        )
        .await;
    assert_eq!(schedule.status(), reqwest::StatusCode::CREATED);

    let pushed = socket
        .next_json_with_timeout(Duration::from_secs(3))
        .await
        .expect("scheduled reactive push should arrive");
    assert_eq!(pushed["type"], json!("subscription_result"));
    assert!(pushed.get("request_id").is_none());
    assert_eq!(pushed["data"][0]["title"], json!("Scheduled task"));
    assert_eq!(pushed["data"][0]["priority"], json!(1));

    let _ = shutdown_tx.send(true);
    scheduler_handle.await.expect("scheduler should shut down");
}
