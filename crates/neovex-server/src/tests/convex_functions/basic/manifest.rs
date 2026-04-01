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
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(
        fixture.service(),
        registry.clone(),
    ))
    .await;
    let api = HttpApiFixture::new(&server);

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
            "_id": body[0]["_id"].clone(),
            "author": "Ada",
            "body": "Hello"
        }])
    );
}
