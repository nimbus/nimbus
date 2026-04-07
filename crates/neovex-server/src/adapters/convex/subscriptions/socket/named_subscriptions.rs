use super::*;

mod direct;
mod runtime;

pub(super) async fn handle_named_subscription(
    ctx: &SocketSessionCtx<'_>,
    current_auth: &Option<InvocationAuth>,
    active_subscriptions: &mut ActiveSubscriptions,
    request: NamedSubscriptionRequest,
) {
    if ctx
        .convex_registry
        .runtime_subscription_kind(&request.name, ConvexFunctionVisibility::Public)
        .is_some()
    {
        runtime::handle_runtime_named_subscription(
            ctx,
            current_auth,
            active_subscriptions,
            request,
        )
        .await;
        return;
    }

    direct::handle_direct_named_subscription(ctx, current_auth, active_subscriptions, request)
        .await;
}

pub(super) async fn send_request_error(
    outbound_tx: &mpsc::Sender<ServerMessage>,
    request_id: String,
    message: String,
) {
    let _ = outbound_tx
        .send(ServerMessage::Error {
            request_id: Some(request_id),
            message,
        })
        .await;
}
