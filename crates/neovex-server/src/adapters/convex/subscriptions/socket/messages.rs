use super::*;

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
            handle_authenticate(ctx, current_auth, token).await;
        }
        Ok(ConvexClientMessage::ClearAuth) => {
            *current_auth = None;
            let _ = ctx.outbound_tx.send(ServerMessage::Authenticated {
                is_authenticated: false,
            });
        }
        Ok(ConvexClientMessage::Subscribe { request_id, query }) => {
            handle_plain_subscription(ctx, active_subscriptions, request_id, query).await;
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
            let _ = ctx.outbound_tx.send(ServerMessage::Error {
                request_id: None,
                message: format!("invalid websocket message: {error}"),
            });
        }
    }
}

async fn handle_authenticate(
    ctx: &SocketSessionCtx<'_>,
    current_auth: &mut Option<InvocationAuth>,
    token: String,
) {
    match ctx.convex_registry.verify_socket_token(&token).await {
        Ok(auth) => {
            *current_auth = Some(auth);
            crate::state::record_authenticated_usage(ctx.state, current_auth.as_ref()).await;
            let _ = ctx.outbound_tx.send(ServerMessage::Authenticated {
                is_authenticated: true,
            });
        }
        Err(error) => {
            let _ = ctx.outbound_tx.send(ServerMessage::AuthError {
                message: error.to_string(),
            });
        }
    }
}

async fn handle_plain_subscription(
    ctx: &SocketSessionCtx<'_>,
    active_subscriptions: &mut ActiveSubscriptions,
    request_id: String,
    query: Query,
) {
    set_pending_transform(
        ctx.transforms,
        request_id.clone(),
        ConvexSubscriptionTransform::Identity,
    );
    let request_id_for_worker = request_id.clone();
    let service = ctx.state.service.clone();
    let tenant_id = ctx.tenant_id.clone();
    let sender = ctx.subscription_tx.clone();
    match service
        .subscribe_async(tenant_id, query, request_id_for_worker, sender)
        .await
    {
        Ok(subscription_id) => {
            active_subscriptions.insert(subscription_id, vec![subscription_id]);
            activate_transform(
                ctx.transforms,
                subscription_id,
                &request_id,
                ConvexSubscriptionTransform::Identity,
            );
        }
        Err(error) => {
            clear_pending_transform(ctx.transforms, &request_id);
            let _ = ctx.outbound_tx.send(ServerMessage::Error {
                request_id: Some(request_id),
                message: error.to_string(),
            });
        }
    }
}

async fn handle_unsubscribe(
    ctx: &SocketSessionCtx<'_>,
    active_subscriptions: &mut ActiveSubscriptions,
    subscription_id: u64,
) {
    if let Some(underlying_ids) = active_subscriptions.remove(&subscription_id) {
        forwarding::unsubscribe_active_subscriptions(
            &ctx.state.service,
            ctx.tenant_id,
            HashMap::from([(subscription_id, underlying_ids)]),
            ctx.outbound_tx,
            true,
            ctx.transforms,
        )
        .await;
    } else {
        remove_subscription_transform(ctx.transforms, subscription_id);
    }
}
