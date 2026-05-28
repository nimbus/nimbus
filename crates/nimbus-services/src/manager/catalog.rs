use std::collections::BTreeMap;

use nimbus_core::TenantId;
use nimbus_sandbox::{SandboxHandle, SandboxStatus};

use crate::SandboxCatalog;

use super::SandboxServiceManager;

impl SandboxCatalog for SandboxServiceManager {
    fn sandboxes_for_tenant(&self, tenant_id: &TenantId) -> BTreeMap<String, SandboxHandle> {
        let keys = {
            self.state
                .lock()
                .expect("manager lock should not be poisoned")
                .handles
                .keys()
                .filter(|key| &key.tenant_id == tenant_id)
                .cloned()
                .collect::<Vec<_>>()
        };

        keys.into_iter()
            .filter_map(|key| {
                self.refresh_handle(&key)
                    .ok()
                    .flatten()
                    .filter(|handle| {
                        !matches!(
                            handle.status,
                            SandboxStatus::Stopped | SandboxStatus::Failed
                        )
                    })
                    .map(|handle| (key.service_name.clone(), handle))
            })
            .collect()
    }
}
