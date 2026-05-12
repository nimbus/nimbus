use axum::Json;
use axum::http::StatusCode;
use nimbus_core::{Error, StorageErrorKind};
use serde_json::{Value, json};
use tonic::Code;

use super::batch_get_request;
use super::batch_write_request;
use super::commit_request;
use super::list_collection_ids_request;
use super::resource_names;
use super::run_aggregation_query_request;
use super::run_query_request;
use super::transaction_request;
use crate::state::AppError;

pub(crate) fn commit_request_error_to_core(
    error: commit_request::FirestoreCommitRequestError,
) -> Error {
    match error {
        commit_request::FirestoreCommitRequestError::InvalidRequest(_)
        | commit_request::FirestoreCommitRequestError::InvalidResource(_)
        | commit_request::FirestoreCommitRequestError::InvalidValue(_)
        | commit_request::FirestoreCommitRequestError::Unsupported(_) => {
            Error::InvalidInput(error.to_string())
        }
    }
}

pub(crate) fn batch_write_request_error_to_core(
    error: batch_write_request::FirestoreBatchWriteRequestError,
) -> Error {
    match error {
        batch_write_request::FirestoreBatchWriteRequestError::InvalidRequest(_)
        | batch_write_request::FirestoreBatchWriteRequestError::Unsupported(_)
        | batch_write_request::FirestoreBatchWriteRequestError::InvalidResource(_)
        | batch_write_request::FirestoreBatchWriteRequestError::InvalidValue(_) => {
            Error::InvalidInput(error.to_string())
        }
    }
}

pub(crate) fn batch_get_request_error_to_core(
    error: batch_get_request::FirestoreBatchGetRequestError,
) -> Error {
    match error {
        batch_get_request::FirestoreBatchGetRequestError::InvalidRequest(_)
        | batch_get_request::FirestoreBatchGetRequestError::InvalidResource(_)
        | batch_get_request::FirestoreBatchGetRequestError::Unsupported(_) => {
            Error::InvalidInput(error.to_string())
        }
    }
}

pub(crate) fn list_collection_ids_request_error_to_core(
    error: list_collection_ids_request::FirestoreListCollectionIdsRequestError,
) -> Error {
    match error {
        list_collection_ids_request::FirestoreListCollectionIdsRequestError::InvalidRequest(_)
        | list_collection_ids_request::FirestoreListCollectionIdsRequestError::Unsupported(_) => {
            Error::InvalidInput(error.to_string())
        }
    }
}

pub(crate) fn run_query_request_error_to_core(
    error: run_query_request::FirestoreRunQueryRequestError,
) -> Error {
    match error {
        run_query_request::FirestoreRunQueryRequestError::InvalidRequest(_)
        | run_query_request::FirestoreRunQueryRequestError::Unsupported(_) => {
            Error::InvalidInput(error.to_string())
        }
    }
}

pub(crate) fn run_aggregation_query_request_error_to_core(
    error: run_aggregation_query_request::FirestoreRunAggregationQueryRequestError,
) -> Error {
    match error {
        run_aggregation_query_request::FirestoreRunAggregationQueryRequestError::InvalidRequest(
            _,
        )
        | run_aggregation_query_request::FirestoreRunAggregationQueryRequestError::Unsupported(_) => {
            Error::InvalidInput(error.to_string())
        }
    }
}

pub(crate) fn transaction_request_error_to_core(
    error: transaction_request::FirestoreTransactionRequestError,
) -> Error {
    match error {
        transaction_request::FirestoreTransactionRequestError::InvalidRequest(_)
        | transaction_request::FirestoreTransactionRequestError::InvalidResource(_)
        | transaction_request::FirestoreTransactionRequestError::Unsupported(_) => {
            Error::InvalidInput(error.to_string())
        }
    }
}

pub(crate) fn resource_name_error_to_core(
    error: resource_names::FirestoreResourceNameError,
) -> Error {
    Error::InvalidInput(error.to_string())
}

pub(crate) fn firebase_error_to_app(error: Error) -> AppError {
    AppError::from(error)
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
    let Error::InvalidInput(message) = error else {
        return None;
    };
    let fields = message.strip_prefix("structured query requires an index covering fields: ")?;
    Some(
        fields
            .split(',')
            .map(str::trim)
            .filter(|field| !field.is_empty())
            .map(ToString::to_string)
            .collect(),
    )
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

pub(crate) fn firestore_grpc_code(error: &Error) -> Code {
    if missing_index_fields(error).is_some() {
        return Code::FailedPrecondition;
    }

    match error {
        Error::Cancelled => Code::Cancelled,
        Error::TenantNotFound(_)
        | Error::DocumentNotFound(_)
        | Error::ScheduledJobNotFound(_)
        | Error::SchemaNotFound(_) => Code::NotFound,
        Error::Conflict(_) => Code::Aborted,
        Error::ResourceExhausted(_) => Code::ResourceExhausted,
        Error::PermissionDenied(_) => Code::PermissionDenied,
        Error::InvalidInput(_) | Error::SchemaValidation(_) => Code::InvalidArgument,
        Error::AlreadyExists(_) => Code::AlreadyExists,
        Error::Storage { kind, .. } => match kind {
            StorageErrorKind::Busy
            | StorageErrorKind::Transient
            | StorageErrorKind::Unavailable => Code::Unavailable,
            StorageErrorKind::Corruption | StorageErrorKind::Io | StorageErrorKind::Other => {
                Code::Internal
            }
        },
        Error::Serialization(_) | Error::Internal(_) => Code::Internal,
    }
}

pub(crate) fn firestore_google_rpc_status_json(error: &Error) -> Value {
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

    let (http_code, status) = match error {
        Error::Cancelled => (cancelled_status_code(), "CANCELLED"),
        Error::TenantNotFound(_)
        | Error::DocumentNotFound(_)
        | Error::ScheduledJobNotFound(_)
        | Error::SchemaNotFound(_) => (StatusCode::NOT_FOUND, "NOT_FOUND"),
        Error::Conflict(_) => (StatusCode::CONFLICT, "ABORTED"),
        Error::ResourceExhausted(_) => (StatusCode::TOO_MANY_REQUESTS, "RESOURCE_EXHAUSTED"),
        Error::PermissionDenied(_) => (StatusCode::FORBIDDEN, "PERMISSION_DENIED"),
        Error::InvalidInput(_) | Error::SchemaValidation(_) => {
            (StatusCode::BAD_REQUEST, "INVALID_ARGUMENT")
        }
        Error::AlreadyExists(_) => (StatusCode::CONFLICT, "ALREADY_EXISTS"),
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
    };
    FirestoreRestError {
        http_code,
        status,
        details: Vec::new(),
    }
}

pub(crate) fn firebase_error_response(error: Error) -> (StatusCode, Json<Value>) {
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

    (rest_error.http_code, Json(json!({ "error": body })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nimbus_core::TenantId;

    #[test]
    fn firebase_rest_error_maps_core_statuses() {
        let cases = vec![(
            Error::Cancelled,
            cancelled_status_code(),
            "CANCELLED",
            false,
        )];

        for (error, http_code, status, has_details) in cases {
            let (actual_http_code, Json(body)) = firebase_error_response(error);
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
            (
                Error::Conflict("conflict".to_string()),
                StatusCode::CONFLICT,
                "ABORTED",
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
            let (actual_http_code, Json(body)) = firebase_error_response(error);
            assert_eq!(actual_http_code, http_code);
            assert_eq!(body["error"]["status"], json!(status));
            assert_eq!(body["error"].get("details"), None);
        }
    }

    #[test]
    fn firebase_rest_error_uses_failed_precondition_for_missing_index() {
        let error = Error::InvalidInput(
            "structured query requires an index covering fields: state, rank".to_string(),
        );

        let (http_code, Json(body)) = firebase_error_response(error);

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
    }
}
