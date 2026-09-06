//! Server-owned admission of declared workloads during startup.
//!
//! The CLI may resolve and schedule declarations, but the server compute
//! realm owns durable recovery and lifecycle submission. This plan crosses
//! that seam without reconstructing a provider or adding another mutation
//! path.

use std::collections::BTreeSet;

use nimbus_compute::services::{ServiceLifecycleVerb, service_lifecycle};
use nimbus_compute::state::{ComputeError, ComputeState};
use nimbus_core::{Error, TenantId, WorkloadId};
use nimbus_tenant::TenantIsolationContext;

/// Ordered static services that a managed server starts during startup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerWorkloadBootPlan {
    tenant_id: TenantId,
    service_names: Vec<String>,
}

impl ServerWorkloadBootPlan {
    pub fn new(
        tenant_id: TenantId,
        service_names: impl IntoIterator<Item = String>,
    ) -> Result<Self, Error> {
        let service_names = service_names.into_iter().collect::<Vec<_>>();
        let mut unique = BTreeSet::new();
        for service_name in &service_names {
            WorkloadId::new(service_name.clone())?;
            if !unique.insert(service_name.clone()) {
                return Err(Error::InvalidInput(format!(
                    "server workload boot plan repeats service `{service_name}`"
                )));
            }
        }
        Ok(Self {
            tenant_id,
            service_names,
        })
    }

    pub fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub fn service_names(&self) -> &[String] {
        &self.service_names
    }

    pub(crate) async fn apply(&self, compute: &ComputeState) -> Result<(), Error> {
        let context =
            TenantIsolationContext::system(self.tenant_id.clone(), "server-start.compose");
        for service_name in &self.service_names {
            let response =
                service_lifecycle(compute, &context, service_name, ServiceLifecycleVerb::Start)
                    .await
                    .map_err(boot_compute_error)?;
            tracing::info!(
                tenant_id = %self.tenant_id,
                service_name,
                lifecycle_state = response.lifecycle_state,
                readiness = response.readiness,
                "submitted declared service through managed startup lifecycle"
            );
        }
        Ok(())
    }
}

fn boot_compute_error(error: ComputeError) -> Error {
    match error {
        ComputeError::Core(error) => error,
        ComputeError::Unauthorized(message) | ComputeError::Forbidden(message) => {
            Error::PermissionDenied(message)
        }
        ComputeError::NotFound(message) => Error::NotFound(message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boot_plan_preserves_order_and_rejects_duplicate_services() {
        let tenant = TenantId::new("tenant").expect("tenant should validate");
        let plan =
            ServerWorkloadBootPlan::new(tenant.clone(), ["api".to_owned(), "worker".to_owned()])
                .expect("unique services should validate");

        assert_eq!(plan.tenant_id(), &tenant);
        assert_eq!(plan.service_names(), ["api", "worker"]);
        assert!(
            ServerWorkloadBootPlan::new(tenant, ["api".to_owned(), "api".to_owned()],).is_err()
        );
    }
}
