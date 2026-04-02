use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use axum::extract::ws::{Message, WebSocket};
use futures::{SinkExt, StreamExt};
use neovex_core::Error;
use neovex_engine::{
    DEFAULT_SUBSCRIPTION_CHANNEL_CAPACITY, SubscriptionBootstrapCancellation,
    SubscriptionCleanupHandle, SubscriptionUpdate,
};
use neovex_runtime::HostCallCancellation;
use tokio::sync::mpsc;

use crate::owned_tasks::OwnedTaskSet;
use crate::protocol::{ClientMessage, ServerMessage};
use crate::state::AppState;

enum InboundSocketEvent {
    Message(ClientMessage),
    Invalid(String),
}

enum PendingSubscriptionEvent {
    Registered(neovex_engine::SubscriptionRegistration),
    Error { request_id: String, message: String },
}

#[derive(Default)]
struct PendingBootstrapCancellations {
    by_request_id: HashMap<String, HostCallCancellation>,
    by_subscription_id: HashMap<u64, HostCallCancellation>,
}

pub(crate) async fn handle_socket_for_tenant(
    socket: WebSocket,
    state: Arc<AppState>,
    tenant_id: neovex_core::TenantId,
) {
    const OUTBOUND_CHANNEL_CAPACITY: usize = 256;
    const INBOUND_CHANNEL_CAPACITY: usize = 256;

    let (mut socket_tx, mut socket_rx) = socket.split();
    let (outbound_tx, mut outbound_rx) = mpsc::channel::<ServerMessage>(OUTBOUND_CHANNEL_CAPACITY);
    let (subscription_tx, mut subscription_rx) =
        mpsc::channel::<SubscriptionUpdate>(DEFAULT_SUBSCRIPTION_CHANNEL_CAPACITY);
    let (inbound_tx, mut inbound_rx) =
        mpsc::channel::<InboundSocketEvent>(INBOUND_CHANNEL_CAPACITY);
    let (pending_subscription_tx, mut pending_subscription_rx) =
        mpsc::channel::<PendingSubscriptionEvent>(INBOUND_CHANNEL_CAPACITY);
    let disconnect_cancellation = HostCallCancellation::default();
    let pending_bootstrap_cancellations =
        Arc::new(Mutex::new(PendingBootstrapCancellations::default()));

    let mut tasks = OwnedTaskSet::new();
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

    let forward_tx = outbound_tx.clone();
    let pending_bootstrap_cancellations_for_forwarder = pending_bootstrap_cancellations.clone();
    tasks.spawn(async move {
        while let Some(event) = subscription_rx.recv().await {
            if let SubscriptionUpdate::Result {
                subscription_id,
                request_id: Some(request_id),
                ..
            } = &event
            {
                let mut pending = pending_bootstrap_cancellations_for_forwarder
                    .lock()
                    .expect("pending bootstrap cancellation lock should not be poisoned");
                if let Some(cancellation) = pending.by_request_id.get(request_id).cloned() {
                    pending
                        .by_subscription_id
                        .insert(*subscription_id, cancellation);
                }
            }
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
    let mut cancelled_pending_subscriptions = HashSet::<u64>::new();
    loop {
        tokio::select! {
            maybe_message = inbound_rx.recv() => {
                let Some(message) = maybe_message else {
                    break;
                };
                match message {
                    InboundSocketEvent::Message(ClientMessage::Authenticate { .. }) => {
                        let _ = outbound_tx
                            .send(ServerMessage::AuthError {
                                message:
                                    "authentication is not supported on the generic websocket route"
                                        .to_string(),
                            })
                            .await;
                    }
                    InboundSocketEvent::Message(ClientMessage::ClearAuth) => {
                        let _ = outbound_tx
                            .send(ServerMessage::Authenticated {
                                is_authenticated: false,
                            })
                            .await;
                    }
                    InboundSocketEvent::Message(ClientMessage::Subscribe { request_id, query }) => {
                        let request_id_for_worker = request_id.clone();
                        let service = state.service.clone();
                        let tenant_id = tenant_id.clone();
                        let sender = subscription_tx.clone();
                        let pending_subscription_tx = pending_subscription_tx.clone();
                        let disconnect_cancellation = disconnect_cancellation.clone();
                        let subscription_cancellation = HostCallCancellation::default();
                        pending_bootstrap_cancellations
                            .lock()
                            .expect("pending bootstrap cancellation lock should not be poisoned")
                            .by_request_id
                            .insert(request_id.clone(), subscription_cancellation.clone());
                        let pending_bootstrap_cancellations_for_worker =
                            pending_bootstrap_cancellations.clone();
                        tasks.spawn(async move {
                            let disconnect_wait = disconnect_cancellation.clone();
                            let disconnect_check = disconnect_cancellation.clone();
                            let subscription_wait = subscription_cancellation.clone();
                            let subscription_check = subscription_cancellation.clone();
                            let result = service
                                .subscribe_async_cancellable(
                                    tenant_id,
                                    query,
                                    request_id_for_worker.clone(),
                                    sender,
                                    SubscriptionBootstrapCancellation::new(
                                        async move {
                                            tokio::select! {
                                                _ = disconnect_wait.cancelled() => {}
                                                _ = subscription_wait.cancelled() => {}
                                            }
                                        },
                                        move || {
                                            if disconnect_check.is_cancelled()
                                                || subscription_check.is_cancelled()
                                            {
                                                Err(Error::Cancelled)
                                            } else {
                                                Ok(())
                                            }
                                        },
                                    ),
                                )
                                .await;
                            {
                                let mut pending = pending_bootstrap_cancellations_for_worker
                                    .lock()
                                    .expect(
                                        "pending bootstrap cancellation lock should not be poisoned",
                                    );
                                pending.by_request_id.remove(&request_id_for_worker);
                                if let Ok(registration) = &result {
                                    pending.by_subscription_id.remove(&registration.id());
                                }
                            }
                            let event = match result {
                                Ok(registration) => PendingSubscriptionEvent::Registered(registration),
                                Err(Error::Cancelled) => return,
                                Err(error) => PendingSubscriptionEvent::Error {
                                    request_id: request_id_for_worker,
                                    message: error.to_string(),
                                },
                            };
                            let _ = pending_subscription_tx.send(event).await;
                        });
                    }
                    InboundSocketEvent::Message(ClientMessage::Unsubscribe { subscription_id }) => {
                        let cleanup_handle = active_subscriptions.remove(&subscription_id);
                        if cleanup_handle.is_none() {
                            cancelled_pending_subscriptions.insert(subscription_id);
                            if let Some(cancellation) = pending_bootstrap_cancellations
                                .lock()
                                .expect(
                                    "pending bootstrap cancellation lock should not be poisoned",
                                )
                                .by_subscription_id
                                .remove(&subscription_id)
                            {
                                cancellation.cancel();
                            }
                        } else {
                            cancelled_pending_subscriptions.remove(&subscription_id);
                        }
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
                    InboundSocketEvent::Invalid(message) => {
                        let _ = outbound_tx
                            .send(ServerMessage::Error {
                                request_id: None,
                                message,
                            })
                            .await;
                    }
                }
            }
            maybe_pending = pending_subscription_rx.recv() => {
                let Some(pending) = maybe_pending else {
                    continue;
                };
                match pending {
                    PendingSubscriptionEvent::Registered(registration) => {
                        let (subscription_id, cleanup_handle) = registration.into_parts();
                        if cancelled_pending_subscriptions.remove(&subscription_id) {
                            drop(cleanup_handle);
                            continue;
                        }
                        active_subscriptions.insert(subscription_id, cleanup_handle);
                    }
                    PendingSubscriptionEvent::Error { request_id, message } => {
                        let _ = outbound_tx
                            .send(ServerMessage::Error {
                                request_id: Some(request_id),
                                message,
                            })
                            .await;
                    }
                }
            }
        }
    }

    disconnect_cancellation.cancel_due_to_disconnect();
    {
        let mut pending = pending_bootstrap_cancellations
            .lock()
            .expect("pending bootstrap cancellation lock should not be poisoned");
        pending.by_request_id.clear();
        pending.by_subscription_id.clear();
    }
    for subscription_id in active_subscriptions.keys().copied().collect::<Vec<_>>() {
        let _ = state
            .service
            .unsubscribe_async(tenant_id.clone(), subscription_id)
            .await;
    }
    drop(active_subscriptions);
    drop(pending_subscription_tx);
    drop(subscription_tx);
    drop(outbound_tx);
    tasks.shutdown_and_drain().await;
}
