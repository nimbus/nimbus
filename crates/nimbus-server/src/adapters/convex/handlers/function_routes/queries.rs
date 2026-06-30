use super::*;
use crate::adapters::convex::execution::RuntimeInvocationContext;
use crate::adapters::convex::runtime_auth_payload;
use crate::latency::{LatencySegment, budgeted_segment};
use nimbus_auth::normalize_principal_context;

pub(crate) async fn query(
    State(state): State<Arc<AppState>>,
    AxumPath(tenant_id): AxumPath<String>,
    headers: HeaderMap,
    Json(request): Json<ConvexQueryRequest>,
) -> Result<Json<Value>, AppError> {
    let service = state.engine.clone();
    let auth_timer = budgeted_segment(LatencySegment::Auth);
    let (registry, auth, tenant_context) = registry_and_auth_for_path(
        &state,
        crate::local_server::LocalServerRouteFamily::ConvexHttp,
        tenant_id,
        &headers,
        "convex query route requires Convex support state",
    )
    .await?;
    auth_timer.finish();
    let tenant_id = tenant_context.tenant_id().clone();
    let trace = match &request {
        ConvexQueryRequest::Named(request) => RunTrace::new(request.name.clone(), "query"),
        ConvexQueryRequest::Raw { .. } => RunTrace::new("<raw-query>", "query"),
    };
    let result = match request {
        ConvexQueryRequest::Named(request)
            if registry.has_runtime_bundle_for_function(&request.name) =>
        {
            let runtime_timer = budgeted_segment(LatencySegment::Runtime);
            let request_cancellation = RequestCancellationGuard::new();
            let runtime_service_registry = state.runtime_service_registry();
            let context = RuntimeInvocationContext::new(
                &service,
                &registry,
                &runtime_service_registry,
                tenant_context.clone(),
                state.tenant_isolation_mode,
            );
            let result = invoke_named_convex_function_async_cancellable(
                &context,
                InvocationRequest {
                    kind: InvocationKind::Query,
                    function_name: request.name,
                    args: request.args,
                    page_size: None,
                    cursor: None,
                    auth: runtime_auth_payload(&auth),
                    services: context.runtime_services(),
                },
                request_cancellation.token(),
                Some(next_runtime_server_request_id("convex-query")),
            )
            .await;
            runtime_timer.finish();
            result
        }
        ConvexQueryRequest::Named(request) => {
            let query = registry.resolve_query(&request.name, &request.args)?;
            let storage_timer = budgeted_segment(LatencySegment::Storage);
            let request_cancellation = RequestCancellationGuard::new();
            let result = execute_query_result_async(
                &service,
                &tenant_id,
                query,
                auth.as_ref(),
                Some(request_cancellation.token()),
            )
            .await;
            storage_timer.finish();
            result
        }
        ConvexQueryRequest::Raw { query } => {
            let request_cancellation = RequestCancellationGuard::new();
            let storage_timer = budgeted_segment(LatencySegment::Storage);
            let result = execute_query_result_async(
                &service,
                &tenant_id,
                ConvexExecutableQuery::Query(query),
                auth.as_ref(),
                Some(request_cancellation.token()),
            )
            .await;
            storage_timer.finish();
            result
        }
    };
    let status = if result.is_ok() { "ok" } else { "error" };
    let error = result.as_ref().err().map(ToString::to_string);
    trace
        .record(&service, &tenant_id, status, error.as_deref())
        .await;
    let data = result?;
    Ok(Json(data))
}

/// Executes a Convex-style paginated query over Nimbus's pagination engine.
pub(crate) async fn paginated_query(
    State(state): State<Arc<AppState>>,
    AxumPath(tenant_id): AxumPath<String>,
    headers: HeaderMap,
    Json(request): Json<ConvexPaginatedQueryRequest>,
) -> Result<Json<nimbus_core::Page>, AppError> {
    let service = state.engine.clone();
    let (registry, auth, tenant_context) = registry_and_auth_for_path(
        &state,
        crate::local_server::LocalServerRouteFamily::ConvexHttp,
        tenant_id,
        &headers,
        "convex paginated query route requires Convex support state",
    )
    .await?;
    let tenant_id = tenant_context.tenant_id().clone();
    let trace = match &request {
        ConvexPaginatedQueryRequest::Named(request) => {
            RunTrace::new(request.name.clone(), "paginated_query")
        }
        ConvexPaginatedQueryRequest::Raw { .. } => RunTrace::new("<raw-paginated-query>", "query"),
    };
    let result = match request {
        ConvexPaginatedQueryRequest::Named(request)
            if registry.has_runtime_bundle_for_function(&request.name) =>
        {
            let request_cancellation = RequestCancellationGuard::new();
            let runtime_service_registry = state.runtime_service_registry();
            let context = RuntimeInvocationContext::new(
                &service,
                &registry,
                &runtime_service_registry,
                tenant_context.clone(),
                state.tenant_isolation_mode,
            );
            let value = invoke_named_convex_function_async_cancellable(
                &context,
                InvocationRequest {
                    kind: InvocationKind::PaginatedQuery,
                    function_name: request.name,
                    args: request.args,
                    page_size: Some(request.page_size),
                    cursor: request.cursor,
                    auth: runtime_auth_payload(&auth),
                    services: context.runtime_services(),
                },
                request_cancellation.token(),
                Some(next_runtime_server_request_id("convex-paginated-query")),
            )
            .await?;
            serde_json::from_value(value).map_err(|error| {
                AppError::from(nimbus_core::Error::Serialization(error.to_string()))
            })
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
                .paginate_documents_async_cancellable_with_principal(
                    tenant_id.clone(),
                    query,
                    normalize_principal_context(auth.as_ref()),
                    cancellation.cancelled(),
                    move || check_host_cancellation(&cancellation_check),
                )
                .await
                .map_err(AppError::from)
        }
        ConvexPaginatedQueryRequest::Raw { query } => {
            let request_cancellation = RequestCancellationGuard::new();
            let cancellation = request_cancellation.token();
            let cancellation_check = cancellation.clone();
            service
                .paginate_documents_async_cancellable_with_principal(
                    tenant_id.clone(),
                    query,
                    normalize_principal_context(auth.as_ref()),
                    cancellation.cancelled(),
                    move || check_host_cancellation(&cancellation_check),
                )
                .await
                .map_err(AppError::from)
        }
    };
    let status = if result.is_ok() { "ok" } else { "error" };
    let error = result.as_ref().err().map(ToString::to_string);
    trace
        .record(&service, &tenant_id, status, error.as_deref())
        .await;
    let page = result?;
    Ok(Json(page))
}
