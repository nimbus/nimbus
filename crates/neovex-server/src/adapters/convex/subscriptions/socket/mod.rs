use super::transforms::{
    activate_transform, apply_subscription_transform, clear_pending_transform,
    remove_subscription_transform, set_pending_transform, subscription_plan_for_named_query,
    update_runtime_transform_read_set,
};
use super::types::{
    ConvexClientMessage, ConvexSubscriptionTransform, ConvexSubscriptionTransforms,
};
use super::*;
use crate::runtime::subscriptions::subscribe_runtime_base_queries;
use neovex_engine::{SubscriptionCleanupHandle, SubscriptionRegistration};

mod forwarding;
mod messages;
mod named_subscriptions;

#[derive(Debug)]
struct ActiveSubscription {
    cleanup_handles: Vec<SubscriptionCleanupHandle>,
}

impl ActiveSubscription {
    fn from_registration(registration: SubscriptionRegistration) -> Self {
        let (_, cleanup_handle) = registration.into_parts();
        Self {
            cleanup_handles: vec![cleanup_handle],
        }
    }

    fn from_runtime_handle(
        handle: crate::runtime::subscriptions::RuntimeSubscriptionHandle,
    ) -> Self {
        Self {
            cleanup_handles: handle.cleanup_handles,
        }
    }

    fn underlying_ids(&self) -> impl Iterator<Item = u64> + '_ {
        self.cleanup_handles
            .iter()
            .map(SubscriptionCleanupHandle::subscription_id)
    }
}

type ActiveSubscriptions = HashMap<u64, ActiveSubscription>;

pub(super) struct SocketSessionCtx<'a> {
    pub(super) state: &'a Arc<AppState>,
    pub(super) tenant_id: &'a TenantId,
    pub(super) convex_registry: &'a Arc<ConvexRegistry>,
    pub(super) subscription_tx: &'a mpsc::Sender<SubscriptionUpdate>,
    pub(super) outbound_tx: &'a mpsc::Sender<ServerMessage>,
    pub(super) transforms: &'a Arc<RwLock<ConvexSubscriptionTransforms>>,
    pub(super) runtime_cancellation: &'a HostCallCancellation,
}

pub(super) struct NamedSubscriptionRequest {
    pub(super) request_id: String,
    pub(super) name: String,
    pub(super) args: Value,
    pub(super) page_size: Option<usize>,
    pub(super) cursor: Option<String>,
}

pub(super) async fn handle_convex_socket_for_tenant(
    socket: WebSocket,
    state: Arc<AppState>,
    tenant_id: TenantId,
    initial_auth: Option<InvocationAuth>,
) {
    const OUTBOUND_CHANNEL_CAPACITY: usize = 256;

    let (socket_tx, mut socket_rx) = socket.split();
    let (outbound_tx, outbound_rx) = mpsc::channel::<ServerMessage>(OUTBOUND_CHANNEL_CAPACITY);
    let (subscription_tx, subscription_rx) =
        mpsc::channel::<SubscriptionUpdate>(neovex_engine::DEFAULT_SUBSCRIPTION_CHANNEL_CAPACITY);
    let transforms = Arc::new(RwLock::new(ConvexSubscriptionTransforms::default()));
    let runtime_cancellation = HostCallCancellation::default();
    let convex_registry = state
        .convex_registry
        .clone()
        .expect("convex websocket route requires Convex support state");

    let forward_task = forwarding::spawn_subscription_forwarder(
        subscription_rx,
        outbound_tx.clone(),
        transforms.clone(),
        state.service.clone(),
        convex_registry.clone(),
        tenant_id.clone(),
        runtime_cancellation.clone(),
    );
    let send_task = forwarding::spawn_socket_sender(socket_tx, outbound_rx);
    let session_ctx = SocketSessionCtx {
        state: &state,
        tenant_id: &tenant_id,
        convex_registry: &convex_registry,
        subscription_tx: &subscription_tx,
        outbound_tx: &outbound_tx,
        transforms: &transforms,
        runtime_cancellation: &runtime_cancellation,
    };

    let mut active_subscriptions = ActiveSubscriptions::new();
    let mut current_auth = initial_auth;
    while let Some(message_result) = socket_rx.next().await {
        let message = match message_result {
            Ok(message) => message,
            Err(_) => break,
        };

        if !messages::handle_socket_message(
            message,
            &session_ctx,
            &mut current_auth,
            &mut active_subscriptions,
        )
        .await
        {
            break;
        }
    }

    forwarding::drop_active_subscriptions(active_subscriptions, &transforms);
    runtime_cancellation.cancel_due_to_disconnect();
    drop(subscription_tx);
    drop(outbound_tx);
    let _ = forward_task.await;
    let _ = send_task.await;
}
