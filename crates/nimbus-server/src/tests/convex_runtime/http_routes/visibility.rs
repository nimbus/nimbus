use super::*;

#[tokio::test]
async fn convex_public_endpoints_reject_internal_functions() {
    let registry = convex_registry(json!([
        {
            "name": "tasks:internalList",
            "kind": "query",
            "visibility": "internal",
            "plan": {
                "table": "tasks",
                "filters": [],
                "order": null,
                "limit": null
            }
        }
    ]));
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

    let response = api
        .convex_named_query("demo", "tasks:internalList", json!({}))
        .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(
        response
            .json::<serde_json::Value>()
            .await
            .expect("internal convex error should parse")["error"]["message"]
            .as_str()
            .expect("internal convex error should be a string")
            .contains("not public")
    );
}
