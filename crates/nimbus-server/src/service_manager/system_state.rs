use std::sync::Arc;

use nimbus_core::Error;
use nimbus_engine::Service;
use nimbus_sandbox::SandboxHandle;

use super::SandboxServiceManager;
use super::types::TenantServiceKey;

impl SandboxServiceManager {
    pub(crate) fn attach_system_state_service(&self, service: Arc<Service>) {
        *self
            .system_state_service
            .lock()
            .expect("system state service lock should not be poisoned") = Some(service);
    }

    pub(super) async fn record_service_handle(
        &self,
        key: &TenantServiceKey,
        handle: &SandboxHandle,
    ) -> Result<(), Error> {
        let service = self
            .system_state_service
            .lock()
            .expect("system state service lock should not be poisoned")
            .clone();
        let Some(service) = service else {
            return Ok(());
        };
        crate::system_tenant::record_service_handle_async(&service, &key.tenant_id, handle).await
    }
}
