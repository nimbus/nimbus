use axum::http::{HeaderMap, HeaderValue, header};

pub(crate) const LOCAL_ADMIN_HEADER_NAME: &str = "x-nimbus-admin-token";
const FIRESTORE_GRPC_SERVICE_PATH_PREFIX: &str = "/google.firestore.v1.Firestore/";
const FIRESTORE_LISTEN_METHOD_PATH: &str = "/google.firestore.v1.Firestore/Listen";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LocalServerRouteFamily {
    Health,
    Demos,
    Ui,
    UiAuthSession,
    NativeApi,
    Debug,
    DeployAdmin,
    NativeWebSocket,
    ConvexHttp,
    ConvexWebSocket,
    FirebaseRest,
    FirebaseGrpc,
    FirebaseGrpcWeb,
    FirebaseWebSocket,
    Unknown,
}

impl LocalServerRouteFamily {
    pub(crate) fn classify(path: &str) -> Self {
        if path == "/health" {
            return Self::Health;
        }
        if path == "/demos" || path.starts_with("/demos/") {
            return Self::Demos;
        }
        if path == "/ui/auth/session" {
            return Self::UiAuthSession;
        }
        if path == "/ui" || path.starts_with("/ui/") {
            return Self::Ui;
        }
        if path == "/api/admin/deploy" {
            return Self::DeployAdmin;
        }
        if path == "/ws" {
            return Self::NativeWebSocket;
        }
        if path.starts_with("/convex/") {
            if path.ends_with("/ws") {
                return Self::ConvexWebSocket;
            }
            return Self::ConvexHttp;
        }
        if path.starts_with("/v1/projects/") {
            return Self::FirebaseRest;
        }
        if path.starts_with(FIRESTORE_GRPC_SERVICE_PATH_PREFIX) {
            return Self::FirebaseGrpc;
        }
        if path.starts_with("/debug/") {
            return Self::Debug;
        }
        if path.starts_with("/api/") {
            return Self::NativeApi;
        }
        Self::Unknown
    }

    // Firebase transport families share one local-security policy boundary, but
    // gRPC-Web and the future Listen WebSocket path are header-sensitive and
    // cannot be recovered from a path-only classifier later in the stack.
    pub(crate) fn classify_request(path: &str, headers: &HeaderMap) -> Self {
        let family = Self::classify(path);
        if family != Self::FirebaseGrpc {
            return family;
        }
        if is_firebase_listen_websocket_request(path, headers) {
            return Self::FirebaseWebSocket;
        }
        if is_firebase_grpc_web_request(headers) {
            return Self::FirebaseGrpcWeb;
        }
        family
    }

    pub(crate) fn requires_origin_allowlist(self) -> bool {
        !matches!(self, Self::Health | Self::Demos)
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Health => "health",
            Self::Demos => "demos",
            Self::Ui => "ui",
            Self::UiAuthSession => "ui_auth_session",
            Self::NativeApi => "native_api",
            Self::Debug => "debug",
            Self::DeployAdmin => "deploy_admin",
            Self::NativeWebSocket => "native_websocket",
            Self::ConvexHttp => "convex_http",
            Self::ConvexWebSocket => "convex_websocket",
            Self::FirebaseRest => "firebase_rest",
            Self::FirebaseGrpc => "firebase_grpc",
            Self::FirebaseGrpcWeb => "firebase_grpc_web",
            Self::FirebaseWebSocket => "firebase_websocket",
            Self::Unknown => "unknown",
        }
    }
}

fn is_firebase_grpc_web_request(headers: &HeaderMap) -> bool {
    headers.contains_key("x-grpc-web")
        || headers
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("application/grpc-web"))
}

fn is_firebase_listen_websocket_request(path: &str, headers: &HeaderMap) -> bool {
    path == FIRESTORE_LISTEN_METHOD_PATH && is_websocket_upgrade(headers)
}

fn is_websocket_upgrade(headers: &HeaderMap) -> bool {
    headers
        .get(header::UPGRADE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("websocket"))
        && headers
            .get(header::CONNECTION)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| {
                value
                    .split(',')
                    .any(|segment| segment.trim().eq_ignore_ascii_case("upgrade"))
            })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ParsedOrigin<'a> {
    pub(crate) scheme: &'a str,
    pub(crate) host: &'a str,
    pub(crate) port: Option<u16>,
}

pub(crate) fn parse_origin(origin: &HeaderValue) -> Option<ParsedOrigin<'_>> {
    let origin = origin.to_str().ok()?;
    let (scheme, authority) = origin.split_once("://")?;
    if authority.is_empty() || authority.contains('/') || authority.contains('?') {
        return None;
    }

    if let Some(rest) = authority.strip_prefix('[') {
        let end = rest.find(']')?;
        let host = &authority[..=end + 1];
        let suffix = &authority[end + 2..];
        if suffix.is_empty() {
            return Some(ParsedOrigin {
                scheme,
                host,
                port: None,
            });
        }
        let port = suffix.strip_prefix(':')?.parse().ok()?;
        return Some(ParsedOrigin {
            scheme,
            host,
            port: Some(port),
        });
    }

    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() && !port.is_empty() => {
            (host, Some(port.parse().ok()?))
        }
        _ => (authority, None),
    };
    Some(ParsedOrigin { scheme, host, port })
}

pub(crate) fn is_loopback_origin(origin: ParsedOrigin<'_>, port: Option<u16>) -> bool {
    if !origin.scheme.eq_ignore_ascii_case("http") {
        return false;
    }
    if !matches!(origin.host, "localhost" | "127.0.0.1" | "[::1]") {
        return false;
    }
    match port {
        Some(expected_port) => origin.port == Some(expected_port),
        None => origin.port.is_some(),
    }
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderValue, header};

    use super::*;

    #[test]
    fn route_family_classifies_local_surfaces() {
        assert_eq!(
            LocalServerRouteFamily::classify("/health"),
            LocalServerRouteFamily::Health
        );
        assert_eq!(
            LocalServerRouteFamily::classify("/api/tenants/demo/documents"),
            LocalServerRouteFamily::NativeApi
        );
        assert_eq!(
            LocalServerRouteFamily::classify("/debug/runtime/metrics"),
            LocalServerRouteFamily::Debug
        );
        assert_eq!(
            LocalServerRouteFamily::classify("/api/admin/deploy"),
            LocalServerRouteFamily::DeployAdmin
        );
        assert_eq!(
            LocalServerRouteFamily::classify("/ws"),
            LocalServerRouteFamily::NativeWebSocket
        );
        assert_eq!(
            LocalServerRouteFamily::classify("/convex/demo/query"),
            LocalServerRouteFamily::ConvexHttp
        );
        assert_eq!(
            LocalServerRouteFamily::classify("/convex/demo/ws"),
            LocalServerRouteFamily::ConvexWebSocket
        );
        assert_eq!(
            LocalServerRouteFamily::classify(
                "/v1/projects/demo/databases/(default)/documents:commit"
            ),
            LocalServerRouteFamily::FirebaseRest
        );
        assert_eq!(
            LocalServerRouteFamily::classify("/google.firestore.v1.Firestore/Commit"),
            LocalServerRouteFamily::FirebaseGrpc
        );
    }

    #[test]
    fn route_family_distinguishes_firebase_grpc_web_and_websocket_requests() {
        let mut grpc_web_headers = HeaderMap::new();
        grpc_web_headers.insert("x-grpc-web", HeaderValue::from_static("1"));
        grpc_web_headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/grpc-web+proto"),
        );
        assert_eq!(
            LocalServerRouteFamily::classify_request(
                "/google.firestore.v1.Firestore/Commit",
                &grpc_web_headers,
            ),
            LocalServerRouteFamily::FirebaseGrpcWeb
        );

        let mut websocket_headers = HeaderMap::new();
        websocket_headers.insert(header::UPGRADE, HeaderValue::from_static("websocket"));
        websocket_headers.insert(header::CONNECTION, HeaderValue::from_static("Upgrade"));
        assert_eq!(
            LocalServerRouteFamily::classify_request(
                "/google.firestore.v1.Firestore/Listen",
                &websocket_headers,
            ),
            LocalServerRouteFamily::FirebaseWebSocket
        );
    }

    #[test]
    fn parse_origin_supports_ipv4_hostnames_and_ipv6() {
        let localhost_header = HeaderValue::from_static("http://localhost:8080");
        let localhost = parse_origin(&localhost_header).expect("localhost origin should parse");
        assert_eq!(localhost.host, "localhost");
        assert_eq!(localhost.port, Some(8080));

        let ipv6_header = HeaderValue::from_static("http://[::1]:8080");
        let ipv6 = parse_origin(&ipv6_header).expect("ipv6 origin should parse");
        assert_eq!(ipv6.host, "[::1]");
        assert_eq!(ipv6.port, Some(8080));
    }

    #[test]
    fn loopback_origin_requires_http_and_matching_port() {
        assert!(is_loopback_origin(
            ParsedOrigin {
                scheme: "http",
                host: "localhost",
                port: Some(8080),
            },
            Some(8080),
        ));
        assert!(!is_loopback_origin(
            ParsedOrigin {
                scheme: "https",
                host: "localhost",
                port: Some(8080),
            },
            Some(8080),
        ));
        assert!(!is_loopback_origin(
            ParsedOrigin {
                scheme: "http",
                host: "example.com",
                port: Some(8080),
            },
            Some(8080),
        ));
        assert!(!is_loopback_origin(
            ParsedOrigin {
                scheme: "http",
                host: "localhost",
                port: Some(3000),
            },
            Some(8080),
        ));
    }
}
