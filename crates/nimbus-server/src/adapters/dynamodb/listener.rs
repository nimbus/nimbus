//! DynamoDB HTTP listener (transport only).
//!
//! Owns the single `POST /` axum route for the dedicated DynamoDB port and
//! forwards the request to `nimbus_dynamodb::dispatch`. All protocol semantics
//! live in `nimbus-dynamodb`; this module is bind/serve glue, mirroring the
//! MongoDB listener composition (`adapters::mongodb::listener`).

use std::sync::Arc;

use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use nimbus_dynamodb::{AccessKeyRegistry, DispatchContext};
use nimbus_engine::Service;
use tokio::net::TcpListener;
use tracing::{error, info};

/// DynamoDB JSON-1.0 content type for success and error response bodies.
const CONTENT_TYPE_AMZ_JSON: &str = "application/x-amz-json-1.0";

/// Shared listener state: the engine handle plus the access-key → tenant
/// registry. `Arc`-wrapped so axum can clone it cheaply per request.
#[derive(Clone)]
struct DynamoDbState {
    service: Arc<Service>,
    access_keys: Arc<AccessKeyRegistry>,
}

/// Build the single-route DynamoDB axum app over the shared `Service` and the
/// access-key registry that authenticates each request.
pub fn router(service: Arc<Service>, access_keys: AccessKeyRegistry) -> Router {
    let state = DynamoDbState {
        service,
        access_keys: Arc::new(access_keys),
    };
    Router::new().route("/", post(handle)).with_state(state)
}

/// Serve the DynamoDB HTTP listener until the spawned task is aborted.
pub async fn run_listener(
    listener: TcpListener,
    service: Arc<Service>,
    access_keys: AccessKeyRegistry,
) {
    info!(
        "DynamoDB listener started on {:?}",
        listener.local_addr().ok()
    );
    if let Err(error) = axum::serve(listener, router(service, access_keys)).await {
        error!("DynamoDB listener error: {error}");
    }
}

/// `POST /` — authenticate by access key and dispatch by `X-Amz-Target`.
async fn handle(State(state): State<DynamoDbState>, headers: HeaderMap, body: Bytes) -> Response {
    let ctx = DispatchContext {
        service: &state.service,
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
    use tower::ServiceExt;

    const ACCESS_KEY: &str = "AKIATEST";

    fn test_router() -> (Router, tempfile::TempDir) {
        let temp = tempfile::tempdir().expect("tempdir should create");
        let service = Arc::new(Service::new(temp.path()).expect("service should create"));
        let registry =
            AccessKeyRegistry::new().bind(ACCESS_KEY, TenantId::new("test").expect("tenant"));
        (router(service, registry), temp)
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
}
