use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use futures::stream::{SplitSink, SplitStream};
use futures::{SinkExt, StreamExt};
use nimbus_engine::SubscriptionUpdate;
use tokio::sync::mpsc;

use crate::owned_tasks::OwnedTaskSet;
use crate::protocol::{ClientMessage, ServerMessage};
use crate::ws::negotiation::NegotiatedWebSocketProtocol;

use super::pending::PendingBootstrapCancellationRegistry;

pub(super) enum InboundSocketEvent {
    Message(ClientMessage),
    Invalid(String),
}

pub(super) fn spawn_socket_reader(
    tasks: &mut OwnedTaskSet,
    mut socket_rx: SplitStream<WebSocket>,
    inbound_tx: mpsc::Sender<InboundSocketEvent>,
) {
    tasks.spawn(async move {
        while let Some(message_result) = socket_rx.next().await {
            let message = match message_result {
                Ok(message) => message,
                Err(_) => break,
            };

            let event = match message {
                Message::Text(text) => match serde_json::from_str::<ClientMessage>(&text) {
                    Ok(message) => InboundSocketEvent::Message(message),
                    Err(error) => {
                        InboundSocketEvent::Invalid(format!("invalid websocket message: {error}"))
                    }
                },
                Message::Close(_) => break,
                Message::Ping(_) | Message::Pong(_) | Message::Binary(_) => continue,
            };

            if inbound_tx.send(event).await.is_err() {
                break;
            }
        }
    });
}

pub(super) fn spawn_subscription_forwarder(
    tasks: &mut OwnedTaskSet,
    mut subscription_rx: mpsc::Receiver<SubscriptionUpdate>,
    outbound_tx: mpsc::Sender<ServerMessage>,
    pending_bootstrap_cancellations: Arc<PendingBootstrapCancellationRegistry>,
) {
    tasks.spawn(async move {
        while let Some(event) = subscription_rx.recv().await {
            if let SubscriptionUpdate::Result {
                subscription_id,
                request_id: Some(request_id),
                ..
            } = &event
            {
                pending_bootstrap_cancellations.link_subscription(*subscription_id, request_id);
            }
            let message = match event {
                SubscriptionUpdate::Result {
                    subscription_id,
                    request_id,
                    snapshot,
                    ..
                } => ServerMessage::SubscriptionResult {
                    subscription_id,
                    request_id,
                    data: serde_json::Value::Array(snapshot.into_json_documents()),
                },
                SubscriptionUpdate::Error {
                    request_id,
                    message,
                    ..
                } => match request_id {
                    Some(request_id) => {
                        ServerMessage::request_error(request_id, "op.failed", message)
                    }
                    None => ServerMessage::session_error("session.subscription_error", message),
                },
            };
            if outbound_tx.send(message).await.is_err() {
                break;
            }
        }
    });
}

pub(super) fn spawn_socket_writer(
    tasks: &mut OwnedTaskSet,
    mut socket_tx: SplitSink<WebSocket, Message>,
    mut outbound_rx: mpsc::Receiver<ServerMessage>,
    protocol: NegotiatedWebSocketProtocol,
) {
    tasks.spawn(async move {
        while let Some(message) = outbound_rx.recv().await {
            let Ok(text) = message.to_text(protocol) else {
                break;
            };
            if socket_tx.send(Message::Text(text.into())).await.is_err() {
                break;
            }
        }
    });
}
