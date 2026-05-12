use super::*;

use axum::http::{HeaderValue, header};
use futures::{SinkExt, StreamExt};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Error as TungsteniteError;

#[tokio::test]
async fn websocket_protocol_rejects_no_overlap_with_structured_http_error() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    assert!(api.create_tenant("demo").await.status().is_success());

    let mut request = api
        .ws_url("/ws?tenant_id=demo")
        .into_client_request()
        .expect("websocket request should build");
    request.headers_mut().insert(
        header::SEC_WEBSOCKET_PROTOCOL,
        HeaderValue::from_static("nimbus.v3"),
    );

    let error = connect_async(request)
        .await
        .expect_err("unsupported protocol should reject upgrade");
    let TungsteniteError::Http(response) = error else {
        panic!("expected HTTP websocket rejection, got {error:?}");
    };
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response
        .body()
        .as_ref()
        .expect("http rejection should include a body");
    let payload: serde_json::Value =
        serde_json::from_slice(body).expect("structured error body should parse");
    assert_eq!(payload["error"]["code"], json!("protocol.no_overlap"));
    assert_eq!(payload["error"]["severity"], json!("fatal"));
    assert_eq!(payload["error"]["retryable"], json!(false));
    assert_eq!(
        payload["error"]["detail"]["clientOffered"],
        json!(["nimbus.v3"])
    );
}

#[tokio::test]
async fn websocket_protocol_v2_echoes_subprotocol_and_sends_hello_immediately() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    assert!(api.create_tenant("demo").await.status().is_success());

    let mut request = api
        .ws_url("/ws?tenant_id=demo")
        .into_client_request()
        .expect("websocket request should build");
    request.headers_mut().insert(
        header::SEC_WEBSOCKET_PROTOCOL,
        HeaderValue::from_static("nimbus.v2"),
    );

    let (mut socket, response) = connect_async(request)
        .await
        .expect("v2 websocket should connect");
    assert_eq!(
        response
            .headers()
            .get(header::SEC_WEBSOCKET_PROTOCOL)
            .expect("negotiated subprotocol should be echoed"),
        "nimbus.v2"
    );

    let hello = match timeout(Duration::from_secs(2), socket.next()).await {
        Ok(Some(Ok(WsMessage::Text(text)))) => {
            serde_json::from_str::<serde_json::Value>(&text).expect("hello should parse")
        }
        other => panic!("expected hello frame, got {other:?}"),
    };
    assert_eq!(hello["type"], json!("hello"));
    assert_eq!(hello["protocol"], json!("nimbus.v2"));

    socket
        .send(WsMessage::Text(
            json!({
                "type": "client_hello",
                "protocol": "nimbus.v2",
                "client": {
                    "kind": "test",
                    "version": "0.0.0"
                },
                "capabilities": ["queries.v1", "subscriptions.v1"]
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("client hello should send");

    socket
        .send(WsMessage::Text(
            json!({
                "type": "subscribe",
                "request_id": "protocol-v2",
                "query": {
                    "table": "tasks",
                    "filters": [],
                    "order": null,
                    "limit": null
                }
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("post-handshake subscribe should send");

    let initial = match timeout(Duration::from_secs(2), socket.next()).await {
        Ok(Some(Ok(WsMessage::Text(text)))) => serde_json::from_str::<serde_json::Value>(&text)
            .expect("subscription result should parse"),
        other => panic!("expected subscription result, got {other:?}"),
    };
    assert_eq!(initial["type"], json!("subscription_result"));
    assert_eq!(initial["request_id"], json!("protocol-v2"));
}

#[tokio::test]
async fn websocket_protocol_v2_times_out_missing_client_hello() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    assert!(api.create_tenant("demo").await.status().is_success());

    let mut request = api
        .ws_url("/ws?tenant_id=demo")
        .into_client_request()
        .expect("websocket request should build");
    request.headers_mut().insert(
        header::SEC_WEBSOCKET_PROTOCOL,
        HeaderValue::from_static("nimbus.v2"),
    );

    let (mut socket, _) = connect_async(request)
        .await
        .expect("v2 websocket should connect");

    let hello = timeout(Duration::from_secs(2), socket.next())
        .await
        .expect("hello should arrive before timeout")
        .expect("socket should stay open for hello")
        .expect("hello frame should be valid");
    assert!(matches!(hello, WsMessage::Text(_)));

    let fatal = timeout(Duration::from_secs(12), socket.next())
        .await
        .expect("fatal hello timeout should arrive")
        .expect("socket should emit fatal frame")
        .expect("fatal frame should be valid");
    let WsMessage::Text(fatal_text) = fatal else {
        panic!("expected fatal error text frame");
    };
    let fatal_payload: serde_json::Value =
        serde_json::from_str(&fatal_text).expect("fatal error frame should parse");
    assert_eq!(fatal_payload["type"], json!("fatal_error"));
    assert_eq!(
        fatal_payload["error"]["code"],
        json!("protocol.hello_timeout")
    );

    let close = timeout(Duration::from_secs(2), socket.next())
        .await
        .expect("close frame should follow fatal frame")
        .expect("close frame should be present")
        .expect("close frame should be valid");
    assert!(matches!(close, WsMessage::Close(_)));
}

#[tokio::test]
async fn websocket_protocol_rejects_missing_subprotocol_header() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(
        fixture.service(),
        ConvexRegistry::empty(),
    ))
    .await;
    let api = HttpApiFixture::new(&server);

    assert!(api.create_tenant("demo").await.status().is_success());

    let error = connect_async(
        api.ws_url("/convex/demo/ws")
            .into_client_request()
            .expect("websocket request should build"),
    )
    .await
    .expect_err("missing websocket subprotocol should reject upgrade");
    let TungsteniteError::Http(response) = error else {
        panic!("expected HTTP websocket rejection, got {error:?}");
    };
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response
        .body()
        .as_ref()
        .expect("http rejection should include a body");
    let payload: serde_json::Value =
        serde_json::from_slice(body).expect("structured error body should parse");
    assert_eq!(payload["error"]["code"], json!("protocol.no_overlap"));
    assert_eq!(payload["error"]["detail"]["clientOffered"], json!([]));
}

#[tokio::test]
async fn websocket_protocol_rejects_explicit_v1_only_offer() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    assert!(api.create_tenant("demo").await.status().is_success());

    let mut request = api
        .ws_url("/ws?tenant_id=demo")
        .into_client_request()
        .expect("websocket request should build");
    request.headers_mut().insert(
        header::SEC_WEBSOCKET_PROTOCOL,
        HeaderValue::from_static("nimbus.v1"),
    );

    let error = connect_async(request)
        .await
        .expect_err("v1-only websocket should reject upgrade");
    let TungsteniteError::Http(response) = error else {
        panic!("expected HTTP websocket rejection, got {error:?}");
    };
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response
        .body()
        .as_ref()
        .expect("http rejection should include a body");
    let payload: serde_json::Value =
        serde_json::from_slice(body).expect("structured error body should parse");
    assert_eq!(payload["error"]["code"], json!("protocol.no_overlap"));
    assert_eq!(
        payload["error"]["detail"]["clientOffered"],
        json!(["nimbus.v1"])
    );
}
