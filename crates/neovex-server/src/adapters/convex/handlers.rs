use std::collections::HashMap;
use std::sync::Arc;

use axum::Json;
use axum::body::Bytes;
use axum::extract::{
    OriginalUri, Path as AxumPath, Query as AxumQuery, State, ws::WebSocketUpgrade,
};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::Response;
use neovex_core::{Error, ScheduleRequest, TenantId, Timestamp};
use neovex_runtime::{InvocationKind, InvocationRequest};
use serde_json::Value;

use super::dispatch::{
    check_host_cancellation, dispatch_convex_mutation_async, execute_convex_action_async,
    execute_query_result_async, invoke_named_convex_function_async_cancellable,
    next_runtime_server_request_id,
};
use super::http_actions::dispatch_http_route;
use super::subscriptions::handle_convex_socket_for_tenant;
use super::templates::{normalize_http_request_path, parse_job_id};
use super::{
    ConvexActionRequest, ConvexExecutableAction, ConvexExecutableMutation, ConvexExecutableQuery,
    ConvexMutationRequest, ConvexPaginatedQueryRequest, ConvexQueryRequest,
    ConvexScheduleAfterRequest, ConvexScheduleAtRequest,
};
use crate::protocol::ScheduleResponse;
use crate::state::{AppError, AppState, RequestCancellationGuard, record_authenticated_usage};

pub(crate) async fn query(
    State(state): State<Arc<AppState>>,
    AxumPath(tenant_id): AxumPath<String>,
    headers: HeaderMap,
    Json(request): Json<ConvexQueryRequest>,
) -> Result<Json<Value>, AppError> {
    let tenant_id = TenantId::new(tenant_id)?;
    let service = state.service.clone();
    let registry = state
        .convex_registry
        .clone()
        .expect("convex query route requires Convex support state");
    let auth = registry.verify_authorization_header(&headers).await?;
    record_authenticated_usage(&state, auth.as_ref()).await;
    let data = match request {
        ConvexQueryRequest::Named(request) if registry.runtime_bundle().is_some() => {
            let request_cancellation = RequestCancellationGuard::new();
            invoke_named_convex_function_async_cancellable(
                &service,
                &registry,
                &tenant_id,
                InvocationRequest {
                    kind: InvocationKind::Query,
                    function_name: request.name,
                    args: request.args,
                    page_size: None,
                    cursor: None,
                    auth: auth.clone(),
                },
                request_cancellation.token(),
                Some(next_runtime_server_request_id("convex-query")),
            )
            .await?
        }
        ConvexQueryRequest::Named(request) => {
            let query = registry.resolve_query(&request.name, &request.args)?;
            let request_cancellation = RequestCancellationGuard::new();
            execute_query_result_async(
                &service,
                &tenant_id,
                query,
                Some(request_cancellation.token()),
            )
            .await?
        }
        ConvexQueryRequest::Raw { query } => {
            let request_cancellation = RequestCancellationGuard::new();
            execute_query_result_async(
                &service,
                &tenant_id,
                ConvexExecutableQuery::Query(query),
                Some(request_cancellation.token()),
            )
            .await?
        }
    };
    Ok(Json(data))
}

/// Executes a Convex-style paginated query over Neovex's pagination engine.
pub(crate) async fn paginated_query(
    State(state): State<Arc<AppState>>,
    AxumPath(tenant_id): AxumPath<String>,
    headers: HeaderMap,
    Json(request): Json<ConvexPaginatedQueryRequest>,
) -> Result<Json<neovex_core::Page>, AppError> {
    let tenant_id = TenantId::new(tenant_id)?;
    let service = state.service.clone();
    let registry = state
        .convex_registry
        .clone()
        .expect("convex paginated query route requires Convex support state");
    let auth = registry.verify_authorization_header(&headers).await?;
    record_authenticated_usage(&state, auth.as_ref()).await;
    let page = match request {
        ConvexPaginatedQueryRequest::Named(request) if registry.runtime_bundle().is_some() => {
            let request_cancellation = RequestCancellationGuard::new();
            let value = invoke_named_convex_function_async_cancellable(
                &service,
                &registry,
                &tenant_id,
                InvocationRequest {
                    kind: InvocationKind::PaginatedQuery,
                    function_name: request.name,
                    args: request.args,
                    page_size: Some(request.page_size),
                    cursor: request.cursor,
                    auth: auth.clone(),
                },
                request_cancellation.token(),
                Some(next_runtime_server_request_id("convex-paginated-query")),
            )
            .await?;
            serde_json::from_value(value)
                .map_err(|error| AppError::from(Error::Serialization(error.to_string())))?
        }
        ConvexPaginatedQueryRequest::Named(request) => {
            let query = registry.resolve_paginated_query(
                &request.name,
                &request.args,
                request.page_size,
                request.cursor,
            )?;
            let request_cancellation = RequestCancellationGuard::new();
            let cancellation = request_cancellation.token();
            let cancellation_check = cancellation.clone();
            service
                .paginate_documents_async_cancellable(
                    tenant_id.clone(),
                    query,
                    cancellation.cancelled(),
                    move || check_host_cancellation(&cancellation_check),
                )
                .await?
        }
        ConvexPaginatedQueryRequest::Raw { query } => {
            let request_cancellation = RequestCancellationGuard::new();
            let cancellation = request_cancellation.token();
            let cancellation_check = cancellation.clone();
            service
                .paginate_documents_async_cancellable(
                    tenant_id.clone(),
                    query,
                    cancellation.cancelled(),
                    move || check_host_cancellation(&cancellation_check),
                )
                .await?
        }
    };
    Ok(Json(page))
}

/// Executes a Convex-style mutation over Neovex's existing mutation engine.
pub(crate) async fn mutation(
    State(state): State<Arc<AppState>>,
    AxumPath(tenant_id): AxumPath<String>,
    headers: HeaderMap,
    Json(request): Json<ConvexMutationRequest>,
) -> Result<Json<Value>, AppError> {
    let tenant_id = TenantId::new(tenant_id)?;
    let service = state.service.clone();
    let registry = state
        .convex_registry
        .clone()
        .expect("convex mutation route requires Convex support state");
    let auth = registry.verify_authorization_header(&headers).await?;
    record_authenticated_usage(&state, auth.as_ref()).await;
    let value = match request {
        ConvexMutationRequest::Named(request) if registry.runtime_bundle().is_some() => {
            let request_cancellation = RequestCancellationGuard::new();
            invoke_named_convex_function_async_cancellable(
                &service,
                &registry,
                &tenant_id,
                InvocationRequest {
                    kind: InvocationKind::Mutation,
                    function_name: request.name,
                    args: request.args,
                    page_size: None,
                    cursor: None,
                    auth: auth.clone(),
                },
                request_cancellation.token(),
                Some(next_runtime_server_request_id("convex-mutation")),
            )
            .await?
        }
        ConvexMutationRequest::Named(request) => {
            let mutation = registry.resolve_mutation(&request.name, &request.args)?;
            dispatch_convex_mutation_async(&service, &registry, &tenant_id, mutation, None).await?
        }
        ConvexMutationRequest::Raw { mutation } => {
            dispatch_convex_mutation_async(
                &service,
                &registry,
                &tenant_id,
                ConvexExecutableMutation::Mutation(mutation),
                None,
            )
            .await?
        }
    };
    Ok(Json(value))
}

/// Executes a Convex-style action backed by an existing Neovex operation.
pub(crate) async fn action(
    State(state): State<Arc<AppState>>,
    AxumPath(tenant_id): AxumPath<String>,
    headers: HeaderMap,
    Json(request): Json<ConvexActionRequest>,
) -> Result<Json<Value>, AppError> {
    let tenant_id = TenantId::new(tenant_id)?;
    let service = state.service.clone();
    let registry = state
        .convex_registry
        .clone()
        .expect("convex action route requires Convex support state");
    let auth = registry.verify_authorization_header(&headers).await?;
    record_authenticated_usage(&state, auth.as_ref()).await;
    let value = match request {
        ConvexActionRequest::Named(request) if registry.runtime_bundle().is_some() => {
            let request_cancellation = RequestCancellationGuard::new();
            invoke_named_convex_function_async_cancellable(
                &service,
                &registry,
                &tenant_id,
                InvocationRequest {
                    kind: InvocationKind::Action,
                    function_name: request.name,
                    args: request.args,
                    page_size: None,
                    cursor: None,
                    auth: auth.clone(),
                },
                request_cancellation.token(),
                Some(next_runtime_server_request_id("convex-action")),
            )
            .await?
        }
        ConvexActionRequest::Named(request) => {
            let action = registry.resolve_action(&request.name, &request.args)?;
            execute_convex_action_async(&service, &registry, &tenant_id, action, None).await?
        }
        ConvexActionRequest::Raw { action } => {
            execute_convex_action_async(
                &service,
                &registry,
                &tenant_id,
                ConvexExecutableAction::Action(action),
                None,
            )
            .await?
        }
    };
    Ok(Json(value))
}

/// Executes a Convex-style httpAction route backed by the convex manifest.
pub(crate) async fn http_route_root(
    State(state): State<Arc<AppState>>,
    AxumPath(tenant_id): AxumPath<String>,
    method: Method,
    headers: HeaderMap,
    original_uri: OriginalUri,
    query: AxumQuery<HashMap<String, String>>,
    body: Bytes,
) -> Result<Response, AppError> {
    dispatch_http_route(
        state,
        tenant_id,
        super::dispatch::ConvexHttpRouteRequest {
            request_path: "/".to_string(),
            method,
            headers,
            original_uri,
            query: query.0,
            body,
        },
    )
    .await
}

/// Executes a Convex-style httpAction route backed by the convex manifest.
pub(crate) async fn http_route(
    State(state): State<Arc<AppState>>,
    AxumPath((tenant_id, path)): AxumPath<(String, String)>,
    method: Method,
    headers: HeaderMap,
    original_uri: OriginalUri,
    query: AxumQuery<HashMap<String, String>>,
    body: Bytes,
) -> Result<Response, AppError> {
    dispatch_http_route(
        state,
        tenant_id,
        super::dispatch::ConvexHttpRouteRequest {
            request_path: normalize_http_request_path(&path),
            method,
            headers,
            original_uri,
            query: query.0,
            body,
        },
    )
    .await
}

/// Schedules a public convex mutation by relative delay.
pub(crate) async fn schedule_after(
    State(state): State<Arc<AppState>>,
    AxumPath(tenant_id): AxumPath<String>,
    headers: HeaderMap,
    Json(request): Json<ConvexScheduleAfterRequest>,
) -> Result<(StatusCode, Json<ScheduleResponse>), AppError> {
    let tenant_id = TenantId::new(tenant_id)?;
    let service = state.service.clone();
    let registry = state
        .convex_registry
        .clone()
        .expect("convex schedule-after route requires Convex support state");
    let auth = registry.verify_authorization_header(&headers).await?;
    record_authenticated_usage(&state, auth.as_ref()).await;
    let request = match request {
        ConvexScheduleAfterRequest::Named(request) => ScheduleRequest {
            run_after_ms: request.run_after_ms,
            mutation: registry.resolve_scheduled_mutation(&request.name, &request.args)?,
        },
        ConvexScheduleAfterRequest::Raw {
            mutation,
            run_after_ms,
        } => ScheduleRequest {
            run_after_ms,
            mutation,
        },
    };

    let job_id = service.schedule_mutation_async(tenant_id, request).await?;
    Ok((
        StatusCode::CREATED,
        Json(ScheduleResponse {
            job_id: job_id.to_string(),
        }),
    ))
}

/// Schedules a public convex mutation for an absolute timestamp.
pub(crate) async fn schedule_at(
    State(state): State<Arc<AppState>>,
    AxumPath(tenant_id): AxumPath<String>,
    headers: HeaderMap,
    Json(request): Json<ConvexScheduleAtRequest>,
) -> Result<(StatusCode, Json<ScheduleResponse>), AppError> {
    let tenant_id = TenantId::new(tenant_id)?;
    let service = state.service.clone();
    let registry = state
        .convex_registry
        .clone()
        .expect("convex schedule-at route requires Convex support state");
    let auth = registry.verify_authorization_header(&headers).await?;
    record_authenticated_usage(&state, auth.as_ref()).await;
    let (run_at_ms, mutation) = match request {
        ConvexScheduleAtRequest::Named(request) => (
            request.run_at_ms,
            registry.resolve_scheduled_mutation(&request.name, &request.args)?,
        ),
        ConvexScheduleAtRequest::Raw {
            mutation,
            run_at_ms,
        } => (run_at_ms, mutation),
    };
    let delay_ms = run_at_ms.saturating_sub(Timestamp::now().0);
    let request = ScheduleRequest {
        run_after_ms: delay_ms,
        mutation,
    };

    let job_id = service.schedule_mutation_async(tenant_id, request).await?;
    Ok((
        StatusCode::CREATED,
        Json(ScheduleResponse {
            job_id: job_id.to_string(),
        }),
    ))
}

/// Cancels a pending convex scheduled job.
pub(crate) async fn cancel_scheduled_job(
    State(state): State<Arc<AppState>>,
    AxumPath((tenant_id, job_id)): AxumPath<(String, String)>,
    headers: HeaderMap,
) -> Result<StatusCode, AppError> {
    let tenant_id = TenantId::new(tenant_id)?;
    let job_id = parse_job_id(&job_id)?;
    let service = state.service.clone();
    let registry = state
        .convex_registry
        .clone()
        .expect("convex scheduled job cancel route requires Convex support state");
    let auth = registry.verify_authorization_header(&headers).await?;
    record_authenticated_usage(&state, auth.as_ref()).await;
    service
        .cancel_scheduled_job_async(tenant_id, job_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// WebSocket endpoint for Convex-style query subscriptions bound to a tenant in the URL.
pub(crate) async fn ws(
    State(state): State<Arc<AppState>>,
    AxumPath(tenant_id): AxumPath<String>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, AppError> {
    let tenant_id = TenantId::new(tenant_id)?;
    let service = state.service.clone();
    let tenant_check = tenant_id.clone();
    service.ensure_tenant_exists_async(tenant_check).await?;
    let registry = state
        .convex_registry
        .clone()
        .expect("convex websocket route requires Convex support state");
    let auth = registry.verify_authorization_header(&headers).await?;
    record_authenticated_usage(&state, auth.as_ref()).await;

    Ok(
        ws.on_upgrade(move |socket| {
            handle_convex_socket_for_tenant(socket, state, tenant_id, auth)
        }),
    )
}
