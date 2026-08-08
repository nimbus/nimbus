use super::*;

pub(super) fn initial_provision_disposition(
    intent: &WorkloadSagaIntent,
) -> Option<WorkloadProvisionDisposition> {
    (intent.desired_state == DesiredWorkloadState::Running)
        .then_some(WorkloadProvisionDisposition::Ready)
}

pub(super) fn validate_provision_disposition(
    record: &WorkloadSagaRecord,
) -> Result<(), WorkloadSagaError> {
    let should_exist = record.active_intent.desired_state == DesiredWorkloadState::Running
        && record.phase.is_provision();
    if record.provision_disposition.is_some() != should_exist {
        return Err(WorkloadSagaError::InvalidTransition(
            "provision disposition presence does not match lifecycle state",
        ));
    }
    let Some(disposition) = record.provision_disposition.as_ref() else {
        return Ok(());
    };
    if let Some(claim) = disposition.claim() {
        validate_attempt_for_record(record, claim.attempt())?;
        validate_claim_revision(record, disposition, claim)?;
    }
    if let WorkloadProvisionDisposition::DefiniteFailure { failure, .. } = disposition {
        failure.validate()?;
    }
    Ok(())
}

fn validate_claim_revision(
    record: &WorkloadSagaRecord,
    disposition: &WorkloadProvisionDisposition,
    claim: &WorkloadProvisionDispatchClaim,
) -> Result<(), WorkloadSagaError> {
    let after_claim = claim.claimed_revision().checked_next();
    let after_inspection = after_claim.and_then(WorkloadSagaRevision::checked_next);
    let valid = match disposition {
        WorkloadProvisionDisposition::Ready => false,
        WorkloadProvisionDisposition::DispatchPending(_) => {
            claim.claimed_revision() == record.revision
        }
        WorkloadProvisionDisposition::InspectionRequired(_) => after_claim == Some(record.revision),
        WorkloadProvisionDisposition::DefiniteFailure { .. } => {
            after_claim == Some(record.revision) || after_inspection == Some(record.revision)
        }
    };
    if valid {
        Ok(())
    } else {
        Err(WorkloadSagaError::InvalidEvidence(
            "provision dispatch claim revision does not exactly bind disposition history",
        ))
    }
}

fn validate_attempt_for_record(
    record: &WorkloadSagaRecord,
    attempt: &WorkloadProvisionAttempt,
) -> Result<(), WorkloadSagaError> {
    let intent = &record.active_intent;
    if attempt.key() != &record.key
        || attempt.saga_id() != &record.saga_id
        || attempt.generation() != intent.generation
        || attempt.desired_digest() != intent.desired_digest
        || attempt.required_node() != intent.admission.assigned_node()
        || attempt.source_digest() != intent.source.source_digest()
        || attempt.execution_provider_id() != intent.source.execution_provider_id()
        || attempt.network_plan_digest() != intent.network.digest()
        || attempt.selection_evidence()
            != intent
                .network
                .compiled_plan()
                .content()
                .capability_selection_evidence()
        || attempt.source_phase() != record.phase
        || attempt.issuing_revision() >= record.revision
    {
        return Err(WorkloadSagaError::InvalidEvidence(
            "provision attempt is crossed with the durable workload generation",
        ));
    }
    let references = record.phase_detail.references();
    let subjects_match = match attempt.subjects() {
        WorkloadProvisionSubjects::Network(reference) => {
            reference == &WorkloadNetworkReference::for_intent(intent)
        }
        WorkloadProvisionSubjects::Execution(reference) => {
            reference == &WorkloadExecutionReference::for_intent(intent)
        }
        WorkloadProvisionSubjects::Readiness { network, execution } => {
            network == &WorkloadNetworkReference::for_intent(intent)
                && execution == &WorkloadExecutionReference::for_intent(intent)
        }
        WorkloadProvisionSubjects::Publication(reference) => {
            references.publication() == Some(reference)
        }
    };
    if !subjects_match {
        return Err(WorkloadSagaError::InvalidEvidence(
            "provision attempt subjects are crossed with durable lifecycle references",
        ));
    }
    if attempt.step() == WorkloadProvisionStep::ActivateWorkload
        && !activation_prerequisite_matches_intent(attempt, intent)
    {
        return Err(WorkloadSagaError::InvalidEvidence(
            "activation prerequisite is crossed with durable lifecycle references",
        ));
    }
    Ok(())
}

fn activation_prerequisite_matches_intent(
    attempt: &WorkloadProvisionAttempt,
    intent: &WorkloadSagaIntent,
) -> bool {
    matches!(
        (attempt.subjects(), attempt.prerequisite().map(WorkloadProvisionPrerequisiteEvidence::evidence)),
        (
            WorkloadProvisionSubjects::Execution(activation_execution),
            Some(WorkloadProvisionSuccessEvidence::ActivationPrerequisitesReady {
                network,
                execution,
                ..
            })
        ) if activation_execution == execution
            && network == &WorkloadNetworkReference::for_intent(intent)
            && execution == &WorkloadExecutionReference::for_intent(intent)
    )
}

fn activation_attempt_follows_inspection(
    previous: &WorkloadProvisionAttempt,
    next: &WorkloadProvisionAttempt,
) -> bool {
    matches!(
        (
            previous.subjects(),
            next.subjects(),
            next.prerequisite(),
        ),
        (
            WorkloadProvisionSubjects::Readiness {
                network: inspected_network,
                execution: inspected_execution,
            },
            WorkloadProvisionSubjects::Execution(activation_execution),
            Some(prerequisite),
        ) if prerequisite.attempt_id() == previous.attempt_id()
            && matches!(
                prerequisite.evidence(),
                WorkloadProvisionSuccessEvidence::ActivationPrerequisitesReady {
                    network,
                    execution,
                    ..
                } if network == inspected_network
                    && execution == inspected_execution
                    && activation_execution == execution
            )
    )
}

fn retry_claim_follows_inspection(
    current: &WorkloadSagaRecord,
    previous: &WorkloadProvisionDispatchClaim,
    next: &WorkloadProvisionDispatchClaim,
    resulting_revision: WorkloadSagaRevision,
) -> bool {
    let WorkloadProvisionDispatchAuthorization::RetryAfterAbsence(absence) = next.authorization()
    else {
        return false;
    };
    previous.attempt() == next.attempt()
        && previous.provider_target() == next.provider_target()
        && previous.dispatch_epoch().checked_next() == Some(next.dispatch_epoch())
        && next.claimed_revision() == resulting_revision
        && absence.matches_inspection(current, previous)
}

pub(super) fn validate_provision_disposition_transition(
    current: &WorkloadSagaRecord,
    candidate: &WorkloadSagaRecord,
    active_changed: bool,
) -> Result<(), WorkloadSagaError> {
    if active_changed {
        return if candidate.provision_disposition
            == initial_provision_disposition(&candidate.active_intent)
        {
            Ok(())
        } else {
            Err(WorkloadSagaError::InvalidTransition(
                "promoted generation must retain its exact initial provision disposition",
            ))
        };
    }
    match (
        current.provision_disposition.as_ref(),
        candidate.provision_disposition.as_ref(),
    ) {
        (None, None) => Ok(()),
        (Some(WorkloadProvisionDisposition::Ready), None)
            if matches!(
                candidate.phase,
                WorkloadSagaPhase::CleanupPending | WorkloadSagaPhase::WithdrawalCommitted
            ) =>
        {
            Ok(())
        }
        (Some(WorkloadProvisionDisposition::Ready), Some(WorkloadProvisionDisposition::Ready))
            if current
                .active_intent
                .network
                .compiled_plan()
                .content()
                .capability_selection_evidence()
                .is_none()
                && matches!(
                    (current.phase, candidate.phase),
                    (
                        WorkloadSagaPhase::IntentCommitted,
                        WorkloadSagaPhase::NetworkReserved
                    ) | (
                        WorkloadSagaPhase::WorkloadPrepared,
                        WorkloadSagaPhase::NetworkAttached
                    )
                ) =>
        {
            Ok(())
        }
        (Some(WorkloadProvisionDisposition::Ready), Some(WorkloadProvisionDisposition::Ready))
            if current.phase == WorkloadSagaPhase::Ready
                && candidate.phase == WorkloadSagaPhase::Observed
                && current.active_intent.publication == WorkloadPublicationIntent::Withheld =>
        {
            Ok(())
        }
        (
            Some(WorkloadProvisionDisposition::Ready),
            Some(WorkloadProvisionDisposition::DispatchPending(claim)),
        ) if candidate.phase == current.phase
            && claim.attempt().issuing_revision() == current.revision
            && claim.claimed_revision() == candidate.revision
            && claim.dispatch_epoch() == WorkloadProvisionDispatchEpoch::new(0)
            && matches!(
                claim.authorization(),
                WorkloadProvisionDispatchAuthorization::Initial
            )
            && claim.attempt().step() != WorkloadProvisionStep::ActivateWorkload =>
        {
            Ok(())
        }
        (
            Some(WorkloadProvisionDisposition::DispatchPending(previous)),
            Some(WorkloadProvisionDisposition::InspectionRequired(next)),
        ) if candidate.phase == current.phase && previous == next => Ok(()),
        (
            Some(
                WorkloadProvisionDisposition::DispatchPending(previous)
                | WorkloadProvisionDisposition::InspectionRequired(previous),
            ),
            Some(WorkloadProvisionDisposition::DefiniteFailure { claim, .. }),
        ) if candidate.phase == current.phase && previous == claim => Ok(()),
        (
            Some(
                WorkloadProvisionDisposition::DispatchPending(previous)
                | WorkloadProvisionDisposition::InspectionRequired(previous),
            ),
            Some(WorkloadProvisionDisposition::Ready),
        ) if candidate.phase == previous.attempt().target_phase()
            && candidate.phase != current.phase =>
        {
            Ok(())
        }
        (
            Some(
                WorkloadProvisionDisposition::DispatchPending(previous)
                | WorkloadProvisionDisposition::InspectionRequired(previous),
            ),
            Some(WorkloadProvisionDisposition::DispatchPending(next)),
        ) if candidate.phase == current.phase
            && previous.attempt().step()
                == WorkloadProvisionStep::InspectActivationPrerequisites
            && next.attempt().step() == WorkloadProvisionStep::ActivateWorkload
            && next.attempt().issuing_revision() == current.revision
            && next.claimed_revision() == candidate.revision
            && next.dispatch_epoch() == WorkloadProvisionDispatchEpoch::new(0)
            && matches!(
                next.authorization(),
                WorkloadProvisionDispatchAuthorization::Initial
            )
            && activation_attempt_follows_inspection(previous.attempt(), next.attempt()) =>
        {
            Ok(())
        }
        (
            Some(
                WorkloadProvisionDisposition::DispatchPending(previous)
                | WorkloadProvisionDisposition::InspectionRequired(previous),
            ),
            Some(WorkloadProvisionDisposition::DispatchPending(next)),
        ) if candidate.phase == current.phase
            && retry_claim_follows_inspection(current, previous, next, candidate.revision) =>
        {
            Ok(())
        }
        _ => Err(WorkloadSagaError::InvalidTransition(
            "provision disposition transition is not legal",
        )),
    }
}
