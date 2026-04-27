use super::*;
use crate::ws::NegotiatedWebSocketProtocol;

pub(super) async fn unsubscribe_active_subscriptions(
    service: &Arc<neovex_engine::Service>,
    tenant_id: &TenantId,
    active_subscriptions: ActiveSubscriptions,
    outbound_tx: &mpsc::Sender<ServerMessage>,
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
                let _ = outbound_tx
                    .send(ServerMessage::session_error(
                        "session.unsubscribe_failed",
                        error.to_string(),
                    ))
                    .await;
            }
        }
        active_subscription.shutdown_and_drain().await;
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn run_subscription_forwarder(
    subscription_rx: mpsc::Receiver<SubscriptionUpdate>,
    outbound_tx: mpsc::Sender<ServerMessage>,
    transforms: Arc<RwLock<ConvexSubscriptionTransforms>>,
    service: Arc<neovex_engine::Service>,
    registry: Arc<ConvexRegistry>,
    runtime_service_registry: Arc<dyn crate::service_registry::RuntimeServiceRegistry>,
    tenant_id: TenantId,
    runtime_cancellation: HostCallCancellation,
) {
    let mut subscription_rx = subscription_rx;
    while let Some(event) = subscription_rx.recv().await {
        let message = match event {
            SubscriptionUpdate::Result {
                subscription_id,
                request_id,
                snapshot,
                commit_hint,
            } => {
                let request_id_for_transform = request_id.clone();
                match apply_subscription_transform(
                    RuntimeTransformContext::new(
                        &service,
                        &registry,
                        &runtime_service_registry,
                        &tenant_id,
                        &transforms,
                        &runtime_cancellation,
                        ConvexSubscriptionEvent {
                            subscription_id,
                            request_id: request_id_for_transform.as_deref(),
                            commit: commit_hint.as_ref(),
                            deleted_documents: &snapshot.deleted_documents,
                        },
                    ),
                    snapshot.to_json_documents(),
                )
                .await
                {
                    Ok(Some(data)) => ServerMessage::SubscriptionResult {
                        subscription_id,
                        request_id,
                        data,
                    },
                    Ok(None) => continue,
                    Err(message) => match request_id {
                        Some(request_id) => {
                            ServerMessage::request_error(request_id, "op.failed", message)
                        }
                        None => ServerMessage::session_error("session.transform_failed", message),
                    },
                }
            }
            SubscriptionUpdate::Error {
                request_id,
                message,
                ..
            } => match request_id {
                Some(request_id) => ServerMessage::request_error(request_id, "op.failed", message),
                None => ServerMessage::session_error("session.subscription_error", message),
            },
        };
        if outbound_tx.send(message).await.is_err() {
            break;
        }
    }
}

pub(super) async fn run_socket_sender(
    mut socket_tx: futures::stream::SplitSink<WebSocket, Message>,
    mut outbound_rx: mpsc::Receiver<ServerMessage>,
    protocol: NegotiatedWebSocketProtocol,
) {
    while let Some(message) = outbound_rx.recv().await {
        let Ok(text) = message.to_text(protocol) else {
            break;
        };
        if socket_tx.send(Message::Text(text.into())).await.is_err() {
            break;
        }
    }
}
