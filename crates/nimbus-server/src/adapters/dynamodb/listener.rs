//! DynamoDB HTTP listener (transport only).
//!
//! Owns the single `POST /` axum route for the dedicated DynamoDB port and
//! forwards the request to `nimbus_dynamodb::dispatch`. All protocol semantics
//! live in `nimbus-dynamodb`; this module is bind/serve glue, mirroring the
//! MongoDB listener composition (`adapters::mongodb::listener`).

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use nimbus_dynamodb::{AccessKeyRegistry, DispatchContext};
use nimbus_engine::Engine;
use tokio::net::TcpListener;
use tracing::{error, info};

/// DynamoDB JSON-1.0 content type for success and error response bodies.
const CONTENT_TYPE_AMZ_JSON: &str = "application/x-amz-json-1.0";

/// Maximum accepted request body, aligned to DynamoDB's documented operation
/// limits: a single item is at most 400 KB and `BatchWriteItem` carries up to
/// 25 items, so a legitimate request body tops out around 10 MB; 16 MiB admits
/// every legal request with headroom. The cap is enforced by the transport
/// (axum returns `413 Payload Too Large`) **before** the body is buffered or
/// JSON-parsed, so an oversized payload cannot force a large pre-authentication
/// allocation/parse (F13). DynamoDB itself returns 413 for oversized requests.
const MAX_REQUEST_BODY_BYTES: usize = 16 * 1024 * 1024;

/// Shared listener state: the engine handle plus the access-key → tenant
/// registry. `Arc`-wrapped so axum can clone it cheaply per request.
#[derive(Clone)]
struct DynamoDbState {
    engine: Arc<Engine>,
    access_keys: Arc<AccessKeyRegistry>,
}

/// Build the single-route DynamoDB axum app over the shared `Engine` and the
/// access-key registry that authenticates each request.
pub fn router(engine: Arc<Engine>, access_keys: AccessKeyRegistry) -> Router {
    let state = DynamoDbState {
        engine,
        access_keys: Arc::new(access_keys),
    };
    Router::new()
        .route("/", post(handle))
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BODY_BYTES))
        .with_state(state)
}

/// The signature-skipping lookup escape hatch ([`AuthMode::LookupOnly`]) is
/// loopback-only. Returns an error when `access_keys` is in lookup mode but
/// `addr` is not a loopback address — the server must never expose an
/// unauthenticated DynamoDB surface on a network-reachable address. Strict mode
/// (the default) is allowed on any address.
///
/// # Errors
/// `InvalidInput` if an insecure lookup-mode registry is bound to a non-loopback
/// address.
pub(crate) fn guard_lookup_is_loopback_only(
    addr: SocketAddr,
    access_keys: &AccessKeyRegistry,
) -> std::io::Result<()> {
    if access_keys.is_insecure_lookup() && !addr.ip().is_loopback() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "DynamoDB insecure_dev_auth (signature-skipping lookup mode) refuses to bind \
                 non-loopback address {addr}; use the default Strict auth mode with signed access \
                 keys for network-reachable listeners"
            ),
        ));
    }
    Ok(())
}

/// Serve the DynamoDB HTTP listener until the spawned task is aborted.
pub async fn run_listener(
    listener: TcpListener,
    engine: Arc<Engine>,
    access_keys: AccessKeyRegistry,
) {
    info!(
        "DynamoDB listener started on {:?}",
        listener.local_addr().ok()
    );
    if let Err(error) = axum::serve(listener, router(engine, access_keys)).await {
        error!("DynamoDB listener error: {error}");
    }
}

/// `POST /` — authenticate by access key and dispatch by `X-Amz-Target`.
async fn handle(State(state): State<DynamoDbState>, headers: HeaderMap, body: Bytes) -> Response {
    let ctx = DispatchContext {
        engine: &state.engine,
        access_keys: &state.access_keys,
    };
    let (status, json) = nimbus_dynamodb::dispatch(&ctx, &headers, &body);
    let status = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let payload = serde_json::to_vec(&json).unwrap_or_default();
    (
        status,
        [(header::CONTENT_TYPE, CONTENT_TYPE_AMZ_JSON)],
        payload,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use nimbus_core::TenantId;
    use nimbus_dynamodb::AuthMode;
    use tower::ServiceExt;

    const ACCESS_KEY: &str = "AKIATEST";

    fn test_router() -> (Router, tempfile::TempDir) {
        let temp = tempfile::tempdir().expect("tempdir should create");
        let engine = Arc::new(Engine::new(temp.path()).expect("engine should create"));
        // Synthetic `Signature=deadbeef` headers → drive the lookup escape hatch.
        let registry = AccessKeyRegistry::new()
            .bind(ACCESS_KEY, TenantId::new("test").expect("tenant"))
            .with_mode(AuthMode::LookupOnly);
        (router(engine, registry), temp)
    }

    fn signed_authorization() -> String {
        format!(
            "AWS4-HMAC-SHA256 Credential={ACCESS_KEY}/20260101/us-east-1/dynamodb/aws4_request, \
             SignedHeaders=host;x-amz-target, Signature=deadbeef"
        )
    }

    async fn post(target: &str, body: &str) -> (StatusCode, serde_json::Value) {
        // `_temp` is held until the request completes so the Service's storage
        // dir is not reclaimed mid-test.
        let (router, _temp) = test_router();
        let request = axum::http::Request::builder()
            .method("POST")
            .uri("/")
            .header("x-amz-target", target)
            .header("authorization", signed_authorization())
            .body(axum::body::Body::from(body.to_owned()))
            .expect("request builds");
        let response = router.oneshot(request).await.expect("route responds");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body collects");
        let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        (status, json)
    }

    #[tokio::test]
    async fn unknown_target_routes_to_unknown_operation_envelope() {
        let (status, body) = post("DynamoDB_20120810.Frobnicate", "{}").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            body["__type"]
                .as_str()
                .unwrap()
                .ends_with("UnknownOperationException"),
            "got {body}"
        );
    }

    #[tokio::test]
    async fn unauthenticated_request_is_rejected() {
        // No registry binding for an arbitrary key: the route must reject it
        // rather than route to a handler.
        let (router, _temp) = test_router();
        let request = axum::http::Request::builder()
            .method("POST")
            .uri("/")
            .header("x-amz-target", "DynamoDB_20120810.CreateTable")
            .body(axum::body::Body::from("{}"))
            .expect("request builds");
        let response = router.oneshot(request).await.expect("route responds");
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body collects");
        let body: serde_json::Value =
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        assert!(
            body["__type"]
                .as_str()
                .unwrap()
                .ends_with("MissingAuthenticationToken"),
            "got {body}"
        );
    }

    #[tokio::test]
    async fn known_target_dispatches_through_to_handler() {
        // DescribeLimits is a no-argument read; an authenticated call returns 200
        // with the limit shape. Proves an authenticated request wires through the
        // `POST /` route into `nimbus_dynamodb::dispatch` and back.
        let (status, body) = post("DynamoDB_20120810.DescribeLimits", "{}").await;
        assert_eq!(status, StatusCode::OK, "got {body}");
        assert_eq!(body["AccountMaxReadCapacityUnits"].as_i64(), Some(80_000));
    }

    #[tokio::test]
    async fn create_table_succeeds_through_the_route() {
        // End-to-end through the HTTP route: an authenticated CreateTable
        // returns 200 with the ACTIVE TableDescription.
        let body = serde_json::json!({
            "TableName": "Orders",
            "KeySchema": [{ "AttributeName": "pk", "KeyType": "HASH" }],
            "AttributeDefinitions": [{ "AttributeName": "pk", "AttributeType": "S" }],
        })
        .to_string();
        let (status, body) = post("DynamoDB_20120810.CreateTable", &body).await;
        assert_eq!(status, StatusCode::OK, "got {body}");
        assert_eq!(
            body["TableDescription"]["TableName"].as_str().unwrap(),
            "Orders"
        );
    }

    /// F13: a body over [`MAX_REQUEST_BODY_BYTES`] is rejected with
    /// `413 Payload Too Large` by the transport — *before* authentication or
    /// JSON parsing. The request carries no auth header, so absent the body cap
    /// it would reach the handler and return `MissingAuthenticationToken (400)`;
    /// asserting 413 proves the cap fires first and bounds the pre-auth parse.
    #[tokio::test]
    async fn oversize_body_is_rejected_before_auth() {
        let (router, _temp) = test_router();
        let oversize = vec![b'a'; MAX_REQUEST_BODY_BYTES + 1];
        let request = axum::http::Request::builder()
            .method("POST")
            .uri("/")
            .header("x-amz-target", "DynamoDB_20120810.CreateTable")
            .body(axum::body::Body::from(oversize))
            .expect("request builds");
        let response = router.oneshot(request).await.expect("route responds");
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[test]
    fn guard_allows_loopback_lookup_and_strict_anywhere() {
        let loopback: SocketAddr = "127.0.0.1:8000".parse().unwrap();
        let routable: SocketAddr = "10.0.0.5:8000".parse().unwrap();
        let lookup = AccessKeyRegistry::new().with_mode(AuthMode::LookupOnly);
        let strict = AccessKeyRegistry::new(); // Strict by default.

        // Lookup is fine on loopback.
        assert!(guard_lookup_is_loopback_only(loopback, &lookup).is_ok());
        // Strict is fine even on a routable address.
        assert!(guard_lookup_is_loopback_only(routable, &strict).is_ok());
    }

    #[test]
    fn guard_refuses_lookup_on_a_routable_address() {
        let routable: SocketAddr = "0.0.0.0:8000".parse().unwrap();
        let lookup = AccessKeyRegistry::new().with_mode(AuthMode::LookupOnly);
        let err = guard_lookup_is_loopback_only(routable, &lookup)
            .expect_err("non-loopback lookup must be refused");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("refuses to bind"), "got {err}");
    }
}
