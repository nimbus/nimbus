use super::*;
use crate::application_auth::{
    normalize_principal_context, verify_optional_application_auth_from_headers_in_deployment,
};
use crate::local_server::authorize_standard_server_access;
use crate::tenant_isolation::TenantIsolationContext;

pub(super) async fn registry_and_auth_for_path(
    state: &Arc<AppState>,
    route_family: crate::local_server::LocalServerRouteFamily,
    tenant_id: String,
    headers: &HeaderMap,
    expectation: &'static str,
) -> Result<
    (
        Arc<ConvexRegistry>,
        Option<InvocationAuth>,
        TenantIsolationContext,
    ),
    AppError,
> {
    let tenant_id = TenantId::new(tenant_id)?;
    registry_and_auth(state, route_family, &tenant_id, headers, expectation).await
}

pub(super) async fn registry_and_auth(
    state: &Arc<AppState>,
    route_family: crate::local_server::LocalServerRouteFamily,
    tenant_id: &TenantId,
    headers: &HeaderMap,
    expectation: &'static str,
) -> Result<
    (
        Arc<ConvexRegistry>,
        Option<InvocationAuth>,
        TenantIsolationContext,
    ),
    AppError,
> {
    if crate::system_tenant::is_system_tenant_id(tenant_id) {
        let registry = state
            .system_convex_registry()
            .ok_or_else(|| AppError::not_found(expectation))?;
        let auth_method =
            match authorize_standard_server_access(headers, state.local_server_security.as_deref())
            {
                Ok(auth_method) => auth_method,
                Err(error) => {
                    state.record_local_server_audit(crate::local_server::LocalServerAuditEvent {
                        route_family,
                        tenant_id: Some(tenant_id.to_string()),
                        auth_scope: "server_access",
                        auth_method: None,
                        success: false,
                        origin: crate::local_server::origin_from_headers(headers),
                        reason: error.to_string(),
                    });
                    return Err(error);
                }
            };
        state.record_local_server_audit(crate::local_server::LocalServerAuditEvent {
            route_family,
            tenant_id: Some(tenant_id.to_string()),
            auth_scope: "server_access",
            auth_method,
            success: true,
            origin: crate::local_server::origin_from_headers(headers),
            reason: "authorized".to_string(),
        });
        return Ok((
            registry,
            None,
            TenantIsolationContext::operator(tenant_id.clone(), route_family.as_str()),
        ));
    }

    let deployment = state.current_deployment();
    let registry = deployment
        .convex_registry()
        .ok_or_else(|| AppError::not_found(expectation))?;
    let auth = match verify_optional_application_auth_from_headers_in_deployment(
        deployment.as_ref(),
        headers,
    )
    .await
    {
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
    let tenant_context = TenantIsolationContext::application(
        tenant_id.clone(),
        normalize_principal_context(auth.as_ref()),
        route_family.as_str(),
    )
    .with_deployment_generation(deployment.generation);
    tenant_context
        .ensure_deployment_generation_matches(deployment.generation, "convex active deployment")?;
    tenant_context.ensure_application_principal_tenant_access("convex route tenant")?;
    Ok((registry, auth, tenant_context))
}
