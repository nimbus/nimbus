use super::*;
use crate::adapters::convex::execution::RuntimeInvocationContext;

/// Executes a Convex-style action backed by an existing Neovex operation.
pub(crate) async fn action(
    State(state): State<Arc<AppState>>,
    AxumPath(tenant_id): AxumPath<String>,
    headers: HeaderMap,
    Json(request): Json<ConvexActionRequest>,
) -> Result<Json<Value>, AppError> {
    let tenant_id = TenantId::new(tenant_id)?;
    let service = state.service.clone();
    let (registry, auth) = registry_and_auth(
        &state,
        crate::local_server::LocalServerRouteFamily::ConvexHttp,
        &tenant_id,
        &headers,
        "convex action route requires Convex support state",
    )
    .await?;
    let value = match request {
        ConvexActionRequest::Named(request) if registry.runtime_bundle().is_some() => {
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
                    kind: InvocationKind::Action,
                    function_name: request.name,
                    args: request.args,
                    page_size: None,
                    cursor: None,
                    auth: auth.clone(),
                    services: context.runtime_services(),
                },
                request_cancellation.token(),
                Some(next_runtime_server_request_id("convex-action")),
            )
            .await?
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
            .await?
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
            .await?
        }
    };
    Ok(Json(value))
}
