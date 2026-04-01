use super::*;

#[tokio::test]
async fn convex_named_unique_query_returns_document_null_or_error() {
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
            "name": "messages:uniqueByAuthor",
            "kind": "query",
            "plan": {
                "type": "unique",
                "query": {
                    "table": "messages",
                    "filters": [
                        {
                            "field": "author",
                            "op": "eq",
                            "value": { "$arg": "author" }
                        }
                    ],
                    "order": null,
                    "limit": 2
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

    let missing = api
        .convex_named_query(
            "demo",
            "messages:uniqueByAuthor",
            json!({ "author": "Ada" }),
        )
        .await;
    assert_eq!(missing.status(), StatusCode::OK);
    assert_eq!(
        missing
            .json::<serde_json::Value>()
            .await
            .expect("missing unique query should parse"),
        json!(null)
    );

    assert!(
        api.convex_named_mutation(
            "demo",
            "messages:send",
            json!({ "author": "Ada", "body": "Hello" }),
        )
        .await
        .status()
        .is_success()
    );

    let single = api
        .convex_named_query(
            "demo",
            "messages:uniqueByAuthor",
            json!({ "author": "Ada" }),
        )
        .await;
    assert_eq!(single.status(), StatusCode::OK);
    let single_body = single
        .json::<serde_json::Value>()
        .await
        .expect("single unique query should parse");
    assert_eq!(single_body["author"], json!("Ada"));
    assert_eq!(single_body["body"], json!("Hello"));

    assert!(
        api.convex_named_mutation(
            "demo",
            "messages:send",
            json!({ "author": "Ada", "body": "Again" }),
        )
        .await
        .status()
        .is_success()
    );

    let duplicate = api
        .convex_named_query(
            "demo",
            "messages:uniqueByAuthor",
            json!({ "author": "Ada" }),
        )
        .await;
    assert_eq!(duplicate.status(), StatusCode::BAD_REQUEST);
    let duplicate_body = duplicate
        .json::<serde_json::Value>()
        .await
        .expect("duplicate unique query error should parse");
    assert!(
        duplicate_body["error"]
            .as_str()
            .expect("error should be a string")
            .contains("multiple documents")
    );
}
