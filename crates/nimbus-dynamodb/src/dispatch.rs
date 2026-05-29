//! X-Amz-Target dispatch entrypoint.
//!
//! Transport-agnostic: `nimbus-server` mounts [`dispatch`] on a `POST /` route
//! for the dedicated DynamoDB port. The flow mirrors real DynamoDB / ExtendDB:
//! parse the target, reject unknown operations *before* auth, reject malformed
//! JSON bodies *before* auth, then route to the per-operation handler.
//!
//! Operation handlers land in later roadmap items (control plane D0.6, item ops
//! D1, Query/Scan D2, …). Until an operation's handler lands it is recognized
//! (so the unknown-vs-known distinction is correct) but routes to a
//! `not-yet-implemented` placeholder.

use extenddb_core::error::DynamoDbError;
use http::HeaderMap;

use crate::wire::{self, WireResponse};

/// Every DynamoDB operation the adapter targets across tiers T0–T7 (data plane,
/// Query/Scan, batch/transact, Streams, TTL, tagging). GSI/LSI changes ride on
/// `CreateTable`/`UpdateTable`; SigV4 (T7) is auth, not an operation. An operation
/// outside this set is rejected with `UnknownOperationException`.
pub const KNOWN_OPERATIONS: &[&str] = &[
    // T0 — control plane
    "CreateTable",
    "DescribeTable",
    "ListTables",
    "UpdateTable",
    "DeleteTable",
    "DescribeEndpoints",
    "DescribeLimits",
    // T1 — single-item
    "PutItem",
    "GetItem",
    "DeleteItem",
    "UpdateItem",
    // T2 — query / scan
    "Query",
    "Scan",
    // T3 — batch / transact
    "BatchGetItem",
    "BatchWriteItem",
    "TransactGetItems",
    "TransactWriteItems",
    // T5 — streams
    "DescribeStream",
    "GetShardIterator",
    "GetRecords",
    "ListStreams",
    // T6 — TTL / tagging
    "UpdateTimeToLive",
    "DescribeTimeToLive",
    "TagResource",
    "UntagResource",
    "ListTagsOfResource",
];

/// True if `operation` is a DynamoDB operation the adapter targets.
#[must_use]
pub fn is_known_operation(operation: &str) -> bool {
    KNOWN_OPERATIONS.contains(&operation)
}

/// Dispatch a DynamoDB request to its operation handler.
///
/// Returns a [`WireResponse`] `(status, body)`; `nimbus-server` turns it into an
/// HTTP response. Capability parameters (`Arc<Service>`, resolved tenant/auth)
/// are threaded in once the first storage-touching/authenticated handler lands
/// (D0.5/D0.6); the X-Amz-Target switch and error envelope are complete now.
#[must_use]
pub fn dispatch(headers: &HeaderMap, body: &[u8]) -> WireResponse {
    // 1. Parse X-Amz-Target.
    let operation = match wire::extract_operation(headers) {
        Ok(op) => op,
        Err(error) => return wire::render_error(&error),
    };

    // 2. Reject unknown operations before auth (real DynamoDB order).
    if !is_known_operation(&operation) {
        return wire::render_error(&DynamoDbError::UnknownOperationException(String::new()));
    }

    // 3. Reject malformed JSON bodies before auth.
    if let Err(error) = serde_json::from_slice::<serde_json::Value>(body) {
        return wire::render_error(&DynamoDbError::SerializationException(format!(
            "Start of structure or map found where not expected: {error}"
        )));
    }

    // 4. Route to the operation handler. No handlers are implemented yet; they
    //    land per roadmap item and replace this placeholder.
    wire::render_error(&DynamoDbError::InternalServerError(format!(
        "{operation} is not yet implemented"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target_headers(target: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert("x-amz-target", http::HeaderValue::from_str(target).unwrap());
        h.insert(
            "authorization",
            http::HeaderValue::from_static("AWS4-HMAC-SHA256 x"),
        );
        h
    }

    #[test]
    fn known_operation_set_is_nonempty_and_deduped() {
        assert!(KNOWN_OPERATIONS.len() >= 26);
        let mut seen = std::collections::HashSet::new();
        for op in KNOWN_OPERATIONS {
            assert!(
                seen.insert(*op),
                "duplicate operation in KNOWN_OPERATIONS: {op}"
            );
        }
    }

    #[test]
    fn unknown_operation_rejected() {
        let (status, body) = dispatch(&target_headers("DynamoDB_20120810.Frobnicate"), b"{}");
        assert_eq!(status, 400);
        assert!(
            body["__type"]
                .as_str()
                .unwrap()
                .ends_with("UnknownOperationException")
        );
    }

    #[test]
    fn known_operation_with_malformed_body_is_serialization_exception() {
        let (status, body) = dispatch(&target_headers("DynamoDB_20120810.PutItem"), b"not json");
        assert_eq!(status, 400);
        assert!(
            body["__type"]
                .as_str()
                .unwrap()
                .ends_with("SerializationException"),
            "got {body}"
        );
    }

    #[test]
    fn known_operation_with_valid_body_reaches_handler_placeholder() {
        // Until handlers land, a recognized op with a valid body hits the
        // not-yet-implemented placeholder (500). Each op's roadmap item replaces
        // this with a real success/modeled-error response.
        let (status, body) = dispatch(&target_headers("DynamoDB_20120810.PutItem"), b"{}");
        assert_eq!(status, 500);
        assert!(
            body["message"]
                .as_str()
                .unwrap()
                .contains("not yet implemented")
        );
    }

    #[test]
    fn missing_target_rejected_before_body() {
        let mut h = HeaderMap::new();
        h.insert(
            "authorization",
            http::HeaderValue::from_static("AWS4-HMAC-SHA256 x"),
        );
        let (status, _body) = dispatch(&h, b"not json");
        // Unknown-operation (missing target) is decided before body parsing, so
        // the malformed body does not surface as SerializationException.
        assert_eq!(status, 400);
    }
}
