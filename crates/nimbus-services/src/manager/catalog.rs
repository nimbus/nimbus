use std::collections::BTreeMap;

use nimbus_core::TenantId;
use nimbus_sandbox::SandboxHandle;

use crate::ServiceInstanceCatalog;

use super::ServiceManager;

impl ServiceManager {
    pub fn service_declared_for_tenant(&self, tenant_id: &TenantId, service_name: &str) -> bool {
        self.service_definition_for_tenant(tenant_id, service_name)
            .is_some()
    }
}

impl ServiceInstanceCatalog for ServiceManager {
    fn service_instances_for_tenant(
        &self,
        tenant_id: &TenantId,
    ) -> BTreeMap<String, SandboxHandle> {
        self.service_instances_for_resolution(tenant_id)
    }
}
