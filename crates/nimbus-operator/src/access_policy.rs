use http::{HeaderMap, HeaderName, Method, header};

use super::access::{LOCAL_SESSION_COOKIE_NAME, LocalServerSecurityState, SessionValidationResult};
use super::policy::{
    LOCAL_ADMIN_HEADER_NAME, LocalServerRouteFamily, is_loopback_origin, parse_origin,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalServerCredentialMode {
    AuthorizationOrAdminHeader,
    AdminHeaderOnly,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ExtractedServerAccess {
    pub status: ExtractedServerAccessStatus,
    pub auth_method: Option<&'static str>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ExtractedServerAccessStatus {
    Authorized,
    Revoked,
    Expired,
    Invalid,
    #[default]
    Missing,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalServerPolicyError {
    kind: LocalServerPolicyErrorKind,
    message: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LocalServerPolicyErrorKind {
    Unauthorized,
    Forbidden,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ExtractedCredential {
    auth_method: &'static str,
    value: String,
}

impl LocalServerCredentialMode {
    pub fn unauthorized_message(self) -> &'static str {
        match self {
            LocalServerCredentialMode::AuthorizationOrAdminHeader => {
                "local admin access requires Authorization: Bearer <token> or X-Nimbus-Admin-Token"
            }
            LocalServerCredentialMode::AdminHeaderOnly => {
                "deploy admin access requires X-Nimbus-Admin-Token in addition to the deploy bearer token"
            }
        }
    }
}

impl LocalServerPolicyError {
    fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            kind: LocalServerPolicyErrorKind::Unauthorized,
            message: message.into(),
        }
    }

    fn forbidden(message: impl Into<String>) -> Self {
        Self {
            kind: LocalServerPolicyErrorKind::Forbidden,
            message: message.into(),
        }
    }

    pub fn is_forbidden(&self) -> bool {
        self.kind == LocalServerPolicyErrorKind::Forbidden
    }

    pub fn into_message(self) -> String {
        self.message
    }
}

impl std::fmt::Display for LocalServerPolicyError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for LocalServerPolicyError {}

pub fn authorize_deploy_admin_bearer(
    expected: Option<&str>,
    headers: &HeaderMap,
) -> Result<(), LocalServerPolicyError> {
    let Some(expected) = expected else {
        return Err(LocalServerPolicyError::unauthorized(
            "deploy admin API is disabled; set NIMBUS_DEPLOY_TOKEN before starting the server",
        ));
    };
    let token = extract_required_bearer_token(
        headers,
        "deploy admin API requires Authorization: Bearer <token>",
    )?;
    if token != expected {
        return Err(LocalServerPolicyError::unauthorized(
            "invalid deploy admin token",
        ));
    }
    Ok(())
}

pub fn authorize_standard_server_access(
    headers: &HeaderMap,
    local_server_security: Option<&LocalServerSecurityState>,
) -> Result<Option<&'static str>, LocalServerPolicyError> {
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
        ExtractedServerAccessStatus::Revoked => {
            Err(LocalServerPolicyError::unauthorized("auth.token_revoked"))
        }
        ExtractedServerAccessStatus::Expired => {
            Err(LocalServerPolicyError::unauthorized("auth.session_expired"))
        }
        ExtractedServerAccessStatus::Invalid | ExtractedServerAccessStatus::Missing => {
            Err(LocalServerPolicyError::unauthorized(
                LocalServerCredentialMode::AuthorizationOrAdminHeader.unauthorized_message(),
            ))
        }
    }
}

pub fn extract_server_access(
    headers: &HeaderMap,
    credential_mode: LocalServerCredentialMode,
    local_server_security: Option<&LocalServerSecurityState>,
) -> Result<ExtractedServerAccess, LocalServerPolicyError> {
    let Some(local_server_security) = local_server_security else {
        return Ok(ExtractedServerAccess::default());
    };

    let session_result = if credential_mode == LocalServerCredentialMode::AuthorizationOrAdminHeader
    {
        let session_cookie = extract_cookie(headers, LOCAL_SESSION_COOKIE_NAME);
        local_server_security.authorize_session_cookie(session_cookie.as_deref())
    } else {
        SessionValidationResult::Missing
    };
    if credential_mode == LocalServerCredentialMode::AuthorizationOrAdminHeader
        && matches!(&session_result, SessionValidationResult::Authorized(_))
    {
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
                extract_bearer_credential(headers)?
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

pub fn credential_method_hint(
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
    if credential_mode == LocalServerCredentialMode::AuthorizationOrAdminHeader
        && headers.contains_key(header::COOKIE)
        && extract_cookie(headers, LOCAL_SESSION_COOKIE_NAME).is_some()
    {
        return Some("local_session_cookie");
    }
    None
}

pub fn extract_required_bearer_token<'a>(
    headers: &'a HeaderMap,
    missing_or_invalid_message: &'static str,
) -> Result<&'a str, LocalServerPolicyError> {
    let value = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| LocalServerPolicyError::unauthorized(missing_or_invalid_message))?;
    bearer_token_from_value(value)
        .map_err(|_| LocalServerPolicyError::unauthorized(missing_or_invalid_message))
}

pub fn validate_origin(
    route_family: LocalServerRouteFamily,
    expected_port: Option<u16>,
    method: &Method,
    headers: &HeaderMap,
) -> Result<(), LocalServerPolicyError> {
    let Some(origin) = headers.get(header::ORIGIN) else {
        return Ok(());
    };
    let parsed = parse_origin(origin)
        .ok_or_else(|| LocalServerPolicyError::forbidden("origin header is invalid"))?;
    let allowed = match route_family {
        LocalServerRouteFamily::Ui | LocalServerRouteFamily::UiAuthSession => {
            if !is_loopback_origin(parsed, expected_port) {
                false
            } else {
                let Some(host) = headers
                    .get(header::HOST)
                    .and_then(|value| value.to_str().ok())
                else {
                    return Err(LocalServerPolicyError::forbidden(
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
        return Err(LocalServerPolicyError::forbidden(
            "private network access preflight requires a loopback origin",
        ));
    }
    Err(LocalServerPolicyError::forbidden(format!(
        "origin {} is not allowed",
        origin.to_str().unwrap_or("<invalid>")
    )))
}

fn extract_admin_header(
    headers: &HeaderMap,
) -> Result<Option<ExtractedCredential>, LocalServerPolicyError> {
    let header_name = HeaderName::from_static(LOCAL_ADMIN_HEADER_NAME);
    let Some(value) = headers.get(&header_name) else {
        return Ok(None);
    };
    let token = value
        .to_str()
        .map_err(|error| {
            LocalServerPolicyError::unauthorized(format!(
                "X-Nimbus-Admin-Token must be valid UTF-8: {error}"
            ))
        })?
        .trim();
    if token.is_empty() {
        return Err(LocalServerPolicyError::unauthorized(
            "X-Nimbus-Admin-Token must not be empty",
        ));
    }
    Ok(Some(ExtractedCredential {
        auth_method: "local_admin_header",
        value: token.to_string(),
    }))
}

fn extract_bearer_credential(
    headers: &HeaderMap,
) -> Result<Option<ExtractedCredential>, LocalServerPolicyError> {
    let Some(value) = headers.get(header::AUTHORIZATION) else {
        return Ok(None);
    };
    let value = value.to_str().map_err(|error| {
        LocalServerPolicyError::unauthorized(format!("invalid authorization header: {error}"))
    })?;
    let token = bearer_token_from_value(value)?;
    Ok(Some(ExtractedCredential {
        auth_method: "local_admin_bearer",
        value: token.to_string(),
    }))
}

fn bearer_token_from_value(value: &str) -> Result<&str, LocalServerPolicyError> {
    let (scheme, token) = value.split_once(' ').ok_or_else(|| {
        LocalServerPolicyError::unauthorized("authorization header must use the Bearer scheme")
    })?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return Err(LocalServerPolicyError::unauthorized(
            "authorization header must use the Bearer scheme",
        ));
    }
    let token = token.trim();
    if token.is_empty() {
        return Err(LocalServerPolicyError::unauthorized(
            "authorization header is missing a token",
        ));
    }
    Ok(token)
}

fn extract_cookie(headers: &HeaderMap, cookie_name: &str) -> Option<String> {
    let cookie_header = headers.get(header::COOKIE)?.to_str().ok()?;
    cookie_header
        .split(';')
        .filter_map(|pair| pair.trim().split_once('='))
        .find_map(|(name, value)| (name == cookie_name).then(|| value.to_string()))
}

#[cfg(test)]
mod tests {
    use http::HeaderValue;

    use super::*;
    use crate::{LocalServerPaths, load_or_create_local_admin_token};

    fn sample_paths(root: &std::path::Path) -> LocalServerPaths {
        LocalServerPaths {
            auth_token_path: root.join("auth").join("token"),
            server_discovery_path: root.join("run").join("server.json"),
            audit_log_path: root.join("logs").join("access.jsonl"),
        }
    }

    #[test]
    fn extract_server_access_accepts_bearer_or_admin_header() {
        let temp = tempfile::tempdir().expect("tempdir should build");
        let paths = sample_paths(temp.path());
        let token = load_or_create_local_admin_token(&paths).expect("token should exist");
        let security = LocalServerSecurityState::new(paths.clone(), token.clone());

        let mut bearer_headers = HeaderMap::new();
        bearer_headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", token.token))
                .expect("authorization header should build"),
        );
        assert_eq!(
            extract_server_access(
                &bearer_headers,
                LocalServerCredentialMode::AuthorizationOrAdminHeader,
                Some(&security),
            )
            .expect("bearer extraction should succeed"),
            ExtractedServerAccess {
                status: ExtractedServerAccessStatus::Authorized,
                auth_method: Some("local_admin_bearer"),
            }
        );

        let mut admin_headers = HeaderMap::new();
        admin_headers.insert(
            HeaderName::from_static(LOCAL_ADMIN_HEADER_NAME),
            HeaderValue::from_str(&token.token).expect("admin header should build"),
        );
        assert_eq!(
            extract_server_access(
                &admin_headers,
                LocalServerCredentialMode::AdminHeaderOnly,
                Some(&security),
            )
            .expect("admin header extraction should succeed"),
            ExtractedServerAccess {
                status: ExtractedServerAccessStatus::Authorized,
                auth_method: Some("local_admin_header"),
            }
        );
    }

    #[test]
    fn deploy_admin_bearer_is_separate_from_local_admin_header_gate() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer deploy-token"),
        );

        authorize_deploy_admin_bearer(Some("deploy-token"), &headers)
            .expect("matching deploy bearer should authorize deploy admin");
        let error = authorize_deploy_admin_bearer(Some("other-token"), &headers)
            .expect_err("wrong deploy bearer should be rejected");
        assert_eq!(error.to_string(), "invalid deploy admin token");
    }

    #[test]
    fn admin_header_only_ignores_local_session_cookies() {
        let temp = tempfile::tempdir().expect("tempdir should build");
        let paths = sample_paths(temp.path());
        let token = load_or_create_local_admin_token(&paths).expect("token should exist");
        let security = LocalServerSecurityState::new(paths.clone(), token.clone());
        let session = security
            .create_session_for_local_admin_token(&token.token)
            .expect("session cookie should issue for local admin token");

        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_str(&format!("{LOCAL_SESSION_COOKIE_NAME}={}", session.value))
                .expect("cookie header should build"),
        );

        assert_eq!(
            extract_server_access(
                &headers,
                LocalServerCredentialMode::AdminHeaderOnly,
                Some(&security),
            )
            .expect("admin-header-only extraction should not fail on session cookie"),
            ExtractedServerAccess {
                status: ExtractedServerAccessStatus::Missing,
                auth_method: None,
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
