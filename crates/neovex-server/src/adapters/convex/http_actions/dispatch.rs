use super::*;
use crate::application_auth::verify_optional_application_auth_from_headers;

pub(in crate::adapters::convex) async fn dispatch_http_route(
    state: Arc<AppState>,
    tenant_id: String,
    route_request: ConvexHttpRouteRequest,
) -> Result<Response, AppError> {
    let tenant_id = TenantId::new(tenant_id)?;
    let registry = state
        .convex_registry
        .current()
        .ok_or_else(|| AppError::not_found("convex http route requires Convex support state"))?;
    let request_auth =
        match verify_optional_application_auth_from_headers(&state, &route_request.headers).await {
            Ok(auth) => {
                state.record_local_server_audit(crate::local_server::LocalServerAuditEvent {
                    route_family: crate::local_server::LocalServerRouteFamily::ConvexHttp,
                    tenant_id: Some(tenant_id.to_string()),
                    auth_scope: "application",
                    auth_method: Some(if auth.is_some() {
                        "application_bearer"
                    } else {
                        "anonymous"
                    }),
                    success: true,
                    origin: crate::local_server::origin_from_headers(&route_request.headers),
                    reason: if auth.is_some() {
                        "application.authenticated".to_string()
                    } else {
                        "application.anonymous".to_string()
                    },
                });
                auth
            }
            Err(error) => {
                state.record_local_server_audit(crate::local_server::LocalServerAuditEvent {
                    route_family: crate::local_server::LocalServerRouteFamily::ConvexHttp,
                    tenant_id: Some(tenant_id.to_string()),
                    auth_scope: "application",
                    auth_method: Some("application_bearer"),
                    success: false,
                    origin: crate::local_server::origin_from_headers(&route_request.headers),
                    reason: error.to_string(),
                });
                return Err(error);
            }
        };
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
