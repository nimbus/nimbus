use super::*;

#[tokio::test]
async fn convex_named_indexed_filter_unique_query_resolves_exact_match() {
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
            "name": "messages:exactByAuthorAndBody",
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
                        },
                        {
                            "field": "body",
                            "op": "eq",
                            "value": { "$arg": "body" }
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
    let schema = json!({
        "table": "messages",
        "fields": [
            { "name": "author", "field_type": "string", "required": true },
            { "name": "body", "field_type": "string", "required": true }
        ],
        "indexes": [
            { "name": "by_author", "field": "author" }
        ]
    });
    assert_eq!(
        api.set_table_schema("demo", "messages", schema)
            .await
            .status(),
        StatusCode::NO_CONTENT
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
    assert!(
        api.convex_named_mutation(
            "demo",
            "messages:send",
            json!({ "author": "Ada", "body": "Other" }),
        )
        .await
        .status()
        .is_success()
    );

    let exact = api
        .convex_named_query(
            "demo",
            "messages:exactByAuthorAndBody",
            json!({ "author": "Ada", "body": "Hello" }),
        )
        .await;
    assert_eq!(exact.status(), StatusCode::OK);
    let body = exact
        .json::<serde_json::Value>()
        .await
        .expect("indexed unique query should parse");
    assert_eq!(body["author"], json!("Ada"));
    assert_eq!(body["body"], json!("Hello"));

    let missing = api
        .convex_named_query(
            "demo",
            "messages:exactByAuthorAndBody",
            json!({ "author": "Ada", "body": "Missing" }),
        )
        .await;
    assert_eq!(missing.status(), StatusCode::OK);
    assert_eq!(
        missing
            .json::<serde_json::Value>()
            .await
            .expect("missing indexed unique query should parse"),
        json!(null)
    );
}
