use super::*;

#[tokio::test]
async fn convex_named_first_query_returns_single_document_or_null() {
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
            "name": "messages:latestByAuthor",
            "kind": "query",
            "plan": {
                "type": "first",
                "query": {
                    "table": "messages",
                    "filters": [
                        {
                            "field": "author",
                            "op": "eq",
                            "value": { "$arg": "author" }
                        }
                    ],
                    "order": {
                        "field": "author",
                        "direction": "desc"
                    },
                    "limit": 1
                }
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

    let insert = api
        .convex_named_mutation(
            "demo",
            "messages:send",
            json!({ "author": "Ada", "body": "Hello" }),
        )
        .await;
    assert_eq!(insert.status(), StatusCode::OK);

    let response = api
        .convex_named_query(
            "demo",
            "messages:latestByAuthor",
            json!({ "author": "Ada" }),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("named first query response should parse");
    assert_eq!(body["author"], json!("Ada"));
    assert_eq!(body["body"], json!("Hello"));

    let missing = api
        .convex_named_query(
            "demo",
            "messages:latestByAuthor",
            json!({ "author": "Missing" }),
        )
        .await;
    assert_eq!(missing.status(), StatusCode::OK);
    assert_eq!(
        missing
            .json::<serde_json::Value>()
            .await
            .expect("missing named first query response should parse"),
        json!(null)
    );
}
