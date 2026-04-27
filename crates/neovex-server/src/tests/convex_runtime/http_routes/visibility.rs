use super::*;

#[tokio::test]
async fn convex_public_endpoints_reject_internal_functions() {
    let registry = convex_registry(json!([
        {
            "name": "tasks:internalList",
            "kind": "query",
            "visibility": "internal",
            "plan": {
                "table": "tasks",
                "filters": [],
                "order": null,
                "limit": null
            }
        }
    ]));
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(fixture.service(), registry)).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let response = api
        .convex_named_query("demo", "tasks:internalList", json!({}))
        .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(
        response
            .json::<serde_json::Value>()
            .await
            .expect("internal convex error should parse")["error"]["message"]
            .as_str()
            .expect("internal convex error should be a string")
            .contains("not public")
    );
}
