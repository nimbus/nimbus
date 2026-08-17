//! Source-owner adapter for exact workload-provision freshness checks.
//!
//! The adapter reads only immutable service-manager catalog state. Provider
//! inspection and lifecycle effects are deliberately absent from this seam.

use std::sync::Arc;

use nimbus_services::ServiceManager;
use nimbus_workloads::{
    WorkloadProvisionSourceEvidence, WorkloadProvisionSourceGeneration,
    WorkloadProvisionSourceIdentity, WorkloadProvisionSourceKind,
    WorkloadProvisionSourceResourceVersion, WorkloadSagaKey,
};

use crate::workload_executable::encode_sandbox_spec;
use crate::workload_saga::{
    WorkloadProvisionSourceAuthority, WorkloadProvisionSourceAuthorityError,
    WorkloadProvisionSourceFuture, sandbox_execution_provider_id,
};

/// Read-only source evidence backed by one services-owned catalog.
pub struct ServiceManagerWorkloadProvisionSourceAuthority {
    service_manager: Arc<ServiceManager>,
}

impl ServiceManagerWorkloadProvisionSourceAuthority {
    pub fn new(service_manager: Arc<ServiceManager>) -> Self {
        Self { service_manager }
    }

    fn current(
        &self,
        key: &WorkloadSagaKey,
        identity: &WorkloadProvisionSourceIdentity,
    ) -> Result<WorkloadProvisionSourceEvidence, WorkloadProvisionSourceAuthorityError> {
        if key.workload_id().as_str() != identity.stable_name() {
            return Err(WorkloadProvisionSourceAuthorityError::Corrupt);
        }
        match identity.kind() {
            WorkloadProvisionSourceKind::StandaloneSandbox => {
                let resource = self
                    .service_manager
                    .sandbox_resource_source_for_tenant(key.tenant_id(), identity.stable_name())
                    .map_err(|_| WorkloadProvisionSourceAuthorityError::Unavailable)?
                    .ok_or(WorkloadProvisionSourceAuthorityError::NotFound)?;
                if identity.profile() != Some(resource.profile.as_str())
                    || resource.id != identity.stable_name()
                    || resource.tenant_id != *key.tenant_id()
                {
                    return Err(WorkloadProvisionSourceAuthorityError::Corrupt);
                }
                let executable = encode_sandbox_spec(&resource.spec)
                    .map_err(|_| WorkloadProvisionSourceAuthorityError::Corrupt)?;
                let providers = providers_for_backend(resource.spec.backend);
                WorkloadProvisionSourceEvidence::standalone_sandbox(
                    identity.clone(),
                    WorkloadProvisionSourceGeneration::new(resource.generation),
                    source_version(resource.resource_version)?,
                    executable.content_digest(),
                    providers.0,
                    providers.1,
                )
                .map_err(|_| WorkloadProvisionSourceAuthorityError::Corrupt)
            }
            WorkloadProvisionSourceKind::SandboxBackedService => {
                let definition = self
                    .service_manager
                    .service_definition_for_tenant(key.tenant_id(), identity.stable_name())
                    .ok_or(WorkloadProvisionSourceAuthorityError::NotFound)?;
                if definition.name != identity.stable_name()
                    || definition.tenant_id != *key.tenant_id()
                {
                    return Err(WorkloadProvisionSourceAuthorityError::Corrupt);
                }
                let spec = definition
                    .backend
                    .sandbox_spec()
                    .ok_or(WorkloadProvisionSourceAuthorityError::Corrupt)?;
                let executable = encode_sandbox_spec(spec)
                    .map_err(|_| WorkloadProvisionSourceAuthorityError::Corrupt)?;
                let providers = providers_for_backend(spec.backend);
                WorkloadProvisionSourceEvidence::sandbox_backed_service(
                    identity.clone(),
                    WorkloadProvisionSourceGeneration::new(definition.generation),
                    source_version(definition.resource_version)?,
                    executable.content_digest(),
                    providers.0,
                    providers.1,
                )
                .map_err(|_| WorkloadProvisionSourceAuthorityError::Corrupt)
            }
        }
    }
}

fn providers_for_backend(
    backend: nimbus_sandbox::SandboxBackendKind,
) -> (
    nimbus_network::NetworkProviderId,
    nimbus_workloads::WorkloadExecutionProviderId,
) {
    let attachment = nimbus_sandbox::sandbox_network_plan_requirements(backend)
        .required_attachment_provider_id()
        .clone();
    (attachment, sandbox_execution_provider_id(backend))
}

impl WorkloadProvisionSourceAuthority for ServiceManagerWorkloadProvisionSourceAuthority {
    fn current_source<'a>(
        &'a self,
        key: &'a WorkloadSagaKey,
        identity: &'a WorkloadProvisionSourceIdentity,
    ) -> WorkloadProvisionSourceFuture<'a> {
        Box::pin(async move { self.current(key, identity) })
    }
}

fn source_version(
    value: String,
) -> Result<WorkloadProvisionSourceResourceVersion, WorkloadProvisionSourceAuthorityError> {
    WorkloadProvisionSourceResourceVersion::new(value)
        .map_err(|_| WorkloadProvisionSourceAuthorityError::Corrupt)
}

#[cfg(test)]
#[path = "workload_provision_source/tests.rs"]
mod tests;
