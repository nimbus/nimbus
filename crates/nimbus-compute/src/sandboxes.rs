//! Sandbox resource orchestration extracted from `nimbus-server`'s
//! `http::sandboxes` handlers (CP3). The transport handlers keep the
//! HeaderMap-dependent authorization and its audit trail; everything past
//! "authorization decided" — service-manager orchestration and response
//! construction — lives here so it can be exercised without a transport.

use std::collections::BTreeMap;
use std::sync::Arc;

use nimbus_core::TenantId;
use nimbus_sandbox::{SandboxHandle, SandboxStatus};
use nimbus_services::{SandboxResourceSnapshot, ServiceManager};
use nimbus_tenant::TenantIsolationContext;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::pagination::{CollectionMetadataResponse, paginate_by_key};
use crate::sandbox_spec::{SandboxSpecInput, SandboxSpecResponse};
use crate::state::{ComputeError, ComputeState};
use crate::workload_provisioner::WorkloadProvisionCancellation;
use crate::workload_saga::WorkloadTeardownCancellationToken;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SandboxCreateRequest {
    pub id: String,
    pub profile: SandboxProfile,
    pub spec: SandboxSpecInput,
    pub labels: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SandboxProfile {
    Worker,
    Desktop,
}

impl SandboxProfile {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Worker => "worker",
            Self::Desktop => "desktop",
        }
    }
}

#[derive(Debug, Default)]
pub struct SandboxListFilter {
    pub page_token: Option<String>,
    pub limit: Option<usize>,
    pub status: Option<String>,
    pub label_key: Option<String>,
    pub label_value: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxCollectionResponse {
    metadata: CollectionMetadataResponse,
    items: Vec<SandboxResourceResponse>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxResourceResponse {
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

pub async fn create_sandbox(
    compute: &ComputeState,
    tenant_context: &nimbus_tenant::TenantIsolationContext,
    request: SandboxCreateRequest,
) -> Result<SandboxResourceResponse, ComputeError> {
    let spec = request.spec.into_spec(tenant_context.tenant_id(), None)?;
    let snapshot = compute
        .resource_provisioner()?
        .provision_standalone_sandbox(
            tenant_context,
            &request.id,
            request.profile.as_str(),
            spec,
            request.labels.unwrap_or_default(),
            &WorkloadProvisionCancellation::default(),
        )
        .await
        .map_err(|error| error.into_compute_error())?;
    Ok(SandboxResourceResponse::from_snapshot(snapshot))
}

/// Lists sandbox resources for `tenant_id`, applying the pure query filters
/// (`status`/`label_key`/`label_value`) unconditionally and the
/// authorization-derived `allows` predicate only when the caller is not an
/// operator (operators see every sandbox). `allows` mirrors the handler's
/// `authorization.allows(SandboxAction::List, Some(id))` check.
pub fn list_sandboxes(
    compute: &ComputeState,
    tenant_id: &TenantId,
    is_operator: bool,
    allows: impl Fn(&str) -> bool,
    filter: SandboxListFilter,
) -> Result<SandboxCollectionResponse, ComputeError> {
    let manager = service_manager(compute)?;
    let mut resources = manager.list_sandbox_resource_snapshots_for_tenant(tenant_id);
    resources.sort_by(|left, right| left.source.id.cmp(&right.source.id));
    if let Some(status) = filter.status.as_deref() {
        resources.retain(|resource| sandbox_snapshot_status(resource) == status);
    }
    if let Some(label_key) = filter.label_key.as_deref() {
        resources.retain(|resource| {
            resource.source.labels.get(label_key).is_some_and(|value| {
                filter
                    .label_value
                    .as_deref()
                    .is_none_or(|expected| value == expected)
            })
        });
    }
    if !is_operator {
        resources.retain(|resource| allows(&resource.source.id));
    }
    let (resources, page) = paginate_by_key(
        resources,
        filter.page_token.as_deref(),
        filter.limit,
        |resource| resource.source.id.as_str(),
    );
    Ok(SandboxCollectionResponse {
        metadata: CollectionMetadataResponse {
            tenant_id: tenant_id.as_str().to_owned(),
            resource_version: format!(
                "sandboxes:{}:{}",
                tenant_id,
                resources.len() + page.remaining_count
            ),
            limit: page.limit,
            next_page_token: page.next_page_token,
            remaining_count: page.remaining_count,
        },
        items: resources
            .into_iter()
            .map(SandboxResourceResponse::from_snapshot)
            .collect(),
    })
}

pub async fn get_sandbox(
    compute: &ComputeState,
    tenant_id: &TenantId,
    sandbox_id: &str,
) -> Result<SandboxResourceResponse, ComputeError> {
    let manager = service_manager(compute)?;
    let snapshot = manager
        .sandbox_resource_snapshot_for_tenant(tenant_id, sandbox_id)?
        .ok_or_else(|| sandbox_not_found(tenant_id, sandbox_id))?;
    Ok(SandboxResourceResponse::from_snapshot(snapshot))
}

pub async fn stop_sandbox(
    compute: &ComputeState,
    tenant_context: &TenantIsolationContext,
    sandbox_id: &str,
) -> Result<SandboxResourceResponse, ComputeError> {
    let retirer = compute.resource_retirer()?;
    let cancellation = WorkloadTeardownCancellationToken::new();
    let snapshot = Box::pin(retirer.submit_sandbox_teardown_until_terminal(
        tenant_context,
        sandbox_id,
        &cancellation,
    ))
    .await
    .map_err(|error| error.into_compute_error())?;
    Ok(SandboxResourceResponse::from_snapshot(snapshot))
}

impl SandboxResourceResponse {
    fn from_snapshot(snapshot: SandboxResourceSnapshot) -> Self {
        let source = snapshot.source;
        let observed = snapshot.observation;
        let updated_at_millis = observed
            .as_ref()
            .map_or(source.updated_at_millis, |observation| {
                observation.observed_at_millis
            });
        let updated_at = format_millis_rfc3339(updated_at_millis);
        let state = observed
            .as_ref()
            .map_or("pending", |observation| sandbox_status(&observation.handle))
            .to_owned();
        let readiness = observed
            .as_ref()
            .map_or("pending", |observation| {
                sandbox_readiness(observation.handle.status)
            })
            .to_owned();
        let health = observed
            .as_ref()
            .map_or("unknown", |observation| {
                sandbox_health(observation.handle.status)
            })
            .to_owned();
        let endpoints = observed.as_ref().map_or_else(Vec::new, |observation| {
            sandbox_endpoints(&observation.handle)
        });
        let ready = observed
            .as_ref()
            .is_some_and(|observation| observation.handle.status == SandboxStatus::Ready);
        Self {
            metadata: SandboxMetadataResponse {
                tenant_id: source.tenant_id.as_str().to_owned(),
                id: source.id.clone(),
                generation: source.generation,
                resource_version: source.resource_version,
                created_at: format_millis_rfc3339(source.created_at_millis),
                updated_at: updated_at.clone(),
                labels: source.labels,
            },
            spec: SandboxResourceSpecResponse {
                profile: source.profile,
                sandbox: SandboxSpecResponse::from_spec(source.spec.clone()),
            },
            status: SandboxStatusResponse {
                lifecycle_state: state.clone(),
                readiness,
                health,
                backend: sandbox_backend(source.spec.backend),
                endpoints,
                conditions: vec![SandboxConditionResponse {
                    condition_type: "Ready",
                    status: if ready { "True" } else { "False" },
                    reason: if observed.is_some() {
                        "BackendState"
                    } else {
                        "DesiredSourceAccepted"
                    },
                    message: format!("sandbox `{}` is {state}", source.id),
                    observed_generation: observed
                        .as_ref()
                        .map_or(source.generation, |observation| {
                            observation.observed_execution_generation
                        }),
                    last_transition_time: updated_at,
                }],
            },
        }
    }
}

fn service_manager(compute: &ComputeState) -> Result<Arc<ServiceManager>, ComputeError> {
    compute.service_manager().ok_or_else(|| {
        ComputeError::not_found("sandbox routes require a server-owned service manager")
    })
}

fn sandbox_endpoints(handle: &SandboxHandle) -> Vec<SandboxEndpointResponse> {
    handle
        .published_endpoints
        .iter()
        .map(|endpoint| SandboxEndpointResponse {
            name: endpoint.name.as_str().to_owned(),
            protocol: nimbus_system::endpoint_protocol(endpoint.protocol).to_owned(),
            host: endpoint.address.ip().to_string(),
            port: endpoint.address.port(),
        })
        .collect()
}

fn sandbox_status(handle: &SandboxHandle) -> &'static str {
    nimbus_system::sandbox_status(handle.status)
}

fn sandbox_snapshot_status(snapshot: &SandboxResourceSnapshot) -> &'static str {
    snapshot
        .observation
        .as_ref()
        .map_or("pending", |observation| sandbox_status(&observation.handle))
}

fn sandbox_backend(backend: nimbus_sandbox::SandboxBackendKind) -> String {
    nimbus_system::sandbox_backend(backend).to_owned()
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

fn sandbox_not_found(tenant_id: &TenantId, sandbox_id: &str) -> ComputeError {
    ComputeError::not_found(format!(
        "sandbox `{sandbox_id}` was not found for tenant `{tenant_id}`"
    ))
}

/// Same epoch-millis-to-RFC3339 formatting as `http::authz::format_millis_rfc3339`
/// (which stays in `nimbus-server` for the routes that don't move in CP3);
/// duplicated here rather than shimmed since it is an 8-line pure utility, not
/// a seam.
fn format_millis_rfc3339(millis: u64) -> String {
    let nanos = (millis as i128).saturating_mul(1_000_000);
    match OffsetDateTime::from_unix_timestamp_nanos(nanos) {
        Ok(timestamp) => timestamp
            .format(&Rfc3339)
            .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned()),
        Err(_) => "1970-01-01T00:00:00Z".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use nimbus_sandbox::{
        SandboxBackendKind, SandboxOwnerSpec, SandboxProcessSpec, SandboxRootSpec, SandboxSpec,
    };
    use nimbus_services::{SandboxResourceSnapshot, SandboxResourceSource};
    use serde_json::json;

    use super::*;

    #[test]
    fn wasm_sandbox_requests_fail_closed() {
        let request = json!({
            "id": "unsupported-wasm",
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

    #[test]
    fn pending_response_uses_accepted_source_generation_without_provider_facts() {
        let tenant_id = TenantId::new("pending-tenant").expect("tenant should validate");
        let spec = SandboxSpec::new(
            tenant_id.clone(),
            SandboxOwnerSpec::standalone_named("pending-worker"),
            SandboxBackendKind::Krun,
            SandboxRootSpec::rootfs("/fixture/rootfs"),
            SandboxProcessSpec::new(["/bin/true"]),
        );
        let source = SandboxResourceSource::new(
            tenant_id,
            "stable-pending-id",
            "worker",
            spec,
            1,
            17,
            BTreeMap::new(),
        );

        let response = SandboxResourceResponse::from_snapshot(SandboxResourceSnapshot {
            source,
            observation: None,
        });

        assert_eq!(response.metadata.generation, 1);
        assert_eq!(response.status.lifecycle_state, "pending");
        assert_eq!(response.status.readiness, "pending");
        assert!(response.status.endpoints.is_empty());
        assert_eq!(
            response.status.conditions[0].reason,
            "DesiredSourceAccepted"
        );
        assert_eq!(response.status.conditions[0].observed_generation, 1);
    }
}
