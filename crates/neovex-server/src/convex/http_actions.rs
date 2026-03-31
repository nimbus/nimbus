#![cfg_attr(test, allow(dead_code))]

use super::dispatch::{
    check_host_cancellation, execute_convex_action_async, execute_convex_action_cancellable,
    invoke_named_convex_function_async_cancellable,
};
use super::*;
use crate::state::RequestCancellationGuard;

pub(super) async fn dispatch_http_route(
    state: Arc<AppState>,
    tenant_id: String,
    route_request: ConvexHttpRouteRequest,
) -> Result<Response, AppError> {
    let tenant_id = TenantId::new(tenant_id)?;
    let registry = state
        .convex_registry
        .clone()
        .expect("convex http route requires Convex support state");
    let request_auth = registry
        .verify_authorization_header(&route_request.headers)
        .await?;
    crate::state::record_authenticated_usage(&state, request_auth.as_ref()).await;
    let route = registry
        .resolve_http_route(&route_request.method, &route_request.request_path)
        .cloned();
    let Some(route) = route else {
        let status = if registry.has_http_route_for_path(&route_request.request_path) {
            StatusCode::METHOD_NOT_ALLOWED
        } else {
            StatusCode::NOT_FOUND
        };
        return Ok((
            status,
            Json(json!({ "error": "convex http route not found" })),
        )
            .into_response());
    };

    let request_context = build_http_request_context(
        &route_request.method,
        &route_request.headers,
        &route_request.original_uri,
        &route_request.request_path,
        route_request.query,
        route_request.body,
    );
    let service = state.service.clone();
    if registry.runtime_bundle().is_some() && route.name.is_some() {
        let request_cancellation = RequestCancellationGuard::new();
        let (runtime_identity, verified_identity) = match request_auth {
            Some(auth) => (auth.identity, auth.verified_identity),
            None => (None, None),
        };
        let runtime_auth = Some(InvocationAuth {
            identity: runtime_identity,
            verified_identity,
            throw_on_missing_identity: true,
        });
        let response = invoke_named_convex_function_async_cancellable(
            &service,
            &registry,
            &tenant_id,
            InvocationRequest {
                kind: InvocationKind::Action,
                function_name: route
                    .name
                    .clone()
                    .expect("runtime-eligible http route should have a name"),
                args: serde_json::to_value(&request_context)
                    .map_err(|error| AppError::from(Error::Serialization(error.to_string())))?,
                page_size: None,
                cursor: None,
                auth: runtime_auth,
            },
            request_cancellation.token(),
        )
        .await?;
        let response: ConvexHttpResponseParts = serde_json::from_value(response)
            .map_err(|error| AppError::from(Error::Serialization(error.to_string())))?;
        return build_http_response_parts(response).map_err(AppError::from);
    }

    execute_http_action_async(
        &service,
        &registry,
        &tenant_id,
        &route.plan,
        &request_context,
    )
    .await
    .map_err(AppError::from)
}

fn build_http_request_context(
    method: &Method,
    headers: &HeaderMap,
    original_uri: &OriginalUri,
    request_path: &str,
    query: HashMap<String, String>,
    body: Bytes,
) -> ConvexHttpRequestContext {
    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("http");
    let host = headers
        .get(axum::http::header::HOST)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("localhost");
    let query_suffix = original_uri
        .0
        .query()
        .map(|query| format!("?{query}"))
        .unwrap_or_default();
    let url = format!("{scheme}://{host}{request_path}{query_suffix}");
    let text = String::from_utf8_lossy(&body).into_owned();
    let normalized_headers = headers
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_ascii_lowercase(), value.to_string()))
        })
        .collect();

    ConvexHttpRequestContext {
        method: method.as_str().to_string(),
        url,
        pathname: request_path.to_string(),
        query,
        headers: normalized_headers,
        body_bytes: body.to_vec(),
        body_text: text,
    }
}

#[cfg(test)]
fn execute_http_action(
    service: &neovex_engine::Service,
    registry: &ConvexRegistry,
    tenant_id: &TenantId,
    plan: &ConvexHttpActionPlan,
    request: &ConvexHttpRequestContext,
) -> Result<Response, Error> {
    let response = prepare_http_action_response(service, registry, tenant_id, plan, request)?;
    build_http_response_parts(response)
}

async fn execute_http_action_async(
    service: &Arc<neovex_engine::Service>,
    registry: &Arc<ConvexRegistry>,
    tenant_id: &TenantId,
    plan: &ConvexHttpActionPlan,
    request: &ConvexHttpRequestContext,
) -> Result<Response, Error> {
    let response =
        prepare_http_action_response_async(service, registry, tenant_id, plan, request).await?;
    build_http_response_parts(response)
}

#[cfg(test)]
pub(super) fn prepare_http_action_response(
    service: &neovex_engine::Service,
    registry: &ConvexRegistry,
    tenant_id: &TenantId,
    plan: &ConvexHttpActionPlan,
    request: &ConvexHttpRequestContext,
) -> Result<ConvexHttpResponseParts, Error> {
    let operation_result = if let Some(operation_template) = plan.operation.as_ref() {
        let resolved = resolve_http_template(operation_template, request, None)?;
        let operation: ConvexExecutableAction =
            serde_json::from_value(resolved).map_err(|error| {
                Error::InvalidInput(format!(
                    "convex http route resolved to invalid operation: {error}"
                ))
            })?;
        Some(super::dispatch::execute_convex_action(
            service, registry, tenant_id, operation,
        )?)
    } else {
        None
    };

    let body = resolve_http_template(&plan.response.body, request, operation_result.as_ref())?;
    let status = plan
        .response
        .status
        .as_ref()
        .map(|status| resolve_http_template(status, request, operation_result.as_ref()))
        .transpose()?;
    let headers = plan
        .response
        .headers
        .as_ref()
        .map(|headers| resolve_http_template(headers, request, operation_result.as_ref()))
        .transpose()?;

    Ok(ConvexHttpResponseParts {
        kind: plan.response.kind,
        body,
        status,
        headers,
    })
}

pub(super) fn prepare_http_action_response_cancellable(
    service: &neovex_engine::Service,
    registry: &ConvexRegistry,
    tenant_id: &TenantId,
    plan: &ConvexHttpActionPlan,
    request: &ConvexHttpRequestContext,
    cancellation: &HostCallCancellation,
) -> Result<ConvexHttpResponseParts, Error> {
    check_host_cancellation(cancellation)?;
    let operation_result = if let Some(operation_template) = plan.operation.as_ref() {
        let resolved = resolve_http_template(operation_template, request, None)?;
        let operation: ConvexExecutableAction =
            serde_json::from_value(resolved).map_err(|error| {
                Error::InvalidInput(format!(
                    "convex http route resolved to invalid operation: {error}"
                ))
            })?;
        Some(execute_convex_action_cancellable(
            service,
            registry,
            tenant_id,
            operation,
            cancellation,
        )?)
    } else {
        None
    };

    let body = resolve_http_template(&plan.response.body, request, operation_result.as_ref())?;
    let status = plan
        .response
        .status
        .as_ref()
        .map(|status| resolve_http_template(status, request, operation_result.as_ref()))
        .transpose()?;
    let headers = plan
        .response
        .headers
        .as_ref()
        .map(|headers| resolve_http_template(headers, request, operation_result.as_ref()))
        .transpose()?;

    Ok(ConvexHttpResponseParts {
        kind: plan.response.kind,
        body,
        status,
        headers,
    })
}

pub(super) async fn prepare_http_action_response_async(
    service: &Arc<neovex_engine::Service>,
    registry: &Arc<ConvexRegistry>,
    tenant_id: &TenantId,
    plan: &ConvexHttpActionPlan,
    request: &ConvexHttpRequestContext,
) -> Result<ConvexHttpResponseParts, Error> {
    let operation_result = if let Some(operation_template) = plan.operation.as_ref() {
        let resolved = resolve_http_template(operation_template, request, None)?;
        let operation: ConvexExecutableAction =
            serde_json::from_value(resolved).map_err(|error| {
                Error::InvalidInput(format!(
                    "convex http route resolved to invalid operation: {error}"
                ))
            })?;
        Some(execute_convex_action_async(service, registry, tenant_id, operation, None).await?)
    } else {
        None
    };

    let body = resolve_http_template(&plan.response.body, request, operation_result.as_ref())?;
    let status = plan
        .response
        .status
        .as_ref()
        .map(|status| resolve_http_template(status, request, operation_result.as_ref()))
        .transpose()?;
    let headers = plan
        .response
        .headers
        .as_ref()
        .map(|headers| resolve_http_template(headers, request, operation_result.as_ref()))
        .transpose()?;

    Ok(ConvexHttpResponseParts {
        kind: plan.response.kind,
        body,
        status,
        headers,
    })
}

fn build_http_response_parts(parts: ConvexHttpResponseParts) -> Result<Response, Error> {
    build_http_response(parts.kind, parts.body, parts.status, parts.headers)
}

fn build_http_response(
    kind: ConvexHttpResponseKind,
    body: Value,
    status: Option<Value>,
    headers: Option<Value>,
) -> Result<Response, Error> {
    let status = parse_http_status(status)?;
    let mut builder = Response::builder().status(status);
    let header_map = parse_http_headers(headers)?;
    let has_content_type = header_map
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("content-type"));

    for (name, value) in header_map {
        builder = builder.header(name, value);
    }

    if kind == ConvexHttpResponseKind::Json && !has_content_type {
        builder = builder.header("content-type", "application/json");
    }

    let body = match kind {
        ConvexHttpResponseKind::Json => {
            serde_json::to_vec(&body).map_err(|error| Error::Serialization(error.to_string()))?
        }
        ConvexHttpResponseKind::Text => render_http_text_body(body)?.into_bytes(),
    };

    builder
        .body(axum::body::Body::from(body))
        .map_err(|error| Error::Internal(error.to_string()))
}

fn parse_http_status(status: Option<Value>) -> Result<StatusCode, Error> {
    let Some(status) = status else {
        return Ok(StatusCode::OK);
    };
    let code = status.as_u64().ok_or_else(|| {
        Error::InvalidInput("convex http response status must be a number".to_string())
    })?;
    StatusCode::from_u16(code as u16).map_err(|error| {
        Error::InvalidInput(format!("invalid convex http response status: {error}"))
    })
}

fn parse_http_headers(headers: Option<Value>) -> Result<Vec<(String, String)>, Error> {
    let Some(headers) = headers else {
        return Ok(Vec::new());
    };
    let Value::Object(object) = headers else {
        return Err(Error::InvalidInput(
            "convex http response headers must resolve to a JSON object".to_string(),
        ));
    };
    object
        .into_iter()
        .filter_map(|(name, value)| match value {
            Value::Null => None,
            Value::String(value) => Some(Ok((name, value))),
            Value::Number(value) => Some(Ok((name, value.to_string()))),
            Value::Bool(value) => Some(Ok((name, value.to_string()))),
            _ => Some(Err(Error::InvalidInput(format!(
                "convex http response header {name} must resolve to a string-coercible value"
            )))),
        })
        .collect()
}

fn render_http_text_body(body: Value) -> Result<String, Error> {
    match body {
        Value::Null => Ok(String::new()),
        Value::String(value) => Ok(value),
        Value::Bool(value) => Ok(value.to_string()),
        Value::Number(value) => Ok(value.to_string()),
        _ => Err(Error::InvalidInput(
            "convex http text responses must resolve to a string-coercible value".to_string(),
        )),
    }
}
