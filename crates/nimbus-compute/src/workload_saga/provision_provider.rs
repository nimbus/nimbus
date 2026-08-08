//! Adapter-side use of the provider-local attempt journal.
//!
//! Concrete provider adapters supply only one phase effect or inspection. This
//! helper authenticates the complete compute command, claims provider-local
//! authority, adopts exact replay, and translates durable observations back to
//! the portable saga vocabulary.

use nimbus_sandbox::{
    ProviderCommandAttemptJournal, ProviderCommandClaim, ProviderCommandClaimDecision,
    ProviderCommandClaimInput, ProviderCommandJournalError, ProviderCommandObservation,
    ProviderCommandObservationKind, ProviderCommandOperation,
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
    attempt_idempotency_journal: ProviderCommandAttemptJournal,
}

impl ProviderProvisionPhaseAdapter {
    pub fn new(attempt_idempotency_journal: ProviderCommandAttemptJournal) -> Self {
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
            Ok(ProviderCommandClaimDecision::ExecuteClaimed(_)) => {
                self.record_effect(command, &claim, effect())
            }
            Ok(ProviderCommandClaimDecision::AdoptExactAttempt(observation)) => {
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
            Ok(ProviderCommandClaimDecision::AdoptExactAttempt(observation))
                if matches!(
                    observation.kind(),
                    ProviderCommandObservationKind::Succeeded
                        | ProviderCommandObservationKind::DefiniteFailure
                        | ProviderCommandObservationKind::Absent
                ) =>
            {
                observation_result(command, &observation)
            }
            Ok(
                ProviderCommandClaimDecision::ExecuteClaimed(_)
                | ProviderCommandClaimDecision::AdoptExactAttempt(_),
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
        claim: &ProviderCommandClaim,
        effect: ProviderProvisionEffectObservation,
    ) -> WorkloadProvisionInspectionResult {
        let (kind, evidence) = match &effect {
            ProviderProvisionEffectObservation::Succeeded { evidence } => (
                ProviderCommandObservationKind::Succeeded,
                evidence.as_slice(),
            ),
            ProviderProvisionEffectObservation::DefiniteFailure { evidence, .. } => (
                ProviderCommandObservationKind::DefiniteFailure,
                evidence.as_slice(),
            ),
            ProviderProvisionEffectObservation::Absent { evidence } => {
                (ProviderCommandObservationKind::Absent, evidence.as_slice())
            }
            ProviderProvisionEffectObservation::InProgress { evidence } => (
                ProviderCommandObservationKind::InProgress,
                evidence.as_slice(),
            ),
            ProviderProvisionEffectObservation::Ambiguous { evidence } => (
                ProviderCommandObservationKind::Ambiguous,
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
) -> Result<ProviderCommandClaim, ProviderCommandJournalError> {
    let effect_subject = serde_json::to_string(command.subjects()).map_err(|error| {
        ProviderCommandJournalError::InvalidClaim {
            message: format!("confirmed provider subject cannot be encoded: {error}"),
        }
    })?;
    let target = serde_json::to_vec(command.provider_target()).map_err(|error| {
        ProviderCommandJournalError::InvalidClaim {
            message: format!("confirmed provider target cannot be encoded: {error}"),
        }
    })?;
    ProviderCommandClaim::new(ProviderCommandClaimInput {
        authority_id: command.saga_id().as_str().to_owned(),
        effect_subject,
        source_attempt_id: None,
        attempt_id: command.attempt_id().as_str().to_owned(),
        dispatch_epoch: command.dispatch_epoch().as_u64(),
        workload_generation: command.generation().as_u64(),
        restart_ordinal: 0,
        desired_digest: command.desired_digest().to_string(),
        source_digest: command.source_digest().to_string(),
        network_plan_digest: command.network_plan_digest().to_string(),
        provider_target_digest: WorkloadOwnerEvidenceDigest::sha256(target).to_string(),
        operation: operation(command.step()),
    })
}

const fn operation(step: WorkloadProvisionStep) -> ProviderCommandOperation {
    match step {
        WorkloadProvisionStep::ReserveNetwork => ProviderCommandOperation::ReserveNetwork,
        WorkloadProvisionStep::PrepareWorkload => ProviderCommandOperation::PrepareWorkload,
        WorkloadProvisionStep::AttachNetwork => ProviderCommandOperation::AttachNetwork,
        WorkloadProvisionStep::InspectActivationPrerequisites => {
            ProviderCommandOperation::InspectActivationPrerequisites
        }
        WorkloadProvisionStep::ActivateWorkload => ProviderCommandOperation::ActivateWorkload,
        WorkloadProvisionStep::InspectWorkloadReadiness => {
            ProviderCommandOperation::InspectWorkloadReadiness
        }
        WorkloadProvisionStep::Publish => ProviderCommandOperation::PublishIngress,
        WorkloadProvisionStep::ObservePublication => ProviderCommandOperation::ObserveIngress,
    }
}

fn observation_result(
    command: &ConfirmedWorkloadProvisionCommand,
    observation: &ProviderCommandObservation,
) -> WorkloadProvisionInspectionResult {
    let evidence = evidence_digest(observation);
    match observation.kind() {
        ProviderCommandObservationKind::Claimed | ProviderCommandObservationKind::InProgress => {
            WorkloadProvisionInspectionResult::InProgress {
                attempt_id: command.attempt_id().clone(),
                dispatch_epoch: command.dispatch_epoch(),
                provider_target: command.provider_target().clone(),
                evidence,
            }
        }
        ProviderCommandObservationKind::Succeeded => WorkloadProvisionInspectionResult::Succeeded {
            attempt_id: command.attempt_id().clone(),
            dispatch_epoch: command.dispatch_epoch(),
            provider_target: command.provider_target().clone(),
            evidence: success_evidence(command, evidence),
        },
        ProviderCommandObservationKind::DefiniteFailure => {
            definite_failure_result(command, "provider_definite_failure", evidence)
        }
        ProviderCommandObservationKind::Absent => WorkloadProvisionInspectionResult::Absent {
            evidence: command.absence_evidence(evidence),
        },
        ProviderCommandObservationKind::Ambiguous => ambiguous_result(command),
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

fn evidence_digest(observation: &ProviderCommandObservation) -> WorkloadOwnerEvidenceDigest {
    let durable = observation.evidence_sha256().unwrap_or_else(|| {
        debug_assert_eq!(observation.kind(), ProviderCommandObservationKind::Claimed);
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
    error: &ProviderCommandJournalError,
) -> WorkloadProvisionInspectionResult {
    match error {
        ProviderCommandJournalError::InvalidClaim { .. }
        | ProviderCommandJournalError::StaleWorkloadGeneration { .. }
        | ProviderCommandJournalError::StaleRestartOrdinal { .. }
        | ProviderCommandJournalError::SkippedRestartOrdinal { .. }
        | ProviderCommandJournalError::StaleDispatchEpoch { .. }
        | ProviderCommandJournalError::SkippedDispatchEpoch { .. }
        | ProviderCommandJournalError::CrossedClaim
        | ProviderCommandJournalError::RetryWithoutAbsence
        | ProviderCommandJournalError::PriorEffectUnresolved => definite_failure_result(
            command,
            "provider_claim_rejected",
            WorkloadOwnerEvidenceDigest::sha256(error.to_string()),
        ),
        ProviderCommandJournalError::Corrupt { .. } | ProviderCommandJournalError::Store { .. } => {
            ambiguous_result(command)
        }
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
