use super::*;

pub(super) async fn registry_and_auth(
    state: &Arc<AppState>,
    route_family: crate::local_server::LocalServerRouteFamily,
    tenant_id: &TenantId,
    headers: &HeaderMap,
    expectation: &'static str,
) -> Result<(Arc<ConvexRegistry>, Option<InvocationAuth>), AppError> {
    let registry = state
        .convex_registry
        .current()
        .ok_or_else(|| AppError::not_found(expectation))?;
    let auth = match registry.verify_authorization_header(headers).await {
        Ok(auth) => {
            state.record_local_server_audit(crate::local_server::LocalServerAuditEvent {
                route_family,
                tenant_id: Some(tenant_id.to_string()),
                auth_scope: "application",
                auth_method: Some(if auth.is_some() {
                    "application_bearer"
                } else {
                    "anonymous"
                }),
                success: true,
                origin: crate::local_server::origin_from_headers(headers),
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
                route_family,
                tenant_id: Some(tenant_id.to_string()),
                auth_scope: "application",
                auth_method: Some("application_bearer"),
                success: false,
                origin: crate::local_server::origin_from_headers(headers),
                reason: error.to_string(),
            });
            return Err(error);
        }
    };
    record_authenticated_usage(state, auth.as_ref()).await;
    Ok((registry, auth))
}
