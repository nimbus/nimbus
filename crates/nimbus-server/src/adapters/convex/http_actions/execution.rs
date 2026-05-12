use super::*;

#[cfg(test)]
pub(super) fn execute_http_action(
    service: &nimbus_engine::Service,
    registry: &ConvexRegistry,
    tenant_id: &TenantId,
    plan: &ConvexHttpActionPlan,
    request: &ConvexHttpRequestContext,
) -> Result<Response, Error> {
    let response = prepare_http_action_response(service, registry, tenant_id, plan, request)?;
    response::build_http_response_parts(response)
}

pub(super) async fn execute_http_action_async(
    service: &Arc<nimbus_engine::Service>,
    registry: &Arc<ConvexRegistry>,
    tenant_id: &TenantId,
    plan: &ConvexHttpActionPlan,
    request: &ConvexHttpRequestContext,
    auth: Option<&InvocationAuth>,
) -> Result<Response, Error> {
    let response =
        prepare_http_action_response_async(service, registry, tenant_id, plan, request, auth, None)
            .await?;
    response::build_http_response_parts(response)
}

#[cfg(test)]
pub(super) fn prepare_http_action_response(
    service: &nimbus_engine::Service,
    registry: &ConvexRegistry,
    tenant_id: &TenantId,
    plan: &ConvexHttpActionPlan,
    request: &ConvexHttpRequestContext,
) -> Result<ConvexHttpResponseParts, Error> {
    let operation = resolve_http_action_operation(plan, request)?;
    let operation_result = operation
        .map(|operation| execute_convex_action(service, registry, tenant_id, operation))
        .transpose()?;
    finalize_http_action_response(plan, request, operation_result.as_ref())
}

pub(in crate::adapters::convex) fn prepare_http_action_response_cancellable(
    service: &nimbus_engine::Service,
    registry: &ConvexRegistry,
    tenant_id: &TenantId,
    plan: &ConvexHttpActionPlan,
    request: &ConvexHttpRequestContext,
    auth: Option<&InvocationAuth>,
    cancellation: &HostCallCancellation,
) -> Result<ConvexHttpResponseParts, Error> {
    check_host_cancellation(cancellation)?;
    let operation = resolve_http_action_operation(plan, request)?;
    let operation_result = operation
        .map(|operation| {
            execute_convex_action_cancellable_with_auth(
                service,
                registry,
                tenant_id,
                operation,
                auth,
                cancellation,
            )
        })
        .transpose()?;
    finalize_http_action_response(plan, request, operation_result.as_ref())
}

pub(in crate::adapters::convex) async fn prepare_http_action_response_async(
    service: &Arc<nimbus_engine::Service>,
    registry: &Arc<ConvexRegistry>,
    tenant_id: &TenantId,
    plan: &ConvexHttpActionPlan,
    request: &ConvexHttpRequestContext,
    auth: Option<&InvocationAuth>,
    cancellation: Option<HostCallCancellation>,
) -> Result<ConvexHttpResponseParts, Error> {
    let operation = resolve_http_action_operation(plan, request)?;
    let operation_result = match operation {
        Some(operation) => Some(
            execute_convex_action_async(
                service,
                registry,
                tenant_id,
                operation,
                auth,
                cancellation,
            )
            .await?,
        ),
        None => None,
    };
    finalize_http_action_response(plan, request, operation_result.as_ref())
}

fn resolve_http_action_operation(
    plan: &ConvexHttpActionPlan,
    request: &ConvexHttpRequestContext,
) -> Result<Option<ConvexExecutableAction>, Error> {
    let Some(operation_template) = plan.operation.as_ref() else {
        return Ok(None);
    };
    let resolved = resolve_http_template(operation_template, request, None)?;
    let operation = serde_json::from_value(resolved).map_err(|error| {
        Error::InvalidInput(format!(
            "convex http route resolved to invalid operation: {error}"
        ))
    })?;
    Ok(Some(operation))
}

fn finalize_http_action_response(
    plan: &ConvexHttpActionPlan,
    request: &ConvexHttpRequestContext,
    operation_result: Option<&Value>,
) -> Result<ConvexHttpResponseParts, Error> {
    let body = resolve_http_template(&plan.response.body, request, operation_result)?;
    let status = plan
        .response
        .status
        .as_ref()
        .map(|status| resolve_http_template(status, request, operation_result))
        .transpose()?;
    let headers = plan
        .response
        .headers
        .as_ref()
        .map(|headers| resolve_http_template(headers, request, operation_result))
        .transpose()?;

    Ok(ConvexHttpResponseParts {
        kind: plan.response.kind,
        body,
        status,
        headers,
    })
}
