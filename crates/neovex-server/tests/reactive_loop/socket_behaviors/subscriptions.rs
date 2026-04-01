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
