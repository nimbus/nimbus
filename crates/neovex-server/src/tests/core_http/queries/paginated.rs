use super::*;

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
