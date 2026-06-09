use axum::http::HeaderMap;
use nimbus_core::PrincipalContext;
use nimbus_operator::{
    ExtractedServerAccessStatus, LocalServerCredentialMode, extract_server_access,
};
use nimbus_runtime::HostCallCancellation;
use nimbus_sandbox::{SandboxHandle, SandboxStatus};
use serde::Serialize;
use serde_json::Value;

use super::*;
use crate::local_server::{LocalServerAuditEvent, LocalServerRouteFamily, origin_from_headers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrincipalClass {
    Operator,
    Tenant,
    SpawnedWorkload,
}

impl PrincipalClass {
    fn as_str(self) -> &'static str {
        match self {
            Self::Operator => "operator",
            Self::Tenant => "tenant",
            Self::SpawnedWorkload => "spawned_workload",
        }
    }
}

#[derive(Debug)]
struct ServiceRouteAuthorization {
    principal_class: PrincipalClass,
    tenant_context: TenantIsolationContext,
    auth_method: Option<&'static str>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ServiceResourceResponse {
    pub(crate) tenant_id: String,
    pub(crate) name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) sandbox_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) backend: Option<String>,
    pub(crate) state: String,
    pub(crate) lifecycle_state: String,
    pub(crate) readiness: String,
    pub(crate) health: String,
    pub(crate) endpoints: Vec<ServiceEndpointResponse>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ServiceEndpointResponse {
    pub(crate) name: String,
    pub(crate) protocol: String,
    pub(crate) host: String,
    pub(crate) port: u16,
}

pub(crate) async fn get_service(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, service_name)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<ServiceResourceResponse>, AppError> {
    let authorization = authorize_service_route(
        &state,
        &headers,
        tenant_id,
        &service_name,
        "native_http.service.get",
    )
    .await?;
    let tenant_context = authorization.tenant_context;
    let manager = service_manager(&state)?;
    if !manager.service_declared_for_tenant(tenant_context.tenant_id(), &service_name) {
        return Err(service_not_found(tenant_context.tenant_id(), &service_name));
    }
    let handle = manager
        .inspect_service_for_context_async(&tenant_context, &service_name)
        .await?;
    record_service_authorization_audit(
        &state,
        &headers,
        tenant_context.tenant_id(),
        authorization.principal_class,
        authorization.auth_method,
        true,
        format!(
            "{} principal authorized with exact service grant or operator authority",
            authorization.principal_class.as_str()
        ),
    );

    Ok(Json(match handle {
        Some(handle) => ServiceResourceResponse::from_handle(tenant_context.tenant_id(), &handle),
        None => {
            ServiceResourceResponse::declared_inactive(tenant_context.tenant_id(), &service_name)
        }
    }))
}

pub(crate) async fn start_service(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, service_name)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<ServiceResourceResponse>, AppError> {
    let authorization = authorize_service_route(
        &state,
        &headers,
        tenant_id,
        &service_name,
        "native_http.service.start",
    )
    .await?;
    let tenant_context = authorization.tenant_context;
    let manager = service_manager(&state)?;
    let handle = manager
        .start_service_for_context_async(
            &tenant_context,
            &service_name,
            HostCallCancellation::default(),
        )
        .await?
        .ok_or_else(|| service_not_found(tenant_context.tenant_id(), &service_name))?;
    record_service_event(&state, tenant_context.tenant_id(), "start", &handle).await?;
    record_service_authorization_audit(
        &state,
        &headers,
        tenant_context.tenant_id(),
        authorization.principal_class,
        authorization.auth_method,
        true,
        format!(
            "{} principal authorized with exact service grant or operator authority",
            authorization.principal_class.as_str()
        ),
    );

    Ok(Json(ServiceResourceResponse::from_handle(
        tenant_context.tenant_id(),
        &handle,
    )))
}

pub(crate) async fn stop_service(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, service_name)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<ServiceResourceResponse>, AppError> {
    let authorization = authorize_service_route(
        &state,
        &headers,
        tenant_id,
        &service_name,
        "native_http.service.stop",
    )
    .await?;
    let tenant_context = authorization.tenant_context;
    let manager = service_manager(&state)?;
    let handle = manager
        .stop_service_for_context_async(&tenant_context, &service_name)
        .await?
        .ok_or_else(|| service_not_found(tenant_context.tenant_id(), &service_name))?;
    record_service_event(&state, tenant_context.tenant_id(), "stop", &handle).await?;
    record_service_authorization_audit(
        &state,
        &headers,
        tenant_context.tenant_id(),
        authorization.principal_class,
        authorization.auth_method,
        true,
        format!(
            "{} principal authorized with exact service grant or operator authority",
            authorization.principal_class.as_str()
        ),
    );

    Ok(Json(ServiceResourceResponse::from_handle(
        tenant_context.tenant_id(),
        &handle,
    )))
}

pub(crate) async fn restart_service(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, service_name)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<ServiceResourceResponse>, AppError> {
    let authorization = authorize_service_route(
        &state,
        &headers,
        tenant_id,
        &service_name,
        "native_http.service.restart",
    )
    .await?;
    let tenant_context = authorization.tenant_context;
    let manager = service_manager(&state)?;
    let handle = manager
        .restart_service_for_context_async(
            &tenant_context,
            &service_name,
            HostCallCancellation::default(),
        )
        .await?
        .ok_or_else(|| service_not_found(tenant_context.tenant_id(), &service_name))?;
    record_service_event(&state, tenant_context.tenant_id(), "restart", &handle).await?;
    record_service_authorization_audit(
        &state,
        &headers,
        tenant_context.tenant_id(),
        authorization.principal_class,
        authorization.auth_method,
        true,
        format!(
            "{} principal authorized with exact service grant or operator authority",
            authorization.principal_class.as_str()
        ),
    );

    Ok(Json(ServiceResourceResponse::from_handle(
        tenant_context.tenant_id(),
        &handle,
    )))
}

impl ServiceResourceResponse {
    fn from_handle(tenant_id: &TenantId, handle: &SandboxHandle) -> Self {
        let state = crate::system_tenant::sandbox_status(handle.status).to_owned();
        Self {
            tenant_id: tenant_id.as_str().to_owned(),
            name: handle.name.clone(),
            sandbox_id: Some(handle.id.as_str().to_owned()),
            backend: Some(crate::system_tenant::sandbox_backend(handle.backend).to_owned()),
            state: state.clone(),
            lifecycle_state: state,
            readiness: readiness_from_status(handle.status).to_owned(),
            health: health_from_status(handle.status).to_owned(),
            endpoints: handle
                .published_endpoints
                .iter()
                .map(|endpoint| ServiceEndpointResponse {
                    name: endpoint.name.as_str().to_owned(),
                    protocol: crate::system_tenant::endpoint_protocol(endpoint.protocol).to_owned(),
                    host: endpoint.address.ip().to_string(),
                    port: endpoint.address.port(),
                })
                .collect(),
        }
    }

    fn declared_inactive(tenant_id: &TenantId, service_name: &str) -> Self {
        Self {
            tenant_id: tenant_id.as_str().to_owned(),
            name: service_name.to_owned(),
            sandbox_id: None,
            backend: None,
            state: "stopped".to_owned(),
            lifecycle_state: "stopped".to_owned(),
            readiness: "stopped".to_owned(),
            health: "unknown".to_owned(),
            endpoints: Vec::new(),
        }
    }
}

fn service_manager(state: &AppState) -> Result<Arc<nimbus_services::ServiceManager>, AppError> {
    state.service_manager().ok_or_else(|| {
        AppError::not_found("service lifecycle endpoints require a server-owned service manager")
    })
}

async fn authorize_service_route(
    state: &Arc<AppState>,
    headers: &HeaderMap,
    tenant_id: String,
    service_name: &str,
    surface: &'static str,
) -> Result<ServiceRouteAuthorization, AppError> {
    if let Some(operator) = authorize_operator_service_route(state, headers, &tenant_id, surface)? {
        return Ok(operator);
    }

    let resolved = crate::application_auth::resolve_application_auth_from_headers(state, headers)
        .await
        .map_err(|error| {
            record_service_authorization_audit(
                state,
                headers,
                &parse_user_tenant_id_lossy(&tenant_id),
                PrincipalClass::Tenant,
                Some("application_bearer"),
                false,
                format!("tenant/spawned service authorization failed: {error}"),
            );
            error
        })?;
    if !resolved.principal.authenticated {
        let tenant = parse_user_tenant_id(tenant_id)?;
        record_service_authorization_audit(
            state,
            headers,
            &tenant,
            PrincipalClass::Tenant,
            None,
            false,
            "service lifecycle route requires operator credentials or authenticated tenant/spawned workload identity",
        );
        return Err(AppError::unauthorized(
            "service lifecycle route requires operator credentials or authenticated tenant/spawned workload identity",
        ));
    }

    let tenant = parse_user_tenant_id(tenant_id)?;
    let principal_class = principal_class_from_principal(&resolved.principal)?;
    let tenant_context =
        TenantIsolationContext::application(tenant.clone(), resolved.principal.clone(), surface);
    if let Err(error) = tenant_context
        .require_matching_principal_claim("service lifecycle principal-class route policy")
    {
        record_service_authorization_audit(
            state,
            headers,
            &tenant,
            principal_class,
            Some("application_bearer"),
            false,
            format!(
                "{} cross-tenant service route rejected: {error}",
                principal_class.as_str()
            ),
        );
        return Err(AppError::from(error));
    }

    if !principal_has_exact_service_grant(&resolved.principal, service_name) {
        record_service_authorization_audit(
            state,
            headers,
            &tenant,
            principal_class,
            Some("application_bearer"),
            false,
            format!(
                "{} principal lacks exact service grant for `{service_name}`",
                principal_class.as_str()
            ),
        );
        return Err(AppError::forbidden(format!(
            "{} principal requires an exact service grant for `{service_name}`",
            principal_class.as_str()
        )));
    }

    Ok(ServiceRouteAuthorization {
        principal_class,
        tenant_context,
        auth_method: Some("application_bearer"),
    })
}

fn authorize_operator_service_route(
    state: &AppState,
    headers: &HeaderMap,
    tenant_id: &str,
    surface: &'static str,
) -> Result<Option<ServiceRouteAuthorization>, AppError> {
    let route_tenant = parse_user_tenant_id(tenant_id)?;
    if state.local_server_security.is_none() {
        return Ok(None);
    }

    let extracted = extract_server_access(
        headers,
        LocalServerCredentialMode::AuthorizationOrAdminHeader,
        state.local_server_security.as_deref(),
    )
    .map_err(AppError::from)?;
    match extracted.status {
        ExtractedServerAccessStatus::Authorized => Ok(Some(ServiceRouteAuthorization {
            principal_class: PrincipalClass::Operator,
            tenant_context: parse_operator_tenant_context(tenant_id.to_owned(), surface)?,
            auth_method: extracted.auth_method,
        })),
        ExtractedServerAccessStatus::Missing => Ok(None),
        ExtractedServerAccessStatus::Invalid
            if extracted.auth_method == Some("local_admin_bearer") =>
        {
            Ok(None)
        }
        ExtractedServerAccessStatus::Revoked => {
            record_service_authorization_audit(
                state,
                headers,
                &route_tenant,
                PrincipalClass::Operator,
                extracted.auth_method,
                false,
                "operator service route rejected: auth.token_revoked",
            );
            Err(AppError::unauthorized("auth.token_revoked"))
        }
        ExtractedServerAccessStatus::Expired => {
            record_service_authorization_audit(
                state,
                headers,
                &route_tenant,
                PrincipalClass::Operator,
                extracted.auth_method,
                false,
                "operator service route rejected: auth.session_expired",
            );
            Err(AppError::unauthorized("auth.session_expired"))
        }
        ExtractedServerAccessStatus::Invalid => {
            record_service_authorization_audit(
                state,
                headers,
                &route_tenant,
                PrincipalClass::Operator,
                extracted.auth_method,
                false,
                "operator service route rejected: invalid local admin credential",
            );
            Err(AppError::unauthorized(
                LocalServerCredentialMode::AuthorizationOrAdminHeader.unauthorized_message(),
            ))
        }
    }
}

fn readiness_from_status(status: SandboxStatus) -> &'static str {
    match status {
        SandboxStatus::Ready => "ready",
        SandboxStatus::Stopped => "stopped",
        SandboxStatus::Failed => "failed",
        SandboxStatus::Starting | SandboxStatus::NotReady | SandboxStatus::Stopping => "not_ready",
    }
}

fn health_from_status(status: SandboxStatus) -> &'static str {
    match status {
        SandboxStatus::Ready => "healthy",
        SandboxStatus::Failed => "unhealthy",
        SandboxStatus::Starting
        | SandboxStatus::NotReady
        | SandboxStatus::Stopping
        | SandboxStatus::Stopped => "unknown",
    }
}

fn principal_class_from_principal(
    principal: &PrincipalContext,
) -> Result<PrincipalClass, AppError> {
    let Some(value) = principal_claim_string(
        principal,
        &[
            "nimbus_principal_class",
            "nimbusPrincipalClass",
            "principal_class",
            "principalClass",
        ],
    ) else {
        return Ok(PrincipalClass::Tenant);
    };
    match value {
        "tenant" => Ok(PrincipalClass::Tenant),
        "spawned" | "spawned_workload" | "spawnedWorkload" | "workload" | "workload_identity" => {
            Ok(PrincipalClass::SpawnedWorkload)
        }
        "operator" => Err(AppError::forbidden(
            "application credentials cannot resolve to operator principal class",
        )),
        other => Err(AppError::forbidden(format!(
            "unknown service route principal class `{other}`"
        ))),
    }
}

fn principal_claim_string<'a>(
    principal: &'a PrincipalContext,
    claim_names: &[&str],
) -> Option<&'a str> {
    for claims in [&principal.verified_claims, &principal.claims] {
        for claim_name in claim_names {
            if let Some(value) = claims.get(*claim_name).and_then(Value::as_str) {
                return Some(value);
            }
        }
    }
    None
}

fn principal_has_exact_service_grant(principal: &PrincipalContext, service_name: &str) -> bool {
    let mut found_exact = false;
    for claims in [&principal.verified_claims, &principal.claims] {
        for claim_name in [
            "nimbus_service_grants",
            "nimbusServiceGrants",
            "service_grants",
            "serviceGrants",
        ] {
            let Some(value) = claims.get(claim_name) else {
                continue;
            };
            if service_grant_value_contains_wildcard(value) {
                return false;
            }
            found_exact |= service_grant_value_contains_exact(value, service_name);
        }
    }
    found_exact
}

fn service_grant_value_contains_exact(value: &Value, service_name: &str) -> bool {
    match value {
        Value::String(grant) => grant == service_name,
        Value::Array(grants) => grants
            .iter()
            .any(|grant| service_grant_value_contains_exact(grant, service_name)),
        _ => false,
    }
}

fn service_grant_value_contains_wildcard(value: &Value) -> bool {
    match value {
        Value::String(grant) => {
            matches!(grant.as_str(), "*" | "all" | "service:*" | "services:*")
        }
        Value::Array(grants) => grants.iter().any(service_grant_value_contains_wildcard),
        _ => false,
    }
}

fn record_service_authorization_audit(
    state: &AppState,
    headers: &HeaderMap,
    tenant_id: &TenantId,
    principal_class: PrincipalClass,
    auth_method: Option<&'static str>,
    success: bool,
    reason: impl Into<String>,
) {
    state.record_local_server_audit(LocalServerAuditEvent {
        route_family: LocalServerRouteFamily::NativeApi,
        tenant_id: Some(tenant_id.as_str().to_owned()),
        auth_scope: "service_principal_class",
        auth_method,
        success,
        origin: origin_from_headers(headers),
        reason: format!(
            "principal_class={} {}",
            principal_class.as_str(),
            reason.into()
        ),
    });
}

fn parse_user_tenant_id_lossy(value: &str) -> TenantId {
    parse_user_tenant_id(value.to_owned()).unwrap_or_else(|_| {
        TenantId::new("invalid-tenant").expect("fallback tenant id should parse")
    })
}

async fn record_service_event(
    state: &AppState,
    tenant_id: &TenantId,
    action: &str,
    handle: &SandboxHandle,
) -> Result<(), AppError> {
    let service_state = crate::system_tenant::sandbox_status(handle.status);
    let message = format!(
        "service `{}` for tenant `{}` {} completed with state {}",
        handle.name, tenant_id, action, service_state
    );
    let correlation_id = format!("service:{}:{}:{action}", tenant_id, handle.name);
    crate::system_tenant::record_system_event_async(
        &state.engine,
        "service",
        "info",
        "service.lifecycle",
        &message,
        serde_json::json!({
            "action": action,
            "tenantId": tenant_id.as_str(),
            "serviceName": handle.name.as_str(),
            "sandboxId": handle.id.as_str(),
            "state": service_state,
            "backend": crate::system_tenant::sandbox_backend(handle.backend),
        }),
        Some(&correlation_id),
    )
    .await
    .map_err(AppError::from)
}

fn service_not_found(tenant_id: &TenantId, service_name: &str) -> AppError {
    AppError::not_found(format!(
        "service `{service_name}` is not declared for tenant `{tenant_id}`"
    ))
}
