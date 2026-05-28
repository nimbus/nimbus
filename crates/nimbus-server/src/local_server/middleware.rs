use std::sync::Arc;

use axum::extract::State;
use axum::http::Request;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use super::{
    LocalServerAuditEvent, LocalServerRouteFamily, origin_from_headers, tenant_id_from_request,
};
use crate::state::{AppError, AppState};
use nimbus_operator::{
    ExtractedServerAccess, ExtractedServerAccessStatus, LocalServerCredentialMode,
    credential_method_hint, extract_server_access, validate_origin,
};

#[derive(Clone)]
pub(crate) struct LocalServerAccessPolicy {
    app_state: Arc<AppState>,
    credential_mode: LocalServerCredentialMode,
}

impl LocalServerAccessPolicy {
    pub(crate) fn standard(app_state: Arc<AppState>) -> Self {
        Self {
            app_state,
            credential_mode: LocalServerCredentialMode::AuthorizationOrAdminHeader,
        }
    }

    pub(crate) fn deploy(app_state: Arc<AppState>) -> Self {
        Self {
            app_state,
            credential_mode: LocalServerCredentialMode::AdminHeaderOnly,
        }
    }

    fn unauthorized_message(&self) -> &'static str {
        self.credential_mode.unauthorized_message()
    }
}

pub(crate) async fn origin_allowlist_middleware(
    State(state): State<Arc<AppState>>,
    mut request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let path = request.uri().path().to_string();
    let route_family = LocalServerRouteFamily::classify_request(&path, request.headers());
    request.extensions_mut().insert(route_family);
    if route_family.requires_origin_allowlist()
        && let Err(error) = validate_origin(
            route_family,
            state.listen_addr.map(|addr| addr.port()),
            request.method(),
            request.headers(),
        )
    {
        state.record_local_server_audit(LocalServerAuditEvent {
            route_family,
            tenant_id: tenant_id_from_request(&path, request.headers()),
            auth_scope: "origin",
            auth_method: None,
            success: false,
            origin: origin_from_headers(request.headers()),
            reason: error.to_string(),
        });
        return AppError::from(error).into_response();
    }
    next.run(request).await
}

pub(crate) async fn server_access_extract_middleware(
    State(policy): State<LocalServerAccessPolicy>,
    mut request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let route_family = request
        .extensions()
        .get::<LocalServerRouteFamily>()
        .copied()
        .unwrap_or_else(|| {
            LocalServerRouteFamily::classify_request(request.uri().path(), request.headers())
        });
    let tenant_id = tenant_id_from_request(request.uri().path(), request.headers());
    let origin = origin_from_headers(request.headers());
    let extracted = match extract_server_access(
        request.headers(),
        policy.credential_mode,
        policy.app_state.local_server_security.as_deref(),
    ) {
        Ok(extracted) => extracted,
        Err(error) => {
            policy
                .app_state
                .record_local_server_audit(LocalServerAuditEvent {
                    route_family,
                    tenant_id,
                    auth_scope: "server_access",
                    auth_method: credential_method_hint(request.headers(), policy.credential_mode),
                    success: false,
                    origin,
                    reason: error.to_string(),
                });
            return AppError::from(error).into_response();
        }
    };
    request.extensions_mut().insert(extracted);
    next.run(request).await
}

pub(crate) async fn route_family_gate_middleware(
    State(policy): State<LocalServerAccessPolicy>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    if policy.app_state.local_server_security.is_none() {
        return next.run(request).await;
    }
    let extracted = request
        .extensions()
        .get::<ExtractedServerAccess>()
        .copied()
        .unwrap_or_default();
    let route_family = request
        .extensions()
        .get::<LocalServerRouteFamily>()
        .copied()
        .unwrap_or_else(|| {
            LocalServerRouteFamily::classify_request(request.uri().path(), request.headers())
        });
    let tenant_id = tenant_id_from_request(request.uri().path(), request.headers());
    let origin = origin_from_headers(request.headers());
    match extracted.status {
        ExtractedServerAccessStatus::Authorized => {
            policy
                .app_state
                .record_local_server_audit(LocalServerAuditEvent {
                    route_family,
                    tenant_id,
                    auth_scope: "server_access",
                    auth_method: extracted.auth_method,
                    success: true,
                    origin,
                    reason: "authorized".to_string(),
                });
            next.run(request).await
        }
        ExtractedServerAccessStatus::Revoked => {
            policy
                .app_state
                .record_local_server_audit(LocalServerAuditEvent {
                    route_family,
                    tenant_id,
                    auth_scope: "server_access",
                    auth_method: extracted.auth_method,
                    success: false,
                    origin,
                    reason: "auth.token_revoked".to_string(),
                });
            AppError::unauthorized("auth.token_revoked").into_response()
        }
        ExtractedServerAccessStatus::Expired => {
            policy
                .app_state
                .record_local_server_audit(LocalServerAuditEvent {
                    route_family,
                    tenant_id,
                    auth_scope: "server_access",
                    auth_method: extracted.auth_method,
                    success: false,
                    origin,
                    reason: "auth.session_expired".to_string(),
                });
            AppError::unauthorized("auth.session_expired").into_response()
        }
        ExtractedServerAccessStatus::Invalid | ExtractedServerAccessStatus::Missing => {
            policy
                .app_state
                .record_local_server_audit(LocalServerAuditEvent {
                    route_family,
                    tenant_id,
                    auth_scope: "server_access",
                    auth_method: extracted.auth_method,
                    success: false,
                    origin,
                    reason: policy.unauthorized_message().to_string(),
                });
            AppError::unauthorized(policy.unauthorized_message()).into_response()
        }
    }
}
