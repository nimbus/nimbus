//! Compute-owned coordinator for portable workload-saga transitions.

use std::sync::Arc;

use nimbus_workloads::{
    WorkloadSagaCommit, WorkloadSagaKey, WorkloadSagaPage, WorkloadSagaPageRequest,
    WorkloadSagaRecord, WorkloadSagaStore, WorkloadSagaStoreError,
};

mod ingress;
mod provision_decision;
mod provision_dispatch;
mod provision_dispatcher;
mod provision_driver;
pub mod provision_provider;
mod provision_sandbox;
mod recovery;

pub use ingress::{ConfirmedWorkloadSagaIntent, WorkloadSagaIngressDisposition};
pub use nimbus_workloads::{WorkloadProvisionCommandId, WorkloadProvisionCommandMode};
pub use provision_decision::{
    ProposedWorkloadProvisionTransition, WorkloadProvisionDecision, WorkloadProvisionSymbolicAction,
};
pub use provision_dispatch::{
    ConfirmedWorkloadProvisionCommand, ConfirmedWorkloadProvisionTransition,
    WorkloadProvisionCommandResult, WorkloadSagaConfirmation, reduce_command_result,
};
pub use provision_dispatcher::{
    IngressProvisionCapabilities, IngressPublicationCapability,
    IngressPublicationInspectionCapability, NetworkAttachmentCapability,
    NetworkAttachmentProvisionCapabilities, NetworkReservationCapability,
    WorkloadActivationCapability, WorkloadActivationPrerequisiteCapability,
    WorkloadExecutionProvisionCapabilities, WorkloadPreparationCapability,
    WorkloadProjectionCapabilityError, WorkloadProvisionCapabilityFuture,
    WorkloadProvisionCapabilityRegistry, WorkloadProvisionCapabilityRegistryError,
    WorkloadProvisionDispatchError, WorkloadProvisionDispatcher, WorkloadProvisionSourceAuthority,
    WorkloadProvisionSourceAuthorityError, WorkloadProvisionSourceFuture,
    WorkloadReadinessCapability,
};
pub use provision_driver::{
    WorkloadProvisionDriver, WorkloadProvisionRun, WorkloadProvisionRunDisposition,
    WorkloadProvisionRunError,
};
pub use provision_sandbox::{
    ContainerProvisionAdapter, KrunProvisionAdapter, ValidatedSandboxProvisionCommand,
    sandbox_execution_provider_id, validate_sandbox_provision_command,
};
pub use recovery::{WorkloadSagaAction, WorkloadSagaDecision, WorkloadSagaDecisionPage};

/// Sole cross-domain writer of portable workload-saga transitions.
pub struct WorkloadSagaCoordinator {
    store: Arc<dyn WorkloadSagaStore>,
}

impl WorkloadSagaCoordinator {
    pub fn new(store: Arc<dyn WorkloadSagaStore>) -> Self {
        Self { store }
    }

    pub async fn load(
        &self,
        key: &WorkloadSagaKey,
    ) -> Result<Option<WorkloadSagaRecord>, WorkloadSagaStoreError> {
        self.store.load(key).await
    }

    async fn commit_loaded(
        &self,
        loaded: Option<&WorkloadSagaRecord>,
        next: WorkloadSagaRecord,
    ) -> Result<WorkloadSagaCommit, WorkloadSagaStoreError> {
        match self.confirm_transition(loaded, next).await? {
            WorkloadSagaConfirmation::AppliedByThisCall
            | WorkloadSagaConfirmation::ConfirmedAfterAmbiguity => Ok(WorkloadSagaCommit::Applied),
            WorkloadSagaConfirmation::ConfirmedReplay => Ok(WorkloadSagaCommit::Unchanged),
            WorkloadSagaConfirmation::Conflict { expected, observed } => {
                Err(WorkloadSagaStoreError::Conflict { expected, observed })
            }
            WorkloadSagaConfirmation::UnresolvedAmbiguity => Err(WorkloadSagaStoreError::Ambiguous),
        }
    }

    pub async fn list_recoverable(
        &self,
        request: WorkloadSagaPageRequest,
    ) -> Result<WorkloadSagaPage, WorkloadSagaStoreError> {
        self.store.list_recoverable(request).await
    }
}

#[cfg(test)]
pub(crate) mod test_support;

#[cfg(test)]
#[path = "workload_saga/tests.rs"]
mod tests;
