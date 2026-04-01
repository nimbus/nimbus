use super::*;

pub(super) async fn handle_named_subscription(
    ctx: &SocketSessionCtx<'_>,
    current_auth: &Option<InvocationAuth>,
    active_subscriptions: &mut ActiveSubscriptions,
    request: NamedSubscriptionRequest,
) {
    let NamedSubscriptionRequest {
        request_id,
        name,
        args,
        page_size,
        cursor,
    } = request;

    if ctx
        .convex_registry
        .runtime_subscription_kind(&name, ConvexFunctionVisibility::Public)
        .is_some()
    {
        let setup = {
            let service = ctx.state.service.clone();
            let registry = ctx.convex_registry.clone();
            let tenant_id_for_worker = ctx.tenant_id.clone();
            let name_for_worker = name.clone();
            let args_for_worker = args.clone();
            let cursor_for_worker = cursor.clone();
            let runtime_cancellation = ctx.runtime_cancellation.clone();
            match bootstrap_runtime_named_subscription_async(
                &service,
                &registry,
                &tenant_id_for_worker,
                &name_for_worker,
                &args_for_worker,
                page_size,
                cursor_for_worker,
                current_auth.clone(),
                runtime_cancellation,
                Some(super::next_runtime_subscription_server_request_id(
                    "convex-ws-subscription-bootstrap",
                )),
            )
            .await
            {
                Ok(result) => result,
                Err(error) => {
                    send_request_error(ctx.outbound_tx, request_id, error.to_string());
                    return;
                }
            }
        };

        let handle = match subscribe_runtime_base_queries(
            ctx.state.service.clone(),
            ctx.tenant_id.clone(),
            setup.base_queries,
            normalize_principal_context(current_auth.as_ref()),
            ctx.subscription_tx.clone(),
        )
        .await
        {
            Ok(handle) => handle,
            Err(error) => {
                send_request_error(ctx.outbound_tx, request_id, error.to_string());
                return;
            }
        };
        let primary_subscription_id = handle.primary_subscription_id;

        update_runtime_transform_read_set(ctx.transforms, primary_subscription_id, setup.transform);
        active_subscriptions.insert(
            primary_subscription_id,
            ActiveSubscription::from_runtime_handle(handle),
        );
        let _ = ctx.outbound_tx.send(ServerMessage::SubscriptionResult {
            subscription_id: primary_subscription_id,
            request_id: Some(request_id),
            data: setup.initial_value,
        });
        return;
    }

    let (base_query, transform) = {
        let query = match ctx.convex_registry.resolve_subscription_query(&name, &args) {
            Ok(query) => query,
            Err(error) => {
                send_request_error(ctx.outbound_tx, request_id, error.to_string());
                return;
            }
        };

        subscription_plan_for_named_query(
            ctx.convex_registry,
            &name,
            &args,
            page_size,
            cursor,
            query,
        )
    };
    set_pending_transform(ctx.transforms, request_id.clone(), transform.clone());
    let request_id_for_worker = request_id.clone();
    let service = ctx.state.service.clone();
    let tenant_id = ctx.tenant_id.clone();
    let sender = ctx.subscription_tx.clone();
    let principal = normalize_principal_context(current_auth.as_ref());
    match service
        .subscribe_async_with_principal(tenant_id, base_query, principal, request_id_for_worker, sender)
        .await
    {
        Ok(registration) => {
            let subscription_id = registration.id();
            active_subscriptions.insert(
                subscription_id,
                ActiveSubscription::from_registration(registration),
            );
            activate_transform(ctx.transforms, subscription_id, &request_id, transform);
        }
        Err(error) => {
            clear_pending_transform(ctx.transforms, &request_id);
            send_request_error(ctx.outbound_tx, request_id, error.to_string());
        }
    }
}

fn send_request_error(
    outbound_tx: &mpsc::UnboundedSender<ServerMessage>,
    request_id: String,
    message: String,
) {
    let _ = outbound_tx.send(ServerMessage::Error {
        request_id: Some(request_id),
        message,
    });
}
