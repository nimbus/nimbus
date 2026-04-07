use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use neovex_core::{Error, Query, TenantId};
use neovex_engine::{
    SubscriptionBootstrapCancellation, SubscriptionCleanupHandle, SubscriptionRegistration,
    SubscriptionUpdate,
};
use neovex_runtime::HostCallCancellation;
use tokio::sync::mpsc;

use crate::owned_tasks::OwnedTaskSet;
use crate::protocol::{ClientMessage, ServerMessage};
use crate::state::AppState;

use super::pending::PendingBootstrapCancellationRegistry;
use super::transport::InboundSocketEvent;

pub(super) enum PendingSubscriptionEvent {
    Registered(SubscriptionRegistration),
    Error { request_id: String, message: String },
}

pub(super) struct GenericSocketSession {
    pub(super) state: Arc<AppState>,
    pub(super) tenant_id: TenantId,
    pub(super) inbound_rx: mpsc::Receiver<InboundSocketEvent>,
    pub(super) pending_subscription_rx: mpsc::Receiver<PendingSubscriptionEvent>,
    pub(super) outbound_tx: mpsc::Sender<ServerMessage>,
    pub(super) subscription_tx: mpsc::Sender<SubscriptionUpdate>,
    pub(super) pending_subscription_tx: mpsc::Sender<PendingSubscriptionEvent>,
    pub(super) disconnect_cancellation: HostCallCancellation,
    pub(super) pending_bootstrap_cancellations: Arc<PendingBootstrapCancellationRegistry>,
}

impl GenericSocketSession {
    pub(super) async fn run(mut self, tasks: &mut OwnedTaskSet) {
        let mut active_subscriptions = HashMap::<u64, SubscriptionCleanupHandle>::new();
        let mut cancelled_pending_subscriptions = HashSet::<u64>::new();
        loop {
            tokio::select! {
                maybe_message = self.inbound_rx.recv() => {
                    let Some(message) = maybe_message else {
                        break;
                    };
                    self.handle_inbound_event(
                        tasks,
                        &mut active_subscriptions,
                        &mut cancelled_pending_subscriptions,
                        message,
                    )
                    .await;
                }
                maybe_pending = self.pending_subscription_rx.recv() => {
                    let Some(pending) = maybe_pending else {
                        continue;
                    };
                    self.handle_pending_event(
                        &mut active_subscriptions,
                        &mut cancelled_pending_subscriptions,
                        pending,
                    )
                    .await;
                }
            }
        }

        self.disconnect_cancellation.cancel_due_to_disconnect();
        self.pending_bootstrap_cancellations.clear();
        for subscription_id in active_subscriptions.keys().copied().collect::<Vec<_>>() {
            let _ = self
                .state
                .service
                .unsubscribe_async(self.tenant_id.clone(), subscription_id)
                .await;
        }
        drop(active_subscriptions);
    }

    async fn handle_inbound_event(
        &self,
        tasks: &mut OwnedTaskSet,
        active_subscriptions: &mut HashMap<u64, SubscriptionCleanupHandle>,
        cancelled_pending_subscriptions: &mut HashSet<u64>,
        message: InboundSocketEvent,
    ) {
        match message {
            InboundSocketEvent::Message(ClientMessage::Authenticate { .. }) => {
                let _ = self
                    .outbound_tx
                    .send(ServerMessage::AuthError {
                        message: "authentication is not supported on the generic websocket route"
                            .to_string(),
                    })
                    .await;
            }
            InboundSocketEvent::Message(ClientMessage::ClearAuth) => {
                let _ = self
                    .outbound_tx
                    .send(ServerMessage::Authenticated {
                        is_authenticated: false,
                    })
                    .await;
            }
            InboundSocketEvent::Message(ClientMessage::Subscribe { request_id, query }) => {
                self.spawn_subscription_registration(tasks, request_id, query);
            }
            InboundSocketEvent::Message(ClientMessage::Unsubscribe { subscription_id }) => {
                self.handle_unsubscribe(
                    active_subscriptions,
                    cancelled_pending_subscriptions,
                    subscription_id,
                )
                .await;
            }
            InboundSocketEvent::Invalid(message) => {
                let _ = self
                    .outbound_tx
                    .send(ServerMessage::Error {
                        request_id: None,
                        message,
                    })
                    .await;
            }
        }
    }

    fn spawn_subscription_registration(
        &self,
        tasks: &mut OwnedTaskSet,
        request_id: String,
        query: Query,
    ) {
        let request_id_for_worker = request_id.clone();
        let service = self.state.service.clone();
        let tenant_id = self.tenant_id.clone();
        let sender = self.subscription_tx.clone();
        let pending_subscription_tx = self.pending_subscription_tx.clone();
        let disconnect_cancellation = self.disconnect_cancellation.clone();
        let subscription_cancellation = HostCallCancellation::default();
        self.pending_bootstrap_cancellations
            .track_request(request_id, subscription_cancellation.clone());
        let pending_bootstrap_cancellations = self.pending_bootstrap_cancellations.clone();
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
                            if disconnect_check.is_cancelled() || subscription_check.is_cancelled()
                            {
                                Err(Error::Cancelled)
                            } else {
                                Ok(())
                            }
                        },
                    ),
                )
                .await;
            pending_bootstrap_cancellations.finish_request(
                &request_id_for_worker,
                result.as_ref().ok().map(SubscriptionRegistration::id),
            );
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

    async fn handle_unsubscribe(
        &self,
        active_subscriptions: &mut HashMap<u64, SubscriptionCleanupHandle>,
        cancelled_pending_subscriptions: &mut HashSet<u64>,
        subscription_id: u64,
    ) {
        let cleanup_handle = active_subscriptions.remove(&subscription_id);
        if cleanup_handle.is_none() {
            cancelled_pending_subscriptions.insert(subscription_id);
            self.pending_bootstrap_cancellations
                .cancel_subscription(subscription_id);
        } else {
            cancelled_pending_subscriptions.remove(&subscription_id);
        }
        if let Err(error) = self
            .state
            .service
            .unsubscribe_async(self.tenant_id.clone(), subscription_id)
            .await
        {
            let _ = self
                .outbound_tx
                .send(ServerMessage::Error {
                    request_id: None,
                    message: error.to_string(),
                })
                .await;
        }
        drop(cleanup_handle);
    }

    async fn handle_pending_event(
        &self,
        active_subscriptions: &mut HashMap<u64, SubscriptionCleanupHandle>,
        cancelled_pending_subscriptions: &mut HashSet<u64>,
        pending: PendingSubscriptionEvent,
    ) {
        match pending {
            PendingSubscriptionEvent::Registered(registration) => {
                let (subscription_id, cleanup_handle) = registration.into_parts();
                if cancelled_pending_subscriptions.remove(&subscription_id) {
                    drop(cleanup_handle);
                    return;
                }
                active_subscriptions.insert(subscription_id, cleanup_handle);
            }
            PendingSubscriptionEvent::Error {
                request_id,
                message,
            } => {
                let _ = self
                    .outbound_tx
                    .send(ServerMessage::Error {
                        request_id: Some(request_id),
                        message,
                    })
                    .await;
            }
        }
    }
}
