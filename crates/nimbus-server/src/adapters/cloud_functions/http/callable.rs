use std::collections::HashMap;
use std::sync::Arc;

use axum::body::{Body, Bytes};
use axum::extract::OriginalUri;
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, header};
use axum::response::Response;
use nimbus_cloud_functions::build_callable_request_args;
use nimbus_core::InvocationAuth;
use nimbus_core::{Error, TenantId};
use serde_json::Value;

use super::*;
use crate::application_auth::verify_optional_application_auth_from_headers_in_deployment;
use crate::error_envelope::StructuredHttpError;
use crate::state::DeploymentState;
use crate::state::{AppError, AppState, record_authenticated_usage};

const CALLABLE_ALLOWED_HEADERS: &str =
    "Content-Type, Authorization, Firebase-Instance-ID-Token, X-Firebase-AppCheck";
const CALLABLE_ALLOWED_METHODS: &str = "POST, OPTIONS";
const APP_CHECK_HEADER_NAME: &str = "x-firebase-appcheck";

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
    deployment: Arc<DeploymentState>,
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

    let auth = match resolve_callable_auth(deployment.as_ref(), request.headers).await {
        Ok(auth) => auth,
        Err(error) => return Ok(callable_response_for_app_error(request.headers, error)),
    };
    record_authenticated_usage(&state, auth.as_ref()).await;

    let args = match build_callable_request_args(
        request.headers,
        request.original_uri.0.query(),
        request.request_path,
        request.query,
        &request.body,
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
    match execute_http_target(ServerCloudFunctionsHttpInvocation {
        engine: state.engine.clone(),
        runtime_service_registry: state.runtime_service_registry(),
        tenant_isolation_mode: state.tenant_isolation_mode(),
        registry,
        deployment_generation: deployment.generation,
        tenant_id,
        function_name,
        args,
        auth,
    }) {
        Ok(mut response) => {
            apply_callable_cors_headers(request.headers, &mut response);
            Ok(response)
        }
        Err(error) => Ok(callable_response_for_app_error(request.headers, error)),
    }
}

async fn resolve_callable_auth(
    deployment: &DeploymentState,
    headers: &HeaderMap,
) -> std::result::Result<Option<InvocationAuth>, AppError> {
    verify_optional_application_auth_from_headers_in_deployment(deployment, headers).await
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
    if let Some(response) = callable_legacy_status_override(headers, &error) {
        return response;
    }
    let structured = StructuredHttpError::from_app_error(error);
    let status = structured.status();
    callable_error_response(
        headers,
        status,
        callable_status_for_http_status(status),
        structured.message(),
        None,
    )
}

/// The callable surface previously matched every `Error` variant by hand and disagreed
/// with the canonical `StructuredHttpError::from_app_error` mapping (or with the generic
/// [`callable_status_for_http_status`] text derived from it) for six of them:
///
/// - `Cancelled` used HTTP 499, not the canonical 408.
/// - `MissingIndex` and `HistoricalRead` fell through to a flat 500/`INTERNAL`, not the
///   canonical per-kind status (412 and varies-by-kind, respectively).
/// - `SchemaValidation` used HTTP 400, not the canonical 422.
/// - `AlreadyExists` and `PreconditionFailed` already match the canonical HTTP status, but
///   the generic status-to-text mapping would relabel them `ABORTED`/`INTERNAL` instead of
///   the callable-specific `ALREADY_EXISTS`/`FAILED_PRECONDITION` text.
///
/// Preserve today's exact status and text for these six variants; everything else defers
/// to the canonical mapping.
fn callable_legacy_status_override(headers: &HeaderMap, error: &AppError) -> Option<Response> {
    let AppError::Core(core_error) = error else {
        return None;
    };
    match core_error {
        Error::Cancelled => Some(callable_error_response(
            headers,
            StatusCode::from_u16(499).expect("499 should be a valid status code"),
            "CANCELLED",
            &core_error.to_string(),
            None,
        )),
        Error::AlreadyExists(_) => Some(callable_error_response(
            headers,
            StatusCode::CONFLICT,
            "ALREADY_EXISTS",
            &core_error.to_string(),
            None,
        )),
        Error::PreconditionFailed(_) => Some(callable_error_response(
            headers,
            StatusCode::PRECONDITION_FAILED,
            "FAILED_PRECONDITION",
            &core_error.to_string(),
            None,
        )),
        Error::MissingIndex { .. } => Some(callable_error_response(
            headers,
            StatusCode::INTERNAL_SERVER_ERROR,
            "INTERNAL",
            &core_error.to_string(),
            None,
        )),
        Error::SchemaValidation(_) => Some(callable_error_response(
            headers,
            StatusCode::BAD_REQUEST,
            "INVALID_ARGUMENT",
            &core_error.to_string(),
            None,
        )),
        Error::HistoricalRead { .. } => Some(callable_error_response(
            headers,
            StatusCode::INTERNAL_SERVER_ERROR,
            "INTERNAL",
            &core_error.to_string(),
            None,
        )),
        _ => None,
    }
}

fn callable_status_for_http_status(status: StatusCode) -> &'static str {
    match status {
        StatusCode::BAD_REQUEST => "INVALID_ARGUMENT",
        StatusCode::UNAUTHORIZED => "UNAUTHENTICATED",
        StatusCode::FORBIDDEN => "PERMISSION_DENIED",
        StatusCode::NOT_FOUND => "NOT_FOUND",
        StatusCode::CONFLICT => "ABORTED",
        StatusCode::TOO_MANY_REQUESTS => "RESOURCE_EXHAUSTED",
        StatusCode::SERVICE_UNAVAILABLE => "UNAVAILABLE",
        _ => "INTERNAL",
    }
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
