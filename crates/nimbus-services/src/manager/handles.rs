use nimbus_core::{Error, TenantId};
use nimbus_sandbox::{SandboxCleanupObservation, SandboxHandle, SandboxInspection, SandboxStatus};
use nimbus_tenant::TenantIsolationContext;

use super::ServiceManager;
use super::types::{TenantServiceKey, sandbox_backend_error};

impl ServiceManager {
    pub(super) fn current_handle(&self, key: &TenantServiceKey) -> Option<SandboxHandle> {
        self.state
            .lock()
            .expect("manager lock should not be poisoned")
            .handles
            .get(key)
            .cloned()
    }

    pub(super) async fn refresh_inspection_async(
        &self,
        key: &TenantServiceKey,
    ) -> Result<Option<SandboxInspection>, Error> {
        let Some(handle) = self.current_handle(key) else {
            return Ok(None);
        };
        let inspected = self
            .sandbox_backend
            .inspect(&handle.id)
            .await
            .map_err(|error| sandbox_backend_error(key, "inspect", &error))?;
        if let Some(inspection) = inspected.as_ref() {
            validate_service_inspection_identity(key, &handle, inspection)?;
        }
        let refreshed = {
            let mut state = self
                .state
                .lock()
                .expect("manager lock should not be poisoned");
            match inspected {
                Some(inspection) => {
                    let handle = &inspection.handle;
                    if matches!(
                        handle.status,
                        SandboxStatus::Stopped | SandboxStatus::Failed
                    ) && inspection.cleanup == SandboxCleanupObservation::Finalized
                    {
                        state.handles.remove(key);
                    } else {
                        state.handles.insert(key.clone(), handle.clone());
                    }
                    Some(inspection)
                }
                None => {
                    state.handles.remove(key);
                    None
                }
            }
        };

        if let Some(inspection) = refreshed.as_ref() {
            self.record_service_handle(key, &inspection.handle).await?;
        }

        Ok(refreshed)
    }

    pub(super) async fn refresh_handle_async(
        &self,
        key: &TenantServiceKey,
    ) -> Result<Option<SandboxHandle>, Error> {
        Ok(self
            .refresh_inspection_async(key)
            .await?
            .map(|inspection| inspection.handle))
    }

    pub async fn inspect_service_lifecycle_for_context_async(
        &self,
        isolation: &TenantIsolationContext,
        service_name: &str,
    ) -> Result<Option<SandboxInspection>, Error> {
        let decision = self.service_lifecycle_decision(isolation, service_name)?;
        let key = TenantServiceKey::new(decision.tenant_id(), service_name);
        self.refresh_inspection_async(&key).await
    }

    pub async fn inspect_service_for_context_async(
        &self,
        isolation: &TenantIsolationContext,
        service_name: &str,
    ) -> Result<Option<SandboxHandle>, Error> {
        Ok(self
            .inspect_service_lifecycle_for_context_async(isolation, service_name)
            .await?
            .map(|inspection| inspection.handle))
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

fn validate_service_inspection_identity(
    key: &TenantServiceKey,
    expected: &SandboxHandle,
    inspection: &SandboxInspection,
) -> Result<(), Error> {
    let observed = &inspection.handle;
    if observed.id == expected.id
        && observed.tenant_id == key.tenant_id
        && observed.name == key.service_name
        && observed.backend == expected.backend
    {
        return Ok(());
    }

    Err(Error::Internal(format!(
        "sandbox backend returned crossed inspection identity for service {} tenant {}: \
         expected sandbox {} tenant {} name {} backend {:?}, observed sandbox {} tenant {} \
         name {} backend {:?}",
        key.service_name,
        key.tenant_id,
        expected.id,
        key.tenant_id,
        key.service_name,
        expected.backend,
        observed.id,
        observed.tenant_id,
        observed.name,
        observed.backend
    )))
}
