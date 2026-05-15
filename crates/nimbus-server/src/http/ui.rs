use std::path::Path;
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path as AxumPath, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::{Html, IntoResponse, Redirect, Response};
use rust_embed::Embed;
use serde::Deserialize;
use serde_json::json;

use super::{AppError, AppState};
use crate::local_server::{
    IssuedSessionCookie, LOCAL_SESSION_COOKIE_NAME, LocalServerAuditEvent, LocalServerRouteFamily,
    SessionBootstrapFailure, SessionValidationResult, origin_from_headers,
};

const UI_CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self' data:; connect-src 'self' ws://127.0.0.1:* ws://localhost:*;";

const SPA_INDEX: &str = "index.html";

#[derive(Embed)]
#[folder = "$CARGO_MANIFEST_DIR/../../packages/nimbus-ui/dist/"]
struct UiAssets;

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UiAuthSessionRequest {
    token: Option<String>,
    launch_ticket: Option<String>,
}

pub(crate) async fn ui_root(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    serve_spa_shell(&state, &headers)
}

pub(crate) async fn ui_path(
    State(state): State<Arc<AppState>>,
    AxumPath(path): AxumPath<String>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let normalized = path.trim_start_matches('/');

    if looks_like_static_asset(normalized) {
        return match UiAssets::get(normalized) {
            Some(asset) => Ok(asset_response(normalized, asset.data.into_owned())),
            None => Ok(StatusCode::NOT_FOUND.into_response()),
        };
    }

    serve_spa_shell(&state, &headers)
}

pub(crate) async fn ui_auth() -> Html<&'static str> {
    Html(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Nimbus Sign In</title></head><body><main><h1>Nimbus</h1><form method=\"post\" action=\"/ui/auth/session\"><label>Local admin token <input type=\"password\" name=\"token\" autocomplete=\"off\" autofocus /></label><button type=\"submit\">Continue</button></form></main></body></html>",
    )
}

pub(crate) async fn create_ui_session(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AppError> {
    let local_server_security = state.local_server_security.as_ref().ok_or_else(|| {
        AppError::unauthorized(
            "ui session bootstrap is unavailable because server access auth is not configured",
        )
    })?;
    let request = parse_ui_auth_session_request(&body)?;
    let origin = origin_from_headers(&headers);
    let (issued, auth_method) = if let Some(token) = request.token.as_deref() {
        let issued = local_server_security
            .create_session_for_local_admin_token(token)
            .map_err(|error| {
                state.record_local_server_audit(LocalServerAuditEvent {
                    route_family: LocalServerRouteFamily::UiAuthSession,
                    tenant_id: None,
                    auth_scope: "session",
                    auth_method: Some("local_admin_token_post"),
                    success: false,
                    origin: origin.clone(),
                    reason: map_session_bootstrap_error(error.clone()).to_string(),
                });
                map_session_bootstrap_error(error)
            })?;
        (issued, Some("local_admin_token_post"))
    } else if let Some(launch_ticket) = request.launch_ticket.as_deref() {
        let issued = local_server_security
            .create_session_for_launch_ticket(launch_ticket)
            .map_err(|error| {
                state.record_local_server_audit(LocalServerAuditEvent {
                    route_family: LocalServerRouteFamily::UiAuthSession,
                    tenant_id: None,
                    auth_scope: "session",
                    auth_method: Some("launch_ticket"),
                    success: false,
                    origin: origin.clone(),
                    reason: map_session_bootstrap_error(error.clone()).to_string(),
                });
                map_session_bootstrap_error(error)
            })?;
        (issued, Some("launch_ticket"))
    } else {
        let error = AppError::unauthorized(
            "ui session bootstrap requires a local admin token or launch ticket in the POST body",
        );
        state.record_local_server_audit(LocalServerAuditEvent {
            route_family: LocalServerRouteFamily::UiAuthSession,
            tenant_id: None,
            auth_scope: "session",
            auth_method: None,
            success: false,
            origin,
            reason: error.to_string(),
        });
        return Err(error);
    };
    state.record_local_server_audit(LocalServerAuditEvent {
        route_family: LocalServerRouteFamily::UiAuthSession,
        tenant_id: None,
        auth_scope: "session",
        auth_method,
        success: true,
        origin,
        reason: "session.created".to_string(),
    });

    let accept = headers
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok());
    let mut response = if accept.is_some_and(|value| value.contains("application/json")) {
        axum::Json(json!({ "ok": true })).into_response()
    } else {
        Html(
            "<!doctype html><html><head><meta charset=\"utf-8\"><title>Nimbus Ready</title></head><body><main><p>Session created.</p><p><a href=\"/ui/\">Open Nimbus UI</a></p></main></body></html>",
        )
        .into_response()
    };
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_str(&build_session_set_cookie(&issued)).map_err(|error| {
            AppError::from(nimbus_core::Error::Internal(format!(
                "failed to encode session cookie: {error}"
            )))
        })?,
    );
    Ok(response)
}

pub(crate) async fn ui_csp_middleware(
    request: axum::http::Request<axum::body::Body>,
    next: Next,
) -> Response {
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(UI_CSP),
    );
    response
}

fn serve_spa_shell(state: &Arc<AppState>, headers: &HeaderMap) -> Result<Response, AppError> {
    let local_server_security = state.local_server_security.as_ref().ok_or_else(|| {
        AppError::unauthorized(
            "ui session bootstrap is unavailable because server access auth is not configured",
        )
    })?;
    match local_server_security.authorize_session_cookie(extract_session_cookie(headers).as_deref())
    {
        SessionValidationResult::Authorized(_) => Ok(spa_index_response()),
        SessionValidationResult::Missing => Ok(Redirect::temporary("/ui/auth").into_response()),
        SessionValidationResult::Revoked => Err(AppError::unauthorized("auth.token_revoked")),
        SessionValidationResult::Expired => Err(AppError::unauthorized("auth.session_expired")),
        SessionValidationResult::Invalid => {
            Err(AppError::unauthorized("invalid ui session cookie"))
        }
    }
}

fn spa_index_response() -> Response {
    match UiAssets::get(SPA_INDEX) {
        Some(asset) => asset_response(SPA_INDEX, asset.data.into_owned()),
        None => Html(BOOTSTRAP_FALLBACK_HTML).into_response(),
    }
}

fn asset_response(path: &str, bytes: Vec<u8>) -> Response {
    let mime = guess_content_type(path);
    let mut response = (StatusCode::OK, bytes).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(mime).unwrap_or_else(|_| HeaderValue::from_static("text/plain")),
    );
    response
}

fn looks_like_static_asset(path: &str) -> bool {
    if path.is_empty() {
        return false;
    }
    let last_segment = match path.rsplit('/').next() {
        Some(segment) => segment,
        None => return false,
    };
    last_segment.contains('.')
}

fn guess_content_type(path: &str) -> &'static str {
    let extension = Path::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match extension.as_str() {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "application/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "map" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "txt" => "text/plain; charset=utf-8",
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
}

const BOOTSTRAP_FALLBACK_HTML: &str = "<!doctype html><html><head><meta charset=\"utf-8\"><title>Nimbus UI</title></head><body><main><h1>Nimbus UI</h1><p>Embedded UI assets are missing. Run <code>make build-ui</code>.</p></main></body></html>";

fn extract_session_cookie(headers: &HeaderMap) -> Option<String> {
    let cookie_header = headers.get(header::COOKIE)?.to_str().ok()?;
    cookie_header
        .split(';')
        .filter_map(|pair| pair.trim().split_once('='))
        .find_map(|(name, value)| (name == LOCAL_SESSION_COOKIE_NAME).then(|| value.to_string()))
}

fn build_session_set_cookie(issued: &IssuedSessionCookie) -> String {
    let max_age = (issued.expires_at - issued.issued_at)
        .whole_seconds()
        .max(0);
    format!(
        "{LOCAL_SESSION_COOKIE_NAME}={}; HttpOnly; SameSite=Strict; Path=/; Max-Age={max_age}",
        issued.value
    )
}

fn map_session_bootstrap_error(error: SessionBootstrapFailure) -> AppError {
    match error {
        SessionBootstrapFailure::InvalidToken => {
            AppError::unauthorized("invalid local admin token")
        }
        SessionBootstrapFailure::InvalidLaunchTicket => {
            AppError::unauthorized("invalid or expired launch ticket")
        }
    }
}

fn parse_ui_auth_session_request(body: &[u8]) -> Result<UiAuthSessionRequest, AppError> {
    if body.is_empty() {
        return Ok(UiAuthSessionRequest::default());
    }
    if body.starts_with(b"{") {
        return serde_json::from_slice::<UiAuthSessionRequest>(body).map_err(|error| {
            AppError::from(nimbus_core::Error::InvalidInput(format!(
                "ui session request body is not valid JSON: {error}"
            )))
        });
    }

    let body = std::str::from_utf8(body).map_err(|error| {
        AppError::from(nimbus_core::Error::InvalidInput(format!(
            "ui session request body must be UTF-8: {error}"
        )))
    })?;
    let mut request = UiAuthSessionRequest::default();
    for pair in body.split('&').filter(|pair| !pair.is_empty()) {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        let value = value.replace('+', " ");
        match name {
            "token" => request.token = Some(value),
            "launchTicket" | "launch_ticket" => request.launch_ticket = Some(value),
            _ => {}
        }
    }
    Ok(request)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_shaped_paths_are_recognized_by_filename_extension() {
        assert!(looks_like_static_asset("assets/index-abc.js"));
        assert!(looks_like_static_asset("favicon.ico"));
        assert!(looks_like_static_asset("logo.svg"));
        assert!(looks_like_static_asset("nested/deep/file.css"));
    }

    #[test]
    fn route_shaped_paths_are_not_static_assets() {
        assert!(!looks_like_static_asset("machines"));
        assert!(!looks_like_static_asset("storage/tenants/demo"));
        assert!(!looks_like_static_asset(""));
        assert!(!looks_like_static_asset("settings/"));
    }

    #[test]
    fn content_type_covers_vite_outputs() {
        assert_eq!(
            guess_content_type("assets/index-abc.js"),
            "application/javascript; charset=utf-8"
        );
        assert_eq!(
            guess_content_type("assets/style.css"),
            "text/css; charset=utf-8"
        );
        assert_eq!(guess_content_type("index.html"), "text/html; charset=utf-8");
        assert_eq!(
            guess_content_type("unknown.xyz"),
            "application/octet-stream"
        );
    }
}
