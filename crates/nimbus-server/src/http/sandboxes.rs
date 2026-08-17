use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query as QueryParams, State};
use axum::http::{HeaderMap, StatusCode};
use nimbus_compute::sandboxes::SandboxCreateRequest;
use nimbus_compute::sandboxes::{
    SandboxCollectionResponse, SandboxListFilter, SandboxResourceResponse,
};
use serde::Deserialize;

use super::authz::{OperatorAuthScope, record_operator_authorization_audit};
use super::resource_control::sandboxes::{SandboxAction, authorize_sandbox_route};
use super::{AppError, AppState};

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SandboxListQuery {
    limit: Option<usize>,
    page_token: Option<String>,
    status: Option<String>,
    label_key: Option<String>,
    label_value: Option<String>,
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
    let profile = request.profile.as_str();
    let response =
        nimbus_compute::sandboxes::create_sandbox(&state, &authorization.tenant_context, request)
            .await?;
    record_operator_authorization_audit(
        &state,
        &headers,
        authorization.tenant_id.as_str(),
        OperatorAuthScope::Sandbox,
        authorization.principal_class,
        authorization.auth_method,
        true,
        format!("sandbox create authorized with profile {profile}"),
    );
    Ok((StatusCode::CREATED, Json(response)))
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
    let is_operator = authorization.is_operator();
    let response = nimbus_compute::sandboxes::list_sandboxes(
        &state,
        &authorization.tenant_id,
        is_operator,
        |sandbox_id| authorization.allows(SandboxAction::List, Some(sandbox_id)),
        SandboxListFilter {
            page_token: query.page_token,
            limit: query.limit,
            status: query.status,
            label_key: query.label_key,
            label_value: query.label_value,
        },
    )?;
    Ok(Json(response))
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
    let response =
        nimbus_compute::sandboxes::get_sandbox(&state, &authorization.tenant_id, &sandbox_id)
            .await?;
    Ok(Json(response))
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
    let response =
        nimbus_compute::sandboxes::stop_sandbox(&state, &authorization.tenant_context, &sandbox_id)
            .await?;
    Ok(Json(response))
}
