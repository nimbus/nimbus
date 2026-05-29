//! DynamoDB JSON wire envelope: `X-Amz-Target` parsing and the `__type` error
//! envelope.
//!
//! Transport-agnostic. `nimbus-server` owns the `POST /` route and converts the
//! `(status, body)` pairs produced here into HTTP responses. Behavior mirrors
//! the AWS DynamoDB wire contract and the ExtendDB reference (parity-critical):
//! the `__type` prefix, HTTP status, message-omission, and special-envelope
//! fields all come from `extenddb_core::error::DynamoDbError`.

use extenddb_core::error::DynamoDbError;
use http::HeaderMap;
use serde_json::{Value, json};

/// The DynamoDB JSON-1.0 target prefix (data plane).
pub const TARGET_PREFIX: &str = "DynamoDB_20120810.";
/// The DynamoDB Streams JSON-1.0 target prefix.
pub const STREAMS_TARGET_PREFIX: &str = "DynamoDBStreams_20120810.";

/// A rendered wire response: an HTTP status code and a JSON body.
pub type WireResponse = (u16, Value);

/// Extract the operation name from the `X-Amz-Target` header.
///
/// Mirrors real DynamoDB / ExtendDB (`crates/server/src/request_helpers.rs`):
/// - Missing `X-Amz-Target` with an `Authorization` header present →
///   `UnknownOperationException`; without auth → `MissingAuthenticationToken`.
/// - A target that does not carry the `DynamoDB_20120810.` or
///   `DynamoDBStreams_20120810.` prefix → `UnknownOperationException`.
pub fn extract_operation(headers: &HeaderMap) -> Result<String, DynamoDbError> {
    let target = headers
        .get("x-amz-target")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
            if headers.contains_key("authorization") {
                DynamoDbError::UnknownOperationException(String::new())
            } else {
                DynamoDbError::MissingAuthenticationToken("Missing Authentication Token".to_owned())
            }
        })?;

    target
        .strip_prefix(TARGET_PREFIX)
        .or_else(|| target.strip_prefix(STREAMS_TARGET_PREFIX))
        .map(str::to_owned)
        .ok_or_else(|| DynamoDbError::UnknownOperationException(String::new()))
}

/// Render a `DynamoDbError` into the DynamoDB JSON error envelope.
///
/// `{ "__type": "<prefix>#<Code>", "message": "..." }` — the `message` field is
/// omitted when empty (real DynamoDB omits it for `UnknownOperationException`
/// and similar). `CancellationReasons` (TransactWriteItems) and `Item`
/// (ConditionalCheckFailed with `ReturnValuesOnConditionCheckFailure`) are added
/// when present. HTTP status comes from `DynamoDbError::status_code`.
pub fn render_error(error: &DynamoDbError) -> WireResponse {
    let mut body = json!({ "__type": error.full_error_type() });

    let message = error.message();
    if !message.is_empty() {
        body["message"] = Value::String(message.to_owned());
    }
    if let Some(reasons) = error.cancellation_reasons() {
        body["CancellationReasons"] = serde_json::to_value(reasons).unwrap_or(Value::Null);
    }
    if let Some(item) = error.condition_check_item() {
        body["Item"] = serde_json::to_value(item).unwrap_or(Value::Null);
    }

    (error.status_code(), body)
}

/// Render a successful operation result (HTTP 200, no envelope).
pub fn render_success(body: Value) -> WireResponse {
    (200, body)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(
                http::header::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                http::HeaderValue::from_str(v).unwrap(),
            );
        }
        h
    }

    #[test]
    fn extracts_data_plane_operation() {
        let h = headers(&[("x-amz-target", "DynamoDB_20120810.PutItem")]);
        assert_eq!(extract_operation(&h).unwrap(), "PutItem");
    }

    #[test]
    fn extracts_streams_operation() {
        let h = headers(&[("x-amz-target", "DynamoDBStreams_20120810.GetRecords")]);
        assert_eq!(extract_operation(&h).unwrap(), "GetRecords");
    }

    #[test]
    fn missing_target_with_auth_is_unknown_operation() {
        let h = headers(&[("authorization", "AWS4-HMAC-SHA256 ...")]);
        let err = extract_operation(&h).unwrap_err();
        assert!(matches!(err, DynamoDbError::UnknownOperationException(_)));
    }

    #[test]
    fn missing_target_without_auth_is_missing_token() {
        let h = headers(&[]);
        let err = extract_operation(&h).unwrap_err();
        assert!(matches!(err, DynamoDbError::MissingAuthenticationToken(_)));
    }

    #[test]
    fn wrong_prefix_is_unknown_operation() {
        let h = headers(&[("x-amz-target", "Frobnicate.PutItem")]);
        let err = extract_operation(&h).unwrap_err();
        assert!(matches!(err, DynamoDbError::UnknownOperationException(_)));
    }

    #[test]
    fn error_envelope_has_type_prefix_and_omits_empty_message() {
        let (status, body) = render_error(&DynamoDbError::UnknownOperationException(String::new()));
        assert_eq!(status, 400);
        let ty = body["__type"].as_str().unwrap();
        assert!(ty.contains('#'), "__type must carry a wire prefix: {ty}");
        assert!(ty.ends_with("UnknownOperationException"), "got {ty}");
        assert!(
            body.get("message").is_none(),
            "empty message must be omitted, got {body}"
        );
    }

    #[test]
    fn error_envelope_includes_non_empty_message() {
        let (_status, body) = render_error(&DynamoDbError::SerializationException(
            "bad json".to_owned(),
        ));
        assert_eq!(body["message"].as_str().unwrap(), "bad json");
    }
}
