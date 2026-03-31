use std::time::Duration;

use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::time::timeout;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Error as WebSocketError, Message, client::IntoClientRequest},
};

pub struct WebSocketFixture {
    socket: WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
}

impl WebSocketFixture {
    pub async fn connect_raw(url: &str) -> Result<Self, WebSocketError> {
        let (socket, _) = connect_async(url).await?;
        Ok(Self { socket })
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

        let (socket, _) = connect_async(request).await?;
        Ok(Self { socket })
    }

    pub async fn connect_for_browser(url: &str, tenant_id: &str) -> Result<Self, WebSocketError> {
        let connector = format!("{url}?tenant_id={tenant_id}");
        let (socket, _) = connect_async(connector).await?;
        Ok(Self { socket })
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

    pub async fn next_json(&mut self) -> Value {
        self.next_json_with_timeout(Duration::from_secs(5))
            .await
            .expect("timed out waiting for websocket message")
    }

    pub async fn next_json_with_timeout(&mut self, duration: Duration) -> Option<Value> {
        let message = match timeout(duration, self.socket.next()).await {
            Ok(Some(Ok(message))) => message,
            Ok(Some(Err(error))) => panic!("websocket message should be valid: {error}"),
            Ok(None) | Err(_) => return None,
        };

        match message {
            Message::Text(text) => {
                Some(serde_json::from_str(&text).expect("message should be json"))
            }
            other => panic!("unexpected websocket message: {other:?}"),
        }
    }
}
