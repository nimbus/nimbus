use super::transforms::update_runtime_transform_read_set;
use super::*;

fn spawn_runtime_subscription_bridge(
    convex_subscription_id: u64,
    mut receiver: mpsc::UnboundedReceiver<SubscriptionUpdate>,
    sender: mpsc::UnboundedSender<SubscriptionUpdate>,
) {
    tokio::spawn(async move {
        while let Some(event) = receiver.recv().await {
            match event {
                SubscriptionUpdate::Result { commit: None, .. } => {}
                SubscriptionUpdate::Result {
                    commit,
                    deleted_documents,
                    data,
                    ..
                } => {
                    let _ = sender.send(SubscriptionUpdate::Result {
                        subscription_id: convex_subscription_id,
                        request_id: None,
                        commit,
                        deleted_documents,
                        data,
                    });
                }
                SubscriptionUpdate::Error { message, .. } => {
                    let _ = sender.send(SubscriptionUpdate::Error {
                        subscription_id: convex_subscription_id,
                        request_id: None,
                        message,
                    });
                }
            }
        }
    });
}

pub(super) async fn subscribe_runtime_base_queries(
    service: Arc<neovex_engine::Service>,
    tenant_id: TenantId,
    base_queries: Vec<Query>,
    transform: ConvexSubscriptionTransform,
    transforms: &RwLock<ConvexSubscriptionTransforms>,
    sender: mpsc::UnboundedSender<SubscriptionUpdate>,
) -> Result<ConvexRuntimeSubscriptionHandle, AppError> {
    let mut underlying = Vec::with_capacity(base_queries.len());

    for (index, query) in base_queries.into_iter().enumerate() {
        let (bridge_tx, bridge_rx) = mpsc::unbounded_channel();
        let request_id = format!("convex-runtime-internal-{index}");
        let subscribe_service = service.clone();
        let subscribe_tenant_id = tenant_id.clone();
        match subscribe_service
            .subscribe_async(subscribe_tenant_id, query, request_id, bridge_tx)
            .await
        {
            Ok(subscription_id) => underlying.push((subscription_id, bridge_rx)),
            Err(error) => {
                for (subscription_id, _) in underlying {
                    let cleanup_service = service.clone();
                    let cleanup_tenant_id = tenant_id.clone();
                    let _ = cleanup_service
                        .unsubscribe_async(cleanup_tenant_id, subscription_id)
                        .await;
                }
                return Err(error.into());
            }
        }
    }

    let convex_subscription_id = underlying
        .first()
        .map(|(subscription_id, _)| *subscription_id)
        .expect("runtime base query bootstrap should produce at least one subscription");
    update_runtime_transform_read_set(transforms, convex_subscription_id, transform);

    let mut underlying_subscription_ids = Vec::with_capacity(underlying.len());
    for (subscription_id, receiver) in underlying {
        underlying_subscription_ids.push(subscription_id);
        spawn_runtime_subscription_bridge(convex_subscription_id, receiver, sender.clone());
    }

    Ok(ConvexRuntimeSubscriptionHandle {
        convex_subscription_id,
        underlying_subscription_ids,
    })
}
