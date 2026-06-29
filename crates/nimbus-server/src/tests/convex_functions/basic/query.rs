use super::*;

#[tokio::test]
async fn convex_query_returns_documents_as_plain_json() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server = ServerFixture::start(router_for_convex_team(
        fixture.engine(),
        ConvexRegistry::empty(),
    ))
    .await;
    let api = HttpApiFixture::with_convex_bearer(&server, convex_team_bearer());

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );
    assert_eq!(
        api.insert_document("demo", "tasks", json!({ "title": "Hello" }))
            .await
            .status(),
        StatusCode::CREATED
    );

    // #41 non-vacuous: an anonymous (no-bearer) selection of this silo is refused
    // by the all-fail-closed gate; only the team-bound bearer below is admitted.
    assert_convex_anonymous_query_refused(&server, "demo").await;

    let response = api
        .convex_query(
            "demo",
            json!({
                "table": "tasks",
                "filters": [],
                "order": null,
                "limit": null
            }),
        )
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("convex query response should parse");
    assert_eq!(body[0]["title"], json!("Hello"));
    assert!(body[0]["_id"].is_string());
    assert!(body[0]["_creationTime"].is_u64());
    assert!(body[0]["_updateTime"].is_u64());
}
