use std::sync::Arc;

use axum::http::HeaderMap;
use nimbus_core::{PrincipalContext, TenantId};
use nimbus_tenant::TenantIsolationContext;
use serde_json::Value;

use super::super::authz::{
    OperatorRouteAccess, PrincipalClass, extract_operator_route_access, permission_actions_allow,
    permission_claim_values, principal_class_from_principal,
};
use super::super::parse_user_tenant_id;
use crate::local_server::{LocalServerAuditEvent, LocalServerRouteFamily, origin_from_headers};
use crate::state::{AppError, AppState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::http) enum SandboxAction {
    Create,
    List,
    Get,
    Stop,
}

impl SandboxAction {
    fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::List => "list",
            Self::Get => "get",
            Self::Stop => "stop",
        }
    }
}

#[derive(Debug)]
pub(in crate::http) struct SandboxAuthorization {
    pub(in crate::http) principal_class: PrincipalClass,
    pub(in crate::http) tenant_id: TenantId,
    pub(in crate::http) tenant_context: TenantIsolationContext,
    pub(in crate::http) auth_method: Option<&'static str>,
    principal: Option<PrincipalContext>,
}

impl SandboxAuthorization {
    pub(in crate::http) fn is_operator(&self) -> bool {
        self.principal_class == PrincipalClass::Operator
    }

    pub(in crate::http) fn allows(&self, action: SandboxAction, sandbox_id: Option<&str>) -> bool {
        self.is_operator()
            || self.principal.as_ref().is_some_and(|principal| {
                principal_has_sandbox_permission(principal, action, sandbox_id)
            })
    }
}

pub(in crate::http) async fn authorize_sandbox_route(
    state: &Arc<AppState>,
    headers: &HeaderMap,
    tenant_id: String,
    action: SandboxAction,
    sandbox_id: Option<&str>,
    surface: &'static str,
) -> Result<SandboxAuthorization, AppError> {
    let route_tenant = parse_user_tenant_id(tenant_id.clone())?;
    if let Some(operator) = authorize_operator_sandbox_route(state, headers, &tenant_id, surface)? {
        return Ok(operator);
    }

    let resolved = crate::application_auth::resolve_application_auth_from_headers(state, headers)
        .await
        .map_err(|error| {
            record_sandbox_authorization_audit(
                state,
                headers,
                &route_tenant,
                PrincipalClass::Tenant,
                Some("application_bearer"),
                false,
                format!("tenant/spawned sandbox authorization failed: {error}"),
            );
            error
        })?;
    if !resolved.principal.authenticated {
        record_sandbox_authorization_audit(
            state,
            headers,
            &route_tenant,
            PrincipalClass::Tenant,
            None,
            false,
            "sandbox route requires operator credentials or authenticated tenant/spawned workload identity",
        );
        return Err(AppError::unauthorized(
            "sandbox route requires operator credentials or authenticated tenant/spawned workload identity",
        ));
    }

    let principal_class = principal_class_from_principal(&resolved.principal, "sandbox")?;
    let tenant_context = crate::tenant::TenantIsolationContext::application(
        route_tenant.clone(),
        resolved.principal.clone(),
        surface,
    );
    tenant_context.require_matching_principal_claim("sandbox route policy")?;
    let route_allowed = if action == SandboxAction::List && sandbox_id.is_none() {
        principal_has_sandbox_list_permission(&resolved.principal)
    } else {
        principal_has_sandbox_permission(&resolved.principal, action, sandbox_id)
    };
    if !route_allowed {
        return Err(AppError::forbidden(format!(
            "{} principal requires sandbox `{}` permission",
            principal_class.as_str(),
            action.as_str()
        )));
    }

    Ok(SandboxAuthorization {
        principal_class,
        tenant_id: route_tenant.clone(),
        tenant_context,
        auth_method: Some("application_bearer"),
        principal: Some(resolved.principal),
    })
}

fn authorize_operator_sandbox_route(
    state: &AppState,
    headers: &HeaderMap,
    tenant_id: &str,
    surface: &'static str,
) -> Result<Option<SandboxAuthorization>, AppError> {
    let route_tenant = parse_user_tenant_id(tenant_id.to_owned())?;
    match extract_operator_route_access(headers, state.local_server_security.as_deref())? {
        Ok(OperatorRouteAccess::Authorized { auth_method }) => Ok(Some(SandboxAuthorization {
            principal_class: PrincipalClass::Operator,
            tenant_context: TenantIsolationContext::operator(route_tenant.clone(), surface),
            tenant_id: route_tenant,
            auth_method,
            principal: None,
        })),
        Ok(OperatorRouteAccess::Missing) => Ok(None),
        Err(rejection) => {
            record_sandbox_authorization_audit(
                state,
                headers,
                &route_tenant,
                PrincipalClass::Operator,
                rejection.auth_method(),
                false,
                format!("operator sandbox route rejected: {}", rejection.reason()),
            );
            Err(rejection.app_error())
        }
    }
}

fn principal_has_sandbox_permission(
    principal: &PrincipalContext,
    action: SandboxAction,
    sandbox_id: Option<&str>,
) -> bool {
    sandbox_permission_values(principal)
        .into_iter()
        .any(|permission| {
            permission_actions_allow(permission, action.as_str())
                && sandbox_permission_scope_allows(permission, sandbox_id)
        })
}

fn principal_has_sandbox_list_permission(principal: &PrincipalContext) -> bool {
    sandbox_permission_values(principal)
        .into_iter()
        .any(|permission| {
            permission_actions_allow(permission, SandboxAction::List.as_str())
                && sandbox_permission_scope_is_listable(permission)
        })
}

fn sandbox_permission_values(principal: &PrincipalContext) -> Vec<&Value> {
    permission_claim_values(
        principal,
        &[
            "nimbus_sandbox_permissions",
            "nimbusSandboxPermissions",
            "sandbox_permissions",
            "sandboxPermissions",
        ],
    )
}

fn sandbox_permission_scope_allows(permission: &Value, sandbox_id: Option<&str>) -> bool {
    let Some(scope) = permission.get("scope") else {
        return false;
    };
    let kind = scope.get("kind").and_then(Value::as_str);
    match (kind, sandbox_id) {
        (Some("tenant"), _) => true,
        (Some("exactId"), Some(sandbox_id)) => scope
            .get("id")
            .and_then(Value::as_str)
            .is_some_and(|id| id == sandbox_id),
        (Some("idPrefix"), Some(sandbox_id)) => scope
            .get("prefix")
            .and_then(Value::as_str)
            .is_some_and(|prefix| sandbox_id.starts_with(prefix)),
        _ => false,
    }
}

fn sandbox_permission_scope_is_listable(permission: &Value) -> bool {
    let Some(scope) = permission.get("scope") else {
        return false;
    };
    match scope.get("kind").and_then(Value::as_str) {
        Some("tenant") => true,
        Some("exactId") => scope.get("id").and_then(Value::as_str).is_some(),
        Some("idPrefix") => scope.get("prefix").and_then(Value::as_str).is_some(),
        _ => false,
    }
}

pub(in crate::http) fn record_sandbox_authorization_audit(
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
        auth_scope: "sandbox_principal_class",
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
