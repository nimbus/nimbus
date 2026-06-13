use std::sync::Arc;

use axum::http::HeaderMap;
use nimbus_core::{PrincipalContext, TenantId};
use nimbus_services::{SessionResource, SessionTarget};
use serde_json::Value;

use super::super::authz::{
    OperatorRouteAccess, PrincipalClass, extract_operator_route_access, permission_actions_allow,
    permission_claim_values, principal_claim_string, principal_class_from_principal,
};
use super::super::parse_operator_tenant_context;
use super::super::parse_user_tenant_id;
use super::super::service_grants::principal_has_exact_service_grant;
use crate::local_server::{LocalServerAuditEvent, LocalServerRouteFamily, origin_from_headers};
use crate::state::{AppError, AppState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::http) enum SessionAction {
    Open,
    List,
    Get,
    Close,
}

impl SessionAction {
    fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::List => "list",
            Self::Get => "get",
            Self::Close => "close",
        }
    }
}

#[derive(Debug)]
pub(in crate::http) struct SessionAuthorization {
    pub(in crate::http) principal_class: PrincipalClass,
    pub(in crate::http) tenant_id: TenantId,
    pub(in crate::http) auth_method: Option<&'static str>,
    principal: Option<PrincipalContext>,
}

impl SessionAuthorization {
    pub(in crate::http) fn can_list_session(&self, session: &SessionResource) -> bool {
        self.principal
            .as_ref()
            .is_none_or(|principal| principal_can_list_session(principal, session))
    }
}

pub(in crate::http) async fn authorize_session_resource_lookup(
    state: &Arc<AppState>,
    headers: &HeaderMap,
    action: SessionAction,
    _session_id: &str,
    route_tenant_id: Option<&TenantId>,
) -> Result<SessionAuthorization, AppError> {
    let surface = match action {
        SessionAction::Get => "native_http.session.get",
        SessionAction::Close => "native_http.session.close",
        SessionAction::Open | SessionAction::List => unreachable!("session resource lookup"),
    };
    match extract_operator_route_access(headers, state.local_server_security.as_deref())? {
        Ok(OperatorRouteAccess::Authorized { auth_method }) => {
            let tenant_id = route_tenant_id.ok_or_else(|| {
                AppError::from(nimbus_core::Error::InvalidInput(format!(
                    "{surface} with operator credentials requires tenantId"
                )))
            })?;
            let tenant_context = parse_operator_tenant_context(tenant_id.as_str(), surface)?;
            return Ok(SessionAuthorization {
                principal_class: PrincipalClass::Operator,
                tenant_id: tenant_context.tenant_id().clone(),
                auth_method,
                principal: None,
            });
        }
        Ok(OperatorRouteAccess::Missing) => {}
        Err(rejection) => {
            return Err(rejection.app_error());
        }
    }

    let resolved =
        crate::application_auth::resolve_application_auth_from_headers(state, headers).await?;
    if !resolved.principal.authenticated {
        return Err(AppError::unauthorized(
            "session resource lookup requires operator credentials or authenticated tenant/spawned workload identity",
        ));
    }
    let tenant_id = match route_tenant_id {
        Some(tenant_id) => tenant_id.clone(),
        None => application_session_tenant_id(&resolved.principal, surface)?,
    };
    let principal_class = principal_class_from_principal(&resolved.principal, "session")?;
    let tenant_context = crate::tenant::TenantIsolationContext::application(
        tenant_id.clone(),
        resolved.principal.clone(),
        surface,
    );
    tenant_context.require_matching_principal_claim("session resource lookup")?;
    if !principal_has_session_action_permission(&resolved.principal, action) {
        return Err(AppError::forbidden(format!(
            "{} principal requires session `{}` permission",
            principal_class.as_str(),
            action.as_str()
        )));
    }
    Ok(SessionAuthorization {
        principal_class,
        tenant_id,
        auth_method: Some("application_bearer"),
        principal: Some(resolved.principal),
    })
}

pub(in crate::http) async fn authorize_session_resource_target(
    state: &Arc<AppState>,
    headers: &HeaderMap,
    lookup_authorization: &SessionAuthorization,
    session: &SessionResource,
    action: SessionAction,
) -> Result<SessionAuthorization, AppError> {
    let surface = match action {
        SessionAction::Get => "native_http.session.get",
        SessionAction::Close => "native_http.session.close",
        SessionAction::Open | SessionAction::List => unreachable!("session resource target"),
    };
    let authorization = authorize_session_route(
        state,
        headers,
        SessionRouteAuthorizationRequest {
            tenant_id: &session.tenant_id,
            action,
            session_id: Some(&session.id),
            target: Some(&session.target),
            channels: &[],
            surface,
        },
    )
    .await?;
    Ok(SessionAuthorization {
        auth_method: lookup_authorization.auth_method,
        ..authorization
    })
}

fn application_session_tenant_id(
    principal: &PrincipalContext,
    context: &str,
) -> Result<TenantId, AppError> {
    let Some(tenant_id) = principal_claim_string(
        principal,
        &[
            "nimbus_tenant_id",
            "nimbusTenantId",
            "tenant_id",
            "tenantId",
        ],
    ) else {
        return Err(AppError::forbidden(format!(
            "application principal has no tenant claim for {context}"
        )));
    };
    parse_user_tenant_id(tenant_id.to_owned())
}

pub(in crate::http) struct SessionRouteAuthorizationRequest<'a> {
    pub(in crate::http) tenant_id: &'a TenantId,
    pub(in crate::http) action: SessionAction,
    pub(in crate::http) session_id: Option<&'a str>,
    pub(in crate::http) target: Option<&'a SessionTarget>,
    pub(in crate::http) channels: &'a [String],
    pub(in crate::http) surface: &'static str,
}

pub(in crate::http) async fn authorize_session_route(
    state: &Arc<AppState>,
    headers: &HeaderMap,
    request: SessionRouteAuthorizationRequest<'_>,
) -> Result<SessionAuthorization, AppError> {
    let tenant_id = request.tenant_id;
    let action = request.action;
    let session_id = request.session_id;
    let target = request.target;
    let channels = request.channels;
    let surface = request.surface;

    if let Some(operator) = authorize_operator_session_route(state, headers, tenant_id, surface)? {
        return Ok(operator);
    }

    let resolved = crate::application_auth::resolve_application_auth_from_headers(state, headers)
        .await
        .map_err(|error| {
            record_session_authorization_audit(
                state,
                headers,
                tenant_id,
                PrincipalClass::Tenant,
                Some("application_bearer"),
                false,
                format!("tenant/spawned session authorization failed: {error}"),
            );
            error
        })?;
    if !resolved.principal.authenticated {
        record_session_authorization_audit(
            state,
            headers,
            tenant_id,
            PrincipalClass::Tenant,
            None,
            false,
            "session route requires operator credentials or authenticated tenant/spawned workload identity",
        );
        return Err(AppError::unauthorized(
            "session route requires operator credentials or authenticated tenant/spawned workload identity",
        ));
    }

    let principal_class = principal_class_from_principal(&resolved.principal, "session")?;
    let tenant_context = crate::tenant::TenantIsolationContext::application(
        tenant_id.clone(),
        resolved.principal.clone(),
        surface,
    );
    tenant_context.require_matching_principal_claim("session route policy")?;
    let route_allowed = if action == SessionAction::List
        && session_id.is_none()
        && target.is_none()
        && channels.is_empty()
    {
        principal_has_session_list_permission(&resolved.principal)
    } else {
        principal_has_session_permission(&resolved.principal, action, session_id, target, channels)
    };
    if !route_allowed {
        return Err(AppError::forbidden(format!(
            "{} principal requires session `{}` permission",
            principal_class.as_str(),
            action.as_str()
        )));
    }
    if let Some(SessionTarget::Service { name }) = target
        && !principal_has_exact_service_grant(&resolved.principal, name)
    {
        return Err(AppError::forbidden(format!(
            "{} principal requires an exact service grant for `{name}` before opening a service-targeted session",
            principal_class.as_str()
        )));
    }
    if let Some(SessionTarget::Sandbox { id }) = target
        && !principal_has_sandbox_reach(&resolved.principal, id)
    {
        return Err(AppError::forbidden(format!(
            "{} principal requires sandbox reach for `{id}` before opening a sandbox-targeted session",
            principal_class.as_str()
        )));
    }

    Ok(SessionAuthorization {
        principal_class,
        tenant_id: tenant_id.clone(),
        auth_method: Some("application_bearer"),
        principal: Some(resolved.principal),
    })
}

fn authorize_operator_session_route(
    state: &AppState,
    headers: &HeaderMap,
    tenant_id: &TenantId,
    surface: &'static str,
) -> Result<Option<SessionAuthorization>, AppError> {
    match extract_operator_route_access(headers, state.local_server_security.as_deref())? {
        Ok(OperatorRouteAccess::Authorized { auth_method }) => {
            let tenant_context = parse_operator_tenant_context(tenant_id.as_str(), surface)?;
            Ok(Some(SessionAuthorization {
                principal_class: PrincipalClass::Operator,
                tenant_id: tenant_context.tenant_id().clone(),
                auth_method,
                principal: None,
            }))
        }
        Ok(OperatorRouteAccess::Missing) => Ok(None),
        Err(rejection) => {
            record_session_authorization_audit(
                state,
                headers,
                tenant_id,
                PrincipalClass::Operator,
                rejection.auth_method(),
                false,
                format!("operator session route rejected: {}", rejection.reason()),
            );
            Err(rejection.app_error())
        }
    }
}

fn session_target_reachable(principal: &PrincipalContext, target: &SessionTarget) -> bool {
    match target {
        SessionTarget::Service { name } => principal_has_exact_service_grant(principal, name),
        SessionTarget::Sandbox { id } => principal_has_sandbox_reach(principal, id),
    }
}

fn principal_can_list_session(principal: &PrincipalContext, session: &SessionResource) -> bool {
    principal_has_session_permission(
        principal,
        SessionAction::List,
        Some(&session.id),
        Some(&session.target),
        &[],
    ) && session_target_reachable(principal, &session.target)
}

fn principal_has_session_permission(
    principal: &PrincipalContext,
    action: SessionAction,
    session_id: Option<&str>,
    target: Option<&SessionTarget>,
    channels: &[String],
) -> bool {
    session_permission_values(principal)
        .into_iter()
        .any(|permission| {
            permission_actions_allow(permission, action.as_str())
                && session_permission_scope_allows(permission, session_id, target)
                && session_permission_channels_allow(permission, channels)
        })
}

fn principal_has_session_list_permission(principal: &PrincipalContext) -> bool {
    session_permission_values(principal)
        .into_iter()
        .any(|permission| {
            permission_actions_allow(permission, SessionAction::List.as_str())
                && session_permission_scope_is_listable(permission)
        })
}

fn principal_has_session_action_permission(
    principal: &PrincipalContext,
    action: SessionAction,
) -> bool {
    session_permission_values(principal)
        .into_iter()
        .any(|permission| permission_actions_allow(permission, action.as_str()))
}

fn session_permission_values(principal: &PrincipalContext) -> Vec<&Value> {
    permission_claim_values(
        principal,
        &[
            "nimbus_session_permissions",
            "nimbusSessionPermissions",
            "session_permissions",
            "sessionPermissions",
        ],
    )
}

fn session_permission_scope_allows(
    permission: &Value,
    session_id: Option<&str>,
    target: Option<&SessionTarget>,
) -> bool {
    let Some(scope) = permission.get("scope") else {
        return false;
    };
    match scope.get("kind").and_then(Value::as_str) {
        Some("tenant") => true,
        Some("exactId") => session_id.is_some_and(|session_id| {
            scope
                .get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| id == session_id)
        }),
        Some("service") => matches!(target, Some(SessionTarget::Service { name }) if scope
            .get("name")
            .and_then(Value::as_str)
            .is_some_and(|scope_name| scope_name == name)),
        Some("sandbox") => matches!(target, Some(SessionTarget::Sandbox { id }) if scope
            .get("id")
            .and_then(Value::as_str)
            .is_some_and(|scope_id| scope_id == id)),
        _ => false,
    }
}

fn session_permission_scope_is_listable(permission: &Value) -> bool {
    let Some(scope) = permission.get("scope") else {
        return false;
    };
    match scope.get("kind").and_then(Value::as_str) {
        Some("tenant") => true,
        Some("exactId") | Some("sandbox") => scope.get("id").and_then(Value::as_str).is_some(),
        Some("service") => scope.get("name").and_then(Value::as_str).is_some(),
        _ => false,
    }
}

fn session_permission_channels_allow(permission: &Value, channels: &[String]) -> bool {
    if channels.is_empty() {
        return true;
    }
    let Some(allowed) = permission.get("channels") else {
        return false;
    };
    let Value::Array(allowed) = allowed else {
        return false;
    };
    channels.iter().all(|channel| {
        allowed
            .iter()
            .any(|allowed_channel| allowed_channel.as_str() == Some(channel.as_str()))
    })
}

fn principal_has_sandbox_reach(principal: &PrincipalContext, sandbox_id: &str) -> bool {
    sandbox_permission_values(principal)
        .into_iter()
        .any(|permission| {
            permission_actions_allow(permission, "get")
                && sandbox_permission_scope_allows(permission, sandbox_id)
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

fn sandbox_permission_scope_allows(permission: &Value, sandbox_id: &str) -> bool {
    let Some(scope) = permission.get("scope") else {
        return false;
    };
    match scope.get("kind").and_then(Value::as_str) {
        Some("tenant") => true,
        Some("exactId") => scope
            .get("id")
            .and_then(Value::as_str)
            .is_some_and(|id| id == sandbox_id),
        Some("idPrefix") => scope
            .get("prefix")
            .and_then(Value::as_str)
            .is_some_and(|prefix| sandbox_id.starts_with(prefix)),
        _ => false,
    }
}

pub(in crate::http) fn record_session_authorization_audit(
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
        auth_scope: "session_principal_class",
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
