use super::*;

pub(in crate::adapters::convex) async fn dispatch_http_route(
    state: Arc<AppState>,
    tenant_id: String,
    route_request: ConvexHttpRouteRequest,
) -> Result<Response, AppError> {
    let tenant_id = TenantId::new(tenant_id)?;
    let registry = state
        .convex_registry
        .clone()
        .expect("convex http route requires Convex support state");
    let request_auth = registry
        .verify_authorization_header(&route_request.headers)
        .await?;
    crate::state::record_authenticated_usage(&state, request_auth.as_ref()).await;
    let route = registry
        .resolve_http_route(&route_request.method, &route_request.request_path)
        .cloned();
    let Some(route) = route else {
        let status = if registry.has_http_route_for_path(&route_request.request_path) {
            StatusCode::METHOD_NOT_ALLOWED
        } else {
            StatusCode::NOT_FOUND
        };
        return Ok((
            status,
            Json(json!({ "error": "convex http route not found" })),
        )
            .into_response());
    };

    let request_context = request_context::build_http_request_context(
        &route_request.method,
        &route_request.headers,
        &route_request.original_uri,
        &route_request.request_path,
        route_request.query,
        route_request.body,
    );
    let service = state.service.clone();
    if registry.runtime_bundle().is_some() && route.name.is_some() {
        let request_cancellation = RequestCancellationGuard::new();
        let runtime_auth = runtime_http_route_auth(request_auth);
        let response = invoke_named_convex_function_async_cancellable(
            &service,
            &registry,
            &tenant_id,
            InvocationRequest {
                kind: InvocationKind::Action,
                function_name: route
                    .name
                    .clone()
                    .expect("runtime-eligible http route should have a name"),
                args: serde_json::to_value(&request_context)
                    .map_err(|error| AppError::from(Error::Serialization(error.to_string())))?,
                page_size: None,
                cursor: None,
                auth: runtime_auth,
            },
            request_cancellation.token(),
            Some(next_runtime_server_request_id("convex-http-action")),
        )
        .await?;
        let response: ConvexHttpResponseParts = serde_json::from_value(response)
            .map_err(|error| AppError::from(Error::Serialization(error.to_string())))?;
        return response::build_http_response_parts(response).map_err(AppError::from);
    }

    execution::execute_http_action_async(
        &service,
        &registry,
        &tenant_id,
        &route.plan,
        &request_context,
    )
    .await
    .map_err(AppError::from)
}

fn runtime_http_route_auth(request_auth: Option<InvocationAuth>) -> Option<InvocationAuth> {
    let (runtime_identity, verified_identity) = match request_auth {
        Some(auth) => (auth.identity, auth.verified_identity),
        None => (None, None),
    };
    Some(InvocationAuth {
        identity: runtime_identity,
        verified_identity,
        throw_on_missing_identity: true,
    })
}
