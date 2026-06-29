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
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server = ServerFixture::start(router_for_convex_team(fixture.engine(), registry)).await;
    let api = HttpApiFixture::with_convex_bearer(&server, convex_team_bearer());
    // #41 non-vacuous: an anonymous (no-bearer) selection of this silo is refused
    // by the all-fail-closed team gate; only the team-bound bearer is admitted.
    assert_convex_anonymous_query_refused(&server, "demo").await;

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
