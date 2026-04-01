use super::*;

#[tokio::test]
async fn license_status_route_returns_community_defaults() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    let response = api.license_status().await;

    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("license status json should parse");
    assert_eq!(body["kind"], json!("community"));
    assert_eq!(body["status"], json!("community"));
    assert_eq!(body["source"]["kind"], json!("community_default"));
    assert_eq!(body["revenue_limit_usd"], json!(10_000_000));
    assert_eq!(body["monthly_active_user_limit"], json!(500));
    assert_eq!(body["usage"]["monthly_active_users"], json!(0));
    assert_eq!(body["usage"]["limit"], json!(500));
    assert_eq!(body["usage"]["limit_exceeded"], json!(false));
}
