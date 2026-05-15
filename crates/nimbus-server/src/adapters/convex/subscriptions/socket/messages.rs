use super::*;
use crate::application_auth::normalize_principal_context;

pub(super) async fn handle_socket_message(
    message: Message,
    ctx: &SocketSessionCtx<'_>,
    current_auth: &mut Option<InvocationAuth>,
    active_subscriptions: &mut ActiveSubscriptions,
) -> bool {
    match message {
        Message::Text(text) => {
            handle_text_message(&text, ctx, current_auth, active_subscriptions).await;
            true
        }
        Message::Close(_) => false,
        Message::Ping(_) | Message::Pong(_) | Message::Binary(_) => true,
    }
}

async fn handle_text_message(
    text: &str,
    ctx: &SocketSessionCtx<'_>,
    current_auth: &mut Option<InvocationAuth>,
    active_subscriptions: &mut ActiveSubscriptions,
) {
    match serde_json::from_str::<ConvexClientMessage>(text) {
        Ok(ConvexClientMessage::Authenticate { token }) => {
            handle_authenticate(ctx, current_auth, active_subscriptions, token).await;
        }
        Ok(ConvexClientMessage::ClearAuth) => {
            reset_active_subscriptions_for_auth_change(ctx, active_subscriptions).await;
            *current_auth = None;
            let _ = ctx
                .outbound_tx
                .send(ServerMessage::Authenticated {
                    is_authenticated: false,
                })
                .await;
        }
        Ok(ConvexClientMessage::Subscribe { request_id, query }) => {
            handle_plain_subscription(ctx, current_auth, active_subscriptions, request_id, query)
                .await;
        }
        Ok(ConvexClientMessage::SubscribeNamed {
            request_id,
            name,
            args,
            page_size,
            cursor,
        }) => {
            named_subscriptions::handle_named_subscription(
                ctx,
                current_auth,
                active_subscriptions,
                NamedSubscriptionRequest {
                    request_id,
                    name,
                    args,
                    page_size,
                    cursor,
                },
            )
            .await;
        }
        Ok(ConvexClientMessage::Unsubscribe { subscription_id }) => {
            handle_unsubscribe(ctx, active_subscriptions, subscription_id).await;
        }
        Err(error) => {
            let _ = ctx
                .outbound_tx
                .send(ServerMessage::session_error(
                    "protocol.invalid_json",
                    format!("invalid websocket message: {error}"),
                ))
                .await;
        }
    }
}

async fn handle_authenticate(
    ctx: &SocketSessionCtx<'_>,
    current_auth: &mut Option<InvocationAuth>,
    active_subscriptions: &mut ActiveSubscriptions,
    token: String,
) {
    match ctx.convex_registry.verify_socket_token(&token).await {
        Ok(auth) => {
            reset_active_subscriptions_for_auth_change(ctx, active_subscriptions).await;
            *current_auth = Some(auth);
            crate::state::record_authenticated_usage(ctx.state, current_auth.as_ref()).await;
            let _ = ctx
                .outbound_tx
                .send(ServerMessage::Authenticated {
                    is_authenticated: true,
                })
                .await;
        }
        Err(error) => {
            let _ = ctx
                .outbound_tx
                .send(ServerMessage::auth_error(error.to_string()))
                .await;
        }
    }
}

async fn handle_plain_subscription(
    ctx: &SocketSessionCtx<'_>,
    current_auth: &Option<InvocationAuth>,
    active_subscriptions: &mut ActiveSubscriptions,
    request_id: String,
    query: Query,
) {
    set_pending_transform(
        ctx.transforms,
        request_id.clone(),
        ConvexSubscriptionTransform::Identity,
    );
    let query_key = plain_subscription_query_key(&query);
    let request_id_for_worker = request_id.clone();
    let service = ctx.state.service.clone();
    let tenant_id = ctx.tenant_id.clone();
    let sender = ctx.subscription_tx.clone();
    let principal = normalize_principal_context(current_auth.as_ref());
    match service
        .subscribe_async_with_principal(tenant_id, query, principal, request_id_for_worker, sender)
        .await
    {
        Ok(registration) => {
            let subscription_id = registration.id();
            active_subscriptions.insert(
                subscription_id,
                ActiveSubscription::from_registration(registration),
            );
            activate_transform(
                ctx.transforms,
                subscription_id,
                &request_id,
                ConvexSubscriptionTransform::Identity,
            );
            record_active_subscription_status(ctx, subscription_id, query_key).await;
        }
        Err(error) => {
            clear_pending_transform(ctx.transforms, &request_id);
            let _ = ctx
                .outbound_tx
                .send(ServerMessage::request_error(
                    request_id,
                    "op.failed",
                    error.to_string(),
                ))
                .await;
        }
    }
}

async fn reset_active_subscriptions_for_auth_change(
    ctx: &SocketSessionCtx<'_>,
    active_subscriptions: &mut ActiveSubscriptions,
) {
    if active_subscriptions.is_empty() {
        return;
    }

    let to_unsubscribe = std::mem::take(active_subscriptions);
    forwarding::unsubscribe_active_subscriptions(
        &ctx.state.service,
        ctx.tenant_id,
        to_unsubscribe,
        ctx.outbound_tx,
        false,
        ctx.transforms,
        ctx.subscription_statuses,
    )
    .await;
    let _ = ctx
        .outbound_tx
        .send(ServerMessage::session_warning(
            "session.auth_context_changed",
            "authentication context changed; resubscribe active subscriptions",
        ))
        .await;
}

async fn handle_unsubscribe(
    ctx: &SocketSessionCtx<'_>,
    active_subscriptions: &mut ActiveSubscriptions,
    subscription_id: u64,
) {
    if let Some(active_subscription) = active_subscriptions.remove(&subscription_id) {
        forwarding::unsubscribe_active_subscriptions(
            &ctx.state.service,
            ctx.tenant_id,
            HashMap::from([(subscription_id, active_subscription)]),
            ctx.outbound_tx,
            true,
            ctx.transforms,
            ctx.subscription_statuses,
        )
        .await;
    } else {
        remove_subscription_transform(ctx.transforms, subscription_id);
    }
}
