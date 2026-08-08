//! Adapter-side use of the provider-local attempt journal.
//!
//! Concrete provider adapters supply only one phase effect or inspection. This
//! helper authenticates the complete compute command, claims provider-local
//! authority, adopts exact replay, and translates durable observations back to
//! the portable saga vocabulary.

use nimbus_sandbox::{
    ProviderProvisionAttemptJournal, ProviderProvisionClaim, ProviderProvisionClaimDecision,
    ProviderProvisionClaimInput, ProviderProvisionJournalError, ProviderProvisionObservation,
    ProviderProvisionObservationKind, ProviderProvisionOperation,
};
use nimbus_workloads::{
    WorkloadFailureEvidence, WorkloadOwnerEvidenceDigest, WorkloadProvisionInspectionResult,
    WorkloadProvisionStep, WorkloadProvisionSubjects, WorkloadProvisionSuccessEvidence,
};

use super::ConfirmedWorkloadProvisionCommand;

/// One real provider phase outcome before translation to portable saga evidence.
pub enum ProviderProvisionEffectObservation {
    Succeeded { evidence: Vec<u8> },
    DefiniteFailure { code: String, evidence: Vec<u8> },
    Absent { evidence: Vec<u8> },
    InProgress { evidence: Vec<u8> },
    Ambiguous { evidence: Vec<u8> },
}

/// Shared idempotency composition used by small concrete provider adapters.
pub struct ProviderProvisionPhaseAdapter {
    attempt_idempotency_journal: ProviderProvisionAttemptJournal,
}

impl ProviderProvisionPhaseAdapter {
    pub fn new(attempt_idempotency_journal: ProviderProvisionAttemptJournal) -> Self {
        Self {
            attempt_idempotency_journal,
        }
    }

    /// Claim provider-local effect authority, then invoke at most one phase effect.
    pub fn execute(
        &self,
        command: &ConfirmedWorkloadProvisionCommand,
        effect: impl FnOnce() -> ProviderProvisionEffectObservation,
    ) -> WorkloadProvisionInspectionResult {
        let claim = match claim_for_command(command) {
            Ok(claim) => claim,
            Err(error) => return journal_error_result(command, &error),
        };
        match self
            .attempt_idempotency_journal
            .claim_dispatch_epoch(&claim)
        {
            Ok(ProviderProvisionClaimDecision::ExecuteClaimed(_)) => {
                self.record_effect(command, &claim, effect())
            }
            Ok(ProviderProvisionClaimDecision::AdoptExactAttempt(observation)) => {
                observation_result(command, &observation)
            }
            Err(error) => journal_error_result(command, &error),
        }
    }

    /// Inspect exact provider state without granting effect authority.
    pub fn inspect(
        &self,
        command: &ConfirmedWorkloadProvisionCommand,
        inspect: impl FnOnce() -> ProviderProvisionEffectObservation,
    ) -> WorkloadProvisionInspectionResult {
        let claim = match claim_for_command(command) {
            Ok(claim) => claim,
            Err(error) => return journal_error_result(command, &error),
        };
        match self
            .attempt_idempotency_journal
            .claim_dispatch_epoch(&claim)
        {
            Ok(ProviderProvisionClaimDecision::AdoptExactAttempt(observation))
                if matches!(
                    observation.kind(),
                    ProviderProvisionObservationKind::Succeeded
                        | ProviderProvisionObservationKind::DefiniteFailure
                        | ProviderProvisionObservationKind::Absent
                ) =>
            {
                observation_result(command, &observation)
            }
            Ok(
                ProviderProvisionClaimDecision::ExecuteClaimed(_)
                | ProviderProvisionClaimDecision::AdoptExactAttempt(_),
            ) => self.record_effect(command, &claim, inspect()),
            Err(error) => journal_error_result(command, &error),
        }
    }

    /// Inspect a process-bound publish effect even after its journal recorded success.
    ///
    /// A provider-proven absence replaces that success at the same epoch so
    /// compute can authorize one exact next-epoch retry. Durable providers use
    /// [`Self::inspect`] and retain terminal replay without a live recheck.
    pub fn inspect_live(
        &self,
        command: &ConfirmedWorkloadProvisionCommand,
        inspect: impl FnOnce() -> ProviderProvisionEffectObservation,
    ) -> WorkloadProvisionInspectionResult {
        let claim = match claim_for_command(command) {
            Ok(claim) => claim,
            Err(error) => return journal_error_result(command, &error),
        };
        if let Err(error) = self
            .attempt_idempotency_journal
            .claim_dispatch_epoch(&claim)
        {
            return journal_error_result(command, &error);
        }
        let effect = inspect();
        if let ProviderProvisionEffectObservation::Absent { evidence } = &effect {
            return match self
                .attempt_idempotency_journal
                .record_reconciled_absence(&claim, evidence)
            {
                Ok(observation) => observation_result(command, &observation),
                Err(error) => journal_error_result(command, &error),
            };
        }
        self.record_effect(command, &claim, effect)
    }

    fn record_effect(
        &self,
        command: &ConfirmedWorkloadProvisionCommand,
        claim: &ProviderProvisionClaim,
        effect: ProviderProvisionEffectObservation,
    ) -> WorkloadProvisionInspectionResult {
        let (kind, evidence) = match &effect {
            ProviderProvisionEffectObservation::Succeeded { evidence } => (
                ProviderProvisionObservationKind::Succeeded,
                evidence.as_slice(),
            ),
            ProviderProvisionEffectObservation::DefiniteFailure { evidence, .. } => (
                ProviderProvisionObservationKind::DefiniteFailure,
                evidence.as_slice(),
            ),
            ProviderProvisionEffectObservation::Absent { evidence } => (
                ProviderProvisionObservationKind::Absent,
                evidence.as_slice(),
            ),
            ProviderProvisionEffectObservation::InProgress { evidence } => (
                ProviderProvisionObservationKind::InProgress,
                evidence.as_slice(),
            ),
            ProviderProvisionEffectObservation::Ambiguous { evidence } => (
                ProviderProvisionObservationKind::Ambiguous,
                evidence.as_slice(),
            ),
        };
        let observation = match self
            .attempt_idempotency_journal
            .record_observation(claim, kind, evidence)
        {
            Ok(observation) => observation,
            Err(_) => return ambiguous_result(command),
        };
        observation_result(command, &observation)
    }
}

fn claim_for_command(
    command: &ConfirmedWorkloadProvisionCommand,
) -> Result<ProviderProvisionClaim, ProviderProvisionJournalError> {
    let effect_subject = serde_json::to_string(command.subjects()).map_err(|error| {
        ProviderProvisionJournalError::InvalidClaim {
            message: format!("confirmed provider subject cannot be encoded: {error}"),
        }
    })?;
    let target = serde_json::to_vec(command.provider_target()).map_err(|error| {
        ProviderProvisionJournalError::InvalidClaim {
            message: format!("confirmed provider target cannot be encoded: {error}"),
        }
    })?;
    ProviderProvisionClaim::new(ProviderProvisionClaimInput {
        authority_id: command.saga_id().as_str().to_owned(),
        effect_subject,
        attempt_id: command.attempt_id().as_str().to_owned(),
        dispatch_epoch: command.dispatch_epoch().as_u64(),
        generation: command.generation().as_u64(),
        desired_digest: command.desired_digest().to_string(),
        source_digest: command.source_digest().to_string(),
        network_plan_digest: command.network_plan_digest().to_string(),
        provider_target_digest: WorkloadOwnerEvidenceDigest::sha256(target).to_string(),
        operation: operation(command.step()),
    })
}

const fn operation(step: WorkloadProvisionStep) -> ProviderProvisionOperation {
    match step {
        WorkloadProvisionStep::ReserveNetwork => ProviderProvisionOperation::ReserveNetwork,
        WorkloadProvisionStep::PrepareWorkload => ProviderProvisionOperation::PrepareWorkload,
        WorkloadProvisionStep::AttachNetwork => ProviderProvisionOperation::AttachNetwork,
        WorkloadProvisionStep::InspectActivationPrerequisites => {
            ProviderProvisionOperation::InspectActivationPrerequisites
        }
        WorkloadProvisionStep::ActivateWorkload => ProviderProvisionOperation::ActivateWorkload,
        WorkloadProvisionStep::InspectWorkloadReadiness => {
            ProviderProvisionOperation::InspectWorkloadReadiness
        }
        WorkloadProvisionStep::Publish => ProviderProvisionOperation::PublishIngress,
        WorkloadProvisionStep::ObservePublication => ProviderProvisionOperation::ObserveIngress,
    }
}

fn observation_result(
    command: &ConfirmedWorkloadProvisionCommand,
    observation: &ProviderProvisionObservation,
) -> WorkloadProvisionInspectionResult {
    let evidence = evidence_digest(observation);
    match observation.kind() {
        ProviderProvisionObservationKind::Claimed
        | ProviderProvisionObservationKind::InProgress => {
            WorkloadProvisionInspectionResult::InProgress {
                attempt_id: command.attempt_id().clone(),
                dispatch_epoch: command.dispatch_epoch(),
                provider_target: command.provider_target().clone(),
                evidence,
            }
        }
        ProviderProvisionObservationKind::Succeeded => {
            WorkloadProvisionInspectionResult::Succeeded {
                attempt_id: command.attempt_id().clone(),
                dispatch_epoch: command.dispatch_epoch(),
                provider_target: command.provider_target().clone(),
                evidence: success_evidence(command, evidence),
            }
        }
        ProviderProvisionObservationKind::DefiniteFailure => {
            definite_failure_result(command, "provider_definite_failure", evidence)
        }
        ProviderProvisionObservationKind::Absent => WorkloadProvisionInspectionResult::Absent {
            evidence: command.absence_evidence(evidence),
        },
        ProviderProvisionObservationKind::Ambiguous => ambiguous_result(command),
    }
}

fn success_evidence(
    command: &ConfirmedWorkloadProvisionCommand,
    evidence: WorkloadOwnerEvidenceDigest,
) -> WorkloadProvisionSuccessEvidence {
    match (command.step(), command.subjects()) {
        (WorkloadProvisionStep::ReserveNetwork, WorkloadProvisionSubjects::Network(reference)) => {
            WorkloadProvisionSuccessEvidence::NetworkReserved {
                reference: reference.clone(),
                evidence,
            }
        }
        (
            WorkloadProvisionStep::PrepareWorkload,
            WorkloadProvisionSubjects::Execution(reference),
        ) => WorkloadProvisionSuccessEvidence::WorkloadPrepared {
            reference: reference.clone(),
            evidence,
        },
        (WorkloadProvisionStep::AttachNetwork, WorkloadProvisionSubjects::Network(reference)) => {
            WorkloadProvisionSuccessEvidence::NetworkAttached {
                reference: reference.clone(),
                evidence,
            }
        }
        (
            WorkloadProvisionStep::InspectActivationPrerequisites,
            WorkloadProvisionSubjects::Readiness { network, execution },
        ) => WorkloadProvisionSuccessEvidence::ActivationPrerequisitesReady {
            network: network.clone(),
            execution: execution.clone(),
            evidence,
        },
        (
            WorkloadProvisionStep::ActivateWorkload,
            WorkloadProvisionSubjects::Execution(reference),
        ) => WorkloadProvisionSuccessEvidence::WorkloadActivated {
            reference: reference.clone(),
            evidence,
        },
        (
            WorkloadProvisionStep::InspectWorkloadReadiness,
            WorkloadProvisionSubjects::Readiness { network, execution },
        ) => WorkloadProvisionSuccessEvidence::WorkloadReady {
            network: network.clone(),
            execution: execution.clone(),
            evidence,
        },
        (WorkloadProvisionStep::Publish, WorkloadProvisionSubjects::Publication(reference)) => {
            WorkloadProvisionSuccessEvidence::Published {
                reference: reference.clone(),
                evidence,
            }
        }
        (
            WorkloadProvisionStep::ObservePublication,
            WorkloadProvisionSubjects::Publication(reference),
        ) => WorkloadProvisionSuccessEvidence::PublicationObserved {
            reference: reference.clone(),
            evidence,
        },
        _ => unreachable!("confirmed command construction validates step subjects"),
    }
}

fn evidence_digest(observation: &ProviderProvisionObservation) -> WorkloadOwnerEvidenceDigest {
    let durable = observation.evidence_sha256().unwrap_or_else(|| {
        debug_assert_eq!(
            observation.kind(),
            ProviderProvisionObservationKind::Claimed
        );
        "provider_claimed_without_outcome_evidence"
    });
    WorkloadOwnerEvidenceDigest::sha256(
        [
            b"nimbus.compute.provider-provision.observation.v1\0".as_slice(),
            durable.as_bytes(),
        ]
        .concat(),
    )
}

fn journal_error_result(
    command: &ConfirmedWorkloadProvisionCommand,
    error: &ProviderProvisionJournalError,
) -> WorkloadProvisionInspectionResult {
    match error {
        ProviderProvisionJournalError::InvalidClaim { .. }
        | ProviderProvisionJournalError::StaleGeneration { .. }
        | ProviderProvisionJournalError::StaleDispatchEpoch { .. }
        | ProviderProvisionJournalError::SkippedDispatchEpoch { .. }
        | ProviderProvisionJournalError::CrossedClaim
        | ProviderProvisionJournalError::RetryWithoutAbsence
        | ProviderProvisionJournalError::PriorEffectUnresolved => definite_failure_result(
            command,
            "provider_claim_rejected",
            WorkloadOwnerEvidenceDigest::sha256(error.to_string()),
        ),
        ProviderProvisionJournalError::Corrupt { .. }
        | ProviderProvisionJournalError::Store { .. } => ambiguous_result(command),
    }
}

fn definite_failure_result(
    command: &ConfirmedWorkloadProvisionCommand,
    code: &str,
    evidence: WorkloadOwnerEvidenceDigest,
) -> WorkloadProvisionInspectionResult {
    let failure = WorkloadFailureEvidence::new(code, evidence).unwrap_or_else(|_| {
        WorkloadFailureEvidence::new("provider_definite_failure", evidence)
            .expect("the fallback provider failure code is a valid static identifier")
    });
    WorkloadProvisionInspectionResult::DefiniteFailure {
        attempt_id: command.attempt_id().clone(),
        dispatch_epoch: command.dispatch_epoch(),
        provider_target: command.provider_target().clone(),
        failure,
    }
}

fn ambiguous_result(
    command: &ConfirmedWorkloadProvisionCommand,
) -> WorkloadProvisionInspectionResult {
    WorkloadProvisionInspectionResult::Ambiguous {
        attempt_id: command.attempt_id().clone(),
        dispatch_epoch: command.dispatch_epoch(),
        provider_target: command.provider_target().clone(),
    }
}

#[cfg(test)]
#[path = "provision_provider/tests.rs"]
pub(crate) mod tests;
