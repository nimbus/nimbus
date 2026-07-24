use super::registry_auth::socket_admission_for_path;
use super::*;

/// WebSocket endpoint for Convex-style query subscriptions bound to a tenant in the URL.
pub(crate) async fn ws(
    State(state): State<Arc<AppState>>,
    AxumPath(tenant_id): AxumPath<String>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, AppError> {
    let negotiated_protocol = crate::ws::negotiate(&headers)?;
    let service = state.engine.clone();
    let admission = socket_admission_for_path(
        &state,
        crate::local_server::LocalServerRouteFamily::ConvexWebSocket,
        tenant_id,
        &headers,
        "convex websocket route requires Convex support state",
    )
    .await?;
    let tenant_id = admission.tenant_id().clone();
    if !nimbus_system::is_system_tenant_id(&tenant_id) {
        service
            .ensure_tenant_exists_async(tenant_id.clone())
            .await?;
    }

    Ok(
        crate::ws::configure_upgrade(ws).on_upgrade(move |socket| async move {
            let Some(socket) = crate::ws::complete_handshake(
                socket,
                negotiated_protocol,
                crate::ws::HelloContext::convex(),
            )
            .await
            else {
                return;
            };
            handle_convex_socket_for_tenant(socket, state, admission, negotiated_protocol).await;
        }),
    )
}
