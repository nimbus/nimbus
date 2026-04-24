use super::common::registry_and_auth;
use super::*;

/// WebSocket endpoint for Convex-style query subscriptions bound to a tenant in the URL.
pub(crate) async fn ws(
    State(state): State<Arc<AppState>>,
    AxumPath(tenant_id): AxumPath<String>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, AppError> {
    let tenant_id = TenantId::new(tenant_id)?;
    let service = state.service.clone();
    let tenant_check = tenant_id.clone();
    service.ensure_tenant_exists_async(tenant_check).await?;
    let (registry, auth) = registry_and_auth(
        &state,
        crate::local_server::LocalServerRouteFamily::ConvexWebSocket,
        &tenant_id,
        &headers,
        "convex websocket route requires Convex support state",
    )
    .await?;

    Ok(ws.on_upgrade(move |socket| {
        handle_convex_socket_for_tenant(socket, state, registry, tenant_id, auth)
    }))
}
