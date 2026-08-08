//! Adapter-side use of the provider-local journal for restart commands.
//!
//! The compute saga confirms one command before this adapter can claim
//! provider-local authority. Concrete role adapters supply one effect or one
//! exact inspection. This module persists the complete source/target attempt
//! chain and translates the owner observation back to the portable restart
//! result without owning the effect itself.

use nimbus_sandbox::{
    ProviderCommandAttemptJournal, ProviderCommandClaim, ProviderCommandClaimDecision,
    ProviderCommandClaimInput, ProviderCommandJournalError, ProviderCommandObservation,
    ProviderCommandObservationKind, ProviderCommandOperation,
};
use nimbus_workloads::{
    WorkloadOwnerEvidenceDigest, WorkloadRestartEvidenceDigest, WorkloadRestartStep,
};

use super::restart_provider::{
    WorkloadRestartProviderObservation, WorkloadRestartProviderObservationInput,
};
use super::{ConfirmedWorkloadRestartCommand, WorkloadRestartCommandOutcome};

/// One real provider role observation before portable result translation.
pub enum ProviderRestartEffectObservation {
    Succeeded { evidence: Vec<u8> },
    DefiniteFailure { evidence: Vec<u8> },
    Absent { evidence: Vec<u8> },
    InProgress { evidence: Vec<u8> },
    Ambiguous { evidence: Vec<u8> },
}

/// Shared idempotency composition for small restart role adapters.
pub struct ProviderRestartPhaseAdapter {
    attempt_idempotency_journal: ProviderCommandAttemptJournal,
}

impl ProviderRestartPhaseAdapter {
    pub fn new(attempt_idempotency_journal: ProviderCommandAttemptJournal) -> Self {
        Self {
            attempt_idempotency_journal,
        }
    }

    /// Claim provider-local authority, then invoke at most one effect.
    pub fn execute(
        &self,
        command: &ConfirmedWorkloadRestartCommand,
        effect: impl FnOnce() -> ProviderRestartEffectObservation,
    ) -> WorkloadRestartProviderObservation {
        let claim = match claim_for_command(command) {
            Ok(claim) => claim,
            Err(error) => return journal_error_observation(command, &error),
        };
        match self
            .attempt_idempotency_journal
            .claim_dispatch_epoch(&claim)
        {
            Ok(ProviderCommandClaimDecision::ExecuteClaimed(_)) => {
                self.record_effect(command, &claim, effect())
            }
            Ok(ProviderCommandClaimDecision::AdoptExactAttempt(observation)) => {
                provider_observation(command, observation_outcome(&observation))
            }
            Err(error) => journal_error_observation(command, &error),
        }
    }

    /// Inspect exact durable provider state without granting a second effect.
    pub fn inspect(
        &self,
        command: &ConfirmedWorkloadRestartCommand,
        inspect: impl FnOnce() -> ProviderRestartEffectObservation,
    ) -> WorkloadRestartProviderObservation {
        let claim = match claim_for_command(command) {
            Ok(claim) => claim,
            Err(error) => return journal_error_observation(command, &error),
        };
        match self
            .attempt_idempotency_journal
            .claim_dispatch_epoch(&claim)
        {
            Ok(ProviderCommandClaimDecision::AdoptExactAttempt(observation))
                if matches!(
                    observation.kind(),
                    ProviderCommandObservationKind::Succeeded
                        | ProviderCommandObservationKind::DefiniteFailure
                        | ProviderCommandObservationKind::Absent
                ) =>
            {
                provider_observation(command, observation_outcome(&observation))
            }
            Ok(
                ProviderCommandClaimDecision::ExecuteClaimed(_)
                | ProviderCommandClaimDecision::AdoptExactAttempt(_),
            ) => self.record_effect(command, &claim, inspect()),
            Err(error) => journal_error_observation(command, &error),
        }
    }

    /// Reinspect a process-bound effect after a previously recorded success.
    ///
    /// Only provider-proven current absence may replace that success and
    /// authorize the coordinator's exact next dispatch epoch.
    pub fn inspect_live(
        &self,
        command: &ConfirmedWorkloadRestartCommand,
        inspect: impl FnOnce() -> ProviderRestartEffectObservation,
    ) -> WorkloadRestartProviderObservation {
        let claim = match claim_for_command(command) {
            Ok(claim) => claim,
            Err(error) => return journal_error_observation(command, &error),
        };
        if let Err(error) = self
            .attempt_idempotency_journal
            .claim_dispatch_epoch(&claim)
        {
            return journal_error_observation(command, &error);
        }
        let effect = inspect();
        if let ProviderRestartEffectObservation::Absent { evidence } = &effect {
            return match self
                .attempt_idempotency_journal
                .record_reconciled_absence(&claim, evidence)
            {
                Ok(observation) => provider_observation(command, observation_outcome(&observation)),
                Err(error) => journal_error_observation(command, &error),
            };
        }
        self.record_effect(command, &claim, effect)
    }

    fn record_effect(
        &self,
        command: &ConfirmedWorkloadRestartCommand,
        claim: &ProviderCommandClaim,
        effect: ProviderRestartEffectObservation,
    ) -> WorkloadRestartProviderObservation {
        let (kind, evidence) = match &effect {
            ProviderRestartEffectObservation::Succeeded { evidence } => (
                ProviderCommandObservationKind::Succeeded,
                evidence.as_slice(),
            ),
            ProviderRestartEffectObservation::DefiniteFailure { evidence } => (
                ProviderCommandObservationKind::DefiniteFailure,
                evidence.as_slice(),
            ),
            ProviderRestartEffectObservation::Absent { evidence } => {
                (ProviderCommandObservationKind::Absent, evidence.as_slice())
            }
            ProviderRestartEffectObservation::InProgress { evidence } => (
                ProviderCommandObservationKind::InProgress,
                evidence.as_slice(),
            ),
            ProviderRestartEffectObservation::Ambiguous { evidence } => (
                ProviderCommandObservationKind::Ambiguous,
                evidence.as_slice(),
            ),
        };
        match self
            .attempt_idempotency_journal
            .record_observation(claim, kind, evidence)
        {
            Ok(observation) => provider_observation(command, observation_outcome(&observation)),
            Err(_) => provider_observation(command, WorkloadRestartCommandOutcome::Ambiguous),
        }
    }
}

pub(super) fn claim_for_command(
    command: &ConfirmedWorkloadRestartCommand,
) -> Result<ProviderCommandClaim, ProviderCommandJournalError> {
    let effect_subject = serde_json::to_string(&(
        command.step(),
        command.source_execution(),
        command.execution(),
        command.compiled_network_plan().plan().plan_id(),
    ))
    .map_err(|error| ProviderCommandJournalError::InvalidClaim {
        message: format!("confirmed restart subject cannot be encoded: {error}"),
    })?;
    let provider_realm = serde_json::to_vec(&(
        command.provider_selection(),
        command
            .compiled_network_plan()
            .content()
            .capability_selection(),
    ))
    .map_err(|error| ProviderCommandJournalError::InvalidClaim {
        message: format!("confirmed restart provider realm cannot be encoded: {error}"),
    })?;
    ProviderCommandClaim::new(ProviderCommandClaimInput {
        authority_id: command.saga_id().as_str().to_owned(),
        effect_subject,
        source_attempt_id: Some(command.source_attempt_id().to_string()),
        attempt_id: command.attempt_id().to_string(),
        dispatch_epoch: command.dispatch_epoch().as_u64(),
        workload_generation: command.generation().as_u64(),
        restart_ordinal: command.restart_epoch().as_u64(),
        desired_digest: command.desired_digest().to_string(),
        source_digest: command.source_digest().to_string(),
        network_plan_digest: command.network_plan_digest().to_string(),
        provider_target_digest: WorkloadOwnerEvidenceDigest::sha256(provider_realm).to_string(),
        operation: operation(command.step()),
    })
}

const fn operation(step: WorkloadRestartStep) -> ProviderCommandOperation {
    match step {
        WorkloadRestartStep::WithdrawPublication => ProviderCommandOperation::WithdrawPublication,
        WorkloadRestartStep::QuiesceExecution => ProviderCommandOperation::ResetWorkloadForRestart,
        WorkloadRestartStep::PrepareExecution => ProviderCommandOperation::PrepareRestartAttempt,
        WorkloadRestartStep::AttachNetwork => ProviderCommandOperation::AttachRetainedNetwork,
        WorkloadRestartStep::InspectActivationPrerequisites => {
            ProviderCommandOperation::InspectRestartActivationPrerequisites
        }
        WorkloadRestartStep::ActivateExecution => {
            ProviderCommandOperation::ActivateRestartedWorkload
        }
        WorkloadRestartStep::InspectReadiness => ProviderCommandOperation::InspectRestartReadiness,
        WorkloadRestartStep::Publish => ProviderCommandOperation::PublishRestartIngress,
        WorkloadRestartStep::ObservePublication => {
            ProviderCommandOperation::ObserveRestartPublication
        }
    }
}

fn observation_outcome(observation: &ProviderCommandObservation) -> WorkloadRestartCommandOutcome {
    let evidence = evidence_digest(observation);
    match observation.kind() {
        ProviderCommandObservationKind::Claimed | ProviderCommandObservationKind::InProgress => {
            WorkloadRestartCommandOutcome::InProgress { evidence }
        }
        ProviderCommandObservationKind::Succeeded => {
            WorkloadRestartCommandOutcome::Succeeded { evidence }
        }
        ProviderCommandObservationKind::DefiniteFailure => {
            WorkloadRestartCommandOutcome::DefiniteFailure { evidence }
        }
        ProviderCommandObservationKind::Absent => {
            WorkloadRestartCommandOutcome::AuthenticatedAbsent { evidence }
        }
        ProviderCommandObservationKind::Ambiguous => WorkloadRestartCommandOutcome::Ambiguous,
    }
}

fn evidence_digest(observation: &ProviderCommandObservation) -> WorkloadRestartEvidenceDigest {
    let durable = observation.evidence_sha256().unwrap_or_else(|| {
        debug_assert_eq!(observation.kind(), ProviderCommandObservationKind::Claimed);
        "provider_claimed_without_outcome_evidence"
    });
    WorkloadRestartEvidenceDigest::sha256(
        [
            b"nimbus.compute.provider-restart.observation.v1\0".as_slice(),
            durable.as_bytes(),
        ]
        .concat(),
    )
}

fn journal_error_observation(
    command: &ConfirmedWorkloadRestartCommand,
    error: &ProviderCommandJournalError,
) -> WorkloadRestartProviderObservation {
    let outcome = match error {
        ProviderCommandJournalError::InvalidClaim { .. }
        | ProviderCommandJournalError::StaleWorkloadGeneration { .. }
        | ProviderCommandJournalError::StaleRestartOrdinal { .. }
        | ProviderCommandJournalError::SkippedRestartOrdinal { .. }
        | ProviderCommandJournalError::StaleDispatchEpoch { .. }
        | ProviderCommandJournalError::SkippedDispatchEpoch { .. }
        | ProviderCommandJournalError::CrossedClaim
        | ProviderCommandJournalError::RetryWithoutAbsence
        | ProviderCommandJournalError::PriorEffectUnresolved => {
            WorkloadRestartCommandOutcome::DefiniteFailure {
                evidence: WorkloadRestartEvidenceDigest::sha256(error.to_string()),
            }
        }
        ProviderCommandJournalError::Corrupt { .. } | ProviderCommandJournalError::Store { .. } => {
            WorkloadRestartCommandOutcome::Ambiguous
        }
    };
    provider_observation(command, outcome)
}

fn provider_observation(
    command: &ConfirmedWorkloadRestartCommand,
    outcome: WorkloadRestartCommandOutcome,
) -> WorkloadRestartProviderObservation {
    WorkloadRestartProviderObservation::new(WorkloadRestartProviderObservationInput {
        command_id: command.command_id().clone(),
        transition_id: command.transition_id().clone(),
        generation: command.generation(),
        desired_digest: command.desired_digest(),
        request_id: command.request_id().clone(),
        source_attempt_id: command.source_attempt_id().clone(),
        attempt_id: command.attempt_id().clone(),
        restart_epoch: command.restart_epoch(),
        dispatch_epoch: command.dispatch_epoch(),
        provider_selection: command.provider_selection().clone(),
        outcome,
    })
}
