use super::*;
use crate::adapters::convex::execution::RuntimeInvocationContext;
use crate::application_auth::normalize_principal_context;

pub(crate) async fn query(
    State(state): State<Arc<AppState>>,
    AxumPath(tenant_id): AxumPath<String>,
    headers: HeaderMap,
    Json(request): Json<ConvexQueryRequest>,
) -> Result<Json<Value>, AppError> {
    let tenant_id = TenantId::new(tenant_id)?;
    let service = state.service.clone();
    let (registry, auth) = registry_and_auth(
        &state,
        crate::local_server::LocalServerRouteFamily::ConvexHttp,
        &tenant_id,
        &headers,
        "convex query route requires Convex support state",
    )
    .await?;
    let data = match request {
        ConvexQueryRequest::Named(request) if registry.runtime_bundle().is_some() => {
            let request_cancellation = RequestCancellationGuard::new();
            let runtime_service_registry = state.runtime_service_registry();
            let context = RuntimeInvocationContext::new(
                &service,
                &registry,
                &runtime_service_registry,
                &tenant_id,
            );
            invoke_named_convex_function_async_cancellable(
                &context,
                InvocationRequest {
                    kind: InvocationKind::Query,
                    function_name: request.name,
                    args: request.args,
                    page_size: None,
                    cursor: None,
                    auth: auth.clone(),
                    services: context.runtime_services(),
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
                auth.as_ref(),
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
                auth.as_ref(),
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
    let (registry, auth) = registry_and_auth(
        &state,
        crate::local_server::LocalServerRouteFamily::ConvexHttp,
        &tenant_id,
        &headers,
        "convex paginated query route requires Convex support state",
    )
    .await?;
    let page = match request {
        ConvexPaginatedQueryRequest::Named(request) if registry.runtime_bundle().is_some() => {
            let request_cancellation = RequestCancellationGuard::new();
            let runtime_service_registry = state.runtime_service_registry();
            let context = RuntimeInvocationContext::new(
                &service,
                &registry,
                &runtime_service_registry,
                &tenant_id,
            );
            let value = invoke_named_convex_function_async_cancellable(
                &context,
                InvocationRequest {
                    kind: InvocationKind::PaginatedQuery,
                    function_name: request.name,
                    args: request.args,
                    page_size: Some(request.page_size),
                    cursor: request.cursor,
                    auth: auth.clone(),
                    services: context.runtime_services(),
                },
                request_cancellation.token(),
                Some(next_runtime_server_request_id("convex-paginated-query")),
            )
            .await?;
            serde_json::from_value(value).map_err(|error| {
                AppError::from(neovex_core::Error::Serialization(error.to_string()))
            })?
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
                .await?
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
                .await?
        }
    };
    Ok(Json(page))
}
