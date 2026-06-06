use std::path::Path;
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::{Html, IntoResponse, Redirect, Response};
use nimbus_assets::ui;
use serde::Deserialize;
use serde_json::json;

use super::{AppError, AppState};
use crate::local_server::{
    IssuedSessionCookie, LOCAL_SESSION_COOKIE_NAME, LocalServerAuditEvent, LocalServerRouteFamily,
    SessionBootstrapFailure, SessionValidationResult, authorize_standard_server_access,
    origin_from_headers,
};

// The SPA shell embeds a single inline <script> in `index.html` that resolves
// the theme synchronously before paint to avoid FOUC. Its SHA-256 is pinned
// in `script-src` so CSP stays strict (no `'unsafe-inline'`) while still
// allowing exactly that script. The `inline_fouc_script_hash_matches_csp`
// test recomputes the hash from the embedded asset and fails the build if
// the script body drifts without a CSP update.
#[cfg(test)]
const UI_INLINE_FOUC_SCRIPT_HASH: &str = "sha256-j4dvfQhOc0aIpLHfDFjtJTFdEOwAQgL/0GWxK/Rbx/0=";
const UI_CSP: &str = concat!(
    "default-src 'self'; ",
    "script-src 'self' 'sha256-j4dvfQhOc0aIpLHfDFjtJTFdEOwAQgL/0GWxK/Rbx/0='; ",
    // `style-src 'unsafe-inline'` is required because the React shell attaches
    // inline `style={...}` attributes throughout (CSS-variable typography
    // hooks, toast positioning, etc.), and Tailwind's runtime-style escape
    // hatches don't emit a stable hash we can pin. Tightening this to
    // `'self'` only would break those inline styles silently. Don't drop it
    // without auditing every inline `style=` attribute and runtime style
    // injector the bundle emits.
    "style-src 'self' 'unsafe-inline'; ",
    "img-src 'self' data:; ",
    "font-src 'self' data:; ",
    "connect-src 'self' ws://127.0.0.1:* ws://localhost:*;",
);

const SPA_INDEX: &str = "index.html";

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
        return match ui::asset(normalized) {
            Some(asset) => Ok(asset_response(normalized, asset.data.into_owned())),
            None => Ok(StatusCode::NOT_FOUND.into_response()),
        };
    }

    serve_spa_shell(&state, &headers)
}

pub(crate) async fn ui_auth() -> Html<String> {
    Html(render_auth_page(None))
}

pub(crate) async fn ui_auth_script() -> Response {
    let mut response = (StatusCode::OK, ui::auth_page_script()).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/javascript; charset=utf-8"),
    );
    response
}

#[derive(Debug, Deserialize)]
pub(crate) struct LaunchTicketQuery {
    lt: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub(crate) struct LaunchTicketMintResponse {
    ticket: String,
    url: String,
}

pub(crate) async fn mint_ui_launch_ticket(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let local_server_security = state.local_server_security.as_ref().ok_or_else(|| {
        AppError::unauthorized(
            "ui launch ticket mint is unavailable because server access auth is not configured",
        )
    })?;
    let origin = origin_from_headers(&headers);
    let auth_method =
        authorize_standard_server_access(&headers, Some(local_server_security.as_ref()))
            .inspect_err(|error| {
                state.record_local_server_audit(LocalServerAuditEvent {
                    route_family: LocalServerRouteFamily::UiAuthSession,
                    tenant_id: None,
                    auth_scope: "launch_ticket_mint",
                    auth_method: None,
                    success: false,
                    origin: origin.clone(),
                    reason: error.to_string(),
                });
            })?;
    let ticket = local_server_security
        .mint_launch_ticket()
        .map_err(|error| {
            AppError::from(nimbus_core::Error::Internal(format!(
                "failed to mint launch ticket: {error}"
            )))
        })?;
    state.record_local_server_audit(LocalServerAuditEvent {
        route_family: LocalServerRouteFamily::UiAuthSession,
        tenant_id: None,
        auth_scope: "launch_ticket_mint",
        auth_method,
        success: true,
        origin,
        reason: format!("launch_ticket.minted prefix={}", short_prefix(&ticket)),
    });
    let url = format!("/ui/launch?lt={ticket}");
    Ok(axum::Json(LaunchTicketMintResponse { ticket, url }).into_response())
}

pub(crate) async fn consume_ui_launch_ticket(
    State(state): State<Arc<AppState>>,
    Query(query): Query<LaunchTicketQuery>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let local_server_security = state.local_server_security.as_ref().ok_or_else(|| {
        AppError::unauthorized(
            "ui launch ticket consume is unavailable because server access auth is not configured",
        )
    })?;
    let origin = origin_from_headers(&headers);
    let Some(ticket) = query.lt.as_deref().filter(|value| !value.is_empty()) else {
        let error = AppError::unauthorized("ui launch endpoint requires `lt=<ticket>` query");
        state.record_local_server_audit(LocalServerAuditEvent {
            route_family: LocalServerRouteFamily::UiAuthSession,
            tenant_id: None,
            auth_scope: "launch_ticket_consume",
            auth_method: Some("launch_ticket"),
            success: false,
            origin,
            reason: error.to_string(),
        });
        return Err(error);
    };
    let issued = local_server_security
        .create_session_for_launch_ticket(ticket)
        .map_err(|error| {
            state.record_local_server_audit(LocalServerAuditEvent {
                route_family: LocalServerRouteFamily::UiAuthSession,
                tenant_id: None,
                auth_scope: "launch_ticket_consume",
                auth_method: Some("launch_ticket"),
                success: false,
                origin: origin.clone(),
                reason: map_session_bootstrap_error(error.clone()).to_string(),
            });
            map_session_bootstrap_error(error)
        })?;
    state.record_local_server_audit(LocalServerAuditEvent {
        route_family: LocalServerRouteFamily::UiAuthSession,
        tenant_id: None,
        auth_scope: "launch_ticket_consume",
        auth_method: Some("launch_ticket"),
        success: true,
        origin,
        reason: format!("session.created prefix={}", short_prefix(ticket)),
    });

    let mut response = Redirect::to("/ui/").into_response();
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

fn short_prefix(ticket: &str) -> &str {
    let visible = ticket.len().min(8);
    &ticket[..visible]
}

fn render_auth_page(error: Option<&str>) -> String {
    let latin_400 = find_embedded_font("jetbrains-mono-latin-400-normal").unwrap_or_default();
    let latin_500 = find_embedded_font("jetbrains-mono-latin-500-normal").unwrap_or_default();
    let (error_block, aria_invalid) = match error {
        Some(message) => (
            format!(
                "<div class=\"error-message\" role=\"alert\"><span class=\"error-icon\" aria-hidden=\"true\">!</span><span>{}</span></div>",
                escape_html(message)
            ),
            " aria-invalid=\"true\"".to_string(),
        ),
        None => (String::new(), String::new()),
    };
    ui::auth_page_template()
        .replace("{{ JETBRAINS_MONO_400 }}", &latin_400)
        .replace("{{ JETBRAINS_MONO_500 }}", &latin_500)
        .replace("{{ NIMBUS_VERSION }}", env!("CARGO_PKG_VERSION"))
        .replace("{{ ERROR_BLOCK }}", &error_block)
        .replace("{{ ARIA_INVALID }}", &aria_invalid)
}

fn auth_page_with_error_response(message: &str) -> Response {
    let body = render_auth_page(Some(message));
    let mut response = (StatusCode::UNAUTHORIZED, Html(body)).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    response
}

fn escape_html(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

fn find_embedded_font(stem: &str) -> Option<String> {
    ui::iter().find_map(|name| {
        let candidate = name.as_ref();
        if candidate.starts_with("assets/")
            && candidate.contains(stem)
            && candidate.ends_with(".woff2")
        {
            Some(candidate.to_string())
        } else {
            None
        }
    })
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
    let prefers_json = headers
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.contains("application/json"));
    let request = parse_ui_auth_session_request(&body)?;
    let origin = origin_from_headers(&headers);
    let record_failure = |error: SessionBootstrapFailure, auth_method: &'static str| -> Response {
        let app_error = map_session_bootstrap_error(error);
        let reason = app_error.to_string();
        state.record_local_server_audit(LocalServerAuditEvent {
            route_family: LocalServerRouteFamily::UiAuthSession,
            tenant_id: None,
            auth_scope: "session",
            auth_method: Some(auth_method),
            success: false,
            origin: origin.clone(),
            reason: reason.clone(),
        });
        if prefers_json {
            app_error.into_response()
        } else {
            auth_page_with_error_response(&reason)
        }
    };
    let (issued, auth_method) = if let Some(token) = request.token.as_deref() {
        match local_server_security.create_session_for_local_admin_token(token) {
            Ok(issued) => (issued, Some("local_admin_token_post")),
            Err(error) => return Ok(record_failure(error, "local_admin_token_post")),
        }
    } else if let Some(launch_ticket) = request.launch_ticket.as_deref() {
        match local_server_security.create_session_for_launch_ticket(launch_ticket) {
            Ok(issued) => (issued, Some("launch_ticket")),
            Err(error) => return Ok(record_failure(error, "launch_ticket")),
        }
    } else {
        let app_error = AppError::unauthorized(
            "ui session bootstrap requires a local admin token or launch ticket in the POST body",
        );
        state.record_local_server_audit(LocalServerAuditEvent {
            route_family: LocalServerRouteFamily::UiAuthSession,
            tenant_id: None,
            auth_scope: "session",
            auth_method: None,
            success: false,
            origin,
            reason: app_error.to_string(),
        });
        return Err(app_error);
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
    match ui::index_html() {
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

    // The single inline <script> in the SPA shell (theme-resolution / FOUC
    // prevention) is allowlisted in `UI_CSP` by its SHA-256. If the script
    // body drifts and the CSP hash isn't updated, the browser blocks the
    // script. This test recomputes the hash from the embedded asset and
    // fails the build if it no longer matches the pinned constant, so the
    // drift is caught at compile-time rather than at runtime in the
    // browser. It also fails the build if a *second* inline script appears
    // in the SPA shell: every inline script needs its own SHA-256 pin in
    // `script-src`, so silently shipping a second one would break the page
    // in production. The scanner tolerates attribute-bearing open tags
    // (`<script type="module">...`) so a future `vite` upgrade that adds
    // attributes to the FOUC script doesn't accidentally skip the check.
    #[test]
    fn inline_fouc_script_hash_matches_csp() {
        use base64::Engine;
        use sha2::{Digest, Sha256};

        let index = ui::index_html().expect("SPA index.html should be embedded at build time");
        let html = std::str::from_utf8(&index.data).expect("SPA index.html should be valid UTF-8");

        let inline_bodies = inline_script_bodies(html);
        assert_eq!(
            inline_bodies.len(),
            1,
            "SPA index.html must contain exactly one inline <script>; \
             found {}. Each inline script needs its own SHA-256 pin in \
             UI_CSP's `script-src`, so add the new hash there (and a sibling \
             const next to UI_INLINE_FOUC_SCRIPT_HASH) before shipping a \
             second one.",
            inline_bodies.len(),
        );

        let body = inline_bodies[0];
        let mut hasher = Sha256::new();
        hasher.update(body.as_bytes());
        let digest = hasher.finalize();
        let encoded = base64::engine::general_purpose::STANDARD.encode(digest);
        let actual = format!("sha256-{encoded}");

        assert_eq!(
            actual, UI_INLINE_FOUC_SCRIPT_HASH,
            "inline FOUC script SHA-256 in dist/index.html has drifted from \
             the CSP pin; update UI_INLINE_FOUC_SCRIPT_HASH and the matching \
             value in UI_CSP",
        );
        assert!(
            UI_CSP.contains(UI_INLINE_FOUC_SCRIPT_HASH),
            "UI_CSP must contain UI_INLINE_FOUC_SCRIPT_HASH so the inline \
             FOUC script is allowlisted",
        );
    }

    fn inline_script_bodies(html: &str) -> Vec<&str> {
        let mut bodies = Vec::new();
        let mut cursor = 0;
        while let Some(rel) = html[cursor..].find("<script") {
            let tag_start = cursor + rel;
            let after_name = tag_start + "<script".len();
            // Skip false matches like `<scripts>` or `<scriptish>`: a real
            // <script> tag is followed by `>`, whitespace, or `/`.
            let is_tag = match html[after_name..].chars().next() {
                Some('>') | Some('/') => true,
                Some(c) if c.is_ascii_whitespace() => true,
                _ => false,
            };
            if !is_tag {
                cursor = after_name;
                continue;
            }
            let open_end = match html[after_name..].find('>') {
                Some(offset) => after_name + offset,
                None => break,
            };
            let open_tag = &html[tag_start..=open_end];
            // External scripts (`<script src="...">`) don't trigger CSP
            // inline-script enforcement; only ones with an inline body do.
            let is_inline = !has_src_attr(open_tag);
            let body_start = open_end + 1;
            let body_end = match html[body_start..].find("</script>") {
                Some(offset) => body_start + offset,
                None => break,
            };
            if is_inline {
                bodies.push(&html[body_start..body_end]);
            }
            cursor = body_end + "</script>".len();
        }
        bodies
    }

    fn has_src_attr(open_tag: &str) -> bool {
        // Cheap attribute scan: split on ASCII whitespace, then each token is
        // either `name` or `name=value`. We treat `src=...` (case-insensitive
        // name) as evidence of an external script reference.
        open_tag
            .trim_start_matches('<')
            .split(|c: char| c.is_ascii_whitespace())
            .any(|token| {
                let name = token.split('=').next().unwrap_or("");
                name.eq_ignore_ascii_case("src")
            })
    }
}
