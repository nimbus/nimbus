use nimbus_core::{Error, TenantId};
use nimbus_runtime::{HostCallCancellation, InvocationServiceBinding, InvocationServices};
use nimbus_sandbox::{SandboxHandle, SandboxStatus};
use std::collections::BTreeSet;

use crate::ServiceInstanceCatalog;
use crate::registry::{
    RuntimeServiceBindingFuture, RuntimeServiceRegistry, RuntimeServiceTeardownFuture,
    service_binding_from_handle,
};

use super::ServiceManager;
use super::types::{TenantServiceKey, sandbox_backend_error};

impl RuntimeServiceRegistry for ServiceManager {
    fn snapshot_for_tenant(&self, tenant_id: &TenantId) -> InvocationServices {
        self.service_instances_for_tenant(tenant_id)
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
        let Some(handle) = self.current_handle(&key) else {
            return Ok(None);
        };
        if handle.tenant_id != *tenant_id {
            return Err(Error::PermissionDenied(format!(
                "cached service {service_name} belongs to tenant {}, but runtime lookup requested tenant {tenant_id}",
                handle.tenant_id
            )));
        }
        Ok(service_binding_from_handle(&handle))
    }

    fn ensure_service_binding_async<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        service_name: &'a str,
        cancellation: HostCallCancellation,
    ) -> RuntimeServiceBindingFuture<'a> {
        Box::pin(async move {
            let key = TenantServiceKey::new(tenant_id, service_name);
            if let Some(binding) = self
                .refresh_handle_async(&key)
                .await?
                .and_then(|handle| service_binding_from_handle(&handle))
            {
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

    fn teardown_tenant_async<'a>(
        &'a self,
        tenant_id: &'a TenantId,
    ) -> RuntimeServiceTeardownFuture<'a> {
        Box::pin(async move {
            let tenant_handles = self.tenant_handles(tenant_id);
            let tenant_sandbox_resources = self
                .list_sandbox_resources_for_tenant(tenant_id)
                .into_iter()
                .collect::<Vec<_>>();
            let mut stopped_sandbox_ids = BTreeSet::new();
            let mut failed_sandbox_ids = BTreeSet::new();
            let mut stopped_service_keys = BTreeSet::new();
            let mut stopped_resource_ids = BTreeSet::new();
            let mut errors = Vec::new();
            for (key, handle) in &tenant_handles {
                let sandbox_id = handle.id.as_str().to_owned();
                let stop_succeeded = if stopped_sandbox_ids.contains(&sandbox_id) {
                    true
                } else if failed_sandbox_ids.contains(&sandbox_id) {
                    false
                } else {
                    match self.sandbox_backend.stop(&handle.id).await {
                        Ok(()) => {
                            stopped_sandbox_ids.insert(sandbox_id.clone());
                            true
                        }
                        Err(error) => {
                            failed_sandbox_ids.insert(sandbox_id);
                            errors.push(sandbox_backend_error(key, "stop", &error).to_string());
                            false
                        }
                    }
                };
                if stop_succeeded {
                    let mut stopped_handle = handle.clone();
                    stopped_handle.status = SandboxStatus::Stopped;
                    stopped_handle.published_endpoints.clear();
                    match self.record_service_handle(key, &stopped_handle).await {
                        Ok(()) => {
                            stopped_service_keys.insert(key.clone());
                        }
                        Err(error) => {
                            errors.push(format!(
                                "failed to record stopped handle for service {} in tenant {}: {error}",
                                key.service_name, key.tenant_id
                            ));
                        }
                    }
                }
            }
            for resource in &tenant_sandbox_resources {
                let handle = &resource.handle;
                let sandbox_id = handle.id.as_str().to_owned();
                let stop_succeeded = if stopped_sandbox_ids.contains(&sandbox_id) {
                    true
                } else if failed_sandbox_ids.contains(&sandbox_id) {
                    false
                } else {
                    match self.sandbox_backend.stop(&handle.id).await {
                        Ok(()) => {
                            stopped_sandbox_ids.insert(sandbox_id.clone());
                            true
                        }
                        Err(error) => {
                            failed_sandbox_ids.insert(sandbox_id);
                            errors.push(
                                standalone_sandbox_teardown_error(tenant_id, handle, &error)
                                    .to_string(),
                            );
                            false
                        }
                    }
                };
                if stop_succeeded {
                    stopped_resource_ids.insert(resource.id.clone());
                }
            }
            if let Err(error) = self
                .sandbox_backend
                .remove_tenant_artifacts(tenant_id.clone())
                .await
            {
                errors.push(format!(
                    "failed to remove sandbox artifacts for tenant {tenant_id}: {error}"
                ));
            }

            let mut state = self
                .state
                .lock()
                .expect("manager lock should not be poisoned");
            for key in &stopped_service_keys {
                state.handles.remove(key);
                state.activations_in_progress.remove(key);
            }
            if errors.is_empty() {
                state
                    .definitions
                    .retain(|key, _| &key.tenant_id != tenant_id);
                state
                    .sandbox_resources
                    .retain(|_, resource| &resource.tenant_id != tenant_id);
                state
                    .sessions
                    .retain(|_, session| &session.tenant_id != tenant_id);
            } else {
                state
                    .definitions
                    .retain(|key, _| !stopped_service_keys.contains(key));
                state
                    .sandbox_resources
                    .retain(|_, resource| !stopped_resource_ids.contains(resource.id.as_str()));
                state.sessions.retain(|_, session| match &session.target {
                    crate::SessionTarget::Service { name } => {
                        !stopped_service_keys.contains(&TenantServiceKey::new(tenant_id, name))
                    }
                    crate::SessionTarget::Sandbox { id } => !stopped_resource_ids.contains(id),
                });
            }
            self.activation_notify.notify_waiters();
            if errors.is_empty() {
                Ok(())
            } else {
                Err(teardown_tenant_aggregate_error(tenant_id, errors))
            }
        })
    }
}

fn teardown_tenant_aggregate_error(tenant_id: &TenantId, errors: Vec<String>) -> Error {
    Error::Internal(format!(
        "tenant {tenant_id} teardown failed after best-effort cleanup: {}",
        errors.join("; ")
    ))
}

fn standalone_sandbox_teardown_error(
    tenant_id: &TenantId,
    handle: &SandboxHandle,
    error: &nimbus_sandbox::SandboxError,
) -> Error {
    Error::Internal(format!(
        "failed to stop standalone sandbox {} for tenant {} during tenant teardown: {error}",
        handle.id, tenant_id
    ))
}
