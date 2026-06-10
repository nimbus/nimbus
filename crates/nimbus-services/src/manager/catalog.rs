use std::collections::BTreeMap;

use nimbus_core::TenantId;
use nimbus_sandbox::{SandboxHandle, SandboxStatus};

use crate::ServiceInstanceCatalog;

use super::ServiceManager;

impl ServiceManager {
    pub fn service_declared_for_tenant(&self, tenant_id: &TenantId, service_name: &str) -> bool {
        self.service_backend_for_tenant(tenant_id, service_name)
            .is_some()
    }
}

impl ServiceInstanceCatalog for ServiceManager {
    fn service_instances_for_tenant(
        &self,
        tenant_id: &TenantId,
    ) -> BTreeMap<String, SandboxHandle> {
        self.state
            .lock()
            .expect("manager lock should not be poisoned")
            .handles
            .iter()
            .filter(|(key, handle)| {
                &key.tenant_id == tenant_id
                    && handle.tenant_id == *tenant_id
                    && !matches!(
                        handle.status,
                        SandboxStatus::Stopped | SandboxStatus::Failed
                    )
            })
            .map(|(key, handle)| (key.service_name.clone(), handle.clone()))
            .collect()
    }
}
