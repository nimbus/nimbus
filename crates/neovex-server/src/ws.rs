use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, HeaderValue};
use axum::response::Response;
use futures::{SinkExt, StreamExt};
use neovex_core::{Error, TenantId};
use neovex_engine::SubscriptionUpdate;
use tokio::sync::mpsc;

use crate::protocol::{ClientMessage, ServerMessage};
use crate::state::{AppError, AppState, run_blocking};

const TENANT_HEADER: &str = "X-Tenant-Id";

/// WebSocket upgrade handler.
pub(crate) async fn ws_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
    ws: WebSocketUpgrade,
) -> Result<Response, AppError> {
    let tenant_id = extract_tenant_id(&headers, params.get("tenant_id").cloned())?;
    let service = state.service.clone();
    let tenant_check = tenant_id.clone();
    run_blocking(move || service.ensure_tenant_exists(&tenant_check)).await?;

    Ok(ws.on_upgrade(move |socket| handle_socket_for_tenant(socket, state, tenant_id)))
}

pub(crate) async fn handle_socket_for_tenant(
    socket: WebSocket,
    state: Arc<AppState>,
    tenant_id: TenantId,
) {
    let (mut socket_tx, mut socket_rx) = socket.split();
    let (outbound_tx, mut outbound_rx) = mpsc::unbounded_channel::<ServerMessage>();
    let (subscription_tx, mut subscription_rx) = mpsc::unbounded_channel::<SubscriptionUpdate>();

    let forward_tx = outbound_tx.clone();
    let forward_task = tokio::spawn(async move {
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
            let _ = forward_tx.send(message);
        }
    });

    let send_task = tokio::spawn(async move {
        while let Some(message) = outbound_rx.recv().await {
            let Ok(text) = serde_json::to_string(&message) else {
                break;
            };
            if socket_tx.send(Message::Text(text.into())).await.is_err() {
                break;
            }
        }
    });

    let mut active_subscriptions = HashSet::new();
    while let Some(message_result) = socket_rx.next().await {
        let message = match message_result {
            Ok(message) => message,
            Err(_) => break,
        };

        match message {
            Message::Text(text) => match serde_json::from_str::<ClientMessage>(&text) {
                Ok(ClientMessage::Authenticate { .. }) => {
                    let _ = outbound_tx.send(ServerMessage::AuthError {
                        message: "authentication is not supported on the generic websocket route"
                            .to_string(),
                    });
                }
                Ok(ClientMessage::ClearAuth) => {
                    let _ = outbound_tx.send(ServerMessage::Authenticated {
                        is_authenticated: false,
                    });
                }
                Ok(ClientMessage::Subscribe { request_id, query }) => {
                    let request_id_for_worker = request_id.clone();
                    let service = state.service.clone();
                    let tenant_id = tenant_id.clone();
                    let sender = subscription_tx.clone();
                    match run_blocking(move || {
                        service.subscribe(&tenant_id, query, request_id_for_worker, sender)
                    })
                    .await
                    {
                        Ok(subscription_id) => {
                            active_subscriptions.insert(subscription_id);
                        }
                        Err(error) => {
                            let _ = outbound_tx.send(ServerMessage::Error {
                                request_id: Some(request_id),
                                message: error.to_string(),
                            });
                        }
                    }
                }
                Ok(ClientMessage::Unsubscribe { subscription_id }) => {
                    active_subscriptions.remove(&subscription_id);
                    let service = state.service.clone();
                    let tenant_id = tenant_id.clone();
                    if let Err(error) =
                        run_blocking(move || service.unsubscribe(&tenant_id, subscription_id)).await
                    {
                        let _ = outbound_tx.send(ServerMessage::Error {
                            request_id: None,
                            message: error.to_string(),
                        });
                    }
                }
                Err(error) => {
                    let _ = outbound_tx.send(ServerMessage::Error {
                        request_id: None,
                        message: format!("invalid websocket message: {error}"),
                    });
                }
            },
            Message::Close(_) => break,
            Message::Ping(_) | Message::Pong(_) | Message::Binary(_) => {}
        }
    }

    for subscription_id in active_subscriptions {
        let service = state.service.clone();
        let tenant_id = tenant_id.clone();
        let _ = run_blocking(move || service.unsubscribe(&tenant_id, subscription_id)).await;
    }
    drop(subscription_tx);
    drop(outbound_tx);
    let _ = forward_task.await;
    let _ = send_task.await;
}

fn extract_tenant_id(
    headers: &HeaderMap,
    query_tenant_id: Option<String>,
) -> Result<TenantId, AppError> {
    if let Some(value) = headers.get(TENANT_HEADER) {
        let tenant_id = header_value_to_string(value)?;
        return TenantId::new(tenant_id).map_err(AppError::from);
    }

    if let Some(tenant_id) = query_tenant_id {
        return TenantId::new(tenant_id).map_err(AppError::from);
    }

    Err(AppError::from(Error::InvalidInput(
        "missing X-Tenant-Id header or tenant_id query parameter".to_string(),
    )))
}

fn header_value_to_string(value: &HeaderValue) -> Result<String, AppError> {
    value
        .to_str()
        .map(|value| value.to_string())
        .map_err(|error| {
            AppError::from(Error::InvalidInput(format!(
                "invalid X-Tenant-Id header: {error}"
            )))
        })
}
