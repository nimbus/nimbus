use super::*;

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
