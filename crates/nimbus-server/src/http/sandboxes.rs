use std::collections::BTreeMap;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query as QueryParams, State};
use axum::http::{HeaderMap, StatusCode};
use nimbus_core::TenantId;
use nimbus_sandbox::{SandboxHandle, SandboxStatus};
use nimbus_services::SandboxResource;
use serde::{Deserialize, Serialize};

use super::authz::format_millis_rfc3339;
use super::pagination::{CollectionMetadataResponse, paginate_by_key};
use super::resource_control::sandboxes::{
    SandboxAction, authorize_sandbox_route, record_sandbox_authorization_audit,
};
use super::sandbox_spec::{SandboxSpecInput, SandboxSpecResponse};
use super::{AppError, AppState};

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
    metadata: CollectionMetadataResponse,
    items: Vec<SandboxResourceResponse>,
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
    if let Some(status) = query.status.as_deref() {
        resources.retain(|resource| sandbox_status(&resource.handle) == status);
    }
    if let Some(label_key) = query.label_key.as_deref() {
        resources.retain(|resource| {
            resource.labels.get(label_key).is_some_and(|value| {
                query
                    .label_value
                    .as_deref()
                    .is_none_or(|expected| value == expected)
            })
        });
    }
    if !authorization.is_operator() {
        resources.retain(|resource| authorization.allows(SandboxAction::List, Some(&resource.id)));
    }
    let (resources, page) = paginate_by_key(
        resources,
        query.page_token.as_deref(),
        query.limit,
        |resource| resource.id.as_str(),
    );
    Ok(Json(SandboxCollectionResponse {
        metadata: CollectionMetadataResponse {
            tenant_id: authorization.tenant_id.as_str().to_owned(),
            resource_version: format!(
                "sandboxes:{}:{}",
                authorization.tenant_id,
                resources.len() + page.remaining_count
            ),
            limit: page.limit,
            next_page_token: page.next_page_token,
            remaining_count: page.remaining_count,
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn wasm_sandbox_requests_fail_closed() {
        let request = json!({
            "profile": "wasm",
            "spec": {
                "owner": {
                    "kind": "standalone",
                    "displayName": "unsupported-wasm"
                },
                "backend": "container",
                "root": {
                    "kind": "oci_image",
                    "source": {
                        "kind": "reference",
                        "reference": "example.com/sandbox@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                    }
                },
                "process": {
                    "argv": ["/bin/true"]
                }
            }
        });

        let error = serde_json::from_value::<SandboxCreateRequest>(request)
            .expect_err("public sandbox API must not accept a wasm profile yet");

        assert!(
            error.to_string().contains("unknown variant `wasm`"),
            "error should name the unsupported profile: {error}"
        );
        assert!(
            error.to_string().contains("worker") && error.to_string().contains("desktop"),
            "error should list the currently supported sandbox profiles: {error}"
        );
    }
}
