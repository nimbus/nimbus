//! Pure decisions for the durable workload-provision protocol.
//!
//! Values from this module are proposals, never provider commands. The sole
//! saga coordinator must first confirm an exact candidate through its store;
//! NNC6.4 owns any later conversion to effect-owner dispatch.

use nimbus_workloads::{
    WorkloadActivationIntent, WorkloadEffectReferences, WorkloadExecutionReference,
    WorkloadOwnerEvidenceDigest, WorkloadOwnerObservation, WorkloadPhaseDetail,
    WorkloadProvisionAttempt, WorkloadProvisionAttemptInput, WorkloadProvisionDispatchClaim,
    WorkloadProvisionDisposition, WorkloadProvisionEffectResult,
    WorkloadProvisionPrerequisiteEvidence, WorkloadProvisionProviderTarget, WorkloadProvisionStep,
    WorkloadProvisionSubjects, WorkloadProvisionSuccessEvidence, WorkloadPublicationIntent,
    WorkloadPublicationReference, WorkloadSagaError, WorkloadSagaPhase, WorkloadSagaRecord,
};

/// A provider-neutral operation that becomes actionable only after its
/// enclosing candidate is confirmed by the sole saga coordinator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkloadProvisionSymbolicAction {
    /// Start the exact attempt retained by the candidate.
    StartExactAttempt,
    /// Inspect the exact attempt retained by the candidate.
    InspectExactAttempt,
}

/// One exact candidate and optional post-confirmation symbolic operation.
///
/// This type intentionally has no conversion to a provider command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposedWorkloadProvisionTransition {
    candidate: Box<WorkloadSagaRecord>,
    action_after_confirmation: Option<WorkloadProvisionSymbolicAction>,
}

impl ProposedWorkloadProvisionTransition {
    pub(super) fn new(
        candidate: WorkloadSagaRecord,
        action_after_confirmation: Option<WorkloadProvisionSymbolicAction>,
    ) -> Self {
        Self {
            candidate: Box::new(candidate),
            action_after_confirmation,
        }
    }

    /// Exact portable candidate that still requires durable confirmation.
    pub fn candidate(&self) -> &WorkloadSagaRecord {
        &self.candidate
    }

    /// Symbolic operation that remains non-dispatchable in NNC6.3b.
    pub const fn action_after_confirmation(&self) -> Option<WorkloadProvisionSymbolicAction> {
        self.action_after_confirmation
    }

    pub fn into_candidate(self) -> WorkloadSagaRecord {
        *self.candidate
    }
}

/// Exhaustive pure decision for one running provision generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkloadProvisionDecision {
    /// Persist this candidate before interpreting its symbolic action.
    Proposed(ProposedWorkloadProvisionTransition),
    /// Reopen only by inspecting the byte-exact durable attempt.
    InspectExact(Box<WorkloadProvisionDispatchClaim>),
    /// This generation is halted until the later compensation owner acts.
    DefiniteFailure,
    /// No provision transition or inspection is currently required.
    Wait,
}

impl WorkloadProvisionDecision {
    /// Intended lifecycle target. A pending candidate remains at its completed
    /// source phase until its result is reduced, so this value comes from the
    /// exact retained attempt rather than from the candidate phase.
    pub fn target_phase(&self, current_phase: WorkloadSagaPhase) -> WorkloadSagaPhase {
        match self {
            Self::Proposed(proposed) => proposed
                .candidate()
                .provision_disposition()
                .and_then(WorkloadProvisionDisposition::attempt)
                .map_or(proposed.candidate().phase(), |attempt| {
                    attempt.target_phase()
                }),
            Self::InspectExact(claim) => claim.attempt().target_phase(),
            Self::DefiniteFailure | Self::Wait => current_phase,
        }
    }

    /// Plan the next value from durable state without reading a store or
    /// invoking an effect owner.
    pub fn plan(record: &WorkloadSagaRecord) -> Result<Self, WorkloadSagaError> {
        record.validate()?;
        if !record.phase().is_provision() {
            return Err(WorkloadSagaError::InvalidTransition(
                "provision reducer requires a running provision phase",
            ));
        }
        match record.provision_disposition() {
            Some(WorkloadProvisionDisposition::DispatchPending(claim))
            | Some(WorkloadProvisionDisposition::InspectionRequired(claim)) => {
                return Ok(Self::InspectExact(Box::new(claim.clone())));
            }
            Some(WorkloadProvisionDisposition::DefiniteFailure { .. }) => {
                return Ok(Self::DefiniteFailure);
            }
            Some(WorkloadProvisionDisposition::Ready) => {}
            None => {
                return Err(WorkloadSagaError::InvalidTransition(
                    "running provision phase requires a provision disposition",
                ));
            }
        }

        let intent = record.active_intent();
        let planned = match record.phase() {
            WorkloadSagaPhase::IntentCommitted => Some((
                WorkloadProvisionStep::ReserveNetwork,
                WorkloadSagaPhase::NetworkReserved,
                None,
            )),
            WorkloadSagaPhase::NetworkReserved => Some((
                WorkloadProvisionStep::PrepareWorkload,
                WorkloadSagaPhase::WorkloadPrepared,
                None,
            )),
            WorkloadSagaPhase::WorkloadPrepared => Some((
                WorkloadProvisionStep::AttachNetwork,
                WorkloadSagaPhase::NetworkAttached,
                None,
            )),
            WorkloadSagaPhase::NetworkAttached
                if intent.activation() == WorkloadActivationIntent::PrepareOnly =>
            {
                return Ok(Self::Wait);
            }
            WorkloadSagaPhase::NetworkAttached => Some((
                WorkloadProvisionStep::InspectActivationPrerequisites,
                WorkloadSagaPhase::NetworkAttached,
                None,
            )),
            WorkloadSagaPhase::WorkloadActivated => Some((
                WorkloadProvisionStep::InspectWorkloadReadiness,
                WorkloadSagaPhase::Ready,
                None,
            )),
            WorkloadSagaPhase::Ready
                if intent.publication() == WorkloadPublicationIntent::Withheld =>
            {
                let candidate = record.advance(
                    WorkloadSagaPhase::Observed,
                    record.phase_detail().clone(),
                    None,
                )?;
                return Ok(Self::Proposed(ProposedWorkloadProvisionTransition::new(
                    candidate, None,
                )));
            }
            WorkloadSagaPhase::Ready => Some((
                WorkloadProvisionStep::Publish,
                WorkloadSagaPhase::Published,
                None,
            )),
            WorkloadSagaPhase::Published => Some((
                WorkloadProvisionStep::ObservePublication,
                WorkloadSagaPhase::Observed,
                None,
            )),
            WorkloadSagaPhase::Observed => return Ok(Self::Wait),
            _ => {
                return Err(WorkloadSagaError::InvalidTransition(
                    "provision reducer reached a non-provision phase",
                ));
            }
        };
        let (step, target_phase, prerequisite) =
            planned.expect("every non-quiescent provision phase plans one attempt");
        propose_attempt(record, step, target_phase, prerequisite)
    }

    /// Reduce one exact effect result without reading a store or invoking an
    /// effect owner.
    pub fn reduce(
        record: &WorkloadSagaRecord,
        result: WorkloadProvisionEffectResult,
    ) -> Result<Self, WorkloadSagaError> {
        record.validate()?;
        let claim = match record.provision_disposition() {
            Some(
                WorkloadProvisionDisposition::DispatchPending(claim)
                | WorkloadProvisionDisposition::InspectionRequired(claim),
            ) => claim,
            _ => {
                return Err(WorkloadSagaError::InvalidTransition(
                    "provision result requires an exact unresolved attempt",
                ));
            }
        };
        let attempt = claim.attempt();
        let result_attempt_id = match &result {
            WorkloadProvisionEffectResult::Succeeded { attempt_id, .. }
            | WorkloadProvisionEffectResult::DefiniteFailure { attempt_id, .. }
            | WorkloadProvisionEffectResult::Ambiguous { attempt_id } => attempt_id,
        };
        if result_attempt_id != attempt.attempt_id() {
            return Err(WorkloadSagaError::InvalidEvidence(
                "provision result attempt id is crossed with durable state",
            ));
        }

        match result {
            WorkloadProvisionEffectResult::Ambiguous { .. } => {
                if matches!(
                    record.provision_disposition(),
                    Some(WorkloadProvisionDisposition::InspectionRequired(_))
                ) {
                    return Ok(Self::InspectExact(Box::new(claim.clone())));
                }
                let candidate = record.dispatch_to_inspection()?;
                Ok(Self::Proposed(ProposedWorkloadProvisionTransition::new(
                    candidate,
                    Some(WorkloadProvisionSymbolicAction::InspectExactAttempt),
                )))
            }
            WorkloadProvisionEffectResult::DefiniteFailure { failure, .. } => {
                let candidate = record.dispatch_to_definite_failure(failure)?;
                Ok(Self::Proposed(ProposedWorkloadProvisionTransition::new(
                    candidate, None,
                )))
            }
            WorkloadProvisionEffectResult::Succeeded { evidence, .. } => {
                validate_success(attempt, &evidence)?;
                if attempt.step() == WorkloadProvisionStep::InspectActivationPrerequisites {
                    let prerequisite = WorkloadProvisionPrerequisiteEvidence::new(
                        attempt.attempt_id().clone(),
                        evidence,
                    )?;
                    return propose_attempt(
                        record,
                        WorkloadProvisionStep::ActivateWorkload,
                        WorkloadSagaPhase::WorkloadActivated,
                        Some(prerequisite),
                    );
                }
                let phase_detail = phase_detail_after_success(record, &evidence)?;
                let candidate = record.dispatch_to_success(attempt.target_phase(), phase_detail)?;
                Ok(Self::Proposed(ProposedWorkloadProvisionTransition::new(
                    candidate, None,
                )))
            }
        }
    }
}

fn propose_attempt(
    record: &WorkloadSagaRecord,
    step: WorkloadProvisionStep,
    target_phase: WorkloadSagaPhase,
    prerequisite: Option<WorkloadProvisionPrerequisiteEvidence>,
) -> Result<WorkloadProvisionDecision, WorkloadSagaError> {
    let intent = record.active_intent();
    let attempt = WorkloadProvisionAttempt::new(WorkloadProvisionAttemptInput {
        key: record.key().clone(),
        saga_id: record.saga_id().clone(),
        issuing_revision: record.revision(),
        generation: intent.generation(),
        desired_digest: intent.desired_digest(),
        required_node: intent.admission().assigned_node().clone(),
        source_digest: intent.source().source_digest(),
        execution_provider_id: intent.source().execution_provider_id().clone(),
        network_plan_digest: intent.network().digest(),
        selection_evidence: intent
            .network()
            .compiled_plan()
            .content()
            .capability_selection_evidence()
            .cloned(),
        source_phase: record.phase(),
        target_phase,
        step,
        subjects: subjects_for(record, step)?,
        prerequisite,
    })?;
    let provider_target = WorkloadProvisionProviderTarget::for_attempt(&attempt)?;
    let Some(provider_target) = provider_target else {
        let evidence = resource_free_success(record, &attempt)?;
        let phase_detail = phase_detail_after_success(record, &evidence)?;
        let candidate =
            record.record_resource_free_network_step(step, target_phase, phase_detail)?;
        return Ok(WorkloadProvisionDecision::Proposed(
            ProposedWorkloadProvisionTransition::new(candidate, None),
        ));
    };
    let candidate = if step == WorkloadProvisionStep::ActivateWorkload {
        record.dispatch_to_activation(attempt, provider_target)?
    } else {
        record.ready_to_initial_dispatch(attempt, provider_target)?
    };
    Ok(WorkloadProvisionDecision::Proposed(
        ProposedWorkloadProvisionTransition::new(
            candidate,
            Some(WorkloadProvisionSymbolicAction::StartExactAttempt),
        ),
    ))
}

fn resource_free_success(
    record: &WorkloadSagaRecord,
    attempt: &WorkloadProvisionAttempt,
) -> Result<WorkloadProvisionSuccessEvidence, WorkloadSagaError> {
    let reference = nimbus_workloads::WorkloadNetworkReference::for_intent(record.active_intent());
    let evidence = match attempt.step() {
        WorkloadProvisionStep::ReserveNetwork => {
            WorkloadProvisionSuccessEvidence::NetworkReserved {
                reference,
                evidence: WorkloadOwnerEvidenceDigest::sha256(format!(
                    "nimbus.compute.resource-free.reserve:{}",
                    attempt.network_plan_digest()
                )),
            }
        }
        WorkloadProvisionStep::AttachNetwork => WorkloadProvisionSuccessEvidence::NetworkAttached {
            reference,
            evidence: WorkloadOwnerEvidenceDigest::sha256(format!(
                "nimbus.compute.resource-free.attach:{}",
                attempt.network_plan_digest()
            )),
        },
        _ => {
            return Err(WorkloadSagaError::InvalidTransition(
                "only resource-free reserve or attach can omit a provider target",
            ));
        }
    };
    Ok(evidence)
}

fn subjects_for(
    record: &WorkloadSagaRecord,
    step: WorkloadProvisionStep,
) -> Result<WorkloadProvisionSubjects, WorkloadSagaError> {
    let intent = record.active_intent();
    match step {
        WorkloadProvisionStep::ReserveNetwork | WorkloadProvisionStep::AttachNetwork => {
            Ok(WorkloadProvisionSubjects::Network(
                nimbus_workloads::WorkloadNetworkReference::for_intent(intent),
            ))
        }
        WorkloadProvisionStep::PrepareWorkload | WorkloadProvisionStep::ActivateWorkload => Ok(
            WorkloadProvisionSubjects::Execution(WorkloadExecutionReference::for_intent(intent)),
        ),
        WorkloadProvisionStep::InspectActivationPrerequisites
        | WorkloadProvisionStep::InspectWorkloadReadiness => {
            Ok(WorkloadProvisionSubjects::Readiness {
                network: nimbus_workloads::WorkloadNetworkReference::for_intent(intent),
                execution: WorkloadExecutionReference::for_intent(intent),
            })
        }
        WorkloadProvisionStep::Publish | WorkloadProvisionStep::ObservePublication => {
            let reference = record
                .phase_detail()
                .references()
                .publication()
                .cloned()
                .ok_or(WorkloadSagaError::InvalidEvidence(
                    "publication step requires a durable publication reference",
                ))?;
            Ok(WorkloadProvisionSubjects::Publication(reference))
        }
    }
}

fn validate_success(
    attempt: &WorkloadProvisionAttempt,
    evidence: &WorkloadProvisionSuccessEvidence,
) -> Result<(), WorkloadSagaError> {
    if attempt.step() != evidence.step() {
        return Err(WorkloadSagaError::InvalidEvidence(
            "provision success evidence does not match the attempted step",
        ));
    }
    let matches = match (attempt.subjects(), evidence) {
        (
            WorkloadProvisionSubjects::Network(expected),
            WorkloadProvisionSuccessEvidence::NetworkReserved { reference, .. }
            | WorkloadProvisionSuccessEvidence::NetworkAttached { reference, .. },
        ) => expected == reference,
        (
            WorkloadProvisionSubjects::Execution(expected),
            WorkloadProvisionSuccessEvidence::WorkloadPrepared { reference, .. }
            | WorkloadProvisionSuccessEvidence::WorkloadActivated { reference, .. },
        ) => expected == reference,
        (
            WorkloadProvisionSubjects::Readiness {
                network: expected_network,
                execution: expected_execution,
            },
            WorkloadProvisionSuccessEvidence::ActivationPrerequisitesReady {
                network,
                execution,
                ..
            }
            | WorkloadProvisionSuccessEvidence::WorkloadReady {
                network, execution, ..
            },
        ) => expected_network == network && expected_execution == execution,
        (
            WorkloadProvisionSubjects::Publication(expected),
            WorkloadProvisionSuccessEvidence::Published { reference, .. }
            | WorkloadProvisionSuccessEvidence::PublicationObserved { reference, .. },
        ) => expected == reference,
        _ => false,
    };
    if matches {
        Ok(())
    } else {
        Err(WorkloadSagaError::InvalidEvidence(
            "provision success evidence subjects are crossed with the exact attempt",
        ))
    }
}

fn phase_detail_after_success(
    record: &WorkloadSagaRecord,
    evidence: &WorkloadProvisionSuccessEvidence,
) -> Result<WorkloadPhaseDetail, WorkloadSagaError> {
    let intent = record.active_intent();
    let mut references = record.phase_detail().references();
    let mut observations = match record.phase_detail() {
        WorkloadPhaseDetail::Intent => Vec::new(),
        WorkloadPhaseDetail::Provision(detail) => detail.observations().to_vec(),
        _ => {
            return Err(WorkloadSagaError::InvalidEvidence(
                "provision success requires provision lifecycle evidence",
            ));
        }
    };
    let target_phase = match evidence {
        WorkloadProvisionSuccessEvidence::NetworkReserved {
            reference,
            evidence,
        } => {
            references = WorkloadEffectReferences::provision(intent, None)?;
            observations.push(WorkloadOwnerObservation::NetworkReserved {
                reference: reference.clone(),
                evidence: *evidence,
            });
            WorkloadSagaPhase::NetworkReserved
        }
        WorkloadProvisionSuccessEvidence::WorkloadPrepared {
            reference,
            evidence,
        } => {
            observations.push(WorkloadOwnerObservation::ExecutionPrepared {
                reference: reference.clone(),
                evidence: *evidence,
            });
            WorkloadSagaPhase::WorkloadPrepared
        }
        WorkloadProvisionSuccessEvidence::NetworkAttached {
            reference,
            evidence,
        } => {
            observations.push(WorkloadOwnerObservation::NetworkAttached {
                reference: reference.clone(),
                evidence: *evidence,
            });
            WorkloadSagaPhase::NetworkAttached
        }
        WorkloadProvisionSuccessEvidence::ActivationPrerequisitesReady { .. } => {
            return Err(WorkloadSagaError::InvalidEvidence(
                "activation prerequisite success must create the exact activation attempt",
            ));
        }
        WorkloadProvisionSuccessEvidence::WorkloadActivated {
            reference,
            evidence,
        } => {
            observations.push(WorkloadOwnerObservation::ExecutionActivated {
                reference: reference.clone(),
                evidence: *evidence,
            });
            WorkloadSagaPhase::WorkloadActivated
        }
        WorkloadProvisionSuccessEvidence::WorkloadReady {
            network,
            execution,
            evidence,
        } => {
            let listeners = intent.network().compiled_plan().content().listeners();
            let publication = if intent.publication() == WorkloadPublicationIntent::PublishWhenReady
                || listeners.is_empty()
            {
                Some(WorkloadPublicationReference::new(
                    listeners
                        .iter()
                        .map(|listener| listener.endpoint_id().clone()),
                    intent,
                )?)
            } else {
                None
            };
            references = WorkloadEffectReferences::provision(intent, publication)?;
            observations.push(WorkloadOwnerObservation::Ready {
                network: network.clone(),
                execution: execution.clone(),
                evidence: *evidence,
            });
            WorkloadSagaPhase::Ready
        }
        WorkloadProvisionSuccessEvidence::Published {
            reference,
            evidence,
        } => {
            observations.push(WorkloadOwnerObservation::PublicationPresent {
                reference: reference.clone(),
                evidence: *evidence,
            });
            WorkloadSagaPhase::Published
        }
        WorkloadProvisionSuccessEvidence::PublicationObserved {
            reference,
            evidence,
        } => {
            observations.push(WorkloadOwnerObservation::PublicationObserved {
                reference: reference.clone(),
                evidence: *evidence,
            });
            WorkloadSagaPhase::Observed
        }
    };
    WorkloadPhaseDetail::provision(target_phase, intent, references, observations)
}

#[cfg(test)]
#[path = "provision_decision/tests.rs"]
mod tests;
