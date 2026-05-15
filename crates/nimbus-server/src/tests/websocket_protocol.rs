use super::*;

use axum::http::{HeaderValue, header};
use futures::{SinkExt, StreamExt};
use nimbus_core::{Query, TableName};
use nimbus_engine::SubscriptionUpdate;
use tokio::sync::mpsc as tokio_mpsc;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Error as TungsteniteError;

fn direct_query_function(name: &str, table: &str) -> serde_json::Value {
    json!({
        "name": name,
        "kind": "query",
        "plan": {
            "table": table,
            "filters": [],
            "order": null,
            "limit": null
        }
    })
}

async fn next_subscription_documents(
    updates: &mut tokio_mpsc::Receiver<SubscriptionUpdate>,
    description: &str,
) -> Vec<serde_json::Value> {
    match timeout(Duration::from_secs(5), updates.recv()).await {
        Ok(Some(SubscriptionUpdate::Result { snapshot, .. })) => snapshot.to_json_documents(),
        Ok(Some(SubscriptionUpdate::Error { message, .. })) => {
            panic!("{description} failed with subscription error: {message}")
        }
        Ok(None) => panic!("{description} failed because subscription channel closed"),
        Err(_) => panic!("timed out waiting for {description}"),
    }
}

async fn wait_for_subscription_documents(
    updates: &mut tokio_mpsc::Receiver<SubscriptionUpdate>,
    description: &str,
    predicate: impl Fn(&[serde_json::Value]) -> bool,
) -> Vec<serde_json::Value> {
    let deadline = Duration::from_secs(5);
    timeout(deadline, async {
        loop {
            let documents = next_subscription_documents(updates, description).await;
            if predicate(&documents) {
                return documents;
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timed out waiting for {description}"))
}

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
async fn convex_websocket_subscription_projects_live_system_subscription_state() {
    let user_registry =
        convex_registry(json!([direct_query_function("messages:list", "messages")]));
    let system_registry = convex_registry(json!([direct_query_function(
        "subscriptions:list",
        "subscriptions"
    )]));
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    crate::system_tenant::prepare_system_tenant_async(&service, None)
        .await
        .expect("system tenant should prepare");
    let server = ServerFixture::start(
        RouterBuildConfig::core(service.clone())
            .with_application_auth_verifier(crate::router::convex_application_auth_verifier(
                &user_registry,
            ))
            .with_convex(user_registry)
            .with_system_convex_registry(system_registry)
            .build(),
    )
    .await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );
    assert_eq!(
        api.insert_document("demo", "messages", json!({ "body": "hello" }))
            .await
            .status(),
        StatusCode::CREATED
    );

    let (system_tx, mut system_rx) =
        tokio_mpsc::channel(nimbus_engine::DEFAULT_SUBSCRIPTION_CHANNEL_CAPACITY);
    let system_subscription = service
        .subscribe_async(
            crate::system_tenant::system_tenant_id().expect("system id should parse"),
            Query {
                table: TableName::new("subscriptions").expect("table should parse"),
                filters: Vec::new(),
                order: None,
                limit: None,
            },
            "system-subscriptions-watch".to_string(),
            system_tx,
        )
        .await
        .expect("system tenant subscriptions table should be subscribable");
    let initial =
        next_subscription_documents(&mut system_rx, "initial _nimbus subscriptions snapshot").await;
    assert!(
        initial.is_empty(),
        "system subscription table should start empty: {initial:?}"
    );

    let mut socket = WebSocketFixture::connect_raw(&api.ws_url("/convex/demo/ws"))
        .await
        .expect("convex websocket should connect");
    socket
        .subscribe_named("messages-watch", "messages:list", json!({}))
        .await;
    let bootstrap = socket.next_json().await;
    assert_eq!(bootstrap["type"], json!("subscription_result"));
    assert_eq!(bootstrap["request_id"], json!("messages-watch"));
    let subscription_id = bootstrap["subscription_id"]
        .as_u64()
        .expect("bootstrap should include subscription id");

    let persisted = wait_for_value(
        "persisted Convex websocket subscription projection",
        Duration::from_secs(5),
        Duration::from_millis(25),
        || {
            let service = service.clone();
            async move {
                service
                    .list_documents_async(
                        crate::system_tenant::system_tenant_id().expect("system id should parse"),
                        TableName::new("subscriptions").expect("table should parse"),
                    )
                    .await
            }
        },
        |result| {
            result.as_ref().is_ok_and(|documents| {
                documents.iter().any(|document| {
                    document.fields.get("tenantId") == Some(&json!("demo"))
                        && document.fields.get("adapter") == Some(&json!("convex"))
                })
            })
        },
    )
    .await
    .expect("subscription document should persist");
    assert_eq!(
        persisted.len(),
        1,
        "expected one persisted active subscription"
    );

    let active = wait_for_subscription_documents(
        &mut system_rx,
        "active Convex websocket subscription projection",
        |documents| {
            documents.iter().any(|document| {
                document["tenantId"] == json!("demo")
                    && document["adapter"] == json!("convex")
                    && document["clientCount"] == json!(1)
                    && document["queryKey"]
                        .as_str()
                        .is_some_and(|key| key.contains("messages:list"))
            })
        },
    )
    .await;
    assert_eq!(
        active.len(),
        1,
        "expected one active subscription: {active:?}"
    );

    let queried = api
        .convex_named_query("_nimbus", "subscriptions:list", json!({}))
        .await;
    assert_eq!(queried.status(), StatusCode::OK);
    let queried_body = queried
        .json::<serde_json::Value>()
        .await
        .expect("system subscriptions query should parse");
    assert!(
        queried_body.as_array().is_some_and(|documents| {
            documents
                .iter()
                .any(|document| document["tenantId"] == json!("demo"))
        }),
        "system Convex query should expose active subscription state: {queried_body}"
    );

    socket.unsubscribe(subscription_id).await;
    let cleared = wait_for_subscription_documents(
        &mut system_rx,
        "Convex websocket subscription cleanup projection",
        |documents| documents.is_empty(),
    )
    .await;
    assert!(
        cleared.is_empty(),
        "unsubscribe should remove the system subscription document: {cleared:?}"
    );

    drop(system_subscription);
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
