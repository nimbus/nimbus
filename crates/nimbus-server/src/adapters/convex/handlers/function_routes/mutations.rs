use super::*;
use crate::adapters::convex::execution::RuntimeInvocationContext;

/// Executes a Convex-style mutation over Nimbus's existing mutation engine.
pub(crate) async fn mutation(
    State(state): State<Arc<AppState>>,
    AxumPath(tenant_id): AxumPath<String>,
    headers: HeaderMap,
    Json(request): Json<ConvexMutationRequest>,
) -> Result<Json<Value>, AppError> {
    let tenant_id = TenantId::new(tenant_id)?;
    let service = state.service.clone();
    let (registry, auth) = registry_and_auth(
        &state,
        crate::local_server::LocalServerRouteFamily::ConvexHttp,
        &tenant_id,
        &headers,
        "convex mutation route requires Convex support state",
    )
    .await?;
    let value = match request {
        ConvexMutationRequest::Named(request) if registry.runtime_bundle().is_some() => {
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
                    kind: InvocationKind::Mutation,
                    function_name: request.name,
                    args: request.args,
                    page_size: None,
                    cursor: None,
                    auth: auth.clone(),
                    services: context.runtime_services(),
                },
                request_cancellation.token(),
                Some(next_runtime_server_request_id("convex-mutation")),
            )
            .await?
        }
        ConvexMutationRequest::Named(request) => {
            let request_cancellation = RequestCancellationGuard::new();
            let mutation = registry.resolve_mutation(&request.name, &request.args)?;
            dispatch_convex_mutation_async(
                &service,
                &registry,
                &tenant_id,
                mutation,
                auth.as_ref(),
                Some(request_cancellation.token()),
            )
            .await?
        }
        ConvexMutationRequest::Raw { mutation } => {
            let request_cancellation = RequestCancellationGuard::new();
            dispatch_convex_mutation_async(
                &service,
                &registry,
                &tenant_id,
                ConvexExecutableMutation::Mutation(mutation),
                auth.as_ref(),
                Some(request_cancellation.token()),
            )
            .await?
        }
    };
    Ok(Json(value))
}
