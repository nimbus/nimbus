use super::*;

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
            { "name": "by_rank", "fields": ["rank"] }
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
