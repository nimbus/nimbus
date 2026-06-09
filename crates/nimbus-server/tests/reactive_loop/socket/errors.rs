use super::*;

#[tokio::test]
async fn websocket_invalid_message_returns_error_event() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server = ServerFixture::start(router_for_engine(fixture.engine())).await;
    let api = HttpApiFixture::new(&server);

    assert!(api.create_tenant("demo").await.status().is_success());

    let mut socket = WebSocketFixture::connect(&api.ws_url("/ws"), "demo").await;
    socket.send_text("{not json").await;

    let message = socket.next_json().await;
    assert_eq!(message["type"], json!("error"));
    assert!(
        message["error"]["message"]
            .as_str()
            .expect("message should be a string")
            .contains("invalid websocket message")
    );
}
