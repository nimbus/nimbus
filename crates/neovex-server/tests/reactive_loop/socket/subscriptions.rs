use super::*;

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
async fn websocket_disconnect_drops_subscription_without_explicit_unsubscribe() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let server = ServerFixture::start(build_router(service.clone())).await;
    let api = HttpApiFixture::new(&server);

    assert!(api.create_tenant("demo").await.status().is_success());
    let tenant_id = neovex_core::TenantId::new("demo").expect("tenant id should be valid");

    let mut socket = WebSocketFixture::connect(&api.ws_url("/ws"), "demo").await;
    socket.subscribe_all("disconnect", "tasks").await;

    let initial = socket.next_json().await;
    assert_eq!(initial["type"], json!("subscription_result"));
    assert_eq!(
        service
            .active_subscription_count(&tenant_id)
            .expect("subscription count should load"),
        1
    );

    drop(socket);

    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if service
                .active_subscription_count(&tenant_id)
                .expect("subscription count should load")
                == 0
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("connection teardown should release subscription handles");
}
