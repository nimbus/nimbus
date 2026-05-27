use nimbus_core::Error;
use nimbus_sandbox::SandboxHandle;

use crate::local_enforcement::LocalEnforcementBinding;
use crate::sandbox::SandboxServiceLaunch;
use crate::tenant::TenantIsolationDecision;

use super::SandboxServiceManager;
use super::types::{TenantServiceKey, sandbox_backend_error};

impl SandboxServiceManager {
    pub(super) async fn start_launch_async(
        &self,
        key: &TenantServiceKey,
        decision: &TenantIsolationDecision,
        launch: SandboxServiceLaunch,
    ) -> Result<SandboxHandle, Error> {
        let actual_backend = self.sandbox_backend.kind();
        let binding = LocalEnforcementBinding::from_decision(decision)?;
        let service_access = binding.service_access(&key.service_name)?;
        service_access.ensure_sandbox_spec_matches(launch.spec(), actual_backend)?;
        decision
            .network()
            .ensure_sandbox_egress_matches(launch.spec(), "sandbox service launch")?;
        self.admit_launch_image(decision, &launch)?;

        let handle = match launch {
            SandboxServiceLaunch::Image(launch) => {
                self.sandbox_backend.start_from_image(launch).await
            }
            SandboxServiceLaunch::Build(launch) => {
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
