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
async fn rejects_invalid_document_id_and_tenant_name() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    let invalid_tenant = api.create_tenant("../demo").await;
    assert_eq!(invalid_tenant.status(), StatusCode::BAD_REQUEST);

    let create_response = api.create_tenant("demo").await;
    assert_eq!(create_response.status(), StatusCode::CREATED);

    let invalid_document_id = api.get_document("demo", "tasks", "not-a-ulid").await;
    assert_eq!(invalid_document_id.status(), StatusCode::BAD_REQUEST);
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
    let document_id = neovex_core::DocumentId::new().to_string();

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
        api.commit_log("missing", None).await.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        api.delete_tenant("missing").await.status(),
        StatusCode::NOT_FOUND
    );
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
async fn schema_crud_via_http() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let schema = json!({
        "table": "users",
        "fields": [
            { "name": "name", "field_type": "string", "required": true },
            { "name": "age", "field_type": "number", "required": false }
        ],
        "indexes": []
    });

    assert_eq!(
        api.set_table_schema("demo", "users", schema.clone())
            .await
            .status(),
        StatusCode::NO_CONTENT
    );

    let full_schema = api.get_schema("demo").await;
    assert_eq!(full_schema.status(), StatusCode::OK);
    let full_body = full_schema
        .json::<serde_json::Value>()
        .await
        .expect("schema response should parse");
    assert_eq!(full_body["tables"]["users"], schema);

    let table_schema = api.get_table_schema("demo", "users").await;
    assert_eq!(table_schema.status(), StatusCode::OK);
    let table_body = table_schema
        .json::<serde_json::Value>()
        .await
        .expect("table schema response should parse");
    assert_eq!(table_body, schema);

    let valid_insert = api
        .insert_document("demo", "users", json!({ "name": "Alice", "age": 30 }))
        .await;
    assert_eq!(valid_insert.status(), StatusCode::CREATED);

    let invalid_insert = api
        .insert_document("demo", "users", json!({ "age": "old" }))
        .await;
    assert_eq!(invalid_insert.status(), StatusCode::UNPROCESSABLE_ENTITY);

    assert_eq!(
        api.delete_table_schema("demo", "users").await.status(),
        StatusCode::NO_CONTENT
    );

    let permissive_insert = api
        .insert_document("demo", "users", json!({ "anything": { "goes": true } }))
        .await;
    assert_eq!(permissive_insert.status(), StatusCode::CREATED);
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
