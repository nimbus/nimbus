use super::*;
use crate::adapters::convex::execution::RuntimeInvocationContext;
use crate::adapters::convex::runtime_auth_payload;

/// Executes a Convex-style mutation over Nimbus's existing mutation engine.
pub(crate) async fn mutation(
    State(state): State<Arc<AppState>>,
    AxumPath(tenant_id): AxumPath<String>,
    headers: HeaderMap,
    Json(request): Json<ConvexMutationRequest>,
) -> Result<Json<Value>, AppError> {
    let service = state.engine.clone();
    let (registry, auth, tenant_context) = registry_and_auth_for_path(
        &state,
        crate::local_server::LocalServerRouteFamily::ConvexHttp,
        tenant_id,
        &headers,
        "convex mutation route requires Convex support state",
    )
    .await?;
    let tenant_id = tenant_context.tenant_id().clone();
    let trace = match &request {
        ConvexMutationRequest::Named(request) => RunTrace::new(request.name.clone(), "mutation"),
        ConvexMutationRequest::Raw { .. } => RunTrace::new("<raw-mutation>", "mutation"),
    };
    let result = match request {
        ConvexMutationRequest::Named(request)
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
                    kind: InvocationKind::Mutation,
                    function_name: request.name,
                    args: request.args,
                    page_size: None,
                    cursor: None,
                    auth: runtime_auth_payload(&auth),
                    services: context.runtime_services(),
                },
                request_cancellation.token(),
                Some(next_runtime_server_request_id("convex-mutation")),
            )
            .await
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
            .await
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
            .await
        }
    };
    let status = if result.is_ok() { "ok" } else { "error" };
    let error = result.as_ref().err().map(ToString::to_string);
    trace
        .record(&service, &tenant_id, status, error.as_deref())
        .await;
    let value = result?;
    Ok(Json(value))
}
