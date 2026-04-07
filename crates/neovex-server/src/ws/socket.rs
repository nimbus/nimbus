use std::sync::Arc;

use axum::extract::ws::WebSocket;
use futures::StreamExt;
use neovex_engine::{DEFAULT_SUBSCRIPTION_CHANNEL_CAPACITY, SubscriptionUpdate};
use neovex_runtime::HostCallCancellation;
use tokio::sync::mpsc;

use crate::owned_tasks::OwnedTaskSet;
use crate::protocol::ServerMessage;
use crate::state::AppState;

mod pending;
mod session;
mod transport;

pub(crate) async fn handle_socket_for_tenant(
    socket: WebSocket,
    state: Arc<AppState>,
    tenant_id: neovex_core::TenantId,
) {
    const OUTBOUND_CHANNEL_CAPACITY: usize = 256;
    const INBOUND_CHANNEL_CAPACITY: usize = 256;

    let (socket_tx, socket_rx) = socket.split();
    let (outbound_tx, outbound_rx) = mpsc::channel::<ServerMessage>(OUTBOUND_CHANNEL_CAPACITY);
    let (subscription_tx, subscription_rx) =
        mpsc::channel::<SubscriptionUpdate>(DEFAULT_SUBSCRIPTION_CHANNEL_CAPACITY);
    let (inbound_tx, inbound_rx) =
        mpsc::channel::<transport::InboundSocketEvent>(INBOUND_CHANNEL_CAPACITY);
    let (pending_subscription_tx, pending_subscription_rx) =
        mpsc::channel::<session::PendingSubscriptionEvent>(INBOUND_CHANNEL_CAPACITY);
    let disconnect_cancellation = HostCallCancellation::default();
    let pending_bootstrap_cancellations =
        Arc::new(pending::PendingBootstrapCancellationRegistry::default());

    let mut tasks = OwnedTaskSet::new();
    transport::spawn_socket_reader(&mut tasks, socket_rx, inbound_tx);
    transport::spawn_subscription_forwarder(
        &mut tasks,
        subscription_rx,
        outbound_tx.clone(),
        pending_bootstrap_cancellations.clone(),
    );
    transport::spawn_socket_writer(&mut tasks, socket_tx, outbound_rx);

    session::GenericSocketSession {
        state,
        tenant_id,
        inbound_rx,
        pending_subscription_rx,
        outbound_tx,
        subscription_tx,
        pending_subscription_tx,
        disconnect_cancellation,
        pending_bootstrap_cancellations,
    }
    .run(&mut tasks)
    .await;

    tasks.shutdown_and_drain().await;
}
