use nimbus_core::Error;
use nimbus_sandbox::SandboxHandle;
use nimbus_tenant::TenantIsolationDecision;

use crate::{SandboxBackedServiceImplementation, ServiceImplementation};
use nimbus_node::LocalEnforcementBinding;

use super::ServiceManager;
use super::types::{TenantServiceKey, sandbox_backend_error};

impl ServiceManager {
    pub(super) async fn start_launch_async(
        &self,
        key: &TenantServiceKey,
        decision: &TenantIsolationDecision,
        launch: ServiceImplementation,
    ) -> Result<SandboxHandle, Error> {
        let implementation_kind = launch.implementation_kind();
        let Some(sandbox_launch) = launch.into_sandbox_backed() else {
            return Err(Error::InvalidInput(format!(
                "service {} for tenant {} uses a {} implementation, but this service manager can only launch sandbox-backed implementations",
                key.service_name, key.tenant_id, implementation_kind
            )));
        };
        let actual_backend = self.sandbox_backend.kind();
        let binding = LocalEnforcementBinding::from_decision(decision)?;
        let service_access = binding.service_access(&key.service_name)?;
        service_access.ensure_sandbox_spec_matches(sandbox_launch.spec(), actual_backend)?;
        decision.network().ensure_sandbox_egress_matches(
            sandbox_launch.spec(),
            "sandbox-backed service launch",
        )?;
        self.admit_launch_image(decision, &sandbox_launch)?;

        let handle = match sandbox_launch {
            SandboxBackedServiceImplementation::Image(launch) => {
                self.sandbox_backend.start_from_image(launch).await
            }
            SandboxBackedServiceImplementation::Build(launch) => {
                self.sandbox_backend.start_from_build(launch).await
            }
        }
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
