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
use nimbus_engine::Service;
use tokio::net::TcpListener;
use tracing::{error, info};

/// DynamoDB JSON-1.0 content type for success and error response bodies.
const CONTENT_TYPE_AMZ_JSON: &str = "application/x-amz-json-1.0";

/// Build the single-route DynamoDB axum app over the shared `Service`.
pub fn router(service: Arc<Service>) -> Router {
    Router::new().route("/", post(handle)).with_state(service)
}

/// Serve the DynamoDB HTTP listener until the spawned task is aborted.
pub async fn run_listener(listener: TcpListener, service: Arc<Service>) {
    info!(
        "DynamoDB listener started on {:?}",
        listener.local_addr().ok()
    );
    if let Err(error) = axum::serve(listener, router(service)).await {
        error!("DynamoDB listener error: {error}");
    }
}

/// `POST /` — dispatch by `X-Amz-Target`.
///
/// `_service` is threaded for when operation handlers land (D0.5/D0.6); the
/// dispatch entrypoint does not consume it yet.
async fn handle(State(_service): State<Arc<Service>>, headers: HeaderMap, body: Bytes) -> Response {
    let (status, json) = nimbus_dynamodb::dispatch(&headers, &body);
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
    use tower::ServiceExt;

    fn test_service() -> (Arc<Service>, tempfile::TempDir) {
        let temp = tempfile::tempdir().expect("tempdir should create");
        let service = Arc::new(Service::new(temp.path()).expect("service should create"));
        (service, temp)
    }

    async fn post(target: &str, body: &str) -> (StatusCode, serde_json::Value) {
        // `_temp` is held until the request completes so the Service's storage
        // dir is not reclaimed mid-test.
        let (service, _temp) = test_service();
        let request = axum::http::Request::builder()
            .method("POST")
            .uri("/")
            .header("x-amz-target", target)
            .header("authorization", "AWS4-HMAC-SHA256 x")
            .body(axum::body::Body::from(body.to_owned()))
            .expect("request builds");
        let response = router(service)
            .oneshot(request)
            .await
            .expect("route responds");
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
    async fn known_target_dispatches_through_to_handler() {
        // PutItem is recognized; until its handler lands it returns the
        // not-yet-implemented placeholder. This proves the `POST /` route wires
        // into `nimbus_dynamodb::dispatch`.
        let (status, body) = post("DynamoDB_20120810.PutItem", "{}").await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(
            body["message"]
                .as_str()
                .unwrap()
                .contains("not yet implemented"),
            "got {body}"
        );
    }
}
