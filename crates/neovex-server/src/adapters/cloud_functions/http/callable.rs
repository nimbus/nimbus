use std::collections::HashMap;
use std::sync::Arc;

use axum::body::{Body, Bytes};
use axum::extract::OriginalUri;
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, header};
use axum::response::Response;
use neovex_core::{Error, Result, StorageErrorKind, TenantId};
use neovex_runtime::InvocationAuth;
use serde::Serialize;
use serde_json::{Map, Value};

use super::*;
use crate::application_auth::verify_optional_application_auth_from_headers;
use crate::state::{AppError, AppState, record_authenticated_usage};

const CALLABLE_ALLOWED_HEADERS: &str =
    "Content-Type, Authorization, Firebase-Instance-ID-Token, X-Firebase-AppCheck";
const CALLABLE_ALLOWED_METHODS: &str = "POST, OPTIONS";
const APP_CHECK_HEADER_NAME: &str = "x-firebase-appcheck";
const INSTANCE_ID_TOKEN_HEADER_NAME: &str = "firebase-instance-id-token";

pub(super) struct CallableHttpRequest<'a> {
    pub(super) method: &'a Method,
    pub(super) headers: &'a HeaderMap,
    pub(super) original_uri: &'a OriginalUri,
    pub(super) request_path: &'a str,
    pub(super) query: HashMap<String, String>,
    pub(super) body: Bytes,
}

pub(super) async fn handle_callable_target(
    state: Arc<AppState>,
    registry: Arc<CloudFunctionsRegistry>,
    tenant_id: TenantId,
    function_name: String,
    request: CallableHttpRequest<'_>,
) -> std::result::Result<Response, AppError> {
    if request.method == Method::OPTIONS {
        return build_callable_preflight_response(request.headers);
    }
    if request.method != Method::POST {
        return Ok(callable_error_response(
            request.headers,
            StatusCode::METHOD_NOT_ALLOWED,
            "INVALID_ARGUMENT",
            "cloud functions callable handlers only support POST requests",
            None,
        ));
    }
    if header_string(request.headers, APP_CHECK_HEADER_NAME).is_some() {
        return Ok(callable_error_response(
            request.headers,
            StatusCode::NOT_IMPLEMENTED,
            "UNIMPLEMENTED",
            "cloud functions callable App Check verification is not covered in the first callable slice",
            None,
        ));
    }

    let auth = match resolve_callable_auth(&state, request.headers).await {
        Ok(auth) => auth,
        Err(error) => return Ok(callable_response_for_app_error(request.headers, error)),
    };
    record_authenticated_usage(&state, auth.as_ref()).await;

    let args = match build_callable_request_args(
        request.headers,
        request.original_uri,
        request.request_path,
        request.query,
        request.body,
        auth.as_ref(),
    ) {
        Ok(args) => args,
        Err(error) => {
            return Ok(callable_response_for_app_error(
                request.headers,
                AppError::from(error),
            ));
        }
    };
    match execute_http_target(state, registry, tenant_id, function_name, args, auth) {
        Ok(mut response) => {
            apply_callable_cors_headers(request.headers, &mut response);
            Ok(response)
        }
        Err(error) => Ok(callable_response_for_app_error(request.headers, error)),
    }
}

async fn resolve_callable_auth(
    state: &Arc<AppState>,
    headers: &HeaderMap,
) -> std::result::Result<Option<InvocationAuth>, AppError> {
    verify_optional_application_auth_from_headers(state, headers).await
}

pub(super) fn build_callable_request_args(
    headers: &HeaderMap,
    original_uri: &OriginalUri,
    request_path: &str,
    query: HashMap<String, String>,
    body: Bytes,
    auth: Option<&InvocationAuth>,
) -> Result<Value> {
    let normalized_headers = normalized_headers(headers);
    let raw_body = if body.is_empty() {
        return Err(Error::InvalidInput(
            "cloud functions callable handlers require a JSON request body".to_string(),
        ));
    } else {
        std::str::from_utf8(&body)
            .map_err(|error| {
                Error::InvalidInput(format!(
                    "cloud functions callable handlers only cover UTF-8 request bodies in the first slice: {error}"
                ))
            })?
            .to_string()
    };
    if !header_value_contains(headers, header::CONTENT_TYPE, "json") {
        return Err(Error::InvalidInput(
            "cloud functions callable handlers require content-type application/json".to_string(),
        ));
    }
    let body: Value = serde_json::from_str(&raw_body).map_err(|error| {
        Error::InvalidInput(format!(
            "cloud functions callable handler could not parse JSON request body: {error}"
        ))
    })?;
    let data = body
        .as_object()
        .and_then(|body| body.get("data"))
        .cloned()
        .ok_or_else(|| {
            Error::InvalidInput(
                "cloud functions callable handlers require a top-level JSON `data` field"
                    .to_string(),
            )
        })?;

    Ok(serde_json::json!({
        "method": "POST",
        "path": request_path,
        "original_url": request_url(headers, original_uri, request_path),
        "query": query,
        "headers": normalized_headers,
        "body": body,
        "raw_body": raw_body,
        "callable": {
            "data": data,
            "auth": callable_auth_payload(auth)?,
            "instance_id_token": header_string(headers, INSTANCE_ID_TOKEN_HEADER_NAME),
            "accepts_streaming": false,
        },
    }))
}

fn callable_auth_payload(auth: Option<&InvocationAuth>) -> Result<Option<Value>> {
    let Some(auth) = auth else {
        return Ok(None);
    };
    let uid = auth
        .verified_identity
        .as_ref()
        .map(|identity| identity.subject.clone())
        .or_else(|| {
            auth.identity
                .as_ref()
                .map(|identity| identity.subject.clone())
        });
    let token = if let Some(verified_identity) = auth.verified_identity.as_ref() {
        serialize_object(verified_identity)?
    } else if let Some(identity) = auth.identity.as_ref() {
        serialize_object(identity)?
    } else {
        Map::new()
    };
    Ok(Some(serde_json::json!({
        "uid": uid,
        "token": token,
    })))
}

fn serialize_object<T>(value: &T) -> Result<Map<String, Value>>
where
    T: Serialize,
{
    match serde_json::to_value(value).map_err(|error| Error::Serialization(error.to_string()))? {
        Value::Object(map) => Ok(map),
        _ => Ok(Map::new()),
    }
}

pub(super) fn build_callable_preflight_response(
    headers: &HeaderMap,
) -> std::result::Result<Response, AppError> {
    let allow_origin = callable_allow_origin(headers);
    let allow_headers = callable_allow_headers(headers);
    let mut builder = Response::builder().status(StatusCode::NO_CONTENT);
    builder = builder.header("access-control-allow-origin", allow_origin);
    builder = builder.header("access-control-allow-methods", CALLABLE_ALLOWED_METHODS);
    builder = builder.header("access-control-allow-headers", allow_headers);
    builder = builder.header("access-control-max-age", "3600");
    builder = builder.header("vary", "Origin");
    builder.body(Body::empty()).map_err(|error| {
        AppError::from(Error::Internal(format!(
            "cloud functions callable preflight response could not build: {error}"
        )))
    })
}

pub(super) fn callable_response_for_app_error(headers: &HeaderMap, error: AppError) -> Response {
    let (status, callable_status, message) = match error {
        AppError::Structured(error) => {
            let status = error.status();
            let callable_status = match status {
                StatusCode::BAD_REQUEST => "INVALID_ARGUMENT",
                StatusCode::UNAUTHORIZED => "UNAUTHENTICATED",
                StatusCode::FORBIDDEN => "PERMISSION_DENIED",
                StatusCode::NOT_FOUND => "NOT_FOUND",
                StatusCode::CONFLICT => "ABORTED",
                StatusCode::TOO_MANY_REQUESTS => "RESOURCE_EXHAUSTED",
                StatusCode::SERVICE_UNAVAILABLE => "UNAVAILABLE",
                _ => "INTERNAL",
            };
            (status, callable_status, error.message().to_string())
        }
        AppError::Unauthorized(message) => (StatusCode::UNAUTHORIZED, "UNAUTHENTICATED", message),
        AppError::Forbidden(message) => (StatusCode::FORBIDDEN, "PERMISSION_DENIED", message),
        AppError::NotFound(message) => (StatusCode::NOT_FOUND, "NOT_FOUND", message),
        AppError::Core(error) => match error {
            Error::Cancelled => (
                StatusCode::from_u16(499).expect("499 should be a valid status code"),
                "CANCELLED",
                error.to_string(),
            ),
            Error::TenantNotFound(_)
            | Error::DocumentNotFound(_)
            | Error::ScheduledJobNotFound(_)
            | Error::SchemaNotFound(_) => (StatusCode::NOT_FOUND, "NOT_FOUND", error.to_string()),
            Error::Conflict(_) => (StatusCode::CONFLICT, "ABORTED", error.to_string()),
            Error::ResourceExhausted(_) => (
                StatusCode::TOO_MANY_REQUESTS,
                "RESOURCE_EXHAUSTED",
                error.to_string(),
            ),
            Error::PermissionDenied(_) => (
                StatusCode::FORBIDDEN,
                "PERMISSION_DENIED",
                error.to_string(),
            ),
            Error::InvalidInput(_) | Error::SchemaValidation(_) => (
                StatusCode::BAD_REQUEST,
                "INVALID_ARGUMENT",
                error.to_string(),
            ),
            Error::AlreadyExists(_) => (StatusCode::CONFLICT, "ALREADY_EXISTS", error.to_string()),
            Error::Storage { kind, .. } => match kind {
                StorageErrorKind::Busy
                | StorageErrorKind::Transient
                | StorageErrorKind::Unavailable => (
                    StatusCode::SERVICE_UNAVAILABLE,
                    "UNAVAILABLE",
                    error.to_string(),
                ),
                StorageErrorKind::Corruption | StorageErrorKind::Io | StorageErrorKind::Other => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "INTERNAL",
                    error.to_string(),
                ),
            },
            _ => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL",
                error.to_string(),
            ),
        },
    };
    callable_error_response(headers, status, callable_status, &message, None)
}

pub(super) fn callable_error_response(
    request_headers: &HeaderMap,
    status: StatusCode,
    callable_status: &str,
    message: &str,
    details: Option<Value>,
) -> Response {
    let mut error = serde_json::json!({
        "status": callable_status,
        "message": message,
    });
    if let Some(details) = details
        && let Some(object) = error.as_object_mut()
    {
        object.insert("details".to_string(), details);
    }
    let body = serde_json::json!({ "error": error });
    let bytes = serde_json::to_vec(&body).expect("callable error body should encode");
    let mut response = Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(bytes))
        .expect("callable error response should build");
    apply_callable_cors_headers(request_headers, &mut response);
    response
}

pub(super) fn apply_callable_cors_headers(request_headers: &HeaderMap, response: &mut Response) {
    let allow_origin = callable_allow_origin(request_headers);
    response.headers_mut().insert(
        HeaderName::from_static("access-control-allow-origin"),
        HeaderValue::from_str(&allow_origin).expect("callable allow-origin should be valid"),
    );
    response
        .headers_mut()
        .insert(header::VARY, HeaderValue::from_static("Origin"));
}

fn callable_allow_origin(headers: &HeaderMap) -> String {
    header_string(headers, header::ORIGIN.as_str()).unwrap_or_else(|| "*".to_string())
}

fn callable_allow_headers(headers: &HeaderMap) -> String {
    header_string(headers, "access-control-request-headers")
        .unwrap_or_else(|| CALLABLE_ALLOWED_HEADERS.to_string())
}

fn header_string(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}
