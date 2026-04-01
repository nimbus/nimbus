use super::super::*;

#[tokio::test]
async fn convex_query_returns_documents_as_plain_json() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(
        fixture.service(),
        ConvexRegistry::empty(),
    ))
    .await;
    let api = HttpApiFixture::new(&server);

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
}

#[tokio::test]
async fn convex_mutation_dispatches_existing_document_operations() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(
        fixture.service(),
        ConvexRegistry::empty(),
    ))
    .await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let insert = api
        .convex_mutation(
            "demo",
            json!({
                "type": "insert",
                "table": "tasks",
                "fields": { "title": "Inserted from convex" }
            }),
        )
        .await;
    assert_eq!(insert.status(), StatusCode::OK);
    let document_id = insert
        .json::<serde_json::Value>()
        .await
        .expect("convex mutation response should parse")
        .as_str()
        .expect("convex mutation insert should return a document id")
        .to_string();

    let list = api.list_documents("demo", "tasks").await;
    let body = list
        .json::<serde_json::Value>()
        .await
        .expect("document list should parse");
    assert_eq!(body["data"][0]["title"], json!("Inserted from convex"));

    let update = api
        .convex_mutation(
            "demo",
            json!({
                "type": "update",
                "table": "tasks",
                "id": document_id,
                "patch": { "title": "Updated from convex" }
            }),
        )
        .await;
    assert_eq!(update.status(), StatusCode::OK);

    let body = api
        .list_documents("demo", "tasks")
        .await
        .json::<serde_json::Value>()
        .await
        .expect("updated list should parse");
    assert_eq!(body["data"][0]["title"], json!("Updated from convex"));
}

#[tokio::test]
async fn convex_action_can_execute_query_and_paginated_query_shapes() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(
        fixture.service(),
        ConvexRegistry::empty(),
    ))
    .await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );
    assert_eq!(
        api.insert_document("demo", "tasks", json!({ "title": "Alpha" }))
            .await
            .status(),
        StatusCode::CREATED
    );

    let query = api
        .convex_action(
            "demo",
            json!({
                "type": "query",
                "query": {
                    "table": "tasks",
                    "filters": [],
                    "order": null,
                    "limit": null
                }
            }),
        )
        .await;
    assert_eq!(query.status(), StatusCode::OK);
    assert_eq!(
        query
            .json::<serde_json::Value>()
            .await
            .expect("convex action query response should parse")[0]["title"],
        json!("Alpha")
    );

    let paginated = api
        .convex_action(
            "demo",
            json!({
                "type": "paginated_query",
                "query": {
                    "query": {
                        "table": "tasks",
                        "filters": [],
                        "order": null,
                        "limit": null
                    },
                    "page_size": 10,
                    "after": null
                }
            }),
        )
        .await;
    assert_eq!(paginated.status(), StatusCode::OK);
    let page = paginated
        .json::<serde_json::Value>()
        .await
        .expect("convex action paginated response should parse");
    assert_eq!(page["data"][0]["title"], json!("Alpha"));
    assert_eq!(page["has_more"], json!(false));
}

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
