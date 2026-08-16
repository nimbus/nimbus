//! Compute-owned coordinator for portable workload-saga transitions.

use std::sync::Arc;

use nimbus_workloads::{
    WorkloadProvisionDisposition, WorkloadRestartCandidatePage,
    WorkloadRestartCandidatePageRequest, WorkloadSagaCommit, WorkloadSagaError, WorkloadSagaKey,
    WorkloadSagaPage, WorkloadSagaPageRequest, WorkloadSagaPhase, WorkloadSagaRecord,
    WorkloadSagaStore, WorkloadSagaStoreError, WorkloadSagaTenantPage,
    WorkloadSagaTenantPageRequest, WorkloadTeardownCause,
};

mod desire_admission;
mod ingress;
mod provision_compensation;
mod provision_decision;
mod provision_dispatch;
mod provision_dispatcher;
mod provision_driver;
pub mod provision_provider;
mod provision_sandbox;
pub(crate) use provision_sandbox::sandbox_network_plan_for;
mod recovery;
mod restart_decision;
mod restart_dispatch;
mod restart_dispatcher;
mod restart_driver;
mod restart_provider;
pub mod restart_provider_command;
pub(crate) mod restart_resolution;
pub(super) mod restart_runtime;
pub mod restart_sandbox;
mod restart_submission;
mod restart_supervisor;
mod restart_watch;
mod startup_recovery;
mod teardown_command;
mod teardown_decision;
mod teardown_dispatch;
mod teardown_driver;
mod teardown_node;
pub mod teardown_provider_command;
mod teardown_registry;
mod teardown_runtime;
mod teardown_sandbox;

pub use desire_admission::{
    WorkloadDesireAdmissionError, WorkloadDesireAdmissionFuture, WorkloadDesireAdmissionGuard,
    WorkloadDesireAdmissionPermit, WorkloadDesireAdmissionRequest,
};
pub use ingress::{
    ConfirmedWorkloadSagaIntent, WorkloadSagaIngressDisposition, WorkloadSagaIngressError,
};
pub use nimbus_workloads::{
    WorkloadProvisionCommandId, WorkloadProvisionCommandMode, WorkloadTeardownCommandMode,
};
pub use provision_compensation::WorkloadProvisionCompensationError;
pub(crate) use provision_compensation::WorkloadProvisionCompensator;
#[cfg(any(test, feature = "test-hooks"))]
pub use provision_compensation::compensate_definite_provision_failure_once_for_test;
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
#[cfg(any(test, feature = "test-hooks"))]
pub use restart_runtime::settle_restart_for_teardown_once_for_test;
pub use restart_sandbox::{ValidatedSandboxRestartCommand, validate_sandbox_restart_command};
pub(crate) use restart_submission::{
    ExplicitWorkloadRestartDisposition, ExplicitWorkloadRestartError,
    ExplicitWorkloadRestartRequest,
};
pub(crate) use startup_recovery::WorkloadStartupRecovery;
pub use startup_recovery::{
    WorkloadStartupDisposition, WorkloadStartupRecoveryError, WorkloadStartupRecoveryOutcome,
    WorkloadStartupRecoveryReport,
};
pub use teardown_command::{
    ConfirmedWorkloadTeardownCommand, ConfirmedWorkloadTeardownTransition,
    WorkloadTeardownCommandResult, WorkloadTeardownExecuteOutcome, WorkloadTeardownInspectOutcome,
    WorkloadTeardownProviderOutcome,
};
pub use teardown_dispatch::WorkloadTeardownDispatchError;
pub use teardown_driver::{
    WorkloadTeardownRun, WorkloadTeardownRunDisposition, WorkloadTeardownRunError,
};
pub use teardown_node::NodeExecutionTeardownAdapter;
pub use teardown_registry::{
    ExactWorkloadTeardownCapabilityRealm, FinalIngressWithdrawalCapability,
    IngressTeardownCapabilities, NetworkAttachmentTeardownCapabilities,
    NetworkDetachmentCapability, NetworkReleaseCapability, WorkloadExecutionDrainCapability,
    WorkloadExecutionStopCapability, WorkloadExecutionTeardownCapabilities,
    WorkloadTeardownCapabilityFuture, WorkloadTeardownCapabilityRegistry,
    WorkloadTeardownCapabilityRegistryError, WorkloadTeardownProviderObservation,
};
pub use teardown_runtime::{
    WorkloadTeardownCancellationToken, WorkloadTeardownRuntime, WorkloadTeardownSubmissionError,
};
pub use teardown_sandbox::krun::{KrunAttachmentTeardownAdapter, KrunTeardownAdapter};
pub use teardown_sandbox::{
    ContainerAttachmentTeardownAdapter, ContainerTeardownAdapter, ValidatedSandboxTeardownCommand,
    validate_sandbox_teardown_command,
};
/// Sole cross-domain writer of portable workload-saga transitions.
pub struct WorkloadSagaCoordinator {
    store: Arc<dyn WorkloadSagaStore>,
    desire_admission_guard: Option<Arc<dyn WorkloadDesireAdmissionGuard>>,
}

impl WorkloadSagaCoordinator {
    pub fn new(store: Arc<dyn WorkloadSagaStore>) -> Self {
        Self {
            store,
            desire_admission_guard: None,
        }
    }

    pub fn with_desire_admission_guard(
        store: Arc<dyn WorkloadSagaStore>,
        desire_admission_guard: Arc<dyn WorkloadDesireAdmissionGuard>,
    ) -> Self {
        Self {
            store,
            desire_admission_guard: Some(desire_admission_guard),
        }
    }

    pub async fn load(
        &self,
        key: &WorkloadSagaKey,
    ) -> Result<Option<WorkloadSagaRecord>, WorkloadSagaStoreError> {
        self.store.load(key).await
    }

    /// Promote the exact queued successor after its predecessor reaches
    /// `Recorded` and return the confirmed durable terminal record.
    pub(crate) async fn promote_recorded_successor(
        &self,
        recorded: &WorkloadSagaRecord,
    ) -> Result<WorkloadSagaRecord, WorkloadSagaStoreError> {
        let promoted = recorded.promote_successor()?;
        self.commit_loaded(Some(recorded), promoted.clone()).await?;
        Ok(promoted)
    }

    /// Hand an exact terminal restart result to the durable teardown state.
    pub(crate) async fn commit_restart_settlement_teardown(
        &self,
        settled: &WorkloadSagaRecord,
    ) -> Result<WorkloadSagaRecord, WorkloadSagaStoreError> {
        let withdrawal = settled.commit_restart_settlement_teardown()?;
        self.commit_loaded(Some(settled), withdrawal.clone())
            .await?;
        Ok(withdrawal)
    }

    /// Hand one exact settled prior-process provision result to teardown.
    pub(crate) async fn commit_provision_settlement_teardown(
        &self,
        settled: &WorkloadSagaRecord,
    ) -> Result<WorkloadSagaRecord, WorkloadSagaStoreError> {
        let withdrawal = settled.commit_queued_successor_teardown()?;
        self.commit_loaded(Some(settled), withdrawal.clone())
            .await?;
        Ok(withdrawal)
    }

    /// Persist the inspection boundary that a fresh composition owner needs
    /// before it can recreate process-bound publication effects.
    pub(crate) async fn reopen_observed_publication_for_owner_recovery(
        &self,
        observed: &WorkloadSagaRecord,
    ) -> Result<WorkloadSagaRecord, WorkloadSagaStoreError> {
        let reopened = observed.reopen_observed_publication_for_owner_recovery()?;
        self.commit_loaded(Some(observed), reopened.clone()).await?;
        Ok(reopened)
    }

    /// Commit the exact retained definite provision failure as the sole
    /// durable compensation cause. A lost response or competing coordinator
    /// is adopted only after an exact read authenticates the same lifecycle.
    pub(crate) async fn commit_failed_provision_compensation(
        &self,
        failed: &WorkloadSagaRecord,
    ) -> Result<WorkloadSagaRecord, WorkloadSagaStoreError> {
        let (claim, failure) = match failed.provision_disposition() {
            Some(WorkloadProvisionDisposition::DefiniteFailure { claim, failure }) => {
                (claim.clone(), failure.clone())
            }
            _ => {
                return Err(WorkloadSagaError::InvalidTransition(
                    "failed-provision compensation requires exact durable definite failure",
                )
                .into());
            }
        };
        let cause = WorkloadTeardownCause::FailedProvision {
            claim: Box::new(claim),
            failure,
        };
        let withdrawal = failed.commit_teardown_cause(cause.clone())?;
        match self
            .confirm_transition(Some(failed), withdrawal.clone())
            .await?
        {
            WorkloadSagaConfirmation::AppliedByThisCall
            | WorkloadSagaConfirmation::ConfirmedAfterAmbiguity
            | WorkloadSagaConfirmation::ConfirmedReplay => Ok(withdrawal),
            WorkloadSagaConfirmation::Conflict { .. }
            | WorkloadSagaConfirmation::UnresolvedAmbiguity => {
                let observed = self
                    .load(failed.key())
                    .await?
                    .ok_or(WorkloadSagaStoreError::Corrupt)?;
                authenticate_failed_provision_compensation(failed, &withdrawal, &cause, &observed)?;
                Ok(observed)
            }
        }
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

    pub(crate) async fn list_for_tenant(
        &self,
        tenant_id: &nimbus_core::TenantId,
        request: WorkloadSagaTenantPageRequest,
    ) -> Result<WorkloadSagaTenantPage, WorkloadSagaStoreError> {
        self.store.list_for_tenant(tenant_id, request).await
    }

    pub(super) async fn list_restart_candidates(
        &self,
        request: WorkloadRestartCandidatePageRequest,
    ) -> Result<WorkloadRestartCandidatePage, WorkloadSagaStoreError> {
        self.store.list_restart_candidates(request).await
    }
}

fn authenticate_failed_provision_compensation(
    failed: &WorkloadSagaRecord,
    withdrawal: &WorkloadSagaRecord,
    cause: &WorkloadTeardownCause,
    observed: &WorkloadSagaRecord,
) -> Result<(), WorkloadSagaStoreError> {
    observed.validate()?;
    let same_lifecycle = observed.key() == failed.key()
        && observed.saga_id() == failed.saga_id()
        && observed.active_intent() == failed.active_intent()
        && observed.successor_intent() == failed.successor_intent()
        && observed.revision() >= withdrawal.revision();
    let exact_cause = observed
        .teardown_disposition()
        .is_some_and(|disposition| disposition.cause() == cause);
    let terminal_same_generation =
        observed.phase() == WorkloadSagaPhase::Recorded && observed.successor_intent().is_none();
    if same_lifecycle && (observed == withdrawal || exact_cause || terminal_same_generation) {
        Ok(())
    } else {
        Err(WorkloadSagaError::InvalidEvidence(
            "failed-provision compensation readback is crossed with durable failure",
        )
        .into())
    }
}

#[cfg(test)]
pub(crate) mod test_support;

#[cfg(test)]
mod teardown_test_support;

#[cfg(test)]
#[path = "workload_saga/tests.rs"]
mod tests;
