use nimbus_core::{Error, TenantId};
use nimbus_runtime::{InvocationServiceBinding, InvocationServices};

use crate::ServiceInstanceCatalog;
use crate::registry::{RuntimeServiceRegistry, service_binding_from_handle};

use super::ServiceManager;

/// Read-only runtime naming projection over services-owned observations.
///
/// This implementation has no provider handle, cancellation token, future,
/// or lifecycle capability. An exact compute projection is visible
/// immediately; missing/pending observations resolve as absent without
/// provider inspection.
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
        let Some(observation) =
            self.service_definition_observation_for_tenant(tenant_id, service_name)
        else {
            return Ok(None);
        };
        if observation.tenant_id != *tenant_id || observation.name != service_name {
            return Err(Error::PermissionDenied(format!(
                "service observation for `{service_name}` is crossed with tenant `{tenant_id}`"
            )));
        }
        Ok(service_binding_from_handle(&observation.handle))
    }
}
