//! Services-owned logical resolution fencing for durable restart withdrawal.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use nimbus_core::Error;
use nimbus_services::ServiceManager;
use nimbus_workloads::{WorkloadProvisionSourceKind, WorkloadSagaRecord};

use crate::workload_projection::{WorkloadProjectionOrchestrator, WorkloadProjectionState};

pub(crate) type WorkloadRestartResolutionFuture<'a> =
    Pin<Box<dyn Future<Output = Result<(), Error>> + Send + 'a>>;

pub(crate) trait WorkloadRestartResolutionFence: Send + Sync {
    fn withdraw(&self, record: &WorkloadSagaRecord) -> Result<(), Error>;

    fn restore<'a>(&'a self, record: &'a WorkloadSagaRecord)
    -> WorkloadRestartResolutionFuture<'a>;
}

pub(crate) struct ServiceManagerWorkloadRestartResolutionFence {
    manager: Arc<ServiceManager>,
    projector: Arc<WorkloadProjectionOrchestrator>,
}

impl ServiceManagerWorkloadRestartResolutionFence {
    pub(crate) fn new(
        manager: Arc<ServiceManager>,
        projector: Arc<WorkloadProjectionOrchestrator>,
    ) -> Self {
        Self { manager, projector }
    }
}

impl WorkloadRestartResolutionFence for ServiceManagerWorkloadRestartResolutionFence {
    fn withdraw(&self, record: &WorkloadSagaRecord) -> Result<(), Error> {
        let source = record.active_intent().source();
        if source.source_identity().kind() == WorkloadProvisionSourceKind::StandaloneSandbox {
            return Ok(());
        }
        let active = record.restart_state().active().ok_or_else(|| {
            Error::Internal("restart resolution withdrawal requires an active restart".to_owned())
        })?;
        self.manager.claim_service_resolution_withdrawal(
            record.key().tenant_id(),
            source.source_identity().stable_name(),
            source.source_generation().as_u64(),
            source.resource_version().as_str(),
            active.admission().source_attempt_id(),
            active.admission().attempt_id(),
        )
    }

    fn restore<'a>(
        &'a self,
        record: &'a WorkloadSagaRecord,
    ) -> WorkloadRestartResolutionFuture<'a> {
        Box::pin(async move {
            let source = record.active_intent().source();
            if source.source_identity().kind() == WorkloadProvisionSourceKind::StandaloneSandbox {
                return Ok(());
            }
            let completed = record.restart_state().last_completed().ok_or_else(|| {
                Error::Internal(
                    "restart resolution restore requires completed restart evidence".to_owned(),
                )
            })?;
            match self.projector.project_observed_record(record).await {
                WorkloadProjectionState::Projected => {}
                WorkloadProjectionState::Pending(reason) => {
                    return Err(Error::Overloaded {
                        message: format!(
                            "restart target projection remains pending before resolution restore: {reason:?}"
                        ),
                    });
                }
                WorkloadProjectionState::Rejected(reason) => {
                    return Err(Error::PreconditionFailed(format!(
                        "restart target projection was rejected before resolution restore: {reason:?}"
                    )));
                }
            }
            self.manager.release_service_resolution_withdrawal(
                record.key().tenant_id(),
                source.source_identity().stable_name(),
                source.source_generation().as_u64(),
                source.resource_version().as_str(),
                completed.admission().attempt_id(),
            )
        })
    }
}

#[cfg(any(test, feature = "test-hooks"))]
pub(super) struct NoopWorkloadRestartResolutionFence;

#[cfg(any(test, feature = "test-hooks"))]
impl WorkloadRestartResolutionFence for NoopWorkloadRestartResolutionFence {
    fn withdraw(&self, _record: &WorkloadSagaRecord) -> Result<(), Error> {
        Ok(())
    }

    fn restore<'a>(
        &'a self,
        _record: &'a WorkloadSagaRecord,
    ) -> WorkloadRestartResolutionFuture<'a> {
        Box::pin(async { Ok(()) })
    }
}
