use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode, header};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::time::{Duration, Instant, timeout};

use crate::error_envelope::{
    FATAL_PROTOCOL_CLOSE_CODE, PublicError, StructuredHttpError, send_fatal_error_and_close,
};
use crate::state::AppError;

pub(crate) const NEOVEX_PROTOCOL_V2: &str = "neovex.v2";
pub(crate) const CLIENT_HELLO_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NegotiatedWebSocketProtocol {
    V2,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct HelloContext {
    features: &'static [&'static str],
}

impl HelloContext {
    pub(crate) fn native() -> Self {
        Self {
            features: &["queries.v1", "subscriptions.v1"],
        }
    }

    pub(crate) fn convex() -> Self {
        Self {
            features: &[
                "auth.socket.v1",
                "queries.v1",
                "subscriptions.v1",
                "convex.named_subscriptions.v1",
            ],
        }
    }
}

#[derive(Debug, Serialize)]
struct HelloFrame<'a> {
    #[serde(rename = "type")]
    frame_type: &'static str,
    protocol: &'static str,
    server: HelloServer<'a>,
    features: &'a [&'a str],
    session: HelloSession,
}

#[derive(Debug, Serialize)]
struct HelloServer<'a> {
    version: &'static str,
    build: &'a str,
}

#[derive(Debug, Serialize)]
struct HelloSession {
    id: String,
    #[serde(rename = "serverNow")]
    server_now: i64,
}

#[derive(Debug, Deserialize)]
struct ClientHelloFrame {
    #[serde(rename = "type")]
    frame_type: String,
    protocol: String,
}

pub(crate) fn negotiate(headers: &HeaderMap) -> Result<NegotiatedWebSocketProtocol, AppError> {
    let Some(value) = headers.get(header::SEC_WEBSOCKET_PROTOCOL) else {
        return Err(AppError::Structured(Box::new(StructuredHttpError::new(
            StatusCode::BAD_REQUEST,
            PublicError::protocol_no_overlap(Vec::new()),
        ))));
    };
    let value = value.to_str().map_err(|error| {
        AppError::from(neovex_core::Error::InvalidInput(format!(
            "invalid Sec-WebSocket-Protocol header: {error}"
        )))
    })?;
    let offered = value
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    if offered.iter().any(|entry| entry == NEOVEX_PROTOCOL_V2) {
        return Ok(NegotiatedWebSocketProtocol::V2);
    }
    Err(AppError::Structured(Box::new(StructuredHttpError::new(
        StatusCode::BAD_REQUEST,
        PublicError::protocol_no_overlap(offered),
    ))))
}

pub(crate) fn configure_upgrade(ws: WebSocketUpgrade) -> WebSocketUpgrade {
    ws.protocols([NEOVEX_PROTOCOL_V2])
}

pub(crate) async fn complete_handshake(
    mut socket: WebSocket,
    protocol: NegotiatedWebSocketProtocol,
    hello_context: HelloContext,
) -> Option<WebSocket> {
    match protocol {
        NegotiatedWebSocketProtocol::V2 => {
            if send_hello(&mut socket, hello_context).await.is_err() {
                return None;
            }
            wait_for_client_hello(socket).await
        }
    }
}

async fn send_hello(socket: &mut WebSocket, hello_context: HelloContext) -> Result<(), ()> {
    let hello = HelloFrame {
        frame_type: "hello",
        protocol: NEOVEX_PROTOCOL_V2,
        server: HelloServer {
            version: env!("CARGO_PKG_VERSION"),
            build: option_env!("NEOVEX_BUILD_ID").unwrap_or("unknown"),
        },
        features: hello_context.features,
        session: HelloSession {
            id: crate::execution::invocations::next_runtime_server_request_id("ws-session"),
            server_now: time::OffsetDateTime::now_utc().unix_timestamp_nanos() as i64 / 1_000_000,
        },
    };
    let text = serde_json::to_string(&hello).map_err(|_| ())?;
    socket
        .send(Message::Text(text.into()))
        .await
        .map_err(|_| ())
}

async fn wait_for_client_hello(mut socket: WebSocket) -> Option<WebSocket> {
    let deadline = Instant::now() + CLIENT_HELLO_TIMEOUT;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            send_fatal_error_and_close(
                &mut socket,
                PublicError::protocol_hello_timeout(CLIENT_HELLO_TIMEOUT.as_millis() as u64),
                FATAL_PROTOCOL_CLOSE_CODE,
            )
            .await;
            return None;
        }

        let inbound = match timeout(remaining, socket.next()).await {
            Ok(inbound) => inbound,
            Err(_) => {
                send_fatal_error_and_close(
                    &mut socket,
                    PublicError::protocol_hello_timeout(CLIENT_HELLO_TIMEOUT.as_millis() as u64),
                    FATAL_PROTOCOL_CLOSE_CODE,
                )
                .await;
                return None;
            }
        };

        match inbound {
            Some(Ok(Message::Text(text))) => match validate_client_hello(&text) {
                Ok(()) => return Some(socket),
                Err(error) => {
                    send_fatal_error_and_close(&mut socket, *error, FATAL_PROTOCOL_CLOSE_CODE)
                        .await;
                    return None;
                }
            },
            Some(Ok(Message::Ping(payload))) => {
                if socket.send(Message::Pong(payload)).await.is_err() {
                    return None;
                }
            }
            Some(Ok(Message::Pong(_))) => {}
            Some(Ok(Message::Binary(_))) => {
                send_fatal_error_and_close(
                    &mut socket,
                    PublicError::protocol_unsupported_binary(),
                    FATAL_PROTOCOL_CLOSE_CODE,
                )
                .await;
                return None;
            }
            Some(Ok(Message::Close(_))) | Some(Err(_)) | None => return None,
        }
    }
}

fn validate_client_hello(text: &str) -> Result<(), Box<PublicError>> {
    let value: Value = serde_json::from_str(text).map_err(|error| {
        Box::new(PublicError::protocol_invalid_json(format!(
            "Invalid client_hello JSON: {error}"
        )))
    })?;
    let message_type = value.get("type").and_then(Value::as_str);
    if message_type != Some("client_hello") {
        return Err(Box::new(PublicError::protocol_unsupported_message_type(
            message_type,
        )));
    }
    let frame: ClientHelloFrame = serde_json::from_value(value).map_err(|error| {
        Box::new(PublicError::protocol_invalid_json(format!(
            "Invalid client_hello payload: {error}"
        )))
    })?;
    if frame.frame_type != "client_hello" {
        return Err(Box::new(PublicError::protocol_unsupported_message_type(
            Some(frame.frame_type.as_str()),
        )));
    }
    if frame.protocol != NEOVEX_PROTOCOL_V2 {
        return Err(Box::new(PublicError::protocol_unsupported_version(
            frame.protocol,
        )));
    }
    Ok(())
}
