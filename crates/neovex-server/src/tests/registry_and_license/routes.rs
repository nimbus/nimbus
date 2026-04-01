use super::*;

#[tokio::test]
async fn health_route_returns_ok() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
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
async fn neovex_demo_html_is_served_without_convex_support() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;

    let response = server
        .client()
        .get(server.http_url("/demos/neovex/html/"))
        .send()
        .await
        .expect("demo request should succeed");

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.text().await.expect("demo body should load");
    assert!(body.contains("Neovex HTML Demo"));
    assert!(body.contains("Live tasks over HTTP writes and WebSocket subscriptions."));
}
