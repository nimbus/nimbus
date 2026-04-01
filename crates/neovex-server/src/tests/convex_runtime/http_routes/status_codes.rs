use super::*;

#[tokio::test]
async fn convex_http_routes_return_404_and_405_when_appropriate() {
    let registry = convex_registry_with_routes(
        json!([]),
        json!([
            {
                "method": "GET",
                "path": "/healthz",
                "name": "http:inline:0",
                "plan": {
                    "response": {
                        "kind": "text",
                        "body": "ok"
                    }
                }
            }
        ]),
    );
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(fixture.service(), registry)).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    assert_eq!(
        api.convex_http("demo", reqwest::Method::GET, "/missing")
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        api.convex_http("demo", reqwest::Method::POST, "/healthz")
            .await
            .status(),
        StatusCode::METHOD_NOT_ALLOWED
    );
}
