use super::*;

#[tokio::test]
async fn query_endpoint_returns_filtered_results() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );
    assert_eq!(
        api.insert_document(
            "demo",
            "tasks",
            json!({ "title": "Alpha", "status": "todo" })
        )
        .await
        .status(),
        StatusCode::CREATED
    );
    assert_eq!(
        api.insert_document(
            "demo",
            "tasks",
            json!({ "title": "Beta", "status": "done" })
        )
        .await
        .status(),
        StatusCode::CREATED
    );

    let response = api
        .query_documents(
            "demo",
            json!({
                "table": "tasks",
                "filters": [{
                    "field": "status",
                    "op": "eq",
                    "value": "todo"
                }],
                "order": {
                    "field": "title",
                    "direction": "asc"
                },
                "limit": null
            }),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);

    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("query response should parse");
    assert_eq!(
        body["data"],
        json!([{
            "_creationTime": body["data"][0]["_creationTime"].clone(),
            "_id": body["data"][0]["_id"].clone(),
            "status": "todo",
            "title": "Alpha"
        }])
    );
}

#[tokio::test]
async fn paginated_query_endpoint_returns_pages() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    for title in ["alpha", "bravo", "charlie", "delta", "echo"] {
        assert_eq!(
            api.insert_document("demo", "tasks", json!({ "title": title }))
                .await
                .status(),
            StatusCode::CREATED
        );
    }

    let first_page = api
        .query_documents_paginated(
            "demo",
            json!({
                "query": {
                    "table": "tasks",
                    "filters": [],
                    "order": { "field": "title", "direction": "asc" },
                    "limit": null
                },
                "page_size": 2,
                "after": null
            }),
        )
        .await;
    assert_eq!(first_page.status(), StatusCode::OK);
    let first_body = first_page
        .json::<serde_json::Value>()
        .await
        .expect("first page should parse");
    assert_eq!(
        first_body["data"]
            .as_array()
            .expect("data should be an array")
            .len(),
        2
    );
    assert_eq!(first_body["data"][0]["title"], json!("alpha"));
    assert_eq!(first_body["data"][1]["title"], json!("bravo"));
    assert_eq!(first_body["has_more"], json!(true));
    let cursor = first_body["next_cursor"]
        .as_str()
        .expect("next_cursor should be a string")
        .to_string();

    let second_page = api
        .query_documents_paginated(
            "demo",
            json!({
                "query": {
                    "table": "tasks",
                    "filters": [],
                    "order": { "field": "title", "direction": "asc" },
                    "limit": null
                },
                "page_size": 2,
                "after": cursor
            }),
        )
        .await;
    assert_eq!(second_page.status(), StatusCode::OK);
    let second_body = second_page
        .json::<serde_json::Value>()
        .await
        .expect("second page should parse");
    assert_eq!(second_body["data"][0]["title"], json!("charlie"));
    assert_eq!(second_body["data"][1]["title"], json!("delta"));
    assert_eq!(second_body["has_more"], json!(true));
}

#[tokio::test]
async fn paginated_query_rejects_cursor_for_different_query_shape() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    for title in ["alpha", "bravo", "charlie"] {
        assert_eq!(
            api.insert_document("demo", "tasks", json!({ "title": title }))
                .await
                .status(),
            StatusCode::CREATED
        );
    }

    let first_page = api
        .query_documents_paginated(
            "demo",
            json!({
                "query": {
                    "table": "tasks",
                    "filters": [],
                    "order": { "field": "title", "direction": "asc" },
                    "limit": null
                },
                "page_size": 2,
                "after": null
            }),
        )
        .await;
    assert_eq!(first_page.status(), StatusCode::OK);
    let first_body = first_page
        .json::<serde_json::Value>()
        .await
        .expect("first page should parse");
    let cursor = first_body["next_cursor"]
        .as_str()
        .expect("next_cursor should be present")
        .to_string();

    let invalid_page = api
        .query_documents_paginated(
            "demo",
            json!({
                "query": {
                    "table": "tasks",
                    "filters": [],
                    "order": { "field": "title", "direction": "desc" },
                    "limit": null
                },
                "page_size": 2,
                "after": cursor
            }),
        )
        .await;
    assert_eq!(invalid_page.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn query_endpoint_returns_range_filtered_results_with_indexed_schema() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let schema = json!({
        "table": "tasks",
        "fields": [
            { "name": "rank", "field_type": "number", "required": false }
        ],
        "indexes": [
            { "name": "by_rank", "field": "rank" }
        ]
    });
    assert_eq!(
        api.set_table_schema("demo", "tasks", schema).await.status(),
        StatusCode::NO_CONTENT
    );

    for rank in 0..10 {
        assert_eq!(
            api.insert_document("demo", "tasks", json!({ "rank": rank }))
                .await
                .status(),
            StatusCode::CREATED
        );
    }

    let response = api
        .query_documents(
            "demo",
            json!({
                "table": "tasks",
                "filters": [
                    { "field": "rank", "op": "gte", "value": 3 },
                    { "field": "rank", "op": "lt", "value": 6 }
                ],
                "order": { "field": "rank", "direction": "asc" },
                "limit": null
            }),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("query response should parse");
    let data = body["data"]
        .as_array()
        .expect("response data should be an array");
    assert_eq!(data.len(), 3);
    assert_eq!(data[0]["rank"], json!(3));
    assert_eq!(data[1]["rank"], json!(4));
    assert_eq!(data[2]["rank"], json!(5));
}
