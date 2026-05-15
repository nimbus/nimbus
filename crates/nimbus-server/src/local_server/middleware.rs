use std::sync::Arc;

use axum::extract::State;
use axum::http::{HeaderMap, Method, Request, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use http::HeaderName;

use super::policy::{
    LOCAL_ADMIN_HEADER_NAME, LocalServerRouteFamily, is_loopback_origin, parse_origin,
};
use super::{
    LOCAL_SESSION_COOKIE_NAME, LocalServerAuditEvent, LocalServerSecurityState,
    SessionValidationResult, origin_from_headers, tenant_id_from_request,
};
use crate::state::{AppError, AppState};

#[derive(Clone)]
pub(crate) struct LocalServerAccessPolicy {
    app_state: Arc<AppState>,
    credential_mode: LocalServerCredentialMode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LocalServerCredentialMode {
    AuthorizationOrAdminHeader,
    AdminHeaderOnly,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ExtractedServerAccess {
    status: ExtractedServerAccessStatus,
    auth_method: Option<&'static str>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum ExtractedServerAccessStatus {
    Authorized,
    Revoked,
    Expired,
    Invalid,
    #[default]
    Missing,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ExtractedCredential {
    auth_method: &'static str,
    value: String,
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
        match self.credential_mode {
            LocalServerCredentialMode::AuthorizationOrAdminHeader => {
                "local admin access requires Authorization: Bearer <token> or X-Nimbus-Admin-Token"
            }
            LocalServerCredentialMode::AdminHeaderOnly => {
                "deploy admin access requires X-Nimbus-Admin-Token in addition to the deploy bearer token"
            }
        }
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
        return error.into_response();
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
            return error.into_response();
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

pub(crate) fn authorize_standard_server_access(
    headers: &HeaderMap,
    local_server_security: Option<&LocalServerSecurityState>,
) -> Result<Option<&'static str>, AppError> {
    if local_server_security.is_none() {
        return Ok(None);
    }
    let extracted = extract_server_access(
        headers,
        LocalServerCredentialMode::AuthorizationOrAdminHeader,
        local_server_security,
    )?;
    match extracted.status {
        ExtractedServerAccessStatus::Authorized => Ok(extracted.auth_method),
        ExtractedServerAccessStatus::Revoked => Err(AppError::unauthorized("auth.token_revoked")),
        ExtractedServerAccessStatus::Expired => Err(AppError::unauthorized("auth.session_expired")),
        ExtractedServerAccessStatus::Invalid | ExtractedServerAccessStatus::Missing => {
            Err(AppError::unauthorized(
                "local admin access requires Authorization: Bearer <token> or X-Nimbus-Admin-Token",
            ))
        }
    }
}

fn extract_server_access(
    headers: &HeaderMap,
    credential_mode: LocalServerCredentialMode,
    local_server_security: Option<&crate::local_server::LocalServerSecurityState>,
) -> Result<ExtractedServerAccess, AppError> {
    let Some(local_server_security) = local_server_security else {
        return Ok(ExtractedServerAccess::default());
    };

    let session_cookie = extract_cookie(headers, LOCAL_SESSION_COOKIE_NAME);
    let session_result = local_server_security.authorize_session_cookie(session_cookie.as_deref());
    if matches!(&session_result, SessionValidationResult::Authorized(_)) {
        return Ok(ExtractedServerAccess {
            status: ExtractedServerAccessStatus::Authorized,
            auth_method: Some("local_session_cookie"),
        });
    }

    let credential = match credential_mode {
        LocalServerCredentialMode::AuthorizationOrAdminHeader => {
            if let Some(token) = extract_admin_header(headers)? {
                Some(token)
            } else {
                extract_bearer_token(headers)?
            }
        }
        LocalServerCredentialMode::AdminHeaderOnly => extract_admin_header(headers)?,
    };
    if credential
        .as_ref()
        .is_some_and(|credential| local_server_security.authorize_bearer(&credential.value))
    {
        return Ok(ExtractedServerAccess {
            status: ExtractedServerAccessStatus::Authorized,
            auth_method: credential.as_ref().map(|credential| credential.auth_method),
        });
    }

    let auth_method = credential
        .as_ref()
        .map(|credential| credential.auth_method)
        .or(match session_result {
            SessionValidationResult::Revoked
            | SessionValidationResult::Expired
            | SessionValidationResult::Invalid => Some("local_session_cookie"),
            SessionValidationResult::Authorized(_) | SessionValidationResult::Missing => None,
        });
    Ok(ExtractedServerAccess {
        status: match (session_result, credential) {
            (SessionValidationResult::Revoked, _) => ExtractedServerAccessStatus::Revoked,
            (SessionValidationResult::Expired, _) => ExtractedServerAccessStatus::Expired,
            (_, Some(_)) | (SessionValidationResult::Invalid, _) => {
                ExtractedServerAccessStatus::Invalid
            }
            _ => ExtractedServerAccessStatus::Missing,
        },
        auth_method,
    })
}

fn extract_admin_header(headers: &HeaderMap) -> Result<Option<ExtractedCredential>, AppError> {
    let header_name = HeaderName::from_static(LOCAL_ADMIN_HEADER_NAME);
    let Some(value) = headers.get(&header_name) else {
        return Ok(None);
    };
    let token = value
        .to_str()
        .map_err(|error| {
            AppError::unauthorized(format!("X-Nimbus-Admin-Token must be valid UTF-8: {error}"))
        })?
        .trim();
    if token.is_empty() {
        return Err(AppError::unauthorized(
            "X-Nimbus-Admin-Token must not be empty",
        ));
    }
    Ok(Some(ExtractedCredential {
        auth_method: "local_admin_header",
        value: token.to_string(),
    }))
}

fn extract_bearer_token(headers: &HeaderMap) -> Result<Option<ExtractedCredential>, AppError> {
    let Some(value) = headers.get(header::AUTHORIZATION) else {
        return Ok(None);
    };
    let value = value.to_str().map_err(|error| {
        AppError::unauthorized(format!("invalid authorization header: {error}"))
    })?;
    let (scheme, token) = value
        .split_once(' ')
        .ok_or_else(|| AppError::unauthorized("authorization header must use the Bearer scheme"))?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return Err(AppError::unauthorized(
            "authorization header must use the Bearer scheme",
        ));
    }
    let token = token.trim();
    if token.is_empty() {
        return Err(AppError::unauthorized(
            "authorization header is missing a token",
        ));
    }
    Ok(Some(ExtractedCredential {
        auth_method: "local_admin_bearer",
        value: token.to_string(),
    }))
}

fn credential_method_hint(
    headers: &HeaderMap,
    credential_mode: LocalServerCredentialMode,
) -> Option<&'static str> {
    if headers.contains_key(HeaderName::from_static(LOCAL_ADMIN_HEADER_NAME)) {
        return Some("local_admin_header");
    }
    if credential_mode == LocalServerCredentialMode::AuthorizationOrAdminHeader
        && headers.contains_key(header::AUTHORIZATION)
    {
        return Some("local_admin_bearer");
    }
    if headers.contains_key(header::COOKIE)
        && extract_cookie(headers, LOCAL_SESSION_COOKIE_NAME).is_some()
    {
        return Some("local_session_cookie");
    }
    None
}

fn extract_cookie(headers: &HeaderMap, cookie_name: &str) -> Option<String> {
    let cookie_header = headers.get(header::COOKIE)?.to_str().ok()?;
    cookie_header
        .split(';')
        .filter_map(|pair| pair.trim().split_once('='))
        .find_map(|(name, value)| (name == cookie_name).then(|| value.to_string()))
}

fn validate_origin(
    route_family: LocalServerRouteFamily,
    expected_port: Option<u16>,
    method: &Method,
    headers: &HeaderMap,
) -> Result<(), AppError> {
    let Some(origin) = headers.get(header::ORIGIN) else {
        return Ok(());
    };
    let parsed =
        parse_origin(origin).ok_or_else(|| AppError::forbidden("origin header is invalid"))?;
    let allowed = match route_family {
        LocalServerRouteFamily::Ui | LocalServerRouteFamily::UiAuthSession => {
            if !is_loopback_origin(parsed, expected_port) {
                false
            } else {
                let Some(host) = headers
                    .get(header::HOST)
                    .and_then(|value| value.to_str().ok())
                else {
                    return Err(AppError::forbidden(
                        "same-origin UI access requires a Host header",
                    ));
                };
                let expected_origin = format!("http://{host}");
                matches!(
                    origin.to_str(),
                    Ok(origin_value) if origin_value.eq_ignore_ascii_case(&expected_origin)
                )
            }
        }
        _ => is_loopback_origin(parsed, expected_port),
    };
    if allowed {
        return Ok(());
    }
    if method == Method::OPTIONS
        && headers
            .get("access-control-request-private-network")
            .is_some_and(|value| value.as_bytes().eq_ignore_ascii_case(b"true"))
    {
        return Err(AppError::forbidden(
            "private network access preflight requires a loopback origin",
        ));
    }
    Err(AppError::forbidden(format!(
        "origin {} is not allowed",
        origin.to_str().unwrap_or("<invalid>")
    )))
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    #[test]
    fn extract_server_access_accepts_bearer_or_admin_header() {
        let temp = tempfile::tempdir().expect("tempdir should build");
        let paths = crate::local_server::LocalServerPaths {
            auth_token_path: temp.path().join("auth").join("token"),
            server_discovery_path: temp.path().join("run").join("server.json"),
            audit_log_path: temp.path().join("logs").join("access.jsonl"),
        };
        let token = crate::local_server::load_or_create_local_admin_token(&paths)
            .expect("token should exist");
        let security =
            crate::local_server::LocalServerSecurityState::new(paths.clone(), token.clone());

        let mut bearer_headers = HeaderMap::new();
        bearer_headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", token.token))
                .expect("authorization header should build"),
        );
        assert!(
            extract_server_access(
                &bearer_headers,
                LocalServerCredentialMode::AuthorizationOrAdminHeader,
                Some(&security),
            )
            .expect("bearer extraction should succeed")
                == ExtractedServerAccess {
                    status: ExtractedServerAccessStatus::Authorized,
                    auth_method: Some("local_admin_bearer"),
                }
        );

        let mut admin_headers = HeaderMap::new();
        admin_headers.insert(
            HeaderName::from_static(LOCAL_ADMIN_HEADER_NAME),
            HeaderValue::from_str(&token.token).expect("admin header should build"),
        );
        assert!(
            extract_server_access(
                &admin_headers,
                LocalServerCredentialMode::AdminHeaderOnly,
                Some(&security),
            )
            .expect("admin header extraction should succeed")
                == ExtractedServerAccess {
                    status: ExtractedServerAccessStatus::Authorized,
                    auth_method: Some("local_admin_header"),
                }
        );
    }

    #[test]
    fn validate_origin_rejects_non_loopback_and_pna_preflights() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("http://example.com"),
        );
        assert!(
            validate_origin(
                LocalServerRouteFamily::NativeApi,
                Some(8080),
                &Method::GET,
                &headers,
            )
            .is_err()
        );

        headers.insert(
            HeaderName::from_static("access-control-request-private-network"),
            HeaderValue::from_static("true"),
        );
        let error = validate_origin(
            LocalServerRouteFamily::NativeApi,
            Some(8080),
            &Method::OPTIONS,
            &headers,
        )
        .expect_err("origin should be rejected");
        assert_eq!(
            error.to_string(),
            "private network access preflight requires a loopback origin"
        );
    }
}
