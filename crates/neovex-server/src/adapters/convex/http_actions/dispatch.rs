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
    execution::execute_http_action_async(
        &service,
        &registry,
        &tenant_id,
        &route.plan,
        &request_context,
        request_auth.as_ref(),
    )
    .await
    .map_err(AppError::from)
}
