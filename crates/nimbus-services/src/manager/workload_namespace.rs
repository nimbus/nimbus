//! Tenant-local workload-name ownership across services source kinds.

use nimbus_core::{Error, TenantId};

use crate::{ServiceBackend, ServiceDefinition};

use super::types::{ServiceManagerState, TenantSandboxResourceKey, TenantServiceKey};

pub(super) fn require_standalone_name_available(
    state: &ServiceManagerState,
    tenant_id: &TenantId,
    stable_resource_id: &str,
    catalog_definition: Option<&ServiceDefinition>,
) -> Result<(), Error> {
    let dynamic_conflict = state
        .definitions
        .get(&TenantServiceKey::new(tenant_id, stable_resource_id))
        .is_some_and(|definition| matches!(&definition.backend, ServiceBackend::Sandbox(_)));
    let catalog_conflict = catalog_definition
        .is_some_and(|definition| matches!(&definition.backend, ServiceBackend::Sandbox(_)));
    if dynamic_conflict || catalog_conflict {
        return Err(Error::conflict(format!(
            "workload name `{stable_resource_id}` for tenant `{tenant_id}` is already owned by a sandbox-backed service"
        )));
    }
    Ok(())
}

pub(super) fn require_sandbox_service_name_available(
    state: &ServiceManagerState,
    tenant_id: &TenantId,
    service_name: &str,
) -> Result<(), Error> {
    if state
        .sandbox_resource_sources
        .contains_key(&TenantSandboxResourceKey::new(tenant_id, service_name))
    {
        return Err(Error::conflict(format!(
            "workload name `{service_name}` for tenant `{tenant_id}` is already owned by a standalone sandbox"
        )));
    }
    Ok(())
}
