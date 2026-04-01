use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::response::Response;

use crate::state::{AppError, AppState};

mod socket;
mod tenant;

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
    let service = state.service.clone();
    let tenant_check = tenant_id.clone();
    service.ensure_tenant_exists_async(tenant_check).await?;

    Ok(ws.on_upgrade(move |socket| handle_socket_for_tenant(socket, state, tenant_id)))
}
