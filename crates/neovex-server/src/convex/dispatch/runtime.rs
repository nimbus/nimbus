#[cfg(test)]
use super::sync_ops::{
    execute_named_action_request_direct, execute_named_mutation_request_direct,
    execute_named_paginated_query_request_direct, execute_named_query_request_direct,
};
use super::*;

#[cfg(test)]
pub(in crate::convex) fn execute_named_query_request(
    service: &Arc<neovex_engine::Service>,
    registry: &Arc<ConvexRegistry>,
    tenant_id: &TenantId,
    request: ConvexNamedRequest,
) -> Result<Value, Error> {
    if registry.runtime_bundle().is_some() {
        return invoke_named_convex_function(
            service,
            registry,
            tenant_id,
            InvocationRequest {
                kind: InvocationKind::Query,
                function_name: request.name,
                args: request.args,
                page_size: None,
                cursor: None,
                auth: None,
            },
        );
    }

    execute_named_query_request_direct(service, registry, tenant_id, &request.name, &request.args)
}

#[cfg(test)]
pub(in crate::convex) fn execute_named_paginated_query_request(
    service: &Arc<neovex_engine::Service>,
    registry: &Arc<ConvexRegistry>,
    tenant_id: &TenantId,
    request: ConvexNamedPaginatedQueryRequest,
) -> Result<neovex_core::Page, Error> {
    if registry.runtime_bundle().is_some() {
        let value = invoke_named_convex_function(
            service,
            registry,
            tenant_id,
            InvocationRequest {
                kind: InvocationKind::PaginatedQuery,
                function_name: request.name,
                args: request.args,
                page_size: Some(request.page_size),
                cursor: request.cursor,
                auth: None,
            },
        )?;
        return serde_json::from_value(value)
            .map_err(|error| Error::Serialization(error.to_string()));
    }

    execute_named_paginated_query_request_direct(
        service,
        registry,
        tenant_id,
        &request.name,
        &request.args,
        request.page_size,
        request.cursor,
    )
}

#[cfg(test)]
pub(in crate::convex) fn execute_named_mutation_request(
    service: &Arc<neovex_engine::Service>,
    registry: &Arc<ConvexRegistry>,
    tenant_id: &TenantId,
    request: ConvexNamedRequest,
) -> Result<Value, Error> {
    if registry.runtime_bundle().is_some() {
        return invoke_named_convex_function(
            service,
            registry,
            tenant_id,
            InvocationRequest {
                kind: InvocationKind::Mutation,
                function_name: request.name,
                args: request.args,
                page_size: None,
                cursor: None,
                auth: None,
            },
        );
    }

    execute_named_mutation_request_direct(
        service,
        registry,
        tenant_id,
        &request.name,
        &request.args,
    )
}

#[cfg(test)]
pub(in crate::convex) fn execute_named_action_request(
    service: &Arc<neovex_engine::Service>,
    registry: &Arc<ConvexRegistry>,
    tenant_id: &TenantId,
    request: ConvexNamedRequest,
) -> Result<Value, Error> {
    if registry.runtime_bundle().is_some() {
        return invoke_named_convex_function(
            service,
            registry,
            tenant_id,
            InvocationRequest {
                kind: InvocationKind::Action,
                function_name: request.name,
                args: request.args,
                page_size: None,
                cursor: None,
                auth: None,
            },
        );
    }

    execute_named_action_request_direct(service, registry, tenant_id, &request.name, &request.args)
}

#[cfg(test)]
fn invoke_named_convex_function(
    service: &Arc<neovex_engine::Service>,
    registry: &Arc<ConvexRegistry>,
    tenant_id: &TenantId,
    request: InvocationRequest,
) -> Result<Value, Error> {
    invoke_named_convex_function_with_trace(service, registry, tenant_id, request)
        .map(|(value, _)| value)
}

pub(in crate::convex) fn next_runtime_server_request_id(prefix: &str) -> String {
    static NEXT_RUNTIME_SERVER_REQUEST_ID: AtomicU64 = AtomicU64::new(1);
    format!(
        "{prefix}-{}",
        NEXT_RUNTIME_SERVER_REQUEST_ID.fetch_add(1, Ordering::Relaxed)
    )
}

pub(in crate::convex) async fn invoke_named_convex_function_async_cancellable(
    service: &Arc<neovex_engine::Service>,
    registry: &Arc<ConvexRegistry>,
    tenant_id: &TenantId,
    request: InvocationRequest,
    cancellation: HostCallCancellation,
    server_request_id: Option<String>,
) -> Result<Value, Error> {
    invoke_named_convex_function_with_trace_async_cancellable(
        service,
        registry,
        tenant_id,
        request,
        cancellation,
        server_request_id,
    )
    .await
    .map(|(value, _)| value)
}

#[cfg(test)]
fn invoke_named_convex_function_with_trace(
    service: &Arc<neovex_engine::Service>,
    registry: &Arc<ConvexRegistry>,
    tenant_id: &TenantId,
    request: InvocationRequest,
) -> Result<(Value, ConvexRuntimeReadSet), Error> {
    invoke_named_convex_function_with_trace_cancellable(
        service,
        registry,
        tenant_id,
        request,
        HostCallCancellation::default(),
    )
}

#[cfg(test)]
fn invoke_named_convex_function_with_trace_cancellable(
    service: &Arc<neovex_engine::Service>,
    registry: &Arc<ConvexRegistry>,
    tenant_id: &TenantId,
    request: InvocationRequest,
    cancellation: HostCallCancellation,
) -> Result<(Value, ConvexRuntimeReadSet), Error> {
    let bundle = registry
        .runtime_bundle()
        .cloned()
        .ok_or_else(|| Error::Internal("convex runtime bundle not loaded".to_string()))?;
    let bridge = Arc::new(ConvexRuntimeBridge::new(
        service.clone(),
        registry.clone(),
        tenant_id.clone(),
        None,
    ));
    let runtime = NeovexRuntime::with_policy(bridge.clone(), registry.runtime_policy());
    let response = registry
        .runtime_executor()
        .invoke_blocking_with_cancellation(
            runtime,
            bundle,
            request.clone(),
            RuntimeInvocationContext::top_level_for_tenant(&request, tenant_id.to_string()),
            Some(cancellation),
        )
        .map_err(runtime_error_to_core)?;
    let envelope: ConvexRuntimeResponseEnvelope = serde_json::from_value(response)
        .map_err(|error| Error::Serialization(error.to_string()))?;
    Ok((envelope.into_core_result()?, bridge.snapshot_read_set()))
}

#[cfg(test)]
async fn invoke_named_convex_function_with_trace_async(
    service: &Arc<neovex_engine::Service>,
    registry: &Arc<ConvexRegistry>,
    tenant_id: &TenantId,
    request: InvocationRequest,
) -> Result<(Value, ConvexRuntimeReadSet), Error> {
    invoke_named_convex_function_with_trace_async_cancellable(
        service,
        registry,
        tenant_id,
        request,
        HostCallCancellation::default(),
        None,
    )
    .await
}

pub(in crate::convex) async fn invoke_named_convex_function_with_trace_async_cancellable(
    service: &Arc<neovex_engine::Service>,
    registry: &Arc<ConvexRegistry>,
    tenant_id: &TenantId,
    request: InvocationRequest,
    cancellation: HostCallCancellation,
    server_request_id: Option<String>,
) -> Result<(Value, ConvexRuntimeReadSet), Error> {
    let bundle = registry
        .runtime_bundle()
        .cloned()
        .ok_or_else(|| Error::Internal("convex runtime bundle not loaded".to_string()))?;
    let bridge = Arc::new(ConvexRuntimeBridge::new(
        service.clone(),
        registry.clone(),
        tenant_id.clone(),
        server_request_id.clone(),
    ));
    let runtime = NeovexRuntime::with_policy(bridge.clone(), registry.runtime_policy());
    let response = registry
        .runtime_executor()
        .invoke_on_worker(
            runtime,
            bundle,
            request.clone(),
            match server_request_id.as_deref() {
                Some(server_request_id) => {
                    RuntimeInvocationContext::top_level_for_tenant_and_request(
                        &request,
                        tenant_id.to_string(),
                        server_request_id,
                    )
                }
                None => {
                    RuntimeInvocationContext::top_level_for_tenant(&request, tenant_id.to_string())
                }
            },
            Some(cancellation),
        )
        .await
        .map_err(runtime_error_to_core)?;
    let envelope: ConvexRuntimeResponseEnvelope = serde_json::from_value(response)
        .map_err(|error| Error::Serialization(error.to_string()))?;
    Ok((envelope.into_core_result()?, bridge.snapshot_read_set()))
}

#[allow(clippy::too_many_arguments)]
pub(in crate::convex) async fn bootstrap_runtime_named_subscription_async(
    service: &Arc<neovex_engine::Service>,
    registry: &Arc<ConvexRegistry>,
    tenant_id: &TenantId,
    name: &str,
    args: &Value,
    page_size: Option<usize>,
    cursor: Option<String>,
    auth: Option<InvocationAuth>,
    cancellation: HostCallCancellation,
    server_request_id: Option<String>,
) -> Result<ConvexRuntimeSubscriptionSetup, Error> {
    let kind = if page_size.is_some() {
        InvocationKind::PaginatedQuery
    } else {
        InvocationKind::Query
    };
    let (value, read_set) = invoke_named_convex_function_with_trace_async_cancellable(
        service,
        registry,
        tenant_id,
        InvocationRequest {
            kind: kind.clone(),
            function_name: name.to_string(),
            args: args.clone(),
            page_size,
            cursor: cursor.clone(),
            auth: auth.clone(),
        },
        cancellation,
        server_request_id,
    )
    .await?;
    let base_queries = synthesize_runtime_subscription_base_queries(&read_set)?;
    match kind {
        InvocationKind::Query => Ok(ConvexRuntimeSubscriptionSetup {
            initial_value: value,
            base_queries,
            transform: ConvexSubscriptionTransform::RuntimeNamedQuery {
                name: name.to_string(),
                args: args.clone(),
                auth,
                read_set: Some(read_set),
            },
        }),
        InvocationKind::PaginatedQuery => {
            let page: neovex_core::Page = serde_json::from_value(value)
                .map_err(|error| Error::Serialization(error.to_string()))?;
            Ok(ConvexRuntimeSubscriptionSetup {
                initial_value: Value::Array(page.data),
                base_queries,
                transform: ConvexSubscriptionTransform::RuntimeNamedPaginatedQuery {
                    name: name.to_string(),
                    args: args.clone(),
                    page_size: page_size
                        .expect("paginated runtime bootstrap should carry page size"),
                    cursor,
                    auth,
                    read_set: Some(read_set),
                },
            })
        }
        InvocationKind::Mutation | InvocationKind::Action => Err(Error::InvalidInput(
            "runtime subscription bootstrap only supports queries".to_string(),
        )),
    }
}
