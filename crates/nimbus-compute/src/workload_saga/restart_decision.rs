//! Pure restart decisions and sole-coordinator admission confirmation.
//!
//! A restart request is only fenced input. This module first checks that input
//! against the complete durable record, then lets the existing saga
//! coordinator confirm the proposed transition. Provider effects remain in
//! later command adapters.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use nimbus_workloads::{
    DesiredWorkloadState, WorkloadDesiredDigest, WorkloadExecutionProviderId, WorkloadGeneration,
    WorkloadInspectionVersion, WorkloadProvisionDisposition, WorkloadProvisionSourceGeneration,
    WorkloadRestartAdmissionInput, WorkloadRestartAdmissionUpdate, WorkloadRestartCommandClaim,
    WorkloadRestartDisposition, WorkloadRestartNotBeforeUnixMillis, WorkloadRestartRequestId,
    WorkloadRestartTrigger, WorkloadSagaCommit, WorkloadSagaError, WorkloadSagaId, WorkloadSagaKey,
    WorkloadSagaPhase, WorkloadSagaRecord, WorkloadSagaRevision, WorkloadSagaStoreError,
};
use thiserror::Error;

use super::WorkloadSagaCoordinator;

/// Cancellation observed before durable submission starts.
///
/// Cancellation after a compare-and-swap begins cannot revoke durable work.
#[derive(Debug, Clone, Default)]
pub struct WorkloadRestartCancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl WorkloadRestartCancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

/// Complete stale-read fences and stable identity for one restart request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadRestartAdmissionRequest {
    key: WorkloadSagaKey,
    saga_id: WorkloadSagaId,
    source_revision: WorkloadSagaRevision,
    source_generation: WorkloadProvisionSourceGeneration,
    generation: WorkloadGeneration,
    desired_digest: WorkloadDesiredDigest,
    inspection_version: Option<WorkloadInspectionVersion>,
    provider_selection: WorkloadExecutionProviderId,
    trigger: WorkloadRestartTrigger,
    request_id: WorkloadRestartRequestId,
    not_before_unix_millis: WorkloadRestartNotBeforeUnixMillis,
}

impl WorkloadRestartAdmissionRequest {
    pub fn for_explicit(
        record: &WorkloadSagaRecord,
        idempotency_key: &str,
        not_before_unix_millis: WorkloadRestartNotBeforeUnixMillis,
    ) -> Result<Self, WorkloadSagaError> {
        let request_id = WorkloadRestartRequestId::for_explicit(
            record.saga_id(),
            record.active_intent().source().source_generation(),
            idempotency_key,
        )?;
        Ok(Self::new(
            record,
            WorkloadRestartTrigger::Explicit,
            None,
            request_id,
            not_before_unix_millis,
        ))
    }

    pub fn for_automatic(
        record: &WorkloadSagaRecord,
        exit_code: i32,
        inspection_version: WorkloadInspectionVersion,
        not_before_unix_millis: WorkloadRestartNotBeforeUnixMillis,
    ) -> Self {
        let request_id =
            WorkloadRestartRequestId::for_automatic(record.saga_id(), inspection_version);
        Self::new(
            record,
            WorkloadRestartTrigger::Automatic { exit_code },
            Some(inspection_version),
            request_id,
            not_before_unix_millis,
        )
    }

    fn new(
        record: &WorkloadSagaRecord,
        trigger: WorkloadRestartTrigger,
        inspection_version: Option<WorkloadInspectionVersion>,
        request_id: WorkloadRestartRequestId,
        not_before_unix_millis: WorkloadRestartNotBeforeUnixMillis,
    ) -> Self {
        Self {
            key: record.key().clone(),
            saga_id: record.saga_id().clone(),
            source_revision: record.revision(),
            source_generation: record.active_intent().source().source_generation(),
            generation: record.active_intent().generation(),
            desired_digest: record.active_intent().desired_digest(),
            inspection_version,
            provider_selection: record
                .active_intent()
                .source()
                .execution_provider_id()
                .clone(),
            trigger,
            request_id,
            not_before_unix_millis,
        }
    }

    pub fn key(&self) -> &WorkloadSagaKey {
        &self.key
    }

    pub fn request_id(&self) -> &WorkloadRestartRequestId {
        &self.request_id
    }

    pub const fn source_revision(&self) -> WorkloadSagaRevision {
        self.source_revision
    }
}

/// Result of the pure admission reducer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkloadRestartAdmissionDecision {
    Transition(Box<WorkloadSagaRecord>),
    Unchanged,
}

/// Symbolic work that becomes actionable only after the candidate is durable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkloadRestartSymbolicAction {
    StartExactAttempt,
    InspectExactAttempt,
}

/// One pure restart-state candidate and its post-confirmation operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposedWorkloadRestartTransition {
    candidate: Box<WorkloadSagaRecord>,
    action_after_confirmation: Option<WorkloadRestartSymbolicAction>,
}

impl ProposedWorkloadRestartTransition {
    fn new(
        candidate: WorkloadSagaRecord,
        action_after_confirmation: Option<WorkloadRestartSymbolicAction>,
    ) -> Self {
        Self {
            candidate: Box::new(candidate),
            action_after_confirmation,
        }
    }

    pub fn candidate(&self) -> &WorkloadSagaRecord {
        &self.candidate
    }

    pub const fn action_after_confirmation(&self) -> Option<WorkloadRestartSymbolicAction> {
        self.action_after_confirmation
    }

    pub fn into_candidate(self) -> WorkloadSagaRecord {
        *self.candidate
    }
}

/// Exhaustive pure decision for one admitted restart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkloadRestartDecision {
    Proposed(ProposedWorkloadRestartTransition),
    InspectExact(Box<WorkloadRestartCommandClaim>),
    WaitUntil(WorkloadRestartNotBeforeUnixMillis),
    DefiniteFailure,
    Wait,
}

fn require_exact_revision(
    actual: WorkloadSagaRevision,
    expected: WorkloadSagaRevision,
) -> Result<(), WorkloadSagaError> {
    if actual == expected {
        Ok(())
    } else {
        Err(WorkloadSagaError::InvalidTransition(
            "restart admission revision is stale or crossed",
        ))
    }
}

fn require_exact_generation(
    actual: WorkloadGeneration,
    expected: WorkloadGeneration,
) -> Result<(), WorkloadSagaError> {
    if actual == expected {
        Ok(())
    } else {
        Err(WorkloadSagaError::InvalidTransition(
            "restart admission generation is stale or crossed",
        ))
    }
}

fn require_exact_desired_digest(
    actual: WorkloadDesiredDigest,
    expected: WorkloadDesiredDigest,
) -> Result<(), WorkloadSagaError> {
    if actual == expected {
        Ok(())
    } else {
        Err(WorkloadSagaError::InvalidEvidence(
            "restart admission desired digest is crossed",
        ))
    }
}

fn require_exact_inspection_version(
    trigger: WorkloadRestartTrigger,
    inspection_version: Option<WorkloadInspectionVersion>,
) -> Result<(), WorkloadSagaError> {
    match (trigger, inspection_version) {
        (WorkloadRestartTrigger::Automatic { .. }, Some(_))
        | (WorkloadRestartTrigger::Explicit, None) => Ok(()),
        (WorkloadRestartTrigger::Automatic { .. }, None) => {
            Err(WorkloadSagaError::InvalidEvidence(
                "automatic restart requires an exact inspection version",
            ))
        }
        (WorkloadRestartTrigger::Explicit, Some(_)) => Err(WorkloadSagaError::InvalidEvidence(
            "explicit restart cannot borrow provider inspection evidence",
        )),
    }
}

fn require_exact_provider_selection(
    actual: &WorkloadExecutionProviderId,
    expected: &WorkloadExecutionProviderId,
) -> Result<(), WorkloadSagaError> {
    if actual == expected {
        Ok(())
    } else {
        Err(WorkloadSagaError::InvalidEvidence(
            "restart admission provider selection is crossed",
        ))
    }
}

fn reject_withdrawal_or_successor(record: &WorkloadSagaRecord) -> Result<(), WorkloadSagaError> {
    if record.phase() == WorkloadSagaPhase::Observed
        && record.active_intent().desired_state() == DesiredWorkloadState::Running
        && record.successor_intent().is_none()
        && record.failure().is_none()
        && record.provision_disposition() == Some(&WorkloadProvisionDisposition::Ready)
    {
        Ok(())
    } else {
        Err(WorkloadSagaError::InvalidTransition(
            "restart admission lost to withdrawal, successor, failure, or unresolved provision",
        ))
    }
}

fn admit_automatic_restart(
    record: &WorkloadSagaRecord,
    request: &WorkloadRestartAdmissionRequest,
) -> Result<WorkloadRestartAdmissionUpdate, WorkloadSagaError> {
    record.admit_restart(WorkloadRestartAdmissionInput {
        expected_revision: request.source_revision,
        trigger: request.trigger,
        inspection_version: request.inspection_version,
        request_id: request.request_id.clone(),
        not_before_unix_millis: request.not_before_unix_millis,
    })
}

fn admit_explicit_restart(
    record: &WorkloadSagaRecord,
    request: &WorkloadRestartAdmissionRequest,
) -> Result<WorkloadRestartAdmissionUpdate, WorkloadSagaError> {
    record.admit_restart(WorkloadRestartAdmissionInput {
        expected_revision: request.source_revision,
        trigger: request.trigger,
        inspection_version: request.inspection_version,
        request_id: request.request_id.clone(),
        not_before_unix_millis: request.not_before_unix_millis,
    })
}

/// Normalize automatic and explicit requests through one exact admission path.
pub fn decide_restart_admission(
    record: &WorkloadSagaRecord,
    request: &WorkloadRestartAdmissionRequest,
) -> Result<WorkloadRestartAdmissionDecision, WorkloadSagaError> {
    record.validate()?;
    if record.key() != &request.key || record.saga_id() != &request.saga_id {
        return Err(WorkloadSagaError::InvalidIdentity(
            "restart admission names another workload saga",
        ));
    }
    if record.restart_state().active().is_none() {
        require_exact_revision(record.revision(), request.source_revision())?;
    }
    if record.active_intent().source().source_generation() != request.source_generation {
        return Err(WorkloadSagaError::InvalidTransition(
            "restart admission source generation is stale or crossed",
        ));
    }
    require_exact_generation(record.active_intent().generation(), request.generation)?;
    require_exact_desired_digest(
        record.active_intent().desired_digest(),
        request.desired_digest,
    )?;
    require_exact_inspection_version(request.trigger, request.inspection_version)?;
    require_exact_provider_selection(
        record.active_intent().source().execution_provider_id(),
        &request.provider_selection,
    )?;
    reject_withdrawal_or_successor(record)?;

    let update = match request.trigger {
        WorkloadRestartTrigger::Automatic { .. } => admit_automatic_restart(record, request)?,
        WorkloadRestartTrigger::Explicit => admit_explicit_restart(record, request)?,
    };
    Ok(match update {
        WorkloadRestartAdmissionUpdate::Transition(candidate) => {
            WorkloadRestartAdmissionDecision::Transition(candidate)
        }
        WorkloadRestartAdmissionUpdate::Unchanged => WorkloadRestartAdmissionDecision::Unchanged,
    })
}

/// Plan the next restart-state edge without a store read or provider effect.
pub fn decide_restart_progress(
    record: &WorkloadSagaRecord,
    now_unix_millis: WorkloadRestartNotBeforeUnixMillis,
) -> Result<WorkloadRestartDecision, WorkloadSagaError> {
    record.validate()?;
    let Some(active) = record.restart_state().active() else {
        return Ok(WorkloadRestartDecision::Wait);
    };
    match active.disposition() {
        WorkloadRestartDisposition::DispatchPending { claim }
        | WorkloadRestartDisposition::InspectionRequired { claim } => {
            return Ok(WorkloadRestartDecision::InspectExact(Box::new(
                claim.clone(),
            )));
        }
        WorkloadRestartDisposition::DefiniteFailure { .. } => {
            return Ok(WorkloadRestartDecision::DefiniteFailure);
        }
        WorkloadRestartDisposition::Ready { .. } => {}
    }

    let request_id = active.admission().request_id();
    if active.phase() == nimbus_workloads::WorkloadRestartPhase::Scheduled {
        let deadline = active.admission().not_before_unix_millis();
        if now_unix_millis < deadline {
            return Ok(WorkloadRestartDecision::WaitUntil(deadline));
        }
        return record
            .advance_scheduled_restart(request_id, now_unix_millis)
            .map(|candidate| {
                WorkloadRestartDecision::Proposed(ProposedWorkloadRestartTransition::new(
                    candidate, None,
                ))
            });
    }

    match active.phase() {
        nimbus_workloads::WorkloadRestartPhase::Requested => record
            .advance_restart_without_effect(request_id)
            .map(|candidate| {
                WorkloadRestartDecision::Proposed(ProposedWorkloadRestartTransition::new(
                    candidate, None,
                ))
            }),
        nimbus_workloads::WorkloadRestartPhase::PublicationPending
            if record.active_intent().publication()
                == nimbus_workloads::WorkloadPublicationIntent::Withheld =>
        {
            record
                .advance_restart_without_effect(request_id)
                .map(|candidate| {
                    WorkloadRestartDecision::Proposed(ProposedWorkloadRestartTransition::new(
                        candidate, None,
                    ))
                })
        }
        nimbus_workloads::WorkloadRestartPhase::Idle
        | nimbus_workloads::WorkloadRestartPhase::Scheduled => {
            Err(WorkloadSagaError::InvalidTransition(
                "active restart has an impossible idle or unhandled scheduled phase",
            ))
        }
        nimbus_workloads::WorkloadRestartPhase::PublicationWithdrawalPending
        | nimbus_workloads::WorkloadRestartPhase::ExecutionQuiescencePending
        | nimbus_workloads::WorkloadRestartPhase::PreparationPending
        | nimbus_workloads::WorkloadRestartPhase::AttachmentPending
        | nimbus_workloads::WorkloadRestartPhase::ActivationPrerequisitePending
        | nimbus_workloads::WorkloadRestartPhase::ActivationPending
        | nimbus_workloads::WorkloadRestartPhase::ReadinessPending
        | nimbus_workloads::WorkloadRestartPhase::PublicationPending
        | nimbus_workloads::WorkloadRestartPhase::ObservationPending => {
            record.claim_restart_command(request_id).map(|candidate| {
                WorkloadRestartDecision::Proposed(ProposedWorkloadRestartTransition::new(
                    candidate,
                    Some(WorkloadRestartSymbolicAction::StartExactAttempt),
                ))
            })
        }
    }
}

/// How the sole coordinator confirmed a restart admission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkloadRestartAdmissionDisposition {
    Applied,
    ConfirmedReplay,
}

/// Exact durable record returned from one admission submission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmedWorkloadRestartAdmission {
    record: WorkloadSagaRecord,
    disposition: WorkloadRestartAdmissionDisposition,
}

impl ConfirmedWorkloadRestartAdmission {
    pub fn record(&self) -> &WorkloadSagaRecord {
        &self.record
    }

    pub const fn disposition(&self) -> WorkloadRestartAdmissionDisposition {
        self.disposition
    }
}

#[derive(Debug, Error)]
pub enum WorkloadRestartAdmissionError {
    #[error("workload restart admission was cancelled before durable submission")]
    Cancelled,
    #[error("workload restart admission failed: {0}")]
    Saga(#[from] WorkloadSagaStoreError),
}

impl WorkloadSagaCoordinator {
    /// Load, decide, and confirm one normalized restart admission.
    pub async fn compare_and_swap_restart_admission(
        &self,
        request: &WorkloadRestartAdmissionRequest,
        cancellation: &WorkloadRestartCancellationToken,
    ) -> Result<ConfirmedWorkloadRestartAdmission, WorkloadRestartAdmissionError> {
        if cancellation.is_cancelled() {
            return Err(WorkloadRestartAdmissionError::Cancelled);
        }
        let current = self.load(request.key()).await?.ok_or({
            WorkloadSagaStoreError::InvalidTransition(WorkloadSagaError::InvalidTransition(
                "restart admission requires an existing workload saga",
            ))
        })?;
        if current.key() != request.key() {
            return Err(WorkloadSagaStoreError::Corrupt.into());
        }
        let candidate = match decide_restart_admission(&current, request)
            .map_err(WorkloadSagaStoreError::InvalidTransition)?
        {
            WorkloadRestartAdmissionDecision::Unchanged => {
                return Ok(ConfirmedWorkloadRestartAdmission {
                    record: current,
                    disposition: WorkloadRestartAdmissionDisposition::ConfirmedReplay,
                });
            }
            WorkloadRestartAdmissionDecision::Transition(candidate) => *candidate,
        };
        if cancellation.is_cancelled() {
            return Err(WorkloadRestartAdmissionError::Cancelled);
        }

        match self.commit_loaded(Some(&current), candidate.clone()).await {
            Ok(WorkloadSagaCommit::Applied) => Ok(ConfirmedWorkloadRestartAdmission {
                record: candidate,
                disposition: WorkloadRestartAdmissionDisposition::Applied,
            }),
            Ok(WorkloadSagaCommit::Unchanged) => Ok(ConfirmedWorkloadRestartAdmission {
                record: candidate,
                disposition: WorkloadRestartAdmissionDisposition::ConfirmedReplay,
            }),
            Err(conflict @ WorkloadSagaStoreError::Conflict { .. }) => {
                let observed = self.load(request.key()).await?.ok_or(conflict.clone())?;
                match decide_restart_admission(&observed, request) {
                    Ok(WorkloadRestartAdmissionDecision::Unchanged) => {
                        Ok(ConfirmedWorkloadRestartAdmission {
                            record: observed,
                            disposition: WorkloadRestartAdmissionDisposition::ConfirmedReplay,
                        })
                    }
                    _ => Err(conflict.into()),
                }
            }
            Err(error) => Err(error.into()),
        }
    }
}

#[cfg(test)]
#[path = "restart_decision/tests.rs"]
mod tests;
