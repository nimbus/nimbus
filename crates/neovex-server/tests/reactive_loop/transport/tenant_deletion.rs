use super::*;

#[tokio::test]
async fn delete_tenant_sends_subscription_error() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    let create_response = api.create_tenant("demo").await;
    assert!(create_response.status().is_success());

    let mut socket = WebSocketFixture::connect(&api.ws_url("/ws"), "demo").await;
    socket.subscribe_all("3", "tasks").await;

    let initial = socket.next_json().await;
    assert_eq!(initial["type"], json!("subscription_result"));
    assert_eq!(initial["request_id"], json!("3"));

    let delete_response = api.delete_tenant("demo").await;
    assert_eq!(delete_response.status(), reqwest::StatusCode::NO_CONTENT);

    let teardown = socket.next_json().await;
    assert_eq!(teardown["type"], json!("error"));
    assert!(
        teardown["error"]["message"]
            .as_str()
            .expect("message should be a string")
            .contains("tenant deleted: demo")
    );
}
