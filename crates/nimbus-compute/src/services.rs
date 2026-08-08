//! Service (definition and lifecycle) orchestration extracted from
//! `nimbus-server`'s `http::services` handlers (CP3). As with
//! `sandboxes.rs`, the transport handlers keep the header-dependent
//! authorization and its audit trail; the manager-orchestration body and
//! response construction live here.

use std::collections::BTreeMap;
use std::sync::Arc;

use nimbus_core::TenantId;
use nimbus_sandbox::{SandboxHandle, SandboxStatus};
use nimbus_services::{
    ExternalAuthPolicy, HealthCheckPolicy, ServiceBackend, ServiceDefinition,
    ServiceDefinitionSource, ServiceManager,
};
use nimbus_tenant::TenantIsolationContext;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::pagination::{CollectionMetadataResponse, paginate_by_key};
use crate::sandbox_spec::{SandboxSpecInput, SandboxSpecResponse};
use crate::state::{ComputeError, ComputeState};
use crate::workload_provisioner::WorkloadProvisionCancellation;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceResourceResponse {
    pub tenant_id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sandbox_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend: Option<String>,
    pub state: String,
    pub lifecycle_state: String,
    pub readiness: String,
    pub health: String,
    pub endpoints: Vec<ServiceEndpointResponse>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceEndpointResponse {
    pub name: String,
    pub protocol: String,
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum ServiceBackendInput {
    #[serde(rename = "sandbox")]
    Sandbox { sandbox: Box<SandboxSpecInput> },
    #[serde(rename = "builtIn")]
    BuiltIn { provider: String },
    #[serde(rename = "external")]
    External {
        endpoint: ExternalEndpointPolicyInput,
        auth: ExternalAuthPolicyInput,
        health: HealthCheckPolicyInput,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExternalEndpointPolicyInput {
    url: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum ExternalAuthPolicyInput {
    #[serde(rename = "none")]
    None,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum HealthCheckPolicyInput {
    #[serde(rename = "http")]
    Http { path: String },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceDefinitionCollectionResponse {
    metadata: CollectionMetadataResponse,
    items: Vec<ServiceDefinitionResourceResponse>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceDefinitionResourceResponse {
    metadata: ServiceDefinitionMetadataResponse,
    spec: ServiceDefinitionSpecResponse,
    status: ServiceDefinitionStatusResponse,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ServiceDefinitionMetadataResponse {
    tenant_id: String,
    name: String,
    generation: u64,
    resource_version: String,
    created_at: String,
    updated_at: String,
    labels: BTreeMap<String, String>,
    source: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ServiceDefinitionSpecResponse {
    backend: ServiceBackendResponse,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum ServiceBackendResponse {
    #[serde(rename = "sandbox")]
    Sandbox { sandbox: SandboxSpecResponse },
    #[serde(rename = "builtIn")]
    BuiltIn { provider: String },
    #[serde(rename = "external")]
    External {
        endpoint: ExternalEndpointPolicyResponse,
        auth: ExternalAuthPolicyResponse,
        health: HealthCheckPolicyResponse,
    },
    #[serde(rename = "redacted")]
    Redacted {
        backend: &'static str,
        redacted: bool,
        reason: &'static str,
    },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExternalEndpointPolicyResponse {
    url: String,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum ExternalAuthPolicyResponse {
    #[serde(rename = "none")]
    None,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum HealthCheckPolicyResponse {
    #[serde(rename = "http")]
    Http { path: String },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ServiceDefinitionStatusResponse {
    backend: &'static str,
    lifecycle_state: String,
    readiness: String,
    health: String,
    conditions: Vec<ServiceConditionResponse>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ServiceConditionResponse {
    #[serde(rename = "type")]
    condition_type: &'static str,
    status: &'static str,
    reason: &'static str,
    message: String,
    observed_generation: u64,
    last_transition_time: String,
}

/// Which projection a service-definition list/get response uses: full
/// backend detail (`Inspect`) or a redacted placeholder (`List`), decided by
/// the handler's per-item authorization check before this module is reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceDefinitionProjection {
    List,
    Inspect,
}

/// Lists service definitions for `tenant_id`. `allows_list`/`allows_inspect`
/// mirror the handler's `authorization.allows_service_definition(action,
/// name)` checks: `allows_list` gates which definitions an unauthenticated-
/// for-detail principal sees at all (skipped entirely for operators),
/// `allows_inspect` decides whether each surviving item gets the full or the
/// redacted backend projection.
pub fn list_service_definitions(
    compute: &ComputeState,
    tenant_id: &TenantId,
    is_operator: bool,
    allows_list: impl Fn(&str) -> bool,
    allows_inspect: impl Fn(&str) -> bool,
    page_token: Option<&str>,
    limit: Option<usize>,
) -> Result<ServiceDefinitionCollectionResponse, ComputeError> {
    let manager = service_manager(compute)?;
    let mut definitions = manager.service_definitions_for_tenant(tenant_id);
    definitions.sort_by(|left, right| left.name.cmp(&right.name));
    if !is_operator {
        definitions.retain(|definition| allows_list(&definition.name));
    }
    let (definitions, page) = paginate_by_key(definitions, page_token, limit, |definition| {
        definition.name.as_str()
    });

    Ok(ServiceDefinitionCollectionResponse {
        metadata: CollectionMetadataResponse {
            tenant_id: tenant_id.as_str().to_owned(),
            resource_version: format!(
                "services:{}:{}",
                tenant_id,
                definitions.len() + page.remaining_count
            ),
            limit: page.limit,
            next_page_token: page.next_page_token,
            remaining_count: page.remaining_count,
        },
        items: definitions
            .into_iter()
            .map(|definition| {
                let projection = if allows_inspect(&definition.name) {
                    ServiceDefinitionProjection::Inspect
                } else {
                    ServiceDefinitionProjection::List
                };
                ServiceDefinitionResourceResponse::from_definition(definition, projection)
            })
            .collect(),
    })
}

pub fn create_service_definition(
    compute: &ComputeState,
    tenant_id: &TenantId,
    service_name: &str,
    backend_input: ServiceBackendInput,
    labels: BTreeMap<String, String>,
) -> Result<ServiceDefinitionResourceResponse, ComputeError> {
    let manager = service_manager(compute)?;
    let backend = service_backend_from_input(tenant_id, service_name, backend_input)?;
    let definition = manager.create_service_definition(tenant_id, service_name, backend, labels)?;
    Ok(ServiceDefinitionResourceResponse::from_definition(
        definition,
        ServiceDefinitionProjection::Inspect,
    ))
}

pub fn update_service_definition(
    compute: &ComputeState,
    tenant_id: &TenantId,
    service_name: &str,
    expected_generation: u64,
    backend_input: ServiceBackendInput,
    labels: BTreeMap<String, String>,
) -> Result<ServiceDefinitionResourceResponse, ComputeError> {
    let manager = service_manager(compute)?;
    let backend = service_backend_from_input(tenant_id, service_name, backend_input)?;
    let definition = manager.update_service_definition(
        tenant_id,
        service_name,
        expected_generation,
        backend,
        labels,
    )?;
    Ok(ServiceDefinitionResourceResponse::from_definition(
        definition,
        ServiceDefinitionProjection::Inspect,
    ))
}

/// `force` has already been authorized by the handler (a `force` delete
/// requires a separate policy grant, checked and audited before this is
/// called); this only performs the manager-side delete.
pub async fn delete_service_definition(
    compute: &ComputeState,
    tenant_id: &TenantId,
    service_name: &str,
    expected_generation: u64,
    force: bool,
) -> Result<(), ComputeError> {
    let manager = service_manager(compute)?;
    manager
        .delete_service_definition_async(tenant_id, service_name, expected_generation, force)
        .await?;
    Ok(())
}

/// Which service-lifecycle manager verb a route invokes; drives both the
/// authorization surface string (used by the handler before calling
/// `service_lifecycle`) and the recorded lifecycle event action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceLifecycleVerb {
    Get,
    Start,
    Stop,
}

impl ServiceLifecycleVerb {
    pub fn surface(self) -> &'static str {
        match self {
            Self::Get => "native_http.service.get",
            Self::Start => "native_http.service.start",
            Self::Stop => "native_http.service.stop",
        }
    }

    /// The `record_service_event` action name, or `None` for `Get`, which does
    /// not mutate the service and so records no lifecycle event.
    fn event_action(self) -> Option<&'static str> {
        match self {
            Self::Get => None,
            Self::Start => Some("start"),
            Self::Stop => Some("stop"),
        }
    }
}

pub async fn service_lifecycle(
    compute: &ComputeState,
    tenant_context: &TenantIsolationContext,
    service_name: &str,
    verb: ServiceLifecycleVerb,
) -> Result<ServiceResourceResponse, ComputeError> {
    let manager = service_manager(compute)?;
    let (definition, handle) = match verb {
        ServiceLifecycleVerb::Get => {
            let definition = manager
                .service_definition_for_tenant(tenant_context.tenant_id(), service_name)
                .ok_or_else(|| service_not_found(tenant_context.tenant_id(), service_name))?;
            let observation = manager.service_definition_observation_for_tenant(
                tenant_context.tenant_id(),
                service_name,
            );
            (
                definition,
                observation.map(|observation| observation.handle),
            )
        }
        ServiceLifecycleVerb::Start => {
            let snapshot = compute
                .resource_provisioner()?
                .provision_sandbox_service(
                    tenant_context,
                    service_name,
                    &WorkloadProvisionCancellation::default(),
                )
                .await
                .map_err(|error| error.into_compute_error())?;
            (
                snapshot.definition,
                snapshot.observation.map(|observation| observation.handle),
            )
        }
        ServiceLifecycleVerb::Stop => {
            let retirement = compute
                .resource_provisioner()?
                .retire_sandbox_service(tenant_context, service_name)
                .await
                .map_err(|error| error.into_compute_error())?;
            (retirement.definition, retirement.retired_handle)
        }
    };
    if let (Some(action), Some(handle)) = (verb.event_action(), handle.as_ref()) {
        record_service_event(compute, tenant_context.tenant_id(), action, handle).await?;
    }

    Ok(match handle {
        Some(handle) => ServiceResourceResponse::from_handle(tenant_context.tenant_id(), &handle),
        None => ServiceResourceResponse::from_definition_without_observation(definition),
    })
}

impl ServiceResourceResponse {
    fn from_handle(tenant_id: &TenantId, handle: &SandboxHandle) -> Self {
        let state = nimbus_system::sandbox_status(handle.status).to_owned();
        Self {
            tenant_id: tenant_id.as_str().to_owned(),
            name: handle.name.clone(),
            sandbox_id: Some(handle.id.as_str().to_owned()),
            backend: Some(nimbus_system::sandbox_backend(handle.backend).to_owned()),
            state: state.clone(),
            lifecycle_state: state,
            readiness: readiness_from_status(handle.status).to_owned(),
            health: health_from_status(handle.status).to_owned(),
            endpoints: handle
                .published_endpoints
                .iter()
                .map(|endpoint| ServiceEndpointResponse {
                    name: endpoint.name.as_str().to_owned(),
                    protocol: nimbus_system::endpoint_protocol(endpoint.protocol).to_owned(),
                    host: endpoint.address.ip().to_string(),
                    port: endpoint.address.port(),
                })
                .collect(),
        }
    }

    fn from_definition_without_observation(definition: ServiceDefinition) -> Self {
        let (state, readiness, backend) = match definition.backend {
            ServiceBackend::Sandbox(spec) => (
                "pending",
                "pending",
                Some(nimbus_system::sandbox_backend(spec.backend).to_owned()),
            ),
            ServiceBackend::BuiltIn(_) | ServiceBackend::External(_) => {
                ("declared", "unknown", None)
            }
        };
        Self {
            tenant_id: definition.tenant_id.as_str().to_owned(),
            name: definition.name,
            sandbox_id: None,
            backend,
            state: state.to_owned(),
            lifecycle_state: state.to_owned(),
            readiness: readiness.to_owned(),
            health: "unknown".to_owned(),
            endpoints: Vec::new(),
        }
    }
}

impl ServiceDefinitionResourceResponse {
    fn from_definition(
        definition: ServiceDefinition,
        projection: ServiceDefinitionProjection,
    ) -> Self {
        let backend = match projection {
            ServiceDefinitionProjection::Inspect => {
                ServiceBackendResponse::from_backend(definition.backend.clone())
            }
            ServiceDefinitionProjection::List => {
                ServiceBackendResponse::redacted_from_backend(&definition.backend)
            }
        };
        let backend_kind = service_backend_wire_kind(&definition.backend);
        let lifecycle_state = match &definition.backend {
            ServiceBackend::Sandbox(_) => "stopped",
            ServiceBackend::BuiltIn(_) | ServiceBackend::External(_) => "declared",
        }
        .to_owned();
        let readiness = match &definition.backend {
            ServiceBackend::Sandbox(_) => "stopped",
            ServiceBackend::BuiltIn(_) | ServiceBackend::External(_) => "unknown",
        }
        .to_owned();
        let health = "unknown".to_owned();
        let updated_at = format_millis_rfc3339(definition.updated_at_millis);
        Self {
            metadata: ServiceDefinitionMetadataResponse {
                tenant_id: definition.tenant_id.as_str().to_owned(),
                name: definition.name.clone(),
                generation: definition.generation,
                resource_version: definition.resource_version,
                created_at: format_millis_rfc3339(definition.created_at_millis),
                updated_at: updated_at.clone(),
                labels: definition.labels,
                source: match definition.source {
                    ServiceDefinitionSource::StaticCatalog => "staticCatalog",
                    ServiceDefinitionSource::Dynamic => "dynamic",
                },
            },
            spec: ServiceDefinitionSpecResponse { backend },
            status: ServiceDefinitionStatusResponse {
                backend: backend_kind,
                lifecycle_state,
                readiness,
                health,
                conditions: vec![ServiceConditionResponse {
                    condition_type: "Admitted",
                    status: "True",
                    reason: "DefinitionValidated",
                    message: format!(
                        "service definition `{}` uses {backend_kind} backend",
                        definition.name
                    ),
                    observed_generation: definition.generation,
                    last_transition_time: updated_at,
                }],
            },
        }
    }
}

impl ServiceBackendResponse {
    fn from_backend(backend: ServiceBackend) -> Self {
        match backend {
            ServiceBackend::Sandbox(sandbox) => Self::Sandbox {
                sandbox: SandboxSpecResponse::from_spec(*sandbox),
            },
            ServiceBackend::BuiltIn(spec) => Self::BuiltIn {
                provider: spec.provider().to_owned(),
            },
            ServiceBackend::External(spec) => Self::External {
                endpoint: ExternalEndpointPolicyResponse {
                    url: spec.endpoint().to_owned(),
                },
                auth: external_auth_policy_response(spec.auth()),
                health: health_check_policy_response(spec.health()),
            },
        }
    }

    fn redacted_from_backend(backend: &ServiceBackend) -> Self {
        Self::Redacted {
            backend: service_backend_wire_kind(backend),
            redacted: true,
            reason: "requiresInspectPermission",
        }
    }
}

fn external_auth_policy_response(policy: ExternalAuthPolicy) -> ExternalAuthPolicyResponse {
    match policy {
        ExternalAuthPolicy::None => ExternalAuthPolicyResponse::None,
    }
}

fn health_check_policy_response(policy: &HealthCheckPolicy) -> HealthCheckPolicyResponse {
    match policy {
        HealthCheckPolicy::Http { path } => HealthCheckPolicyResponse::Http { path: path.clone() },
    }
}

/// Availability probe for the transport layer: `delete_service_definition`'s
/// pre-extraction handler resolved the service manager BEFORE evaluating the
/// force-delete permission, so a manager-less server answered not_found ahead
/// of the force 403/audit. The handler calls this at that same position to
/// keep the observable error precedence identical (the delete fn's own
/// internal lookup then re-checks harmlessly).
pub fn ensure_service_manager_available(compute: &ComputeState) -> Result<(), ComputeError> {
    service_manager(compute).map(|_| ())
}

fn service_manager(compute: &ComputeState) -> Result<Arc<ServiceManager>, ComputeError> {
    compute.service_manager().ok_or_else(|| {
        ComputeError::not_found(
            "service lifecycle endpoints require a server-owned service manager",
        )
    })
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

fn service_backend_from_input(
    tenant_id: &TenantId,
    service_name: &str,
    input: ServiceBackendInput,
) -> Result<ServiceBackend, ComputeError> {
    match input {
        ServiceBackendInput::Sandbox { sandbox } => Ok(ServiceBackend::sandbox(
            (*sandbox).into_spec(tenant_id, Some(service_name))?,
        )),
        ServiceBackendInput::BuiltIn { provider } => Ok(ServiceBackend::built_in(provider)),
        ServiceBackendInput::External {
            endpoint,
            auth,
            health,
        } => Ok(ServiceBackend::external(
            endpoint.url,
            external_auth_policy_from_input(auth),
            health_check_policy_from_input(health),
        )),
    }
}

fn external_auth_policy_from_input(input: ExternalAuthPolicyInput) -> ExternalAuthPolicy {
    match input {
        ExternalAuthPolicyInput::None => ExternalAuthPolicy::None,
    }
}

fn health_check_policy_from_input(input: HealthCheckPolicyInput) -> HealthCheckPolicy {
    match input {
        HealthCheckPolicyInput::Http { path } => HealthCheckPolicy::Http { path },
    }
}

fn service_backend_wire_kind(backend: &ServiceBackend) -> &'static str {
    match backend {
        ServiceBackend::Sandbox(_) => "sandbox",
        ServiceBackend::BuiltIn(_) => "builtIn",
        ServiceBackend::External(_) => "external",
    }
}

async fn record_service_event(
    compute: &ComputeState,
    tenant_id: &TenantId,
    action: &str,
    handle: &SandboxHandle,
) -> Result<(), ComputeError> {
    let service_state = nimbus_system::sandbox_status(handle.status);
    let message = format!(
        "service `{}` for tenant `{}` {} completed with state {}",
        handle.name, tenant_id, action, service_state
    );
    let correlation_id = format!("service:{}:{}:{action}", tenant_id, handle.name);
    nimbus_system::record_system_event_async(
        &compute.engine,
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
            "backend": nimbus_system::sandbox_backend(handle.backend),
        }),
        Some(&correlation_id),
    )
    .await
    .map_err(ComputeError::from)
}

fn service_not_found(tenant_id: &TenantId, service_name: &str) -> ComputeError {
    ComputeError::not_found(format!(
        "service `{service_name}` is not declared for tenant `{tenant_id}`"
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
