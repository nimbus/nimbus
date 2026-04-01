use super::*;

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
async fn commit_log_route_returns_sequenced_commits() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    let create_response = api.create_tenant("demo").await;
    assert_eq!(create_response.status(), StatusCode::CREATED);

    let insert_response = api
        .insert_document("demo", "tasks", serde_json::json!({ "title": "Hello" }))
        .await;
    let document_id = insert_response
        .json::<serde_json::Value>()
        .await
        .expect("insert response should parse")["id"]
        .as_str()
        .expect("id should be a string")
        .to_string();

    api.update_document(
        "demo",
        "tasks",
        &document_id,
        serde_json::json!({ "title": "Updated" }),
    )
    .await;

    let commit_log_response = api.commit_log("demo", None).await;
    assert_eq!(commit_log_response.status(), StatusCode::OK);
    let commit_log = commit_log_response
        .json::<serde_json::Value>()
        .await
        .expect("commit log response should parse");
    assert_eq!(commit_log["latest_sequence"], serde_json::json!(2));
    let commits = commit_log["commits"]
        .as_array()
        .expect("commits should be an array");
    assert_eq!(commits.len(), 2);
    assert_eq!(commits[0]["sequence"], serde_json::json!(1));
    assert_eq!(
        commits[0]["writes"][0]["op_type"],
        serde_json::json!("insert")
    );
    assert_eq!(commits[1]["sequence"], serde_json::json!(2));
    assert_eq!(
        commits[1]["writes"][0]["op_type"],
        serde_json::json!("update")
    );

    let filtered_response = api.commit_log("demo", Some(1)).await;
    assert_eq!(filtered_response.status(), StatusCode::OK);
    let filtered = filtered_response
        .json::<serde_json::Value>()
        .await
        .expect("filtered response should parse");
    let commits = filtered["commits"]
        .as_array()
        .expect("commits should be an array");
    assert_eq!(commits.len(), 1);
    assert_eq!(commits[0]["sequence"], serde_json::json!(2));
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
