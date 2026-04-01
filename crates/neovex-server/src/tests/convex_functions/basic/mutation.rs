use super::*;

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
