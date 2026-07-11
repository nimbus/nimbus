use super::*;

#[tokio::test]
async fn health_route_returns_ok() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server = ServerFixture::start(router_for_engine(fixture.engine())).await;
    let api = HttpApiFixture::new(&server);
    let response = api.health().await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .json::<serde_json::Value>()
            .await
            .expect("health json should parse")["ok"],
        serde_json::json!(true)
    );
}

#[tokio::test]
async fn nimbus_demo_html_is_served_without_convex_support() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server = ServerFixture::start(router_for_engine(fixture.engine())).await;

    let response = server
        .client()
        .get(server.http_url("/examples/nimbus/html/"))
        .send()
        .await
        .expect("demo request should succeed");

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.text().await.expect("demo body should load");
    assert!(body.contains("Nimbus HTML Demo"));
    assert!(body.contains("Live tasks over HTTP writes and WebSocket subscriptions."));
}
