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
    pending_receivers: Vec<mpsc::Receiver<SubscriptionUpdate>>,
}

impl RuntimeSubscriptionHandle {
    pub(crate) fn underlying_subscription_ids(&self) -> Vec<u64> {
        self.cleanup_handles
            .iter()
            .map(SubscriptionCleanupHandle::subscription_id)
            .collect()
    }

    pub(crate) fn start_forwarding(&mut self, sender: mpsc::Sender<SubscriptionUpdate>) {
        for receiver in self.pending_receivers.drain(..) {
            let primary_subscription_id = self.primary_subscription_id;
            self.bridge_tasks.spawn({
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
    }

    pub(crate) async fn shutdown_and_drain(self) {
        drop(self.cleanup_handles);
        self.bridge_tasks.shutdown_and_drain().await;
    }

    #[cfg(test)]
    pub(crate) fn new_for_testing(
        primary_subscription_id: u64,
        pending_receiver: mpsc::Receiver<SubscriptionUpdate>,
    ) -> Self {
        Self {
            primary_subscription_id,
            cleanup_handles: Vec::new(),
            bridge_tasks: OwnedTaskSet::new(),
            pending_receivers: vec![pending_receiver],
        }
    }
}

pub(crate) async fn subscribe_runtime_base_queries(
    service: Arc<neovex_engine::Service>,
    tenant_id: TenantId,
    base_queries: Vec<Query>,
    principal: PrincipalContext,
) -> Result<RuntimeSubscriptionHandle, AppError> {
    let mut underlying = Vec::with_capacity(base_queries.len());

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
    let mut pending_receivers = Vec::with_capacity(underlying.len());
    for (registration, receiver) in underlying {
        let (_subscription_id, cleanup_handle) = registration.into_parts();
        cleanup_handles.push(cleanup_handle);
        pending_receivers.push(receiver);
    }

    Ok(RuntimeSubscriptionHandle {
        primary_subscription_id,
        cleanup_handles,
        bridge_tasks: OwnedTaskSet::new(),
        pending_receivers,
    })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use neovex_core::{Document, SequenceNumber};
    use serde_json::json;
    use tokio::time::timeout;

    use super::*;

    #[tokio::test]
    async fn runtime_handle_buffers_updates_until_forwarding_starts() {
        let (pending_tx, pending_rx) = mpsc::channel(DEFAULT_SUBSCRIPTION_CHANNEL_CAPACITY);
        let (forwarded_tx, mut forwarded_rx) = mpsc::channel(DEFAULT_SUBSCRIPTION_CHANNEL_CAPACITY);
        let mut handle = RuntimeSubscriptionHandle {
            primary_subscription_id: 42,
            cleanup_handles: Vec::new(),
            bridge_tasks: OwnedTaskSet::new(),
            pending_receivers: vec![pending_rx],
        };

        pending_tx
            .send(SubscriptionUpdate::Result {
                subscription_id: 7,
                request_id: Some("internal".to_string()),
                commit: Some(neovex_core::CommitEntry {
                    sequence: SequenceNumber(9),
                    timestamp: neovex_core::Timestamp(90),
                    writes: Vec::new(),
                }),
                deleted_documents: Vec::<Document>::new(),
                data: vec![json!({"body": "buffered"})],
            })
            .await
            .expect("buffered update should send");

        assert!(
            timeout(Duration::from_millis(50), forwarded_rx.recv())
                .await
                .is_err(),
            "buffered updates should stay local until forwarding starts",
        );

        handle.start_forwarding(forwarded_tx);

        let SubscriptionUpdate::Result {
            subscription_id,
            request_id,
            data,
            ..
        } = timeout(Duration::from_secs(1), forwarded_rx.recv())
            .await
            .expect("forwarded update should arrive after forwarding starts")
            .expect("forwarded channel should stay open")
        else {
            panic!("expected forwarded result update");
        };

        assert_eq!(subscription_id, 42);
        assert_eq!(request_id, None);
        assert_eq!(data, vec![json!({"body": "buffered"})]);

        handle.bridge_tasks.shutdown_and_drain().await;
    }
}
