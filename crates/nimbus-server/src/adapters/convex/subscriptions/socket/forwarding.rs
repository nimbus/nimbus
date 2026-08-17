use super::*;
use crate::ws::NegotiatedWebSocketProtocol;

// Subscription results must match the Convex wire format the HTTP routes
// emit — in particular, table-scoped `_id` values — so builtin transforms
// receive documents converted exactly like a direct query response.
fn convex_snapshot_documents(
    snapshot: &nimbus_core::SubscriptionResultSnapshot,
) -> Result<Vec<serde_json::Value>, String> {
    snapshot
        .documents
        .iter()
        .cloned()
        .map(|document| {
            nimbus_convex::document_to_convex_json(document).map_err(|error| error.to_string())
        })
        .collect()
}

pub(super) async fn unsubscribe_active_subscriptions(
    service: &Arc<nimbus_engine::Engine>,
    tenant_id: &TenantId,
    active_subscriptions: ActiveSubscriptions,
    outbound_tx: &mpsc::Sender<ServerMessage>,
    emit_errors: bool,
    transforms: &RwLock<ConvexSubscriptionTransforms>,
    subscription_statuses: &SubscriptionStatuses,
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
        delete_subscription_status(service, subscription_statuses, convex_subscription_id).await;
        active_subscription.shutdown_and_drain().await;
    }
}

// Single call site (spawned task); every param is a distinctly-typed handle
// or channel forwarded directly from the caller's locals with no natural
// sub-concept to bundle them under.
#[allow(clippy::too_many_arguments)]
pub(super) async fn run_subscription_forwarder(
    subscription_rx: mpsc::Receiver<SubscriptionUpdate>,
    outbound_tx: mpsc::Sender<ServerMessage>,
    transforms: Arc<RwLock<ConvexSubscriptionTransforms>>,
    service: Arc<nimbus_engine::Engine>,
    registry: Arc<ConvexRegistry>,
    runtime_service_registry: Arc<dyn nimbus_services::RuntimeServiceRegistry>,
    runtime_manager: Arc<nimbus_compute::runtime_manager::RuntimeManager>,
    service_provisioner: Option<nimbus_compute::ComputeResourceProvisioner>,
    tenant_context: nimbus_tenant::TenantIsolationContext,
    subscription_statuses: SubscriptionStatuses,
    runtime_cancellation: HostCallCancellation,
    tenant_isolation_mode: nimbus_tenant::TenantIsolationMode,
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
                record_subscription_delivery_status(
                    &service,
                    &subscription_statuses,
                    subscription_id,
                )
                .await;
                let request_id_for_transform = request_id.clone();
                let transform_result = match convex_snapshot_documents(&snapshot) {
                    Ok(documents) => {
                        apply_subscription_transform(
                            RuntimeTransformContext {
                                engine: &service,
                                registry: &registry,
                                runtime_service_registry: &runtime_service_registry,
                                runtime_manager: &runtime_manager,
                                service_provisioner: service_provisioner.as_ref(),
                                tenant_context: &tenant_context,
                                transforms: &transforms,
                                runtime_cancellation: &runtime_cancellation,
                                tenant_isolation_mode,
                                event: ConvexSubscriptionEvent {
                                    subscription_id,
                                    request_id: request_id_for_transform.as_deref(),
                                    commit: commit_hint.as_ref(),
                                    deleted_documents: &snapshot.deleted_documents,
                                },
                            },
                            documents,
                        )
                        .await
                    }
                    Err(message) => Err(message),
                };
                match transform_result {
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
                subscription_id,
                request_id,
                message,
            } => {
                record_subscription_error_status(
                    &service,
                    &subscription_statuses,
                    subscription_id,
                    &message,
                )
                .await;
                match request_id {
                    Some(request_id) => {
                        ServerMessage::request_error(request_id, "op.failed", message)
                    }
                    None => ServerMessage::session_error("session.subscription_error", message),
                }
            }
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
