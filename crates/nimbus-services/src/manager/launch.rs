use nimbus_core::Error;
use nimbus_sandbox::SandboxHandle;
use nimbus_tenant::{TenantIsolationDecision, TenantVolumePolicyDecision};

use crate::ServiceBackend;
use nimbus_node::LocalEnforcementBinding;

use super::ServiceManager;
use super::types::{TenantServiceKey, sandbox_backend_error};

impl ServiceManager {
    pub(super) async fn start_sandbox_service_async(
        &self,
        key: &TenantServiceKey,
        decision: &TenantIsolationDecision,
        service_backend: ServiceBackend,
        volume_policy: &TenantVolumePolicyDecision,
    ) -> Result<SandboxHandle, Error> {
        let backend_kind = service_backend.kind();
        let Some(sandbox_spec) = service_backend.into_sandbox_spec() else {
            return Err(Error::InvalidInput(format!(
                "service {} for tenant {} uses a {} backend, but this service manager can only start sandbox-backed services",
                key.service_name, key.tenant_id, backend_kind
            )));
        };
        if sandbox_spec.service_name() != Some(key.service_name.as_str()) {
            return Err(Error::InvalidInput(format!(
                "service {} for tenant {} declared sandbox owner {:?}, but service activation requires matching service owner metadata",
                key.service_name, key.tenant_id, sandbox_spec.owner,
            )));
        }
        let actual_backend = self.sandbox_backend.kind();
        let binding = LocalEnforcementBinding::from_decision(decision)?;
        let service_access = binding.service_access(&key.service_name)?;
        service_access.ensure_sandbox_spec_matches(&sandbox_spec, actual_backend)?;
        decision
            .network()
            .ensure_sandbox_egress_matches(&sandbox_spec, "sandbox-backed service launch")?;
        volume_policy
            .ensure_sandbox_mounts_match(&sandbox_spec, "sandbox-backed service launch")?;
        self.admit_sandbox_root(decision, &sandbox_spec)?;

        let handle = self
            .sandbox_backend
            .start(sandbox_spec)
            .await
            .map_err(|error| sandbox_backend_error(key, "start", &error))?;
        if handle.tenant_id != key.tenant_id {
            return Err(Error::InvalidInput(format!(
                "sandbox backend returned handle for tenant {}, but service activation requested tenant {}",
                handle.tenant_id, key.tenant_id
            )));
        }
        if handle.name != key.service_name {
            return Err(Error::InvalidInput(format!(
                "sandbox backend returned handle for service {}, but service activation requested {}",
                handle.name, key.service_name
            )));
        }

        self.state
            .lock()
            .expect("manager lock should not be poisoned")
            .handles
            .insert(key.clone(), handle.clone());
        self.record_service_handle(key, &handle).await?;
        Ok(handle)
    }
}
