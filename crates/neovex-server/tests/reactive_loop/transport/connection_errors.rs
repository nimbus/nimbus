use super::*;

#[tokio::test]
async fn websocket_missing_tenant_header_returns_bad_request() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    match WebSocketFixture::connect_with_tenant(&api.ws_url("/ws"), None).await {
        Err(WebSocketError::Http(response)) => {
            assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
        }
        Ok(_) => panic!("connection should fail"),
        Err(other) => panic!("unexpected websocket error: {other:?}"),
    }
}

#[tokio::test]
async fn websocket_nonexistent_tenant_returns_not_found() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    match WebSocketFixture::connect_with_tenant(&api.ws_url("/ws"), Some("missing")).await {
        Err(WebSocketError::Http(response)) => {
            assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);
        }
        Ok(_) => panic!("connection should fail"),
        Err(other) => panic!("unexpected websocket error: {other:?}"),
    }
}
