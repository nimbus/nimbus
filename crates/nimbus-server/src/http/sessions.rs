use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query as QueryParams, State};
use axum::http::{HeaderMap, StatusCode};
use nimbus_core::TenantId;
use nimbus_services::{
    SessionLifecycleState, SessionResource, SessionTarget, SessionTargetSnapshot,
};
use serde::{Deserialize, Serialize};

use super::authz::{OperatorAuthScope, format_millis_rfc3339, record_operator_authorization_audit};
use super::resource_control::sessions::{
    SessionAction, SessionRouteAuthorizationRequest, authorize_session_resource_lookup,
    authorize_session_resource_target, authorize_session_route,
};
use super::{AppError, AppState, parse_user_tenant_id};
use nimbus_compute::pagination::{CollectionMetadataResponse, paginate_by_key};

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
pub(crate) struct SessionLookupQuery {
    tenant_id: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SessionCloseRequest {
    reason: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionCollectionResponse {
    metadata: CollectionMetadataResponse,
    items: Vec<SessionResourceResponse>,
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
        SessionRouteAuthorizationRequest {
            tenant_id: &tenant_id,
            action: SessionAction::Open,
            session_id: None,
            target: Some(&target),
            channels: &request.channels,
            surface: "native_http.session.open",
        },
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
    record_operator_authorization_audit(
        &state,
        &headers,
        authorization.tenant_id.as_str(),
        OperatorAuthScope::Session,
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
        SessionRouteAuthorizationRequest {
            tenant_id: &tenant_id,
            action: SessionAction::List,
            session_id: None,
            target: None,
            channels: &[],
            surface: "native_http.session.list",
        },
    )
    .await?;
    let manager = service_manager(&state)?;
    let mut sessions = manager.list_sessions_for_tenant(&authorization.tenant_id);
    sessions.sort_by(|left, right| left.id.cmp(&right.id));
    sessions.retain(|session| authorization.can_list_session(session));
    if let Some(state_filter) = query.state.as_deref() {
        sessions.retain(|session| session_state(session.lifecycle_state) == state_filter);
    }
    let (sessions, page) = paginate_by_key(
        sessions,
        query.page_token.as_deref(),
        query.limit,
        |session| session.id.as_str(),
    );
    Ok(Json(SessionCollectionResponse {
        metadata: CollectionMetadataResponse {
            tenant_id: authorization.tenant_id.as_str().to_owned(),
            resource_version: format!(
                "sessions:{}:{}",
                authorization.tenant_id,
                sessions.len() + page.remaining_count
            ),
            limit: page.limit,
            next_page_token: page.next_page_token,
            remaining_count: page.remaining_count,
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
    QueryParams(query): QueryParams<SessionLookupQuery>,
    headers: HeaderMap,
) -> Result<Json<SessionResourceResponse>, AppError> {
    let route_tenant_id = optional_tenant_id(query.tenant_id.as_deref())?;
    let lookup_authorization = authorize_session_resource_lookup(
        &state,
        &headers,
        SessionAction::Get,
        &session_id,
        route_tenant_id.as_ref(),
    )
    .await?;
    let manager = service_manager(&state)?;
    let session = manager
        .get_session(&lookup_authorization.tenant_id, &session_id)
        .ok_or_else(|| session_not_found(&session_id))?;
    let authorization = authorize_session_resource_target(
        &state,
        &headers,
        &lookup_authorization,
        &session,
        SessionAction::Get,
    )
    .await?;
    record_operator_authorization_audit(
        &state,
        &headers,
        authorization.tenant_id.as_str(),
        OperatorAuthScope::Session,
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
    QueryParams(query): QueryParams<SessionLookupQuery>,
    headers: HeaderMap,
    Json(request): Json<SessionCloseRequest>,
) -> Result<Json<SessionResourceResponse>, AppError> {
    let route_tenant_id = optional_tenant_id(query.tenant_id.as_deref())?;
    let lookup_authorization = authorize_session_resource_lookup(
        &state,
        &headers,
        SessionAction::Close,
        &session_id,
        route_tenant_id.as_ref(),
    )
    .await?;
    let manager = service_manager(&state)?;
    let current = manager
        .get_session(&lookup_authorization.tenant_id, &session_id)
        .ok_or_else(|| session_not_found(&session_id))?;
    let authorization = authorize_session_resource_target(
        &state,
        &headers,
        &lookup_authorization,
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
        .close_session(&authorization.tenant_id, &session_id, reason)
        .ok_or_else(|| session_not_found(&session_id))?;
    record_operator_authorization_audit(
        &state,
        &headers,
        authorization.tenant_id.as_str(),
        OperatorAuthScope::Session,
        authorization.principal_class,
        authorization.auth_method,
        true,
        format!("session close authorized id={} reason={reason}", session.id),
    );
    Ok(Json(SessionResourceResponse::from_resource(session)))
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

fn optional_tenant_id(value: Option<&str>) -> Result<Option<TenantId>, AppError> {
    value
        .map(|value| parse_user_tenant_id(value.to_owned()))
        .transpose()
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
