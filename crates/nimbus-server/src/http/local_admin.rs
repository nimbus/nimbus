use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, header};
use serde::Serialize;
use std::sync::Arc;

use super::{AppError, AppState};
use crate::local_server::{LocalServerAuditEvent, LocalServerRouteFamily, origin_from_headers};

#[derive(Debug, Serialize)]
pub(crate) struct RotateLocalAdminTokenResponse {
    generation: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ShutdownSystemResponse {
    accepted: bool,
}

pub(crate) async fn rotate_local_admin_token(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<RotateLocalAdminTokenResponse>, AppError> {
    let local_server_security = state.local_server_security.as_ref().ok_or_else(|| {
        AppError::unauthorized(
            "local admin token rotation is unavailable because server access auth is not configured",
        )
    })?;
    let bearer = extract_bearer_token(&headers)?;
    if !local_server_security.authorize_bearer(bearer) {
        return Err(AppError::unauthorized("invalid local admin token"));
    }
    let rotation = local_server_security
        .rotate_and_persist_token_with_outcome()
        .map_err(|error| {
            AppError::from(nimbus_core::Error::Internal(format!(
                "failed to rotate local admin token: {error}"
            )))
        })?;
    state.record_local_server_audit(LocalServerAuditEvent {
        route_family: LocalServerRouteFamily::NativeApi,
        tenant_id: None,
        auth_scope: "server_access",
        auth_method: Some("local_admin_bearer"),
        success: true,
        origin: origin_from_headers(&headers),
        reason: "token.rotated".to_string(),
    });
    state.record_local_server_audit(LocalServerAuditEvent {
        route_family: LocalServerRouteFamily::NativeApi,
        tenant_id: None,
        auth_scope: "session",
        auth_method: Some("token_rotation"),
        success: true,
        origin: origin_from_headers(&headers),
        reason: format!("session.invalidated:{}", rotation.invalidated_sessions),
    });
    Ok(Json(RotateLocalAdminTokenResponse {
        generation: rotation.token.generation,
    }))
}

pub(crate) async fn shutdown_system(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<ShutdownSystemResponse>, AppError> {
    if let Err(error) = crate::system_tenant::record_system_event_async(
        &state.service,
        "system",
        "info",
        "lifecycle",
        "server shutdown requested",
        serde_json::json!({
            "listenAddress": state.listen_addr.map(|address| address.to_string()),
        }),
        None,
    )
    .await
    {
        tracing::warn!(%error, "failed to record system shutdown event");
    }
    state.record_local_server_audit(LocalServerAuditEvent {
        route_family: LocalServerRouteFamily::NativeApi,
        tenant_id: None,
        auth_scope: "server_access",
        auth_method: Some("local_admin_bearer_or_session"),
        success: true,
        origin: origin_from_headers(&headers),
        reason: "server.shutdown_requested".to_string(),
    });
    state.request_server_shutdown()?;
    Ok(Json(ShutdownSystemResponse { accepted: true }))
}

fn extract_bearer_token(headers: &HeaderMap) -> Result<&str, AppError> {
    let value = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| {
            AppError::unauthorized("local admin rotation requires Authorization: Bearer <token>")
        })?;
    value.strip_prefix("Bearer ").ok_or_else(|| {
        AppError::unauthorized("local admin rotation requires Authorization: Bearer <token>")
    })
}
