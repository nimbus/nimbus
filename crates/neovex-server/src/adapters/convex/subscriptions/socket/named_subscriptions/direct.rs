use super::*;
use crate::application_auth::normalize_principal_context;

pub(super) async fn handle_direct_named_subscription(
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

    let (base_query, transform) = {
        let query = match ctx.convex_registry.resolve_subscription_query(&name, &args) {
            Ok(query) => query,
            Err(error) => {
                super::send_request_error(ctx.outbound_tx, request_id, error.to_string()).await;
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
        .subscribe_async_with_principal(
            tenant_id,
            base_query,
            principal,
            request_id_for_worker,
            sender,
        )
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
            super::send_request_error(ctx.outbound_tx, request_id, error.to_string()).await;
        }
    }
}
