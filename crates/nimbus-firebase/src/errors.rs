use http::StatusCode;
use nimbus_core::{CommitErrorClass, Error, StorageErrorKind};
use serde_json::{Value, json};
use tonic::Code;

use super::request_error::FirestoreRequestError;
use super::resource_names;

/// Maps any of the nine per-RPC Firestore request-parsing failures (now
/// unified as [`FirestoreRequestError`]) onto the shared core `Error`. All
/// nine collapsed to the same `InvalidInput` mapping, so one function covers
/// what used to be seven near-identical per-RPC ones.
pub fn firestore_request_error_to_core(error: FirestoreRequestError) -> Error {
    Error::InvalidInput(error.to_string())
}

pub fn resource_name_error_to_core(error: resource_names::FirestoreResourceNameError) -> Error {
    Error::InvalidInput(error.to_string())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FirestoreRestError {
    http_code: StatusCode,
    status: &'static str,
    details: Vec<Value>,
}

fn cancelled_status_code() -> StatusCode {
    StatusCode::from_u16(499).expect("499 should be a valid HTTP status code")
}

fn missing_index_fields(error: &Error) -> Option<Vec<String>> {
    match error {
        Error::MissingIndex { fields } => Some(fields.clone()),
        _ => None,
    }
}

fn missing_index_details(fields: &[String], description: &str) -> Vec<Value> {
    vec![json!({
        "@type": "type.googleapis.com/google.rpc.PreconditionFailure",
        "violations": [{
            "type": "FIRESTORE_QUERY_INDEX",
            "subject": format!("fields/{}", fields.join(",")),
            "description": description,
        }],
    })]
}

pub fn firestore_grpc_code(error: &Error) -> Code {
    if missing_index_fields(error).is_some() {
        return Code::FailedPrecondition;
    }

    if let Some(class) = error.commit_class() {
        return match class {
            CommitErrorClass::Conflict => Code::Aborted,
            CommitErrorClass::Overloaded
            | CommitErrorClass::CommitterFull
            | CommitErrorClass::RateLimited => Code::ResourceExhausted,
            CommitErrorClass::RejectedBeforeExecution => Code::Unavailable,
            CommitErrorClass::OutOfRetention => Code::FailedPrecondition,
            CommitErrorClass::CapExceeded => Code::InvalidArgument,
        };
    }

    match error {
        Error::Cancelled => Code::Cancelled,
        Error::TenantNotFound(_)
        | Error::DocumentNotFound(_)
        | Error::ScheduledJobNotFound(_)
        | Error::SchemaNotFound(_)
        | Error::NotFound(_) => Code::NotFound,
        Error::PreconditionFailed(_) | Error::MissingIndex { .. } => Code::FailedPrecondition,
        Error::ResourceExhausted(_) => Code::ResourceExhausted,
        Error::PermissionDenied(_) => Code::PermissionDenied,
        Error::InvalidInput(_) | Error::SchemaValidation(_) | Error::HistoricalRead { .. } => {
            Code::InvalidArgument
        }
        Error::AlreadyExists(_) => Code::AlreadyExists,
        Error::Transport(_) => Code::Unavailable,
        Error::Storage { kind, .. } => match kind {
            StorageErrorKind::Busy
            | StorageErrorKind::Transient
            | StorageErrorKind::Unavailable => Code::Unavailable,
            StorageErrorKind::Corruption | StorageErrorKind::Io | StorageErrorKind::Other => {
                Code::Internal
            }
        },
        Error::Serialization(_) | Error::Internal(_) => Code::Internal,
        _ => Code::Internal,
    }
}

pub fn firestore_google_rpc_status_json(error: &Error) -> Value {
    let mut object = serde_json::Map::new();
    object.insert("code".to_string(), json!(firestore_grpc_code(error) as i32));
    object.insert("message".to_string(), Value::String(error.to_string()));
    let details = firebase_rest_error(error).details;
    if !details.is_empty() {
        object.insert("details".to_string(), Value::Array(details));
    }
    Value::Object(object)
}

fn firebase_rest_error(error: &Error) -> FirestoreRestError {
    if let Some(fields) = missing_index_fields(error) {
        return FirestoreRestError {
            http_code: StatusCode::BAD_REQUEST,
            status: "FAILED_PRECONDITION",
            details: missing_index_details(&fields, &error.to_string()),
        };
    }

    if let Some(class) = error.commit_class() {
        let (http_code, status) = match class {
            CommitErrorClass::Conflict => (StatusCode::CONFLICT, "ABORTED"),
            CommitErrorClass::Overloaded
            | CommitErrorClass::CommitterFull
            | CommitErrorClass::RateLimited => {
                (StatusCode::TOO_MANY_REQUESTS, "RESOURCE_EXHAUSTED")
            }
            CommitErrorClass::RejectedBeforeExecution => {
                (StatusCode::SERVICE_UNAVAILABLE, "UNAVAILABLE")
            }
            CommitErrorClass::OutOfRetention => {
                (StatusCode::PRECONDITION_FAILED, "FAILED_PRECONDITION")
            }
            CommitErrorClass::CapExceeded => (StatusCode::BAD_REQUEST, "INVALID_ARGUMENT"),
        };
        return FirestoreRestError {
            http_code,
            status,
            details: Vec::new(),
        };
    }

    let (http_code, status) = match error {
        Error::Cancelled => (cancelled_status_code(), "CANCELLED"),
        Error::TenantNotFound(_)
        | Error::DocumentNotFound(_)
        | Error::ScheduledJobNotFound(_)
        | Error::SchemaNotFound(_)
        | Error::NotFound(_) => (StatusCode::NOT_FOUND, "NOT_FOUND"),
        Error::PreconditionFailed(_) | Error::MissingIndex { .. } => {
            (StatusCode::PRECONDITION_FAILED, "FAILED_PRECONDITION")
        }
        Error::ResourceExhausted(_) => (StatusCode::TOO_MANY_REQUESTS, "RESOURCE_EXHAUSTED"),
        Error::PermissionDenied(_) => (StatusCode::FORBIDDEN, "PERMISSION_DENIED"),
        Error::InvalidInput(_) | Error::SchemaValidation(_) | Error::HistoricalRead { .. } => {
            (StatusCode::BAD_REQUEST, "INVALID_ARGUMENT")
        }
        Error::AlreadyExists(_) => (StatusCode::CONFLICT, "ALREADY_EXISTS"),
        Error::Transport(_) => (StatusCode::SERVICE_UNAVAILABLE, "UNAVAILABLE"),
        Error::Storage { kind, .. } => match kind {
            StorageErrorKind::Busy
            | StorageErrorKind::Transient
            | StorageErrorKind::Unavailable => (StatusCode::SERVICE_UNAVAILABLE, "UNAVAILABLE"),
            StorageErrorKind::Corruption | StorageErrorKind::Io | StorageErrorKind::Other => {
                (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL")
            }
        },
        Error::Serialization(_) | Error::Internal(_) => {
            (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL")
        }
        _ => (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL"),
    };
    FirestoreRestError {
        http_code,
        status,
        details: Vec::new(),
    }
}

pub fn firebase_error_response_json(error: Error) -> (StatusCode, Value) {
    let rest_error = firebase_rest_error(&error);
    let mut body = serde_json::Map::new();
    body.insert("code".to_string(), json!(rest_error.http_code.as_u16()));
    body.insert("message".to_string(), Value::String(error.to_string()));
    body.insert(
        "status".to_string(),
        Value::String(rest_error.status.to_string()),
    );
    if !rest_error.details.is_empty() {
        body.insert("details".to_string(), Value::Array(rest_error.details));
    }

    (rest_error.http_code, json!({ "error": body }))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use nimbus_core::{MutationCap, Retryability, TenantId};

    #[test]
    fn firebase_surfaces_full_commit_taxonomy() {
        let cases = [
            (
                Error::retryable_conflict("race", None),
                Code::Aborted,
                StatusCode::CONFLICT,
                "ABORTED",
                Retryability::Retryable,
            ),
            (
                Error::overloaded("busy"),
                Code::ResourceExhausted,
                StatusCode::TOO_MANY_REQUESTS,
                "RESOURCE_EXHAUSTED",
                Retryability::RetryableAfterBackoff,
            ),
            (
                Error::committer_full("full", 128),
                Code::ResourceExhausted,
                StatusCode::TOO_MANY_REQUESTS,
                "RESOURCE_EXHAUSTED",
                Retryability::RetryableAfterBackoff,
            ),
            (
                Error::rejected_before_execution("not started"),
                Code::Unavailable,
                StatusCode::SERVICE_UNAVAILABLE,
                "UNAVAILABLE",
                Retryability::Retryable,
            ),
            (
                Error::rate_limited("hot", Duration::from_millis(100)),
                Code::ResourceExhausted,
                StatusCode::TOO_MANY_REQUESTS,
                "RESOURCE_EXHAUSTED",
                Retryability::RetryableAfterBackoff,
            ),
            (
                Error::out_of_retention("expired", None),
                Code::FailedPrecondition,
                StatusCode::PRECONDITION_FAILED,
                "FAILED_PRECONDITION",
                Retryability::RestartTransaction,
            ),
            (
                Error::cap_exceeded(MutationCap::DocumentsScanned, 11, 10),
                Code::InvalidArgument,
                StatusCode::BAD_REQUEST,
                "INVALID_ARGUMENT",
                Retryability::Terminal,
            ),
        ];

        for (error, grpc_code, http_code, status, retryability) in cases {
            assert_eq!(firestore_grpc_code(&error), grpc_code, "{error}");
            assert_eq!(error.retryability(), retryability, "{error}");
            let (actual_http_code, body) = firebase_error_response_json(error);
            assert_eq!(actual_http_code, http_code);
            assert_eq!(body["error"]["status"], json!(status));
        }
    }

    #[test]
    fn firebase_rest_error_maps_core_statuses() {
        let cases = vec![(
            Error::Cancelled,
            cancelled_status_code(),
            "CANCELLED",
            false,
        )];

        for (error, http_code, status, has_details) in cases {
            let (actual_http_code, body) = firebase_error_response_json(error);
            assert_eq!(actual_http_code, http_code);
            assert_eq!(body["error"]["status"], json!(status));
            assert_eq!(body["error"]["code"], json!(http_code.as_u16()));
            assert_eq!(body["error"].get("details").is_some(), has_details);
        }
    }

    #[test]
    fn firebase_rest_error_maps_full_core_error_surface() {
        let cases = vec![
            (
                Error::TenantNotFound(TenantId::new("demo").expect("tenant id should parse")),
                StatusCode::NOT_FOUND,
                "NOT_FOUND",
            ),
            (Error::conflict("conflict"), StatusCode::CONFLICT, "ABORTED"),
            (
                Error::PreconditionFailed("stale generation".to_string()),
                StatusCode::PRECONDITION_FAILED,
                "FAILED_PRECONDITION",
            ),
            (
                Error::ResourceExhausted("quota".to_string()),
                StatusCode::TOO_MANY_REQUESTS,
                "RESOURCE_EXHAUSTED",
            ),
            (
                Error::PermissionDenied("nope".to_string()),
                StatusCode::FORBIDDEN,
                "PERMISSION_DENIED",
            ),
            (
                Error::InvalidInput("bad input".to_string()),
                StatusCode::BAD_REQUEST,
                "INVALID_ARGUMENT",
            ),
            (
                Error::SchemaValidation("schema".to_string()),
                StatusCode::BAD_REQUEST,
                "INVALID_ARGUMENT",
            ),
            (
                Error::historical_read(
                    nimbus_core::HistoricalReadErrorKind::UnsupportedAdapter,
                    "historical reads are not exposed through Firestore",
                ),
                StatusCode::BAD_REQUEST,
                "INVALID_ARGUMENT",
            ),
            (
                Error::AlreadyExists("exists".to_string()),
                StatusCode::CONFLICT,
                "ALREADY_EXISTS",
            ),
            (
                Error::storage(StorageErrorKind::Unavailable, "later"),
                StatusCode::SERVICE_UNAVAILABLE,
                "UNAVAILABLE",
            ),
            (
                Error::storage(StorageErrorKind::Other, "broken"),
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL",
            ),
            (
                Error::Internal("broken".to_string()),
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL",
            ),
        ];

        for (error, http_code, status) in cases {
            let (actual_http_code, body) = firebase_error_response_json(error);
            assert_eq!(actual_http_code, http_code);
            assert_eq!(body["error"]["status"], json!(status));
            assert_eq!(body["error"].get("details"), None);
        }
    }

    #[test]
    fn firebase_rest_error_uses_failed_precondition_for_missing_index() {
        let error = Error::MissingIndex {
            fields: vec!["state".to_string(), "rank".to_string()],
        };

        let (http_code, body) = firebase_error_response_json(error);

        assert_eq!(http_code, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["status"], json!("FAILED_PRECONDITION"));
        assert_eq!(
            body["error"]["details"][0]["@type"],
            json!("type.googleapis.com/google.rpc.PreconditionFailure")
        );
        assert_eq!(
            body["error"]["details"][0]["violations"][0]["type"],
            json!("FIRESTORE_QUERY_INDEX")
        );
        assert_eq!(
            body["error"]["details"][0]["violations"][0]["subject"],
            json!("fields/state,rank")
        );
    }

    #[test]
    fn firebase_rest_error_does_not_parse_missing_index_from_invalid_input_text() {
        let error = Error::InvalidInput(
            "structured query requires an index covering fields: state, rank".to_string(),
        );

        let (http_code, body) = firebase_error_response_json(error);

        assert_eq!(http_code, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["status"], json!("INVALID_ARGUMENT"));
        assert_eq!(body["error"].get("details"), None);
    }
}
