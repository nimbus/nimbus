use super::*;
use crate::adapters::convex::execution::RuntimeInvocationContext;
use crate::adapters::convex::runtime_auth_payload;

/// Executes a Convex-style action backed by an existing Nimbus operation.
pub(crate) async fn action(
    State(state): State<Arc<AppState>>,
    AxumPath(tenant_id): AxumPath<String>,
    headers: HeaderMap,
    Json(request): Json<ConvexActionRequest>,
) -> Result<Json<Value>, AppError> {
    let service = state.engine.clone();
    let (registry, auth, tenant_context) = registry_and_auth_for_path(
        &state,
        crate::local_server::LocalServerRouteFamily::ConvexHttp,
        tenant_id,
        &headers,
        "convex action route requires Convex support state",
    )
    .await?;
    let tenant_id = tenant_context.tenant_id().clone();
    let trace = match &request {
        ConvexActionRequest::Named(request) => RunTrace::new(request.name.clone(), "action"),
        ConvexActionRequest::Raw { .. } => RunTrace::new("<raw-action>", "action"),
    };
    let result = match request {
        ConvexActionRequest::Named(request)
            if registry.has_runtime_bundle_for_function(&request.name) =>
        {
            let request_cancellation = RequestCancellationGuard::new();
            let runtime_service_registry = state.runtime_service_registry();
            let context = RuntimeInvocationContext::new(
                &service,
                &registry,
                &runtime_service_registry,
                tenant_context.clone(),
                state.tenant_isolation_mode(),
            );
            invoke_named_convex_function_async_cancellable(
                &context,
                InvocationRequest {
                    kind: InvocationKind::Action,
                    function_name: request.name,
                    args: request.args,
                    page_size: None,
                    cursor: None,
                    auth: runtime_auth_payload(&auth),
                    services: context.runtime_services(),
                },
                request_cancellation.token(),
                Some(next_runtime_server_request_id("convex-action")),
            )
            .await
        }
        ConvexActionRequest::Named(request) => {
            let action = registry.resolve_action(&request.name, &request.args)?;
            execute_convex_action_async(
                &service,
                &registry,
                &tenant_id,
                action,
                auth.as_ref(),
                None,
            )
            .await
        }
        ConvexActionRequest::Raw { action } => {
            execute_convex_action_async(
                &service,
                &registry,
                &tenant_id,
                ConvexExecutableAction::Action(action),
                auth.as_ref(),
                None,
            )
            .await
        }
    };
    let status = if result.is_ok() { "ok" } else { "error" };
    let error = result.as_ref().err().map(ToString::to_string);
    trace
        .record(&service, &tenant_id, status, error.as_deref())
        .await;
    let value = result.map_err(convex_function_error)?;
    Ok(Json(value))
}
