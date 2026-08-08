//! Compute-owned coordinator for portable workload-saga transitions.

use std::sync::Arc;

use nimbus_workloads::{
    WorkloadRestartCandidatePage, WorkloadRestartCandidatePageRequest, WorkloadSagaCommit,
    WorkloadSagaKey, WorkloadSagaPage, WorkloadSagaPageRequest, WorkloadSagaRecord,
    WorkloadSagaStore, WorkloadSagaStoreError,
};

mod ingress;
mod provision_decision;
mod provision_dispatch;
mod provision_dispatcher;
mod provision_driver;
pub mod provision_provider;
mod provision_sandbox;
mod recovery;
mod restart_decision;
mod restart_dispatch;
mod restart_dispatcher;
mod restart_driver;
mod restart_provider;
pub mod restart_provider_command;
pub(super) mod restart_runtime;
pub mod restart_sandbox;
mod restart_submission;
mod restart_supervisor;
mod restart_watch;

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
pub use restart_decision::{
    ConfirmedWorkloadRestartAdmission, ProposedWorkloadRestartTransition,
    WorkloadRestartAdmissionDecision, WorkloadRestartAdmissionDisposition,
    WorkloadRestartAdmissionError, WorkloadRestartAdmissionRequest,
    WorkloadRestartCancellationToken, WorkloadRestartDecision, WorkloadRestartSymbolicAction,
    decide_restart_admission, decide_restart_progress,
};
pub use restart_dispatch::{
    ConfirmedWorkloadRestartCommand, ConfirmedWorkloadRestartTransition,
    WorkloadRestartCommandMode, WorkloadRestartCommandOutcome, WorkloadRestartCommandResult,
    apply_restart_result,
};
pub use restart_provider::{
    NetworkRestartAttachmentCapability, RestartPublicationCapability,
    RestartPublicationObservationCapability, RestartPublicationWithdrawalCapability,
    WorkloadExecutionQuiescenceCapability, WorkloadRestartActivationCapability,
    WorkloadRestartActivationPrerequisiteCapability, WorkloadRestartCapabilities,
    WorkloadRestartCapabilityFuture, WorkloadRestartCapabilityRegistry,
    WorkloadRestartCapabilityRegistryError, WorkloadRestartPreparationCapability,
    WorkloadRestartProviderObservation, WorkloadRestartReadinessCapability,
};
pub use restart_sandbox::{ValidatedSandboxRestartCommand, validate_sandbox_restart_command};
pub(crate) use restart_submission::{
    ExplicitWorkloadRestartDisposition, ExplicitWorkloadRestartError,
    ExplicitWorkloadRestartRequest,
};
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

    pub(super) async fn list_restart_candidates(
        &self,
        request: WorkloadRestartCandidatePageRequest,
    ) -> Result<WorkloadRestartCandidatePage, WorkloadSagaStoreError> {
        self.store.list_restart_candidates(request).await
    }
}

#[cfg(test)]
pub(crate) mod test_support;

#[cfg(test)]
#[path = "workload_saga/tests.rs"]
mod tests;
