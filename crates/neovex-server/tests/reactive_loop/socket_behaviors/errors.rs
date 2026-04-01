use super::*;

#[tokio::test]
async fn websocket_invalid_message_returns_error_event() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    assert!(api.create_tenant("demo").await.status().is_success());

    let mut socket = WebSocketFixture::connect(&api.ws_url("/ws"), "demo").await;
    socket.send_text("{not json").await;

    let message = socket.next_json().await;
    assert_eq!(message["type"], json!("error"));
    assert!(message["request_id"].is_null());
    assert!(
        message["message"]
            .as_str()
            .expect("message should be a string")
            .contains("invalid websocket message")
    );
}
