use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query as QueryParams, State};
use axum::http::{HeaderMap, StatusCode};
use nimbus_core::{PrincipalContext, TenantId};
use nimbus_operator::{
    ExtractedServerAccessStatus, LocalServerCredentialMode, extract_server_access,
};
use nimbus_services::{
    SessionLifecycleState, SessionResource, SessionTarget, SessionTargetSnapshot,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::service_grants::principal_has_exact_service_grant;
use super::{AppError, AppState, parse_operator_tenant_context, parse_user_tenant_id};
use crate::local_server::{LocalServerAuditEvent, LocalServerRouteFamily, origin_from_headers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionPrincipalClass {
    Operator,
    Tenant,
    SpawnedWorkload,
}

impl SessionPrincipalClass {
    fn as_str(self) -> &'static str {
        match self {
            Self::Operator => "operator",
            Self::Tenant => "tenant",
            Self::SpawnedWorkload => "spawned_workload",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionAction {
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
struct SessionAuthorization {
    principal_class: SessionPrincipalClass,
    tenant_id: TenantId,
    auth_method: Option<&'static str>,
    principal: Option<PrincipalContext>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SessionOpenRequest {
    tenant_id: Option<String>,
    target: SessionTargetInput,
    channels: Vec<String>,
    requested_ttl_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SessionTargetInput {
    service: Option<ServiceTargetInput>,
    sandbox: Option<SandboxTargetInput>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ServiceTargetInput {
    name: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SandboxTargetInput {
    id: String,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SessionListQuery {
    tenant_id: Option<String>,
    limit: Option<usize>,
    page_token: Option<String>,
    state: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SessionCloseRequest {
    reason: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionCollectionResponse {
    metadata: SessionCollectionMetadataResponse,
    items: Vec<SessionResourceResponse>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionCollectionMetadataResponse {
    tenant_id: String,
    resource_version: String,
    limit: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_page_token: Option<String>,
    remaining_count: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionResourceResponse {
    metadata: SessionMetadataResponse,
    spec: SessionSpecResponse,
    status: SessionStatusResponse,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionMetadataResponse {
    tenant_id: String,
    id: String,
    generation: u64,
    resource_version: String,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionSpecResponse {
    target: SessionTargetResponse,
    target_snapshot: SessionTargetSnapshotResponse,
    channels: Vec<String>,
    expires_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
enum SessionTargetResponse {
    Service(ServiceTargetResponse),
    Sandbox(SandboxTargetResponse),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ServiceTargetResponse {
    name: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SandboxTargetResponse {
    id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
enum SessionTargetSnapshotResponse {
    Service(ServiceTargetSnapshotResponse),
    Sandbox(SandboxTargetSnapshotResponse),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ServiceTargetSnapshotResponse {
    name: String,
    generation: u64,
    backend: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SandboxTargetSnapshotResponse {
    id: String,
    generation: u64,
    profile: String,
    backend: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionStatusResponse {
    lifecycle_state: String,
    expires_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    closed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    close_reason: Option<String>,
    conditions: Vec<SessionConditionResponse>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionConditionResponse {
    #[serde(rename = "type")]
    condition_type: &'static str,
    status: &'static str,
    reason: &'static str,
    message: String,
    observed_generation: u64,
    last_transition_time: String,
}

pub(crate) async fn open_session(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<SessionOpenRequest>,
) -> Result<(StatusCode, Json<SessionResourceResponse>), AppError> {
    let tenant_id = required_tenant_id(request.tenant_id.as_deref(), "session open")?;
    let target = session_target_from_input(request.target.clone())?;
    let authorization = authorize_session_route(
        &state,
        &headers,
        &tenant_id,
        SessionAction::Open,
        None,
        Some(&target),
        &request.channels,
        "native_http.session.open",
    )
    .await?;
    let manager = service_manager(&state)?;
    let session = manager
        .open_session_async(
            &authorization.tenant_id,
            target,
            request.channels,
            request.requested_ttl_ms,
        )
        .await?;
    record_session_authorization_audit(
        &state,
        &headers,
        &authorization.tenant_id,
        authorization.principal_class,
        authorization.auth_method,
        true,
        format!(
            "session open authorized target={} channels={} ttl_ms={}",
            session_target_audit(&session.target),
            session.channels.join(","),
            session
                .expires_at_millis
                .saturating_sub(session.created_at_millis)
        ),
    );
    Ok((
        StatusCode::CREATED,
        Json(SessionResourceResponse::from_resource(session)),
    ))
}

pub(crate) async fn list_sessions(
    State(state): State<Arc<AppState>>,
    QueryParams(query): QueryParams<SessionListQuery>,
    headers: HeaderMap,
) -> Result<Json<SessionCollectionResponse>, AppError> {
    let tenant_id = required_tenant_id(query.tenant_id.as_deref(), "session list")?;
    let authorization = authorize_session_route(
        &state,
        &headers,
        &tenant_id,
        SessionAction::List,
        None,
        None,
        &[],
        "native_http.session.list",
    )
    .await?;
    let manager = service_manager(&state)?;
    let mut sessions = manager.list_sessions_for_tenant(&authorization.tenant_id);
    sessions.sort_by(|left, right| left.id.cmp(&right.id));
    if let Some(principal) = authorization.principal.as_ref() {
        sessions.retain(|session| principal_can_list_session(principal, session));
    }
    if let Some(token) = query.page_token.as_deref() {
        sessions.retain(|session| session.id.as_str() > token);
    }
    if let Some(state_filter) = query.state.as_deref() {
        sessions.retain(|session| session_state(session.lifecycle_state) == state_filter);
    }
    let limit = query.limit.unwrap_or(100).clamp(1, 100);
    let remaining_count = sessions.len().saturating_sub(limit);
    let next_page_token = if remaining_count > 0 {
        sessions
            .get(limit.saturating_sub(1))
            .map(|session| session.id.clone())
    } else {
        None
    };
    sessions.truncate(limit);
    Ok(Json(SessionCollectionResponse {
        metadata: SessionCollectionMetadataResponse {
            tenant_id: authorization.tenant_id.as_str().to_owned(),
            resource_version: format!(
                "sessions:{}:{}",
                authorization.tenant_id,
                sessions.len() + remaining_count
            ),
            limit,
            next_page_token,
            remaining_count,
        },
        items: sessions
            .into_iter()
            .map(SessionResourceResponse::from_resource)
            .collect(),
    }))
}

pub(crate) async fn get_session(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<SessionResourceResponse>, AppError> {
    let lookup_authorization =
        authorize_session_resource_lookup(&state, &headers, SessionAction::Get, &session_id)
            .await?;
    let manager = service_manager(&state)?;
    let session = manager
        .get_session(&session_id)
        .ok_or_else(|| session_not_found(&session_id))?;
    ensure_session_lookup_tenant_matches(
        lookup_authorization.as_ref().map(|auth| &auth.tenant_id),
        &session,
        &session_id,
    )?;
    let authorization = authorize_session_resource_target(
        &state,
        &headers,
        lookup_authorization,
        &session,
        SessionAction::Get,
    )
    .await?;
    record_session_authorization_audit(
        &state,
        &headers,
        &authorization.tenant_id,
        authorization.principal_class,
        authorization.auth_method,
        true,
        format!("session get authorized id={}", session.id),
    );
    Ok(Json(SessionResourceResponse::from_resource(session)))
}

pub(crate) async fn close_session(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<SessionCloseRequest>,
) -> Result<Json<SessionResourceResponse>, AppError> {
    let lookup_authorization =
        authorize_session_resource_lookup(&state, &headers, SessionAction::Close, &session_id)
            .await?;
    let manager = service_manager(&state)?;
    let current = manager
        .get_session(&session_id)
        .ok_or_else(|| session_not_found(&session_id))?;
    ensure_session_lookup_tenant_matches(
        lookup_authorization.as_ref().map(|auth| &auth.tenant_id),
        &current,
        &session_id,
    )?;
    let authorization = authorize_session_resource_target(
        &state,
        &headers,
        lookup_authorization,
        &current,
        SessionAction::Close,
    )
    .await?;
    let reason = request
        .reason
        .as_deref()
        .map(str::trim)
        .filter(|reason| !reason.is_empty())
        .unwrap_or("client_close");
    let session = manager
        .close_session(&session_id, reason)
        .ok_or_else(|| session_not_found(&session_id))?;
    record_session_authorization_audit(
        &state,
        &headers,
        &authorization.tenant_id,
        authorization.principal_class,
        authorization.auth_method,
        true,
        format!("session close authorized id={} reason={reason}", session.id),
    );
    Ok(Json(SessionResourceResponse::from_resource(session)))
}

async fn authorize_session_resource_lookup(
    state: &Arc<AppState>,
    headers: &HeaderMap,
    action: SessionAction,
    _session_id: &str,
) -> Result<Option<SessionAuthorization>, AppError> {
    let surface = match action {
        SessionAction::Get => "native_http.session.get",
        SessionAction::Close => "native_http.session.close",
        SessionAction::Open | SessionAction::List => unreachable!("session resource lookup"),
    };
    if state.local_server_security.is_some() {
        let extracted = extract_server_access(
            headers,
            LocalServerCredentialMode::AuthorizationOrAdminHeader,
            state.local_server_security.as_deref(),
        )
        .map_err(AppError::from)?;
        match extracted.status {
            ExtractedServerAccessStatus::Authorized => return Ok(None),
            ExtractedServerAccessStatus::Missing => {}
            ExtractedServerAccessStatus::Invalid
                if extracted.auth_method == Some("local_admin_bearer") => {}
            ExtractedServerAccessStatus::Revoked => {
                return Err(AppError::unauthorized("auth.token_revoked"));
            }
            ExtractedServerAccessStatus::Expired => {
                return Err(AppError::unauthorized("auth.session_expired"));
            }
            ExtractedServerAccessStatus::Invalid => {
                return Err(AppError::unauthorized(
                    LocalServerCredentialMode::AuthorizationOrAdminHeader.unauthorized_message(),
                ));
            }
        }
    }

    let resolved =
        crate::application_auth::resolve_application_auth_from_headers(state, headers).await?;
    if !resolved.principal.authenticated {
        return Err(AppError::unauthorized(
            "session resource lookup requires operator credentials or authenticated tenant/spawned workload identity",
        ));
    }
    let tenant_id = application_session_tenant_id(&resolved.principal, surface)?;
    let principal_class = session_principal_class_from_principal(&resolved.principal)?;
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
    Ok(Some(SessionAuthorization {
        principal_class,
        tenant_id,
        auth_method: Some("application_bearer"),
        principal: Some(resolved.principal),
    }))
}

async fn authorize_session_resource_target(
    state: &Arc<AppState>,
    headers: &HeaderMap,
    lookup_authorization: Option<SessionAuthorization>,
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
        &session.tenant_id,
        action,
        Some(&session.id),
        Some(&session.target),
        &[],
        surface,
    )
    .await?;
    if let Some(preauthorized) = lookup_authorization {
        return Ok(SessionAuthorization {
            auth_method: preauthorized.auth_method,
            ..authorization
        });
    }
    Ok(authorization)
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

fn ensure_session_lookup_tenant_matches(
    lookup_tenant: Option<&TenantId>,
    session: &SessionResource,
    session_id: &str,
) -> Result<(), AppError> {
    if lookup_tenant.is_some_and(|tenant_id| tenant_id != &session.tenant_id) {
        return Err(session_not_found(session_id));
    }
    Ok(())
}

impl SessionResourceResponse {
    fn from_resource(resource: SessionResource) -> Self {
        let updated_at = format_millis_rfc3339(resource.updated_at_millis);
        let expires_at = format_millis_rfc3339(resource.expires_at_millis);
        let lifecycle_state = session_state(resource.lifecycle_state).to_owned();
        Self {
            metadata: SessionMetadataResponse {
                tenant_id: resource.tenant_id.as_str().to_owned(),
                id: resource.id.clone(),
                generation: resource.generation,
                resource_version: resource.resource_version,
                created_at: format_millis_rfc3339(resource.created_at_millis),
                updated_at: updated_at.clone(),
            },
            spec: SessionSpecResponse {
                target: session_target_response(resource.target),
                target_snapshot: session_target_snapshot_response(resource.target_snapshot),
                channels: resource.channels,
                expires_at: expires_at.clone(),
            },
            status: SessionStatusResponse {
                lifecycle_state: lifecycle_state.clone(),
                expires_at,
                closed_at: resource.closed_at_millis.map(format_millis_rfc3339),
                close_reason: resource.close_reason,
                conditions: vec![SessionConditionResponse {
                    condition_type: "Open",
                    status: if resource.lifecycle_state == SessionLifecycleState::Open {
                        "True"
                    } else {
                        "False"
                    },
                    reason: match resource.lifecycle_state {
                        SessionLifecycleState::Open => "LeaseActive",
                        SessionLifecycleState::Closed => "ClientClosed",
                        SessionLifecycleState::Expired => "LeaseExpired",
                    },
                    message: format!("session `{}` is {lifecycle_state}", resource.id),
                    observed_generation: resource.generation,
                    last_transition_time: updated_at,
                }],
            },
        }
    }
}

async fn authorize_session_route(
    state: &Arc<AppState>,
    headers: &HeaderMap,
    tenant_id: &TenantId,
    action: SessionAction,
    session_id: Option<&str>,
    target: Option<&SessionTarget>,
    channels: &[String],
    surface: &'static str,
) -> Result<SessionAuthorization, AppError> {
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
                SessionPrincipalClass::Tenant,
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
            SessionPrincipalClass::Tenant,
            None,
            false,
            "session route requires operator credentials or authenticated tenant/spawned workload identity",
        );
        return Err(AppError::unauthorized(
            "session route requires operator credentials or authenticated tenant/spawned workload identity",
        ));
    }

    let principal_class = session_principal_class_from_principal(&resolved.principal)?;
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
    if let Some(SessionTarget::Service { name }) = target {
        if !principal_has_exact_service_grant(&resolved.principal, name) {
            return Err(AppError::forbidden(format!(
                "{} principal requires an exact service grant for `{name}` before opening a service-targeted session",
                principal_class.as_str()
            )));
        }
    }
    if let Some(SessionTarget::Sandbox { id }) = target {
        if !principal_has_sandbox_reach(&resolved.principal, id) {
            return Err(AppError::forbidden(format!(
                "{} principal requires sandbox reach for `{id}` before opening a sandbox-targeted session",
                principal_class.as_str()
            )));
        }
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
        ExtractedServerAccessStatus::Authorized => {
            let tenant_context = parse_operator_tenant_context(tenant_id.as_str(), surface)?;
            Ok(Some(SessionAuthorization {
                principal_class: SessionPrincipalClass::Operator,
                tenant_id: tenant_context.tenant_id().clone(),
                auth_method: extracted.auth_method,
                principal: None,
            }))
        }
        ExtractedServerAccessStatus::Missing => Ok(None),
        ExtractedServerAccessStatus::Invalid
            if extracted.auth_method == Some("local_admin_bearer") =>
        {
            Ok(None)
        }
        ExtractedServerAccessStatus::Revoked => {
            record_session_authorization_audit(
                state,
                headers,
                tenant_id,
                SessionPrincipalClass::Operator,
                extracted.auth_method,
                false,
                "operator session route rejected: auth.token_revoked",
            );
            Err(AppError::unauthorized("auth.token_revoked"))
        }
        ExtractedServerAccessStatus::Expired => {
            record_session_authorization_audit(
                state,
                headers,
                tenant_id,
                SessionPrincipalClass::Operator,
                extracted.auth_method,
                false,
                "operator session route rejected: auth.session_expired",
            );
            Err(AppError::unauthorized("auth.session_expired"))
        }
        ExtractedServerAccessStatus::Invalid => {
            record_session_authorization_audit(
                state,
                headers,
                tenant_id,
                SessionPrincipalClass::Operator,
                extracted.auth_method,
                false,
                "operator session route rejected: invalid local admin credential",
            );
            Err(AppError::unauthorized(
                LocalServerCredentialMode::AuthorizationOrAdminHeader.unauthorized_message(),
            ))
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
    session_permission_values(principal).any(|permission| {
        session_permission_actions_allow(permission, action)
            && session_permission_scope_allows(permission, session_id, target)
            && session_permission_channels_allow(permission, channels)
    })
}

fn principal_has_session_list_permission(principal: &PrincipalContext) -> bool {
    session_permission_values(principal).any(|permission| {
        session_permission_actions_allow(permission, SessionAction::List)
            && session_permission_scope_is_listable(permission)
    })
}

fn principal_has_session_action_permission(
    principal: &PrincipalContext,
    action: SessionAction,
) -> bool {
    session_permission_values(principal)
        .any(|permission| session_permission_actions_allow(permission, action))
}

fn session_permission_values(principal: &PrincipalContext) -> impl Iterator<Item = &Value> {
    [&principal.verified_claims, &principal.claims]
        .into_iter()
        .flat_map(|claims| {
            [
                "nimbus_session_permissions",
                "nimbusSessionPermissions",
                "session_permissions",
                "sessionPermissions",
            ]
            .into_iter()
            .filter_map(|claim_name| claims.get(claim_name))
        })
        .flat_map(|value| match value {
            Value::Array(values) => values.iter().collect::<Vec<_>>(),
            value => vec![value],
        })
}

fn session_permission_actions_allow(permission: &Value, action: SessionAction) -> bool {
    let Some(actions) = permission.get("actions") else {
        return false;
    };
    match actions {
        Value::String(value) => value == action.as_str(),
        Value::Array(values) => values
            .iter()
            .any(|value| value.as_str() == Some(action.as_str())),
        _ => false,
    }
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
    sandbox_permission_values(principal).any(|permission| {
        sandbox_permission_actions_allow(permission, "get")
            && sandbox_permission_scope_allows(permission, sandbox_id)
    })
}

fn sandbox_permission_values(principal: &PrincipalContext) -> impl Iterator<Item = &Value> {
    [&principal.verified_claims, &principal.claims]
        .into_iter()
        .flat_map(|claims| {
            [
                "nimbus_sandbox_permissions",
                "nimbusSandboxPermissions",
                "sandbox_permissions",
                "sandboxPermissions",
            ]
            .into_iter()
            .filter_map(|claim_name| claims.get(claim_name))
        })
        .flat_map(|value| match value {
            Value::Array(values) => values.iter().collect::<Vec<_>>(),
            value => vec![value],
        })
}

fn sandbox_permission_actions_allow(permission: &Value, required_action: &str) -> bool {
    let Some(actions) = permission.get("actions") else {
        return false;
    };
    match actions {
        Value::String(value) => value == required_action,
        Value::Array(values) => values
            .iter()
            .any(|value| value.as_str() == Some(required_action)),
        _ => false,
    }
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

fn session_principal_class_from_principal(
    principal: &PrincipalContext,
) -> Result<SessionPrincipalClass, AppError> {
    let Some(value) = principal_claim_string(
        principal,
        &[
            "nimbus_principal_class",
            "nimbusPrincipalClass",
            "principal_class",
            "principalClass",
        ],
    ) else {
        return Ok(SessionPrincipalClass::Tenant);
    };
    match value {
        "tenant" => Ok(SessionPrincipalClass::Tenant),
        "spawned" | "spawned_workload" | "spawnedWorkload" | "workload" | "workload_identity" => {
            Ok(SessionPrincipalClass::SpawnedWorkload)
        }
        "operator" => Err(AppError::forbidden(
            "application credentials cannot resolve to operator principal class",
        )),
        other => Err(AppError::forbidden(format!(
            "unknown session route principal class `{other}`"
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

fn session_target_from_input(input: SessionTargetInput) -> Result<SessionTarget, AppError> {
    match (input.service, input.sandbox) {
        (Some(service), None) => Ok(SessionTarget::Service { name: service.name }),
        (None, Some(sandbox)) => Ok(SessionTarget::Sandbox { id: sandbox.id }),
        (None, None) | (Some(_), Some(_)) => Err(AppError::from(nimbus_core::Error::InvalidInput(
            "session open target requires exactly one of `service` or `sandbox`".to_owned(),
        ))),
    }
}

fn session_target_response(target: SessionTarget) -> SessionTargetResponse {
    match target {
        SessionTarget::Service { name } => {
            SessionTargetResponse::Service(ServiceTargetResponse { name })
        }
        SessionTarget::Sandbox { id } => {
            SessionTargetResponse::Sandbox(SandboxTargetResponse { id })
        }
    }
}

fn session_target_snapshot_response(
    snapshot: SessionTargetSnapshot,
) -> SessionTargetSnapshotResponse {
    match snapshot {
        SessionTargetSnapshot::Service {
            name,
            generation,
            backend,
            provider,
        } => SessionTargetSnapshotResponse::Service(ServiceTargetSnapshotResponse {
            name,
            generation,
            backend,
            provider,
        }),
        SessionTargetSnapshot::Sandbox {
            id,
            generation,
            profile,
            backend,
        } => SessionTargetSnapshotResponse::Sandbox(SandboxTargetSnapshotResponse {
            id,
            generation,
            profile,
            backend,
        }),
    }
}

fn session_target_audit(target: &SessionTarget) -> String {
    match target {
        SessionTarget::Service { name } => format!("service:{name}"),
        SessionTarget::Sandbox { id } => format!("sandbox:{id}"),
    }
}

fn required_tenant_id(value: Option<&str>, context: &str) -> Result<TenantId, AppError> {
    let Some(value) = value else {
        return Err(AppError::from(nimbus_core::Error::InvalidInput(format!(
            "{context} requires tenantId"
        ))));
    };
    parse_user_tenant_id(value.to_owned())
}

fn service_manager(state: &AppState) -> Result<Arc<nimbus_services::ServiceManager>, AppError> {
    state
        .service_manager()
        .ok_or_else(|| AppError::not_found("session routes require a server-owned service manager"))
}

fn session_not_found(session_id: &str) -> AppError {
    AppError::not_found(format!("session `{session_id}` was not found"))
}

fn session_state(state: SessionLifecycleState) -> &'static str {
    match state {
        SessionLifecycleState::Open => "open",
        SessionLifecycleState::Closed => "closed",
        SessionLifecycleState::Expired => "expired",
    }
}

fn record_session_authorization_audit(
    state: &AppState,
    headers: &HeaderMap,
    tenant_id: &TenantId,
    principal_class: SessionPrincipalClass,
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

fn format_millis_rfc3339(millis: u64) -> String {
    let nanos = (millis as i128).saturating_mul(1_000_000);
    match OffsetDateTime::from_unix_timestamp_nanos(nanos) {
        Ok(timestamp) => timestamp
            .format(&Rfc3339)
            .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned()),
        Err(_) => "1970-01-01T00:00:00Z".to_owned(),
    }
}
