use super::*;

#[tokio::test]
async fn browser_style_websocket_query_parameter_supports_subscriptions() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server = ServerFixture::start(router_for_engine(fixture.engine())).await;
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
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server =
        // `convex_registry` (not `ConvexRegistry::empty()`) so the registry carries
        // the shared #41 customJwt provider that verifies the team bearer.
        ServerFixture::start(router_for_convex(fixture.engine(), convex_registry(json!([])))).await;
    let api = HttpApiFixture::with_convex_bearer(&server, convex_team_bearer());

    assert!(api.create_tenant("demo").await.status().is_success());

    // #41 non-vacuous: an anonymous Convex WS upgrade for this silo is refused.
    assert_convex_anonymous_ws_refused(&server, "demo").await;
    let mut socket = WebSocketFixture::connect_raw_with_bearer(
        &api.ws_url("/convex/demo/ws"),
        &convex_team_bearer(),
    )
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
