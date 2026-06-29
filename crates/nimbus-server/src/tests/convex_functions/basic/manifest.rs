use super::*;

#[tokio::test]
async fn convex_named_query_and_mutation_resolve_from_manifest() {
    let registry = convex_registry(json!([
        {
            "name": "messages:send",
            "kind": "mutation",
            "plan": {
                "type": "insert",
                "table": "messages",
                "fields": {
                    "author": { "$arg": "author" },
                    "body": { "$arg": "body" }
                }
            }
        },
        {
            "name": "messages:byAuthor",
            "kind": "query",
            "plan": {
                "table": "messages",
                "filters": [
                    {
                        "field": "author",
                        "op": "eq",
                        "value": { "$arg": "author" }
                    }
                ],
                "order": null,
                "limit": null
            }
        }
    ]));
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server =
        ServerFixture::start(router_for_convex_team(fixture.engine(), registry.clone())).await;
    let api = HttpApiFixture::with_convex_bearer(&server, convex_team_bearer());
    // #41 non-vacuous: an anonymous (no-bearer) selection of this silo is refused
    // by the all-fail-closed team gate; only the team-bound bearer is admitted.
    assert_convex_anonymous_query_refused(&server, "demo").await;

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    assert_eq!(
        api.convex_named_mutation(
            "demo",
            "messages:send",
            json!({ "author": "Ada", "body": "Hello" }),
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        api.convex_named_mutation(
            "demo",
            "messages:send",
            json!({ "author": "Grace", "body": "World" }),
        )
        .await
        .status(),
        StatusCode::OK
    );

    let response = api
        .convex_named_query("demo", "messages:byAuthor", json!({ "author": "Ada" }))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("named convex query response should parse");
    assert_eq!(
        body,
        json!([{
            "_creationTime": body[0]["_creationTime"].clone(),
            "_updateTime": body[0]["_updateTime"].clone(),
            "_id": body[0]["_id"].clone(),
            "author": "Ada",
            "body": "Hello"
        }])
    );
}
