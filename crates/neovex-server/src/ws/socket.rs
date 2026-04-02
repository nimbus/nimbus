use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use futures::{SinkExt, StreamExt};
use neovex_engine::{
    DEFAULT_SUBSCRIPTION_CHANNEL_CAPACITY, SubscriptionCleanupHandle, SubscriptionUpdate,
};
use tokio::sync::mpsc;

use crate::owned_tasks::OwnedTaskSet;
use crate::protocol::{ClientMessage, ServerMessage};
use crate::state::AppState;

pub(crate) async fn handle_socket_for_tenant(
    socket: WebSocket,
    state: Arc<AppState>,
    tenant_id: neovex_core::TenantId,
) {
    const OUTBOUND_CHANNEL_CAPACITY: usize = 256;

    let (mut socket_tx, mut socket_rx) = socket.split();
    let (outbound_tx, mut outbound_rx) = mpsc::channel::<ServerMessage>(OUTBOUND_CHANNEL_CAPACITY);
    let (subscription_tx, mut subscription_rx) =
        mpsc::channel::<SubscriptionUpdate>(DEFAULT_SUBSCRIPTION_CHANNEL_CAPACITY);

    let mut tasks = OwnedTaskSet::new();
    let forward_tx = outbound_tx.clone();
    tasks.spawn(async move {
        while let Some(event) = subscription_rx.recv().await {
            let message = match event {
                SubscriptionUpdate::Result {
                    subscription_id,
                    request_id,
                    data,
                    ..
                } => ServerMessage::SubscriptionResult {
                    subscription_id,
                    request_id,
                    data: serde_json::Value::Array(data),
                },
                SubscriptionUpdate::Error {
                    request_id,
                    message,
                    ..
                } => ServerMessage::Error {
                    request_id,
                    message,
                },
            };
            if forward_tx.send(message).await.is_err() {
                break;
            }
        }
    });

    tasks.spawn(async move {
        while let Some(message) = outbound_rx.recv().await {
            let Ok(text) = serde_json::to_string(&message) else {
                break;
            };
            if socket_tx.send(Message::Text(text.into())).await.is_err() {
                break;
            }
        }
    });

    let mut active_subscriptions = HashMap::<u64, SubscriptionCleanupHandle>::new();
    while let Some(message_result) = socket_rx.next().await {
        let message = match message_result {
            Ok(message) => message,
            Err(_) => break,
        };

        match message {
            Message::Text(text) => match serde_json::from_str::<ClientMessage>(&text) {
                Ok(ClientMessage::Authenticate { .. }) => {
                    let _ = outbound_tx
                        .send(ServerMessage::AuthError {
                            message:
                                "authentication is not supported on the generic websocket route"
                                    .to_string(),
                        })
                        .await;
                }
                Ok(ClientMessage::ClearAuth) => {
                    let _ = outbound_tx
                        .send(ServerMessage::Authenticated {
                            is_authenticated: false,
                        })
                        .await;
                }
                Ok(ClientMessage::Subscribe { request_id, query }) => {
                    let request_id_for_worker = request_id.clone();
                    let service = state.service.clone();
                    let tenant_id = tenant_id.clone();
                    let sender = subscription_tx.clone();
                    match service
                        .subscribe_async(tenant_id, query, request_id_for_worker, sender)
                        .await
                    {
                        Ok(registration) => {
                            let (subscription_id, cleanup_handle) = registration.into_parts();
                            active_subscriptions.insert(subscription_id, cleanup_handle);
                        }
                        Err(error) => {
                            let _ = outbound_tx
                                .send(ServerMessage::Error {
                                    request_id: Some(request_id),
                                    message: error.to_string(),
                                })
                                .await;
                        }
                    }
                }
                Ok(ClientMessage::Unsubscribe { subscription_id }) => {
                    let cleanup_handle = active_subscriptions.remove(&subscription_id);
                    let service = state.service.clone();
                    let tenant_id = tenant_id.clone();
                    if let Err(error) = service.unsubscribe_async(tenant_id, subscription_id).await
                    {
                        let _ = outbound_tx
                            .send(ServerMessage::Error {
                                request_id: None,
                                message: error.to_string(),
                            })
                            .await;
                    }
                    drop(cleanup_handle);
                }
                Err(error) => {
                    let _ = outbound_tx
                        .send(ServerMessage::Error {
                            request_id: None,
                            message: format!("invalid websocket message: {error}"),
                        })
                        .await;
                }
            },
            Message::Close(_) => break,
            Message::Ping(_) | Message::Pong(_) | Message::Binary(_) => {}
        }
    }

    for subscription_id in active_subscriptions.keys().copied().collect::<Vec<_>>() {
        let _ = state
            .service
            .unsubscribe_async(tenant_id.clone(), subscription_id)
            .await;
    }
    drop(active_subscriptions);
    drop(subscription_tx);
    drop(outbound_tx);
    tasks.shutdown_and_drain().await;
}
