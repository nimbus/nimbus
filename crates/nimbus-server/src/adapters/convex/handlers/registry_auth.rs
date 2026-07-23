use super::*;
use crate::application_auth::verify_optional_convex_auth_from_headers_in_deployment;
use crate::local_server::authorize_standard_server_access;
use nimbus_auth::normalize_principal_context;
use nimbus_tenant::TenantIsolationContext;

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

pub(in crate::adapters::convex) async fn registry_and_auth(
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
    if nimbus_system::is_system_tenant_id(tenant_id) {
        let registry = state
            .system_convex_registry()
            .ok_or_else(|| AppError::not_found(expectation))?;
        let auth_method = match authorize_standard_server_access(
            headers,
            state.local_server_security().as_deref(),
        ) {
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
                return Err(error.into());
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
    let auth = match verify_optional_convex_auth_from_headers_in_deployment(
        deployment.as_ref(),
        tenant_id,
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
    let principal = normalize_principal_context(auth.as_ref());
    // A verified caller is already bound to this silo because its bearer was
    // accepted by the verifier selected above. Anonymous access has no such
    // proof and therefore remains an explicit, fail-closed operator binding.
    if auth.is_none() {
        deployment
            .convex_tenancy()
            .unwrap_or_default()
            .authorize_anonymous_silo_selection(tenant_id)
            .map_err(|error| AppError::forbidden(error.to_string()))?;
    }
    let tenant_context =
        TenantIsolationContext::application(tenant_id.clone(), principal, route_family.as_str())
            .with_deployment_generation(deployment.generation);
    tenant_context
        .ensure_deployment_generation_matches(deployment.generation, "convex active deployment")?;
    Ok((registry, auth, tenant_context))
}
