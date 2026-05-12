use std::time::Duration;

use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::time::timeout;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{
        Error as WebSocketError, Message, client::IntoClientRequest,
        http::header::SEC_WEBSOCKET_PROTOCOL,
    },
};

pub struct WebSocketFixture {
    socket: WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
}

impl WebSocketFixture {
    pub async fn connect_request(
        request: impl IntoClientRequest + Unpin,
    ) -> Result<Self, WebSocketError> {
        let mut request = request.into_client_request()?;
        let should_handshake_nimbus_v2 = should_offer_nimbus_v2(request.uri().path());
        if should_handshake_nimbus_v2 && !request.headers().contains_key(SEC_WEBSOCKET_PROTOCOL) {
            request.headers_mut().insert(
                SEC_WEBSOCKET_PROTOCOL,
                http::HeaderValue::from_static("nimbus.v2"),
            );
        }
        let (mut socket, _) = connect_async(request).await?;
        if should_handshake_nimbus_v2 {
            complete_nimbus_v2_handshake(&mut socket).await?;
        }
        Ok(Self { socket })
    }

    pub async fn connect_raw(url: &str) -> Result<Self, WebSocketError> {
        Self::connect_request(url).await
    }

    pub async fn connect(url: &str, tenant_id: &str) -> Self {
        Self::connect_with_tenant(url, Some(tenant_id))
            .await
            .expect("websocket connection should succeed")
    }

    pub async fn connect_with_tenant(
        url: &str,
        tenant_id: Option<&str>,
    ) -> Result<Self, WebSocketError> {
        let mut request = url
            .into_client_request()
            .expect("websocket request should build");
        if let Some(tenant_id) = tenant_id {
            request.headers_mut().insert(
                "X-Tenant-Id",
                http::HeaderValue::from_str(tenant_id).expect("tenant id header should be valid"),
            );
        }
        Self::connect_request(request).await
    }

    pub async fn connect_for_browser(url: &str, tenant_id: &str) -> Result<Self, WebSocketError> {
        let connector = format!("{url}?tenant_id={tenant_id}");
        Self::connect_request(connector).await
    }

    pub async fn subscribe_all(&mut self, request_id: &str, table: &str) {
        self.send_text(
            json!({
                "type": "subscribe",
                "request_id": request_id,
                "query": {
                    "table": table,
                    "filters": [],
                    "order": null,
                    "limit": null
                }
            })
            .to_string(),
        )
        .await;
    }

    pub async fn subscribe_named(&mut self, request_id: &str, name: &str, args: Value) {
        self.subscribe_named_with_options(request_id, name, args, None, None)
            .await;
    }

    pub async fn subscribe_named_with_options(
        &mut self,
        request_id: &str,
        name: &str,
        args: Value,
        page_size: Option<usize>,
        cursor: Option<&str>,
    ) {
        self.send_text(
            json!({
                "type": "subscribe_named",
                "request_id": request_id,
                "name": name,
                "args": args,
                "page_size": page_size,
                "cursor": cursor,
            })
            .to_string(),
        )
        .await;
    }

    pub async fn unsubscribe(&mut self, subscription_id: u64) {
        self.send_text(
            json!({
                "type": "unsubscribe",
                "subscription_id": subscription_id,
            })
            .to_string(),
        )
        .await;
    }

    pub async fn send_text(&mut self, text: impl Into<String>) {
        self.socket
            .send(Message::Text(text.into().into()))
            .await
            .expect("websocket message should send");
    }

    pub async fn send_binary(&mut self, bytes: impl Into<Vec<u8>>) {
        self.socket
            .send(Message::Binary(bytes.into().into()))
            .await
            .expect("websocket binary message should send");
    }

    pub async fn next_message(&mut self) -> Message {
        self.next_message_with_timeout(Duration::from_secs(5))
            .await
            .expect("timed out waiting for websocket message")
    }

    pub async fn next_message_with_timeout(&mut self, duration: Duration) -> Option<Message> {
        match timeout(duration, self.socket.next()).await {
            Ok(Some(Ok(message))) => Some(message),
            Ok(Some(Err(error))) => panic!("websocket message should be valid: {error}"),
            Ok(None) | Err(_) => None,
        }
    }

    pub async fn next_json(&mut self) -> Value {
        self.next_json_with_timeout(Duration::from_secs(5))
            .await
            .expect("timed out waiting for websocket message")
    }

    pub async fn next_json_with_timeout(&mut self, duration: Duration) -> Option<Value> {
        let message = self.next_message_with_timeout(duration).await?;
        match message {
            Message::Text(text) => {
                Some(serde_json::from_str(&text).expect("message should be json"))
            }
            other => panic!("unexpected websocket message: {other:?}"),
        }
    }

    pub async fn next_binary(&mut self) -> Vec<u8> {
        self.next_binary_with_timeout(Duration::from_secs(5))
            .await
            .expect("timed out waiting for websocket binary message")
    }

    pub async fn next_binary_with_timeout(&mut self, duration: Duration) -> Option<Vec<u8>> {
        let message = self.next_message_with_timeout(duration).await?;
        match message {
            Message::Binary(bytes) => Some(bytes.to_vec()),
            other => panic!("unexpected websocket message: {other:?}"),
        }
    }
}

fn should_offer_nimbus_v2(path: &str) -> bool {
    !path.starts_with("/google.firestore.v1.Firestore/Listen")
}

async fn complete_nimbus_v2_handshake(
    socket: &mut WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
) -> Result<(), WebSocketError> {
    let hello = timeout(Duration::from_secs(2), socket.next())
        .await
        .map_err(|_| {
            WebSocketError::Io(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "timed out waiting for websocket hello frame",
            ))
        })?
        .ok_or_else(|| {
            WebSocketError::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "websocket closed before hello frame",
            ))
        })??;
    match hello {
        Message::Text(text) => {
            let body: Value = serde_json::from_str(&text).map_err(|error| {
                WebSocketError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("invalid websocket hello frame: {error}"),
                ))
            })?;
            if body.get("type").and_then(Value::as_str) != Some("hello") {
                return Err(WebSocketError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("unexpected websocket frame before client_hello: {body}"),
                )));
            }
        }
        other => {
            return Err(WebSocketError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("expected websocket hello text frame, got {other:?}"),
            )));
        }
    }

    socket
        .send(Message::Text(
            json!({
                "type": "client_hello",
                "protocol": "nimbus.v2",
                "client": {
                    "kind": "test",
                    "version": "0.0.0"
                },
                "capabilities": [
                    "queries.v1",
                    "subscriptions.v1",
                    "auth.socket.v1",
                    "convex.named_subscriptions.v1"
                ]
            })
            .to_string()
            .into(),
        ))
        .await
}
