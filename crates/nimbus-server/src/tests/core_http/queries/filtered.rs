use super::*;

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
            "_updateTime": body["data"][0]["_updateTime"].clone(),
            "_id": body["data"][0]["_id"].clone(),
            "status": "todo",
            "title": "Alpha"
        }])
    );
}
