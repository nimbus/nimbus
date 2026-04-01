use super::*;

/// Executes a Convex-style mutation over Neovex's existing mutation engine.
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
        &headers,
        "convex mutation route requires Convex support state",
    )
    .await?;
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
            dispatch_convex_mutation_async(
                &service,
                &registry,
                &tenant_id,
                mutation,
                auth.as_ref(),
                None,
            )
            .await?
        }
        ConvexMutationRequest::Raw { mutation } => {
            dispatch_convex_mutation_async(
                &service,
                &registry,
                &tenant_id,
                ConvexExecutableMutation::Mutation(mutation),
                auth.as_ref(),
                None,
            )
            .await?
        }
    };
    Ok(Json(value))
}
