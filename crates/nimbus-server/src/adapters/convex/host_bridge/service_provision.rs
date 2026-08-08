use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use nimbus_compute::{
    ComputeResourceProvisionError, ComputeResourceProvisioner, WorkloadProvisionCancellation,
    WorkloadProvisionError,
};
use nimbus_core::Error;
use nimbus_tenant::{TenantIsolationContext, TenantServiceAccessDecision};

pub(in crate::adapters::convex) type ConvexServiceProvisionFuture<'a> =
    Pin<Box<dyn Future<Output = Result<(), Error>> + Send + 'a>>;

/// Narrow Convex host input port for activating one already-authorized
/// sandbox-backed service.
///
/// The effect-free [`nimbus_services::RuntimeServiceRegistry`] deliberately
/// does not implement this capability. Production adapts the sole
/// compute-owned provision facade; tests may substitute a recording port.
pub(in crate::adapters::convex) trait ConvexServiceProvisionPort:
    Send + Sync + 'static
{
    fn provision_sandbox_service<'a>(
        &'a self,
        context: &'a TenantIsolationContext,
        access: &'a TenantServiceAccessDecision,
        cancellation: &'a WorkloadProvisionCancellation,
    ) -> ConvexServiceProvisionFuture<'a>;
}

impl ConvexServiceProvisionPort for ComputeResourceProvisioner {
    fn provision_sandbox_service<'a>(
        &'a self,
        context: &'a TenantIsolationContext,
        access: &'a TenantServiceAccessDecision,
        cancellation: &'a WorkloadProvisionCancellation,
    ) -> ConvexServiceProvisionFuture<'a> {
        Box::pin(async move {
            access.ensure_tenant_matches(
                context.tenant_id(),
                "Convex service provisioning context",
            )?;
            ComputeResourceProvisioner::provision_sandbox_service(
                self,
                context,
                access.service_name(),
                cancellation,
            )
            .await
            .map(|_| ())
            .map_err(map_resource_provision_error)
        })
    }
}

fn map_resource_provision_error(error: ComputeResourceProvisionError) -> Error {
    match error {
        ComputeResourceProvisionError::Source(error) => error,
        ComputeResourceProvisionError::Provision(error)
            if matches!(
                error.as_ref(),
                WorkloadProvisionError::CancelledBeforeSubmission
                    | WorkloadProvisionError::WaiterCancelled
            ) =>
        {
            Error::Cancelled
        }
        other => Error::Internal(other.to_string()),
    }
}

#[derive(Clone)]
pub(in crate::adapters::convex) struct ConvexServiceProvisionScope {
    context: TenantIsolationContext,
    port: Arc<dyn ConvexServiceProvisionPort>,
}

impl ConvexServiceProvisionScope {
    pub(in crate::adapters::convex) fn new(
        context: TenantIsolationContext,
        port: Arc<dyn ConvexServiceProvisionPort>,
    ) -> Self {
        Self { context, port }
    }

    pub(in crate::adapters::convex) fn context(&self) -> &TenantIsolationContext {
        &self.context
    }

    pub(in crate::adapters::convex) fn port(&self) -> &Arc<dyn ConvexServiceProvisionPort> {
        &self.port
    }
}
