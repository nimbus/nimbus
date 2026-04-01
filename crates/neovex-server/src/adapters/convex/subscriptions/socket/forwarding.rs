use super::*;

pub(super) fn drop_active_subscriptions(
    active_subscriptions: ActiveSubscriptions,
    transforms: &RwLock<ConvexSubscriptionTransforms>,
) {
    for convex_subscription_id in active_subscriptions.keys().copied() {
        remove_subscription_transform(transforms, convex_subscription_id);
    }
    drop(active_subscriptions);
}

pub(super) async fn unsubscribe_active_subscriptions(
    service: &Arc<neovex_engine::Service>,
    tenant_id: &TenantId,
    active_subscriptions: ActiveSubscriptions,
    outbound_tx: &mpsc::UnboundedSender<ServerMessage>,
    emit_errors: bool,
    transforms: &RwLock<ConvexSubscriptionTransforms>,
) {
    for (convex_subscription_id, active_subscription) in active_subscriptions {
        remove_subscription_transform(transforms, convex_subscription_id);
        for underlying_subscription_id in active_subscription.underlying_ids() {
            let result = service
                .unsubscribe_async(tenant_id.clone(), underlying_subscription_id)
                .await;
            if emit_errors && let Err(error) = result {
                let _ = outbound_tx.send(ServerMessage::Error {
                    request_id: None,
                    message: error.to_string(),
                });
            }
        }
    }
}

pub(super) fn spawn_subscription_forwarder(
    subscription_rx: mpsc::UnboundedReceiver<SubscriptionUpdate>,
    outbound_tx: mpsc::UnboundedSender<ServerMessage>,
    transforms: Arc<RwLock<ConvexSubscriptionTransforms>>,
    service: Arc<neovex_engine::Service>,
    registry: Arc<ConvexRegistry>,
    tenant_id: TenantId,
    runtime_cancellation: HostCallCancellation,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut subscription_rx = subscription_rx;
        while let Some(event) = subscription_rx.recv().await {
            let message = match event {
                SubscriptionUpdate::Result {
                    subscription_id,
                    request_id,
                    commit,
                    deleted_documents,
                    data,
                } => {
                    let request_id_for_transform = request_id.clone();
                    match apply_subscription_transform(
                        &service,
                        &registry,
                        &tenant_id,
                        &transforms,
                        &runtime_cancellation,
                        ConvexSubscriptionEvent {
                            subscription_id,
                            request_id: request_id_for_transform.as_deref(),
                            commit: commit.as_ref(),
                            deleted_documents: &deleted_documents,
                        },
                        data,
                    )
                    .await
                    {
                        Ok(Some(data)) => ServerMessage::SubscriptionResult {
                            subscription_id,
                            request_id,
                            data,
                        },
                        Ok(None) => continue,
                        Err(message) => ServerMessage::Error {
                            request_id,
                            message,
                        },
                    }
                }
                SubscriptionUpdate::Error {
                    request_id,
                    message,
                    ..
                } => ServerMessage::Error {
                    request_id,
                    message,
                },
            };
            let _ = outbound_tx.send(message);
        }
    })
}

pub(super) fn spawn_socket_sender(
    mut socket_tx: futures::stream::SplitSink<WebSocket, Message>,
    mut outbound_rx: mpsc::UnboundedReceiver<ServerMessage>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(message) = outbound_rx.recv().await {
            let Ok(text) = serde_json::to_string(&message) else {
                break;
            };
            if socket_tx.send(Message::Text(text.into())).await.is_err() {
                break;
            }
        }
    })
}
