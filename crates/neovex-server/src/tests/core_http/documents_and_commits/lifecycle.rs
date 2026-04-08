use super::*;
use neovex_testing::BlockingFaultInjector;

#[tokio::test]
async fn create_tenant_and_run_document_lifecycle() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    let create_response = api.create_tenant("demo").await;
    assert_eq!(create_response.status(), StatusCode::CREATED);

    let insert_response = api
        .insert_document("demo", "tasks", serde_json::json!({ "title": "Hello" }))
        .await;
    assert_eq!(insert_response.status(), StatusCode::CREATED);
    let document_id = insert_response
        .json::<serde_json::Value>()
        .await
        .expect("insert response should parse")["id"]
        .as_str()
        .expect("id should be a string")
        .to_string();

    let update_response = api
        .update_document(
            "demo",
            "tasks",
            &document_id,
            serde_json::json!({ "title": "Updated" }),
        )
        .await;
    assert_eq!(update_response.status(), StatusCode::OK);
    assert_eq!(
        update_response
            .json::<serde_json::Value>()
            .await
            .expect("update response should parse")["id"],
        serde_json::json!(document_id)
    );

    let list_response = api.list_documents("demo", "tasks").await;
    assert_eq!(list_response.status(), StatusCode::OK);
    let list_body = list_response
        .json::<serde_json::Value>()
        .await
        .expect("list response should parse");
    assert_eq!(list_body["data"][0]["title"], serde_json::json!("Updated"));

    let get_response = api.get_document("demo", "tasks", &document_id).await;
    assert_eq!(get_response.status(), StatusCode::OK);
    let get_body = get_response
        .json::<serde_json::Value>()
        .await
        .expect("get response should parse");
    assert_eq!(get_body["document"]["title"], serde_json::json!("Updated"));

    let delete_response = api.delete_document("demo", "tasks", &document_id).await;
    assert_eq!(delete_response.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn get_nonexistent_document_returns_not_found() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let response = api
        .get_document("demo", "tasks", &neovex_core::DocumentId::new().to_string())
        .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn dropped_http_insert_after_commit_still_persists_the_document() {
    let faults = BlockingFaultInjector::new(FaultPoint::StorageCommitAfterVisibilityBeforeReturn);
    let faults_for_builder = faults.clone();
    let fixture = ServiceFixture::new(move |path| {
        Service::new_with_simulation(
            path,
            Arc::new(ManualClock::new(neovex_core::Timestamp(30_000))),
            faults_for_builder,
        )
    });
    let service = fixture.service();
    let server = ServerFixture::start(build_router(service.clone())).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let request = open_json_post_stream(
        &server,
        "/api/tenants/demo/documents",
        &serde_json::json!({
            "table": "tasks",
            "fields": { "title": "after-disconnect" }
        }),
    )
    .await;
    timeout(Duration::from_secs(1), faults.wait_until_entered())
        .await
        .expect("write should block after the durable commit point");
    drop(request);
    faults.release();

    let started_at = tokio::time::Instant::now();
    loop {
        let documents = service
            .list_documents(
                &TenantId::new("demo").expect("tenant id should build"),
                &TableName::new("tasks").expect("table should build"),
            )
            .expect("query should succeed");
        if documents.len() == 1 {
            assert_eq!(
                documents[0].fields.get("title"),
                Some(&serde_json::json!("after-disconnect"))
            );
            break;
        }
        assert!(
            started_at.elapsed() < Duration::from_secs(2),
            "timed out waiting for the committed write to become observable"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}
