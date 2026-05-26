use futures::executor::block_on;
use nimbus_core::{Error, TenantId};
use nimbus_sandbox::{SandboxHandle, SandboxStatus};

use super::SandboxServiceManager;
use super::types::{TenantServiceKey, sandbox_backend_error};

impl SandboxServiceManager {
    pub(super) fn current_handle(&self, key: &TenantServiceKey) -> Option<SandboxHandle> {
        self.state
            .lock()
            .expect("manager lock should not be poisoned")
            .handles
            .get(key)
            .cloned()
    }

    pub(super) fn refresh_handle(
        &self,
        key: &TenantServiceKey,
    ) -> Result<Option<SandboxHandle>, Error> {
        let Some(handle) = self.current_handle(key) else {
            return Ok(None);
        };
        let inspected = block_on(self.sandbox_backend.inspect(&handle.id))
            .map_err(|error| sandbox_backend_error(key, "inspect", &error))?;
        let mut state = self
            .state
            .lock()
            .expect("manager lock should not be poisoned");
        match inspected {
            Some(handle) => {
                if matches!(
                    handle.status,
                    SandboxStatus::Stopped | SandboxStatus::Failed
                ) {
                    state.handles.remove(key);
                } else {
                    state.handles.insert(key.clone(), handle.clone());
                }
                Ok(Some(handle))
            }
            None => {
                state.handles.remove(key);
                Ok(None)
            }
        }
    }

    pub(super) async fn refresh_handle_async(
        &self,
        key: &TenantServiceKey,
    ) -> Result<Option<SandboxHandle>, Error> {
        let Some(handle) = self.current_handle(key) else {
            return Ok(None);
        };
        let inspected = self
            .sandbox_backend
            .inspect(&handle.id)
            .await
            .map_err(|error| sandbox_backend_error(key, "inspect", &error))?;
        let refreshed = {
            let mut state = self
                .state
                .lock()
                .expect("manager lock should not be poisoned");
            match inspected {
                Some(handle) => {
                    if matches!(
                        handle.status,
                        SandboxStatus::Stopped | SandboxStatus::Failed
                    ) {
                        state.handles.remove(key);
                    } else {
                        state.handles.insert(key.clone(), handle.clone());
                    }
                    Some(handle)
                }
                None => {
                    state.handles.remove(key);
                    None
                }
            }
        };

        if let Some(handle) = refreshed.as_ref() {
            self.record_service_handle(key, handle).await?;
        }

        Ok(refreshed)
    }

    pub(super) fn tenant_handles(
        &self,
        tenant_id: &TenantId,
    ) -> Vec<(TenantServiceKey, SandboxHandle)> {
        self.state
            .lock()
            .expect("manager lock should not be poisoned")
            .handles
            .iter()
            .filter(|(key, _)| &key.tenant_id == tenant_id)
            .map(|(key, handle)| (key.clone(), handle.clone()))
            .collect()
    }
}
