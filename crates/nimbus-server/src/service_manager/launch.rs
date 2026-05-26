use nimbus_core::Error;
use nimbus_sandbox::SandboxHandle;

use crate::sandbox::SandboxServiceLaunch;
use crate::tenant_isolation::TenantIsolationDecision;

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
        let service_access =
            decision.service_access(&key.service_name, "sandbox service launch")?;
        service_access.ensure_sandbox_launch_matches(&launch, actual_backend)?;
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
