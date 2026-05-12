use super::*;

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
    assert!(body[0]["_updateTime"].is_u64());
}
