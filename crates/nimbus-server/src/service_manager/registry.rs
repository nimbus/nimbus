use futures::executor::block_on;
use nimbus_core::{Error, TenantId};
use nimbus_runtime::{HostCallCancellation, InvocationServiceBinding, InvocationServices};
use nimbus_sandbox::SandboxStatus;

use crate::sandbox::SandboxCatalog;
use crate::service_registry::{
    RuntimeServiceBindingFuture, RuntimeServiceRegistry, service_binding_from_handle,
};

use super::SandboxServiceManager;
use super::types::{TenantServiceKey, sandbox_backend_error};

impl RuntimeServiceRegistry for SandboxServiceManager {
    fn snapshot_for_tenant(&self, tenant_id: &TenantId) -> InvocationServices {
        self.sandboxes_for_tenant(tenant_id)
            .into_iter()
            .filter_map(|(service_name, handle)| {
                service_binding_from_handle(&handle).map(|binding| (service_name, binding))
            })
            .collect()
    }

    fn resolve_service_binding(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
    ) -> Result<Option<InvocationServiceBinding>, Error> {
        let key = TenantServiceKey::new(tenant_id, service_name);
        Ok(self
            .current_handle(&key)
            .and_then(|handle| service_binding_from_handle(&handle)))
    }

    fn ensure_service_binding_async<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        service_name: &'a str,
        cancellation: HostCallCancellation,
    ) -> RuntimeServiceBindingFuture<'a> {
        Box::pin(async move {
            if let Some(binding) = self.resolve_service_binding(tenant_id, service_name)? {
                return Ok(Some(binding));
            }
            let Some(handle) = self
                .start_service_async(tenant_id, service_name, cancellation)
                .await?
            else {
                return Ok(None);
            };
            Ok(service_binding_from_handle(&handle))
        })
    }

    fn teardown_tenant(&self, tenant_id: &TenantId) -> Result<(), Error> {
        let tenant_handles = self.tenant_handles(tenant_id);
        for (key, handle) in &tenant_handles {
            block_on(self.sandbox_backend.stop(&handle.id))
                .map_err(|error| sandbox_backend_error(key, "stop", &error))?;
            let mut stopped_handle = handle.clone();
            stopped_handle.status = SandboxStatus::Stopped;
            stopped_handle.published_endpoints.clear();
            block_on(self.record_service_handle(key, &stopped_handle))?;
        }
        block_on(
            self.sandbox_backend
                .remove_tenant_artifacts(tenant_id.clone()),
        )
        .map_err(|error| {
            Error::Internal(format!(
                "failed to remove sandbox artifacts for tenant {tenant_id}: {error}"
            ))
        })?;

        let mut state = self
            .state
            .lock()
            .expect("manager lock should not be poisoned");
        for (key, _) in tenant_handles {
            state.handles.remove(&key);
            state.activations_in_progress.remove(&key);
        }
        self.activation_notify.notify_waiters();
        Ok(())
    }
}
