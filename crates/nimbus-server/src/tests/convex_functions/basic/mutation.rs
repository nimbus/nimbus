use super::*;

#[tokio::test]
async fn convex_mutation_dispatches_existing_document_operations() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let server = ServerFixture::start(build_router_with_convex(
        service.clone(),
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

    let projected_tables = wait_for_value(
        "convex mutation should project committed table state into _nimbus.tables",
        Duration::from_secs(5),
        Duration::from_millis(25),
        || {
            let service = service.clone();
            async move {
                service
                    .list_documents_async(
                        crate::system_tenant::system_tenant_id().expect("system id should parse"),
                        TableName::new("tables").expect("system table name should parse"),
                    )
                    .await
            }
        },
        |result| {
            result.as_ref().is_ok_and(|documents| {
                documents.iter().any(|document| {
                    document.fields.get("tenantId") == Some(&json!("demo"))
                        && document.fields.get("name") == Some(&json!("tasks"))
                        && document.fields.get("rowCount") == Some(&json!(1))
                })
            })
        },
    )
    .await
    .expect("_nimbus.tables should be readable after projection");
    assert!(
        projected_tables.iter().any(|document| {
            document.fields.get("tenantId") == Some(&json!("demo"))
                && document.fields.get("name") == Some(&json!("tasks"))
                && document.fields.get("rowCount") == Some(&json!(1))
        }),
        "projected table rows should include the Convex-written tasks table: {projected_tables:?}"
    );
}
