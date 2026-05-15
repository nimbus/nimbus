use super::*;

#[tokio::test]
async fn rejects_invalid_tenant_name_and_returns_not_found_for_unknown_document_key() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    let invalid_tenant = api.create_tenant("../demo").await;
    assert_eq!(invalid_tenant.status(), StatusCode::BAD_REQUEST);

    let create_response = api.create_tenant("demo").await;
    assert_eq!(create_response.status(), StatusCode::CREATED);

    let unknown_document_id = api.get_document("demo", "tasks", "not-a-ulid").await;
    assert_eq!(unknown_document_id.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn duplicate_tenant_creation_returns_conflict() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    let first = api.create_tenant("demo").await;
    let duplicate = api.create_tenant("demo").await;

    assert_eq!(first.status(), StatusCode::CREATED);
    assert_eq!(duplicate.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn list_tenants_returns_all_known_tenants() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("bravo").await.status(),
        StatusCode::CREATED
    );
    assert_eq!(
        api.create_tenant("alpha").await.status(),
        StatusCode::CREATED
    );

    let response = api.list_tenants().await;
    assert_eq!(response.status(), StatusCode::OK);

    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("tenant list response should parse");
    assert_eq!(body["tenants"], json!(["alpha", "bravo"]));
}

#[tokio::test]
async fn local_admin_tenant_api_rejects_and_hides_reserved_system_tenants() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    crate::system_tenant::ensure_system_tenant_async(&service)
        .await
        .expect("system tenant should initialize");
    let server = ServerFixture::start(build_router(service)).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("_demo").await.status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        api.delete_tenant("_nimbus").await.status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        api.insert_document("_nimbus", "machines", json!({ "name": "demo" }))
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );

    let response = api.list_tenants().await;
    assert_eq!(response.status(), StatusCode::OK);

    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("tenant list response should parse");
    assert_eq!(body["tenants"], json!([]));
}

#[tokio::test]
async fn delete_tenant_returns_no_content_and_removes_it_from_listing() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );
    let delete = api.delete_tenant("demo").await;
    assert_eq!(delete.status(), StatusCode::NO_CONTENT);

    let list = api.list_tenants().await;
    let body = list
        .json::<serde_json::Value>()
        .await
        .expect("tenant list response should parse");
    assert_eq!(body["tenants"], json!([]));
}

#[tokio::test]
async fn operations_on_nonexistent_tenant_return_not_found() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);
    let document_id = nimbus_core::DocumentId::new().to_string();

    assert_eq!(
        api.insert_document("missing", "tasks", json!({ "title": "Hello" }))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        api.list_documents("missing", "tasks").await.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        api.get_document("missing", "tasks", &document_id)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        api.update_document(
            "missing",
            "tasks",
            &document_id,
            json!({ "title": "Updated" })
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        api.delete_document("missing", "tasks", &document_id)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        api.query_documents(
            "missing",
            json!({
                "table": "tasks",
                "filters": [],
                "order": null,
                "limit": null
            }),
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        api.journal("missing", None, None).await.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        api.delete_tenant("missing").await.status(),
        StatusCode::NOT_FOUND
    );
}
