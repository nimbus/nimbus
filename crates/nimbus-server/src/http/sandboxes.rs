use std::collections::BTreeMap;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query as QueryParams, State};
use axum::http::{HeaderMap, StatusCode};
use nimbus_core::{PrincipalContext, TenantId};
use nimbus_operator::{
    ExtractedServerAccessStatus, LocalServerCredentialMode, extract_server_access,
};
use nimbus_sandbox::{SandboxHandle, SandboxStatus};
use nimbus_services::SandboxResource;
use nimbus_tenant::TenantIsolationContext;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::sandbox_spec::{SandboxSpecInput, SandboxSpecResponse};
use super::{AppError, AppState, parse_user_tenant_id};
use crate::local_server::{LocalServerAuditEvent, LocalServerRouteFamily, origin_from_headers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SandboxPrincipalClass {
    Operator,
    Tenant,
    SpawnedWorkload,
}

impl SandboxPrincipalClass {
    fn as_str(self) -> &'static str {
        match self {
            Self::Operator => "operator",
            Self::Tenant => "tenant",
            Self::SpawnedWorkload => "spawned_workload",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SandboxAction {
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
struct SandboxAuthorization {
    principal_class: SandboxPrincipalClass,
    tenant_id: TenantId,
    tenant_context: TenantIsolationContext,
    auth_method: Option<&'static str>,
    principal: Option<PrincipalContext>,
}

impl SandboxAuthorization {
    fn is_operator(&self) -> bool {
        self.principal_class == SandboxPrincipalClass::Operator
    }

    fn allows(&self, action: SandboxAction, sandbox_id: Option<&str>) -> bool {
        self.is_operator()
            || self.principal.as_ref().is_some_and(|principal| {
                principal_has_sandbox_permission(principal, action, sandbox_id)
            })
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SandboxCreateRequest {
    profile: SandboxProfile,
    spec: SandboxSpecInput,
    labels: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
enum SandboxProfile {
    Worker,
    Desktop,
}

impl SandboxProfile {
    fn as_str(self) -> &'static str {
        match self {
            Self::Worker => "worker",
            Self::Desktop => "desktop",
        }
    }
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SandboxListQuery {
    limit: Option<usize>,
    page_token: Option<String>,
    status: Option<String>,
    label_key: Option<String>,
    label_value: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SandboxCollectionResponse {
    metadata: SandboxCollectionMetadataResponse,
    items: Vec<SandboxResourceResponse>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SandboxCollectionMetadataResponse {
    tenant_id: String,
    resource_version: String,
    limit: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_page_token: Option<String>,
    remaining_count: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SandboxResourceResponse {
    metadata: SandboxMetadataResponse,
    spec: SandboxResourceSpecResponse,
    status: SandboxStatusResponse,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SandboxMetadataResponse {
    tenant_id: String,
    id: String,
    generation: u64,
    resource_version: String,
    created_at: String,
    updated_at: String,
    labels: BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SandboxResourceSpecResponse {
    profile: String,
    sandbox: SandboxSpecResponse,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SandboxStatusResponse {
    lifecycle_state: String,
    readiness: String,
    health: String,
    backend: String,
    endpoints: Vec<SandboxEndpointResponse>,
    conditions: Vec<SandboxConditionResponse>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SandboxEndpointResponse {
    name: String,
    protocol: String,
    host: String,
    port: u16,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SandboxConditionResponse {
    #[serde(rename = "type")]
    condition_type: &'static str,
    status: &'static str,
    reason: &'static str,
    message: String,
    observed_generation: u64,
    last_transition_time: String,
}

pub(crate) async fn create_sandbox(
    State(state): State<Arc<AppState>>,
    Path(tenant_id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<SandboxCreateRequest>,
) -> Result<(StatusCode, Json<SandboxResourceResponse>), AppError> {
    let authorization = authorize_sandbox_route(
        &state,
        &headers,
        tenant_id,
        SandboxAction::Create,
        None,
        "native_http.sandbox.create",
    )
    .await?;
    let manager = service_manager(&state)?;
    let spec = request.spec.into_spec(&authorization.tenant_id, None)?;
    let resource = manager
        .create_sandbox_resource_for_context_async(
            &authorization.tenant_context,
            request.profile.as_str(),
            spec,
            request.labels.unwrap_or_default(),
        )
        .await?;
    record_sandbox_authorization_audit(
        &state,
        &headers,
        &authorization.tenant_id,
        authorization.principal_class,
        authorization.auth_method,
        true,
        format!(
            "sandbox create authorized with profile {}",
            request.profile.as_str()
        ),
    );
    Ok((
        StatusCode::CREATED,
        Json(SandboxResourceResponse::from_resource(resource)),
    ))
}

pub(crate) async fn list_sandboxes(
    State(state): State<Arc<AppState>>,
    Path(tenant_id): Path<String>,
    QueryParams(query): QueryParams<SandboxListQuery>,
    headers: HeaderMap,
) -> Result<Json<SandboxCollectionResponse>, AppError> {
    let authorization = authorize_sandbox_route(
        &state,
        &headers,
        tenant_id,
        SandboxAction::List,
        None,
        "native_http.sandbox.list",
    )
    .await?;
    let manager = service_manager(&state)?;
    let mut resources = manager.list_sandbox_resources_for_tenant(&authorization.tenant_id);
    resources.sort_by(|left, right| left.id.cmp(&right.id));
    if let Some(token) = query.page_token.as_deref() {
        resources.retain(|resource| resource.id.as_str() > token);
    }
    if let Some(status) = query.status.as_deref() {
        resources.retain(|resource| sandbox_status(&resource.handle) == status);
    }
    if let Some(label_key) = query.label_key.as_deref() {
        resources.retain(|resource| {
            resource.labels.get(label_key).is_some_and(|value| {
                query
                    .label_value
                    .as_deref()
                    .map_or(true, |expected| value == expected)
            })
        });
    }
    if !authorization.is_operator() {
        resources.retain(|resource| authorization.allows(SandboxAction::List, Some(&resource.id)));
    }

    let limit = query.limit.unwrap_or(100).clamp(1, 100);
    let remaining_count = resources.len().saturating_sub(limit);
    let next_page_token = if remaining_count > 0 {
        resources
            .get(limit.saturating_sub(1))
            .map(|resource| resource.id.clone())
    } else {
        None
    };
    resources.truncate(limit);
    Ok(Json(SandboxCollectionResponse {
        metadata: SandboxCollectionMetadataResponse {
            tenant_id: authorization.tenant_id.as_str().to_owned(),
            resource_version: format!(
                "sandboxes:{}:{}",
                authorization.tenant_id,
                resources.len() + remaining_count
            ),
            limit,
            next_page_token,
            remaining_count,
        },
        items: resources
            .into_iter()
            .map(SandboxResourceResponse::from_resource)
            .collect(),
    }))
}

pub(crate) async fn get_sandbox(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, sandbox_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<SandboxResourceResponse>, AppError> {
    let authorization = authorize_sandbox_route(
        &state,
        &headers,
        tenant_id,
        SandboxAction::Get,
        Some(&sandbox_id),
        "native_http.sandbox.get",
    )
    .await?;
    let manager = service_manager(&state)?;
    let resource = manager
        .get_sandbox_resource_async(&authorization.tenant_id, &sandbox_id)
        .await?
        .ok_or_else(|| sandbox_not_found(&authorization.tenant_id, &sandbox_id))?;
    Ok(Json(SandboxResourceResponse::from_resource(resource)))
}

pub(crate) async fn stop_sandbox(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, sandbox_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<SandboxResourceResponse>, AppError> {
    let authorization = authorize_sandbox_route(
        &state,
        &headers,
        tenant_id,
        SandboxAction::Stop,
        Some(&sandbox_id),
        "native_http.sandbox.stop",
    )
    .await?;
    let manager = service_manager(&state)?;
    let resource = manager
        .stop_sandbox_resource_async(&authorization.tenant_id, &sandbox_id)
        .await?
        .ok_or_else(|| sandbox_not_found(&authorization.tenant_id, &sandbox_id))?;
    Ok(Json(SandboxResourceResponse::from_resource(resource)))
}

impl SandboxResourceResponse {
    fn from_resource(resource: SandboxResource) -> Self {
        let updated_at = format_millis_rfc3339(resource.updated_at_millis);
        let state = sandbox_status(&resource.handle).to_owned();
        Self {
            metadata: SandboxMetadataResponse {
                tenant_id: resource.tenant_id.as_str().to_owned(),
                id: resource.id.clone(),
                generation: resource.generation,
                resource_version: resource.resource_version,
                created_at: format_millis_rfc3339(resource.created_at_millis),
                updated_at: updated_at.clone(),
                labels: resource.labels,
            },
            spec: SandboxResourceSpecResponse {
                profile: resource.profile,
                sandbox: SandboxSpecResponse::from_spec(resource.spec),
            },
            status: SandboxStatusResponse {
                lifecycle_state: state.clone(),
                readiness: sandbox_readiness(resource.handle.status).to_owned(),
                health: sandbox_health(resource.handle.status).to_owned(),
                backend: sandbox_backend(resource.handle.backend),
                endpoints: sandbox_endpoints(&resource.handle),
                conditions: vec![SandboxConditionResponse {
                    condition_type: "Ready",
                    status: if resource.handle.status == SandboxStatus::Ready {
                        "True"
                    } else {
                        "False"
                    },
                    reason: "BackendState",
                    message: format!("sandbox `{}` is {state}", resource.id),
                    observed_generation: resource.generation,
                    last_transition_time: updated_at,
                }],
            },
        }
    }
}

async fn authorize_sandbox_route(
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
                SandboxPrincipalClass::Tenant,
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
            SandboxPrincipalClass::Tenant,
            None,
            false,
            "sandbox route requires operator credentials or authenticated tenant/spawned workload identity",
        );
        return Err(AppError::unauthorized(
            "sandbox route requires operator credentials or authenticated tenant/spawned workload identity",
        ));
    }

    let principal_class = sandbox_principal_class_from_principal(&resolved.principal)?;
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
        ExtractedServerAccessStatus::Authorized => Ok(Some(SandboxAuthorization {
            principal_class: SandboxPrincipalClass::Operator,
            tenant_context: TenantIsolationContext::operator(route_tenant.clone(), surface),
            tenant_id: route_tenant,
            auth_method: extracted.auth_method,
            principal: None,
        })),
        ExtractedServerAccessStatus::Missing => Ok(None),
        ExtractedServerAccessStatus::Invalid
            if extracted.auth_method == Some("local_admin_bearer") =>
        {
            Ok(None)
        }
        ExtractedServerAccessStatus::Revoked => {
            record_sandbox_authorization_audit(
                state,
                headers,
                &route_tenant,
                SandboxPrincipalClass::Operator,
                extracted.auth_method,
                false,
                "operator sandbox route rejected: auth.token_revoked",
            );
            Err(AppError::unauthorized("auth.token_revoked"))
        }
        ExtractedServerAccessStatus::Expired => {
            record_sandbox_authorization_audit(
                state,
                headers,
                &route_tenant,
                SandboxPrincipalClass::Operator,
                extracted.auth_method,
                false,
                "operator sandbox route rejected: auth.session_expired",
            );
            Err(AppError::unauthorized("auth.session_expired"))
        }
        ExtractedServerAccessStatus::Invalid => {
            record_sandbox_authorization_audit(
                state,
                headers,
                &route_tenant,
                SandboxPrincipalClass::Operator,
                extracted.auth_method,
                false,
                "operator sandbox route rejected: invalid local admin credential",
            );
            Err(AppError::unauthorized(
                LocalServerCredentialMode::AuthorizationOrAdminHeader.unauthorized_message(),
            ))
        }
    }
}

fn principal_has_sandbox_permission(
    principal: &PrincipalContext,
    action: SandboxAction,
    sandbox_id: Option<&str>,
) -> bool {
    sandbox_permission_values(principal).any(|permission| {
        sandbox_permission_actions_allow(permission, action)
            && sandbox_permission_scope_allows(permission, sandbox_id)
    })
}

fn principal_has_sandbox_list_permission(principal: &PrincipalContext) -> bool {
    sandbox_permission_values(principal).any(|permission| {
        sandbox_permission_actions_allow(permission, SandboxAction::List)
            && sandbox_permission_scope_is_listable(permission)
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

fn sandbox_permission_actions_allow(permission: &Value, action: SandboxAction) -> bool {
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

fn sandbox_principal_class_from_principal(
    principal: &PrincipalContext,
) -> Result<SandboxPrincipalClass, AppError> {
    let Some(value) = principal_claim_string(
        principal,
        &[
            "nimbus_principal_class",
            "nimbusPrincipalClass",
            "principal_class",
            "principalClass",
        ],
    ) else {
        return Ok(SandboxPrincipalClass::Tenant);
    };
    match value {
        "tenant" => Ok(SandboxPrincipalClass::Tenant),
        "spawned" | "spawned_workload" | "spawnedWorkload" | "workload" | "workload_identity" => {
            Ok(SandboxPrincipalClass::SpawnedWorkload)
        }
        "operator" => Err(AppError::forbidden(
            "application credentials cannot resolve to operator principal class",
        )),
        other => Err(AppError::forbidden(format!(
            "unknown sandbox route principal class `{other}`"
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

fn service_manager(state: &AppState) -> Result<Arc<nimbus_services::ServiceManager>, AppError> {
    state
        .service_manager()
        .ok_or_else(|| AppError::not_found("sandbox routes require a server-owned service manager"))
}

fn sandbox_endpoints(handle: &SandboxHandle) -> Vec<SandboxEndpointResponse> {
    handle
        .published_endpoints
        .iter()
        .map(|endpoint| SandboxEndpointResponse {
            name: endpoint.name.as_str().to_owned(),
            protocol: crate::system_tenant::endpoint_protocol(endpoint.protocol).to_owned(),
            host: endpoint.address.ip().to_string(),
            port: endpoint.address.port(),
        })
        .collect()
}

fn sandbox_status(handle: &SandboxHandle) -> &'static str {
    crate::system_tenant::sandbox_status(handle.status)
}

fn sandbox_backend(backend: nimbus_sandbox::SandboxBackendKind) -> String {
    crate::system_tenant::sandbox_backend(backend).to_owned()
}

fn sandbox_readiness(status: SandboxStatus) -> &'static str {
    match status {
        SandboxStatus::Ready => "ready",
        SandboxStatus::Stopped => "stopped",
        SandboxStatus::Failed => "failed",
        SandboxStatus::Starting | SandboxStatus::NotReady | SandboxStatus::Stopping => "not_ready",
    }
}

fn sandbox_health(status: SandboxStatus) -> &'static str {
    match status {
        SandboxStatus::Ready => "healthy",
        SandboxStatus::Failed => "unhealthy",
        SandboxStatus::Starting
        | SandboxStatus::NotReady
        | SandboxStatus::Stopping
        | SandboxStatus::Stopped => "unknown",
    }
}

fn sandbox_not_found(tenant_id: &TenantId, sandbox_id: &str) -> AppError {
    AppError::not_found(format!(
        "sandbox `{sandbox_id}` was not found for tenant `{tenant_id}`"
    ))
}

fn record_sandbox_authorization_audit(
    state: &AppState,
    headers: &HeaderMap,
    tenant_id: &TenantId,
    principal_class: SandboxPrincipalClass,
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

fn format_millis_rfc3339(millis: u64) -> String {
    let nanos = (millis as i128).saturating_mul(1_000_000);
    match OffsetDateTime::from_unix_timestamp_nanos(nanos) {
        Ok(timestamp) => timestamp
            .format(&Rfc3339)
            .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned()),
        Err(_) => "1970-01-01T00:00:00Z".to_owned(),
    }
}
