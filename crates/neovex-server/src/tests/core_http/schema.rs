use super::*;

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
