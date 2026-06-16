use std::sync::Arc;

use axum::http::HeaderMap;
use nimbus_core::{PrincipalContext, TenantId};
use serde_json::Value;

use super::super::authz::{
    OperatorRouteAccess, PrincipalClass, extract_operator_route_access, permission_actions_allow,
    permission_claim_values, principal_class_from_principal,
};
use super::super::parse_user_tenant_id;
use super::super::service_grants::principal_has_exact_service_grant;
use crate::local_server::{LocalServerAuditEvent, LocalServerRouteFamily, origin_from_headers};
use crate::state::{AppError, AppState};
use crate::tenant::TenantIsolationContext;

#[derive(Debug)]
pub(in crate::http) struct ServiceRouteAuthorization {
    pub(in crate::http) principal_class: PrincipalClass,
    pub(in crate::http) tenant_context: TenantIsolationContext,
    pub(in crate::http) auth_method: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::http) enum ServiceDefinitionAction {
    Create,
    List,
    Inspect,
    Update,
    Delete,
    ForceDelete,
}

impl ServiceDefinitionAction {
    fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::List => "list",
            Self::Inspect => "inspect",
            Self::Update => "update",
            Self::Delete => "delete",
            Self::ForceDelete => "forceDelete",
        }
    }
}

#[derive(Debug)]
pub(in crate::http) struct ServiceDefinitionAuthorization {
    pub(in crate::http) principal_class: PrincipalClass,
    pub(in crate::http) tenant_context: TenantIsolationContext,
    pub(in crate::http) auth_method: Option<&'static str>,
    principal: Option<PrincipalContext>,
}

impl ServiceDefinitionAuthorization {
    pub(in crate::http) fn is_operator(&self) -> bool {
        self.principal_class == PrincipalClass::Operator
    }

    pub(in crate::http) fn allows_service_definition(
        &self,
        action: ServiceDefinitionAction,
        service_name: &str,
    ) -> bool {
        self.is_operator()
            || self.principal.as_ref().is_some_and(|principal| {
                principal_has_service_definition_permission(principal, action, service_name)
            })
    }

    pub(in crate::http) fn allows_force_delete(&self, service_name: &str) -> bool {
        self.is_operator()
            || self.principal.as_ref().is_some_and(|principal| {
                principal_has_service_definition_permission(
                    principal,
                    ServiceDefinitionAction::ForceDelete,
                    service_name,
                ) && principal_has_exact_service_grant(principal, service_name)
            })
    }
}

pub(in crate::http) async fn authorize_service_definition_route(
    state: &Arc<AppState>,
    headers: &HeaderMap,
    tenant_id: String,
    service_name: Option<&str>,
    action: ServiceDefinitionAction,
    surface: &'static str,
) -> Result<ServiceDefinitionAuthorization, AppError> {
    if let Some(operator) = authorize_operator_service_route(state, headers, &tenant_id, surface)? {
        return Ok(ServiceDefinitionAuthorization {
            principal_class: operator.principal_class,
            tenant_context: operator.tenant_context,
            auth_method: operator.auth_method,
            principal: None,
        });
    }

    let resolved = crate::application_auth::resolve_application_auth_from_headers(state, headers)
        .await
        .map_err(|error| {
            record_service_definition_authorization_audit(
                state,
                headers,
                &parse_user_tenant_id_lossy(&tenant_id),
                PrincipalClass::Tenant,
                Some("application_bearer"),
                false,
                format!("tenant/spawned service definition authorization failed: {error}"),
            );
            error
        })?;
    if !resolved.principal.authenticated {
        let tenant = parse_user_tenant_id(tenant_id)?;
        record_service_definition_authorization_audit(
            state,
            headers,
            &tenant,
            PrincipalClass::Tenant,
            None,
            false,
            "service definition route requires operator credentials or authenticated tenant/spawned workload identity",
        );
        return Err(AppError::unauthorized(
            "service definition route requires operator credentials or authenticated tenant/spawned workload identity",
        ));
    }

    let tenant = parse_user_tenant_id(tenant_id)?;
    let principal_class = principal_class_from_principal(&resolved.principal, "service")?;
    let tenant_context =
        TenantIsolationContext::application(tenant.clone(), resolved.principal.clone(), surface);
    if let Err(error) =
        tenant_context.require_matching_principal_claim("service definition route policy")
    {
        record_service_definition_authorization_audit(
            state,
            headers,
            &tenant,
            principal_class,
            Some("application_bearer"),
            false,
            format!(
                "{} cross-tenant service definition route rejected: {error}",
                principal_class.as_str()
            ),
        );
        return Err(AppError::from(error));
    }

    let allowed = match service_name {
        Some(service_name) => {
            principal_has_service_definition_permission(&resolved.principal, action, service_name)
        }
        None => principal_has_any_service_definition_permission(&resolved.principal, action),
    };
    if !allowed {
        record_service_definition_authorization_audit(
            state,
            headers,
            &tenant,
            principal_class,
            Some("application_bearer"),
            false,
            format!(
                "{} principal lacks service definition `{}` permission",
                principal_class.as_str(),
                action.as_str()
            ),
        );
        return Err(AppError::forbidden(format!(
            "{} principal requires service definition `{}` permission",
            principal_class.as_str(),
            action.as_str()
        )));
    }

    Ok(ServiceDefinitionAuthorization {
        principal_class,
        tenant_context,
        auth_method: Some("application_bearer"),
        principal: Some(resolved.principal),
    })
}

pub(in crate::http) async fn authorize_service_route(
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
    let principal_class = principal_class_from_principal(&resolved.principal, "service")?;
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
    match extract_operator_route_access(headers, state.local_server_security.as_deref())? {
        Ok(OperatorRouteAccess::Authorized { auth_method }) => {
            Ok(Some(ServiceRouteAuthorization {
                principal_class: PrincipalClass::Operator,
                tenant_context: TenantIsolationContext::operator(route_tenant, surface),
                auth_method,
            }))
        }
        Ok(OperatorRouteAccess::Missing) => Ok(None),
        Err(rejection) => {
            record_service_authorization_audit(
                state,
                headers,
                &route_tenant,
                PrincipalClass::Operator,
                rejection.auth_method(),
                false,
                format!("operator service route rejected: {}", rejection.reason()),
            );
            Err(rejection.app_error())
        }
    }
}

fn principal_has_any_service_definition_permission(
    principal: &PrincipalContext,
    action: ServiceDefinitionAction,
) -> bool {
    service_definition_permission_values(principal)
        .into_iter()
        .any(|permission| permission_actions_allow(permission, action.as_str()))
}

fn principal_has_service_definition_permission(
    principal: &PrincipalContext,
    action: ServiceDefinitionAction,
    service_name: &str,
) -> bool {
    service_definition_permission_values(principal)
        .into_iter()
        .any(|permission| {
            permission_actions_allow(permission, action.as_str())
                && service_definition_permission_scope_allows(permission, service_name)
        })
}

fn service_definition_permission_values(principal: &PrincipalContext) -> Vec<&Value> {
    permission_claim_values(
        principal,
        &[
            "nimbus_service_definition_permissions",
            "nimbusServiceDefinitionPermissions",
            "service_definition_permissions",
            "serviceDefinitionPermissions",
        ],
    )
}

fn service_definition_permission_scope_allows(permission: &Value, service_name: &str) -> bool {
    let Some(scope) = permission.get("scope") else {
        return false;
    };
    let kind = scope.get("kind").and_then(Value::as_str);
    match kind {
        Some("exactName") => scope
            .get("name")
            .and_then(Value::as_str)
            .is_some_and(|name| name == service_name),
        Some("namePrefix") => scope
            .get("prefix")
            .and_then(Value::as_str)
            .is_some_and(|prefix| service_name.starts_with(prefix)),
        _ => false,
    }
}

pub(in crate::http) fn record_service_authorization_audit(
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

pub(in crate::http) fn record_service_definition_authorization_audit(
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
        auth_scope: "service_definition_principal_class",
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
