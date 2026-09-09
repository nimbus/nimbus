use axum::Json;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use std::sync::Arc;

use super::{AppError, AppState};
use crate::local_server::{
    LocalServerAuditEvent, LocalServerRouteFamily, authorize_standard_server_access,
    origin_from_headers,
};
use nimbus_compute::deploy::{DeployHistory, RollbackResponse};

/// Every recorded bundle activation, newest first, for the console's Deploys
/// page. Server-access authorized like the other local admin routes.
pub(crate) async fn list_deploys(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<DeployHistory>, AppError> {
    authorize_standard_server_access(&headers, state.local_server_security().as_deref())?;
    let history = nimbus_compute::deploy::deploy_history(&state).await?;
    Ok(Json(history))
}

/// Re-activates a recorded bundle from its retained artifacts. The artifacts
/// pass the runtime's integrity check before the generation changes; a
/// tampered copy is a 400 and the active generation is unchanged.
pub(crate) async fn rollback_deploy(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(sha256): Path<String>,
) -> Result<Json<RollbackResponse>, AppError> {
    let auth_method =
        authorize_standard_server_access(&headers, state.local_server_security().as_deref())?;
    let actor = match auth_method {
        Some(method) => format!("operator:{method}"),
        None => "operator".to_owned(),
    };
    let response = nimbus_compute::deploy::rollback_deploy(&state, &sha256, &actor).await?;
    state.record_local_server_audit(LocalServerAuditEvent {
        route_family: LocalServerRouteFamily::NativeApi,
        tenant_id: None,
        auth_scope: "server_access",
        auth_method,
        success: true,
        origin: origin_from_headers(&headers),
        reason: format!("deploy.rollback:{sha256}"),
    });
    Ok(Json(response))
}
