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
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(fixture.service(), registry)).await;
    let api = HttpApiFixture::new(&server);

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
