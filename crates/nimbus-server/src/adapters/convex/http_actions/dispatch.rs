use super::*;
use crate::adapters::convex::handlers::registry_auth::registry_and_auth;

pub(in crate::adapters::convex) async fn dispatch_http_route(
    state: Arc<AppState>,
    tenant_id: String,
    route_request: ConvexHttpRouteRequest,
) -> Result<Response, AppError> {
    let tenant_id = TenantId::new(tenant_id)?;
    let (registry, request_auth, tenant_context) = registry_and_auth(
        &state,
        crate::local_server::LocalServerRouteFamily::ConvexHttp,
        &tenant_id,
        &route_request.headers,
        "convex http route requires Convex support state",
    )
    .await?;
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
    let service = state.engine.clone();
    execution::execute_http_action_async(
        &service,
        &registry,
        &tenant_context,
        &route.plan,
        &request_context,
        request_auth.as_ref(),
    )
    .await
    .map_err(AppError::from)
}
