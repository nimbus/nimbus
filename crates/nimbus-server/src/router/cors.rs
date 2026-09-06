use std::collections::HashSet;
use std::sync::Arc;

use axum::http::{HeaderName, HeaderValue, Method, header};
use tower_http::cors::{AllowOrigin, CorsLayer};

#[derive(Clone)]
pub(crate) struct CorsOriginPolicy {
    configured_origins: Arc<HashSet<String>>,
}

impl CorsOriginPolicy {
    pub(crate) fn new(configured_origins: &[String]) -> Self {
        let mut allowed = HashSet::new();
        for origin in configured_origins {
            match normalize_cors_origin(origin) {
                Ok(normalized) => {
                    allowed.insert(normalized);
                }
                Err(reason) => {
                    // Fail closed: a bad entry grants nothing extra; the origin
                    // it was meant to allow will visibly fail CORS.
                    tracing::warn!(%origin, %reason, "ignoring invalid configured CORS origin");
                }
            }
        }
        Self {
            configured_origins: Arc::new(allowed),
        }
    }

    pub(crate) fn allows(&self, origin: &HeaderValue) -> bool {
        is_allowed_local_cors_origin(origin) || self.configured_allows(origin)
    }

    pub(crate) fn configured_allows(&self, origin: &HeaderValue) -> bool {
        is_configured_cors_origin(origin, &self.configured_origins)
    }
}

pub(super) fn build_cors_layer(origin_policy: CorsOriginPolicy) -> CorsLayer {
    CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(move |origin, _request_head| {
            origin_policy.allows(origin)
        }))
        .allow_headers([
            header::ACCEPT,
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            HeaderName::from_static("firebase-instance-id-token"),
            HeaderName::from_static("x-nimbus-admin-token"),
            HeaderName::from_static("google-cloud-resource-prefix"),
            HeaderName::from_static("x-goog-request-params"),
            HeaderName::from_static("x-goog-api-client"),
            HeaderName::from_static("x-goog-api-key"),
            HeaderName::from_static("x-firebase-gmpid"),
            HeaderName::from_static("x-firebase-appcheck"),
            HeaderName::from_static("x-grpc-web"),
            HeaderName::from_static("grpc-timeout"),
        ])
        .expose_headers([
            HeaderName::from_static("grpc-status"),
            HeaderName::from_static("grpc-message"),
            HeaderName::from_static("grpc-status-details-bin"),
        ])
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
}

pub(crate) fn is_allowed_local_cors_origin(origin: &HeaderValue) -> bool {
    let Ok(origin) = origin.to_str() else {
        return false;
    };
    let Some(authority) = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))
    else {
        return false;
    };

    matches!(authority, "localhost" | "127.0.0.1" | "[::1]")
        || authority.starts_with("localhost:")
        || authority.starts_with("127.0.0.1:")
        || authority.starts_with("[::1]:")
}

pub(crate) fn is_configured_cors_origin(
    origin: &HeaderValue,
    allowed: &std::collections::HashSet<String>,
) -> bool {
    if allowed.is_empty() {
        return false;
    }
    let Ok(origin) = origin.to_str() else {
        return false;
    };
    let Ok(normalized) = normalize_cors_origin(origin) else {
        return false;
    };
    allowed.contains(&normalized)
}

/// Normalize a configured browser origin to the exact form browsers send in
/// the `Origin` header: lowercase `scheme://host`, default ports stripped,
/// no path/query/fragment. Wildcards are rejected — the CORS allowlist is
/// exact-match only.
pub fn normalize_cors_origin(origin: &str) -> Result<String, String> {
    let trimmed = origin.trim();
    if trimmed.is_empty() {
        return Err("CORS origin must not be empty".to_string());
    }
    if trimmed.contains('*') {
        return Err(
            "wildcard CORS origins are not supported; pass each origin explicitly".to_string(),
        );
    }
    let Some((scheme, rest)) = trimmed.split_once("://") else {
        return Err(format!(
            "CORS origin `{trimmed}` must include an http:// or https:// scheme"
        ));
    };
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return Err(format!(
            "CORS origin `{trimmed}` must use the http or https scheme"
        ));
    }
    let authority = rest.strip_suffix('/').unwrap_or(rest);
    if authority.is_empty() {
        return Err(format!("CORS origin `{trimmed}` is missing a host"));
    }
    if authority.contains('/') || authority.contains('?') || authority.contains('#') {
        return Err(format!(
            "CORS origin `{trimmed}` must not include a path, query, or fragment"
        ));
    }
    let authority = authority.to_ascii_lowercase();
    let (host, port) = split_origin_port(&authority);
    if host.is_empty() {
        return Err(format!("CORS origin `{trimmed}` is missing a host"));
    }
    match port {
        None => Ok(format!("{scheme}://{host}")),
        Some(port) => {
            let Ok(parsed) = port.parse::<u16>() else {
                return Err(format!("CORS origin `{trimmed}` has an invalid port"));
            };
            let is_default =
                (scheme == "http" && parsed == 80) || (scheme == "https" && parsed == 443);
            if is_default {
                Ok(format!("{scheme}://{host}"))
            } else {
                Ok(format!("{scheme}://{host}:{parsed}"))
            }
        }
    }
}

/// Split `host[:port]`, treating a bracketed IPv6 literal as the host
/// boundary so `[::1]:8080` does not split inside the address.
fn split_origin_port(authority: &str) -> (&str, Option<&str>) {
    if let Some(bracket_end) = authority.rfind(']') {
        match authority[bracket_end + 1..].strip_prefix(':') {
            Some(port) => (&authority[..=bracket_end], Some(port)),
            None => (authority, None),
        }
    } else if let Some((host, port)) = authority.rsplit_once(':') {
        (host, Some(port))
    } else {
        (authority, None)
    }
}
