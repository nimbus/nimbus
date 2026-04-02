use std::sync::Arc;

use neovex_core::{PrincipalContext, Query, TenantId};
use neovex_engine::{
    DEFAULT_SUBSCRIPTION_CHANNEL_CAPACITY, SubscriptionCleanupHandle, SubscriptionUpdate,
};
use tokio::sync::mpsc;

use crate::owned_tasks::OwnedTaskSet;
use crate::state::AppError;

#[derive(Debug)]
pub(crate) struct RuntimeSubscriptionHandle {
    pub(crate) primary_subscription_id: u64,
    pub(crate) cleanup_handles: Vec<SubscriptionCleanupHandle>,
    pub(crate) bridge_tasks: OwnedTaskSet,
}

pub(crate) async fn subscribe_runtime_base_queries(
    service: Arc<neovex_engine::Service>,
    tenant_id: TenantId,
    base_queries: Vec<Query>,
    principal: PrincipalContext,
    sender: mpsc::Sender<SubscriptionUpdate>,
) -> Result<RuntimeSubscriptionHandle, AppError> {
    let mut underlying = Vec::with_capacity(base_queries.len());
    let mut bridge_tasks = OwnedTaskSet::new();

    for (index, query) in base_queries.into_iter().enumerate() {
        let (bridge_tx, bridge_rx) = mpsc::channel(DEFAULT_SUBSCRIPTION_CHANNEL_CAPACITY);
        let request_id = format!("runtime-internal-{index}");
        let subscribe_service = service.clone();
        let subscribe_tenant_id = tenant_id.clone();
        match subscribe_service
            .subscribe_async_with_principal(
                subscribe_tenant_id,
                query,
                principal.clone(),
                request_id,
                bridge_tx,
            )
            .await
        {
            Ok(registration) => underlying.push((registration, bridge_rx)),
            Err(error) => return Err(error.into()),
        }
    }

    let primary_subscription_id = underlying
        .first()
        .map(|(registration, _)| registration.id())
        .expect("runtime base query bootstrap should produce at least one subscription");

    let mut cleanup_handles = Vec::with_capacity(underlying.len());
    for (registration, receiver) in underlying {
        let (_subscription_id, cleanup_handle) = registration.into_parts();
        cleanup_handles.push(cleanup_handle);
        bridge_tasks.spawn({
            let sender = sender.clone();
            async move {
                let mut receiver = receiver;
                while let Some(event) = receiver.recv().await {
                    match event {
                        SubscriptionUpdate::Result { commit: None, .. } => {}
                        SubscriptionUpdate::Result {
                            commit,
                            deleted_documents,
                            data,
                            ..
                        } => {
                            if sender
                                .send(SubscriptionUpdate::Result {
                                    subscription_id: primary_subscription_id,
                                    request_id: None,
                                    commit,
                                    deleted_documents,
                                    data,
                                })
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                        SubscriptionUpdate::Error { message, .. } => {
                            if sender
                                .send(SubscriptionUpdate::Error {
                                    subscription_id: primary_subscription_id,
                                    request_id: None,
                                    message,
                                })
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                    }
                }
            }
        });
    }

    Ok(RuntimeSubscriptionHandle {
        primary_subscription_id,
        cleanup_handles,
        bridge_tasks,
    })
}
