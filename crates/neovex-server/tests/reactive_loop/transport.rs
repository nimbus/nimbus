use super::*;

#[tokio::test]
async fn subscribe_insert_and_receive_reactive_push() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    let create_response = api.create_tenant("demo").await;
    assert!(create_response.status().is_success());

    let mut socket = WebSocketFixture::connect(&api.ws_url("/ws"), "demo").await;
    socket.subscribe_all("1", "tasks").await;

    let initial = socket.next_json().await;
    assert_eq!(initial["type"], json!("subscription_result"));
    assert_eq!(initial["request_id"], json!("1"));
    assert_eq!(initial["data"], json!([]));

    let insert_response = api
        .insert_document("demo", "tasks", json!({ "title": "Hello" }))
        .await;
    assert!(insert_response.status().is_success());

    let pushed = socket.next_json().await;
    assert_eq!(pushed["type"], json!("subscription_result"));
    assert!(pushed.get("request_id").is_none());
    let data = pushed["data"].as_array().expect("data should be an array");
    assert_eq!(data.len(), 1);
    assert!(data[0]["_id"].is_string());
    assert!(data[0]["_creationTime"].is_u64());
    assert_eq!(data[0]["title"], json!("Hello"));
}

#[tokio::test]
async fn subscribe_update_and_delete_and_receive_reactive_pushes() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    let create_response = api.create_tenant("demo").await;
    assert!(create_response.status().is_success());

    let insert_response = api
        .insert_document("demo", "tasks", json!({ "title": "Before" }))
        .await;
    assert!(insert_response.status().is_success());
    let document_id = insert_response
        .json::<serde_json::Value>()
        .await
        .expect("insert response should parse")["id"]
        .as_str()
        .expect("id should be a string")
        .to_string();

    let mut socket = WebSocketFixture::connect(&api.ws_url("/ws"), "demo").await;
    socket.subscribe_all("2", "tasks").await;

    let initial = socket.next_json().await;
    assert_eq!(initial["type"], json!("subscription_result"));
    assert_eq!(initial["request_id"], json!("2"));
    assert_eq!(initial["data"][0]["title"], json!("Before"));

    let update_response = api
        .update_document("demo", "tasks", &document_id, json!({ "title": "After" }))
        .await;
    assert!(update_response.status().is_success());

    let updated = socket.next_json().await;
    assert_eq!(updated["type"], json!("subscription_result"));
    assert!(updated.get("request_id").is_none());
    assert_eq!(updated["data"][0]["title"], json!("After"));

    let delete_response = api.delete_document("demo", "tasks", &document_id).await;
    assert_eq!(delete_response.status(), reqwest::StatusCode::NO_CONTENT);

    let deleted = socket.next_json().await;
    assert_eq!(deleted["type"], json!("subscription_result"));
    assert!(deleted.get("request_id").is_none());
    assert_eq!(deleted["data"], json!([]));
}

#[tokio::test]
async fn delete_tenant_sends_subscription_error() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    let create_response = api.create_tenant("demo").await;
    assert!(create_response.status().is_success());

    let mut socket = WebSocketFixture::connect(&api.ws_url("/ws"), "demo").await;
    socket.subscribe_all("3", "tasks").await;

    let initial = socket.next_json().await;
    assert_eq!(initial["type"], json!("subscription_result"));
    assert_eq!(initial["request_id"], json!("3"));

    let delete_response = api.delete_tenant("demo").await;
    assert_eq!(delete_response.status(), reqwest::StatusCode::NO_CONTENT);

    let teardown = socket.next_json().await;
    assert_eq!(teardown["type"], json!("error"));
    assert!(teardown.get("request_id").is_none());
    assert!(
        teardown["message"]
            .as_str()
            .expect("message should be a string")
            .contains("tenant deleted: demo")
    );
}

#[tokio::test]
async fn websocket_missing_tenant_header_returns_bad_request() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    match WebSocketFixture::connect_with_tenant(&api.ws_url("/ws"), None).await {
        Err(WebSocketError::Http(response)) => {
            assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
        }
        Ok(_) => panic!("connection should fail"),
        Err(other) => panic!("unexpected websocket error: {other:?}"),
    }
}

#[tokio::test]
async fn websocket_nonexistent_tenant_returns_not_found() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    match WebSocketFixture::connect_with_tenant(&api.ws_url("/ws"), Some("missing")).await {
        Err(WebSocketError::Http(response)) => {
            assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);
        }
        Ok(_) => panic!("connection should fail"),
        Err(other) => panic!("unexpected websocket error: {other:?}"),
    }
}

#[tokio::test]
async fn browser_style_websocket_query_parameter_supports_subscriptions() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    assert!(api.create_tenant("demo").await.status().is_success());

    let mut socket = WebSocketFixture::connect_for_browser(&api.ws_url("/ws"), "demo")
        .await
        .expect("browser websocket should connect");
    socket.subscribe_all("browser", "tasks").await;

    let initial = socket.next_json().await;
    assert_eq!(initial["type"], json!("subscription_result"));
    assert_eq!(initial["request_id"], json!("browser"));
    assert_eq!(initial["data"], json!([]));
}

#[tokio::test]
async fn convex_websocket_path_supports_subscriptions() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(
        fixture.service(),
        ConvexRegistry::empty(),
    ))
    .await;
    let api = HttpApiFixture::new(&server);

    assert!(api.create_tenant("demo").await.status().is_success());

    let mut socket = WebSocketFixture::connect_raw(&api.ws_url("/convex/demo/ws"))
        .await
        .expect("convex websocket should connect");
    socket.subscribe_all("convex", "tasks").await;

    let initial = socket.next_json().await;
    assert_eq!(initial["type"], json!("subscription_result"));
    assert_eq!(initial["request_id"], json!("convex"));
    assert_eq!(initial["data"], json!([]));

    assert!(
        api.convex_mutation(
            "demo",
            json!({
                "type": "insert",
                "table": "tasks",
                "fields": { "title": "Convex insert" }
            }),
        )
        .await
        .status()
        .is_success()
    );

    let pushed = socket.next_json().await;
    assert_eq!(pushed["type"], json!("subscription_result"));
    assert_eq!(pushed["data"][0]["title"], json!("Convex insert"));
}
