use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::ws::{WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::response::Response;

use crate::state::{AppError, AppState};

mod negotiation;
mod socket;
mod tenant;

pub(crate) use negotiation::{
    HelloContext, NegotiatedWebSocketProtocol, complete_handshake, configure_upgrade, negotiate,
};
pub(crate) use socket::handle_socket_for_tenant;
use tenant::extract_tenant_id;

/// WebSocket upgrade handler.
pub(crate) async fn ws_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
    ws: WebSocketUpgrade,
) -> Result<Response, AppError> {
    let tenant_id = extract_tenant_id(&headers, params.get("tenant_id").cloned())?;
    let negotiated_protocol = negotiate(&headers)?;
    let service = state.service.clone();
    let tenant_check = tenant_id.clone();
    service.ensure_tenant_exists_async(tenant_check).await?;

    Ok(configure_upgrade(ws).on_upgrade(move |socket| {
        complete_upgraded_socket(socket, state, tenant_id, negotiated_protocol)
    }))
}

async fn complete_upgraded_socket(
    socket: WebSocket,
    state: Arc<AppState>,
    tenant_id: nimbus_core::TenantId,
    negotiated_protocol: negotiation::NegotiatedWebSocketProtocol,
) {
    let Some(socket) =
        complete_handshake(socket, negotiated_protocol, HelloContext::native()).await
    else {
        return;
    };
    handle_socket_for_tenant(socket, state, tenant_id, negotiated_protocol).await;
}
