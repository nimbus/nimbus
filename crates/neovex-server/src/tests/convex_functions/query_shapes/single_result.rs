use super::super::super::*;

#[tokio::test]
async fn convex_named_query_can_return_single_document_or_null() {
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
            "name": "messages:byId",
            "kind": "query",
            "plan": {
                "type": "get",
                "table": "messages",
                "id": { "$arg": "id" }
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

    let inserted = api
        .convex_named_mutation(
            "demo",
            "messages:send",
            json!({ "author": "Ada", "body": "Hello" }),
        )
        .await;
    let inserted_id = inserted
        .json::<serde_json::Value>()
        .await
        .expect("insert response should parse")
        .as_str()
        .expect("insert should return id")
        .to_string();

    let found = api
        .convex_named_query("demo", "messages:byId", json!({ "id": inserted_id }))
        .await;
    assert_eq!(found.status(), StatusCode::OK);
    let found_body = found
        .json::<serde_json::Value>()
        .await
        .expect("get query response should parse");
    assert_eq!(found_body["author"], json!("Ada"));
    assert_eq!(found_body["body"], json!("Hello"));

    let missing = api
        .convex_named_query(
            "demo",
            "messages:byId",
            json!({ "id": "01ARZ3NDEKTSV4RRFFQ69G5FAV" }),
        )
        .await;
    assert_eq!(missing.status(), StatusCode::OK);
    assert_eq!(
        missing
            .json::<serde_json::Value>()
            .await
            .expect("missing get query should parse"),
        serde_json::Value::Null
    );
}

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
