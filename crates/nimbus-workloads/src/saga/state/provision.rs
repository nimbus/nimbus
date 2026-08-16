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
        WorkloadProvisionDisposition::InspectionRequired(_) => {
            after_claim == Some(record.revision)
                || claim.claimed_revision() == record.revision
                    && matches!(
                        claim.authorization(),
                        WorkloadProvisionDispatchAuthorization::OwnerReopenedAttachmentInspection
                            |
                        WorkloadProvisionDispatchAuthorization::OwnerReopenedPublicationInspection
                    )
                || record.successor_intent.is_some() && claim.claimed_revision() < record.revision
        }
        WorkloadProvisionDisposition::DefiniteFailure { .. } => {
            after_claim == Some(record.revision)
                || after_inspection == Some(record.revision)
                || record.successor_intent.is_some() && claim.claimed_revision() < record.revision
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

pub(super) fn owner_reopened_publication_source_is_exact(record: &WorkloadSagaRecord) -> bool {
    owner_reopened_observed_authority_is_exact(record)
        && record.provision_disposition == Some(WorkloadProvisionDisposition::Ready)
}

fn owner_reopened_observed_authority_is_exact(record: &WorkloadSagaRecord) -> bool {
    owner_reopened_observed_lineage_is_exact(record) && record.successor_intent.is_none()
}

fn owner_reopened_observed_lineage_is_exact(record: &WorkloadSagaRecord) -> bool {
    record.active_intent.desired_state == DesiredWorkloadState::Running
        && record.active_intent.publication == WorkloadPublicationIntent::PublishWhenReady
        && !record
            .active_intent
            .network
            .compiled_plan()
            .content()
            .listeners()
            .is_empty()
        && record.phase == WorkloadSagaPhase::Observed
        && record.restart.active().is_none()
        && record.failure.is_none()
}

fn owner_reopened_attachment_inspection_is_exact(record: &WorkloadSagaRecord) -> bool {
    let Some(WorkloadProvisionDisposition::InspectionRequired(claim)) =
        record.provision_disposition.as_ref()
    else {
        return false;
    };
    owner_reopened_observed_authority_is_exact(record)
        && record.last_transition.source_phase() == Some(WorkloadSagaPhase::Observed)
        && claim.claimed_revision() == record.revision
        && matches!(
            claim.authorization(),
            WorkloadProvisionDispatchAuthorization::OwnerReopenedAttachmentInspection
        )
}

pub(super) fn owner_reopened_attachment_recovery_is_exact(record: &WorkloadSagaRecord) -> bool {
    record.successor_intent.is_none() && owner_reopened_attachment_claim_is_exact(record)
}

pub(super) fn owner_reopened_attachment_fence_is_exact(record: &WorkloadSagaRecord) -> bool {
    record.successor_intent.is_some()
        && matches!(
            record.provision_disposition.as_ref(),
            Some(WorkloadProvisionDisposition::InspectionRequired(_))
        )
        && owner_reopened_attachment_claim_is_exact(record)
}

fn owner_reopened_attachment_claim_is_exact(record: &WorkloadSagaRecord) -> bool {
    if owner_reopened_attachment_inspection_is_exact(record) {
        return true;
    }
    let Some(
        WorkloadProvisionDisposition::DispatchPending(claim)
        | WorkloadProvisionDisposition::InspectionRequired(claim),
    ) = record.provision_disposition.as_ref()
    else {
        return false;
    };
    let disposition_matches_authority =
        match (record.provision_disposition.as_ref(), claim.authorization()) {
            (
                Some(WorkloadProvisionDisposition::InspectionRequired(_)),
                WorkloadProvisionDispatchAuthorization::OwnerReopenedAttachmentInspection,
            ) => true,
            (
                Some(
                    WorkloadProvisionDisposition::DispatchPending(_)
                    | WorkloadProvisionDisposition::InspectionRequired(_),
                ),
                WorkloadProvisionDispatchAuthorization::RetryAfterAbsence(absence),
            ) => {
                absence.origin()
                    == WorkloadProvisionAbsenceOrigin::OwnerReopenedAttachmentInspection
            }
            _ => false,
        };
    owner_reopened_observed_lineage_is_exact(record)
        && record.last_transition.source_phase() == Some(WorkloadSagaPhase::Observed)
        && disposition_matches_authority
        && owner_reopened_attachment_claim_context_is_exact(record, claim)
}

pub(super) fn owner_reopened_attachment_claim_context_is_exact(
    record: &WorkloadSagaRecord,
    claim: &WorkloadProvisionDispatchClaim,
) -> bool {
    let attempt = claim.attempt();
    attempt.key() == &record.key
        && attempt.saga_id() == &record.saga_id
        && attempt.generation() == record.active_intent.generation
        && attempt.desired_digest() == record.active_intent.desired_digest
        && attempt.required_node() == record.active_intent.admission.assigned_node()
        && attempt.source_digest() == record.active_intent.source.source_digest()
        && attempt.execution_provider_id() == record.active_intent.source.execution_provider_id()
        && attempt.network_plan_digest() == record.active_intent.network.digest()
        && attempt.selection_evidence()
            == record
                .active_intent
                .network
                .compiled_plan()
                .content()
                .capability_selection_evidence()
        && attempt.subjects()
            == &WorkloadProvisionSubjects::Network(WorkloadNetworkReference::for_intent(
                &record.active_intent,
            ))
        && attempt.step() == WorkloadProvisionStep::AttachNetwork
        && attempt.source_phase() == WorkloadSagaPhase::WorkloadPrepared
        && attempt.target_phase() == WorkloadSagaPhase::NetworkAttached
        && match claim.authorization() {
            WorkloadProvisionDispatchAuthorization::OwnerReopenedAttachmentInspection => true,
            WorkloadProvisionDispatchAuthorization::RetryAfterAbsence(absence) => {
                absence.origin()
                    == WorkloadProvisionAbsenceOrigin::OwnerReopenedAttachmentInspection
            }
            _ => false,
        }
}

pub(super) fn owner_reopened_publication_inspection_is_exact(record: &WorkloadSagaRecord) -> bool {
    let Some(WorkloadProvisionDisposition::InspectionRequired(claim)) =
        record.provision_disposition.as_ref()
    else {
        return false;
    };
    record.active_intent.desired_state == DesiredWorkloadState::Running
        && record.active_intent.publication == WorkloadPublicationIntent::PublishWhenReady
        && !record
            .active_intent
            .network
            .compiled_plan()
            .content()
            .listeners()
            .is_empty()
        && record.phase == WorkloadSagaPhase::Published
        && record.last_transition.source_phase() == Some(WorkloadSagaPhase::Observed)
        && record.successor_intent.is_none()
        && record.restart.active().is_none()
        && record.failure.is_none()
        && claim.claimed_revision() == record.revision
        && matches!(
            claim.authorization(),
            WorkloadProvisionDispatchAuthorization::OwnerReopenedPublicationInspection
        )
}

pub(super) fn owner_reopened_publication_transition_is_exact(
    current: &WorkloadSagaRecord,
    candidate: &WorkloadSagaRecord,
) -> bool {
    if owner_reopened_publication_source_is_exact(current)
        && owner_reopened_attachment_inspection_is_exact(candidate)
        && current.active_intent == candidate.active_intent
        && current.successor_intent == candidate.successor_intent
        && current.restart == candidate.restart
        && current.failure == candidate.failure
        && current.phase_detail == candidate.phase_detail
    {
        return true;
    }
    if !owner_reopened_attachment_recovery_is_exact(current)
        || !owner_reopened_publication_inspection_is_exact(candidate)
        || current.active_intent != candidate.active_intent
        || current.successor_intent != candidate.successor_intent
        || current.restart != candidate.restart
        || current.failure != candidate.failure
    {
        return false;
    }
    let (WorkloadPhaseDetail::Provision(previous), WorkloadPhaseDetail::Provision(next)) =
        (&current.phase_detail, &candidate.phase_detail)
    else {
        return false;
    };
    let Some((last, prefix)) = previous.observations.split_last() else {
        return false;
    };
    previous.references == next.references
        && prefix == next.observations
        && matches!(
            last,
            WorkloadOwnerObservation::PublicationObserved { reference, .. }
                if previous.references.publication() == Some(reference)
        )
}

fn validate_attempt_for_record(
    record: &WorkloadSagaRecord,
    attempt: &WorkloadProvisionAttempt,
) -> Result<(), WorkloadSagaError> {
    let intent = &record.active_intent;
    let phase_matches = attempt.source_phase() == record.phase
        || matches!(
            record.provision_disposition.as_ref(),
            Some(
                WorkloadProvisionDisposition::DispatchPending(claim)
                    | WorkloadProvisionDisposition::InspectionRequired(claim)
            ) if claim.attempt() == attempt
                && (owner_reopened_attachment_recovery_is_exact(record)
                    || owner_reopened_attachment_fence_is_exact(record))
        )
        || matches!(
            record.provision_disposition.as_ref(),
            Some(WorkloadProvisionDisposition::InspectionRequired(claim))
                if record.successor_intent.is_some()
                    && claim.attempt() == attempt
                    && attempt.target_phase() == record.phase
        );
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
        || !phase_matches
        || attempt.issuing_revision() >= record.revision
    {
        return Err(WorkloadSagaError::InvalidEvidence(
            "provision attempt is crossed with the durable workload generation",
        ));
    }
    let references = record.phase_detail.references();
    let current_execution = record.current_execution_reference();
    let subjects_match = match attempt.subjects() {
        WorkloadProvisionSubjects::Network(reference) => {
            reference == &WorkloadNetworkReference::for_intent(intent)
        }
        WorkloadProvisionSubjects::Execution(reference) => reference == &current_execution,
        WorkloadProvisionSubjects::Readiness { network, execution } => {
            network == &WorkloadNetworkReference::for_intent(intent)
                && execution == &current_execution
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
        && !activation_prerequisite_matches_intent(attempt, intent, &current_execution)
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
    current_execution: &WorkloadExecutionReference,
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
            && execution == current_execution
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
    let absence = match next.authorization() {
        WorkloadProvisionDispatchAuthorization::RetryAfterAbsence(absence) => absence,
        WorkloadProvisionDispatchAuthorization::RetryRepublishAfterAbsence(lineage) => {
            lineage.publication_absence()
        }
        _ => return false,
    };
    previous.attempt() == next.attempt()
        && previous.provider_target() == next.provider_target()
        && previous.dispatch_epoch().checked_next() == Some(next.dispatch_epoch())
        && next.claimed_revision() == resulting_revision
        && absence.matches_inspection(current, previous)
}

fn republish_claim_follows_observation_absence(
    current: &WorkloadSagaRecord,
    previous: &WorkloadProvisionDispatchClaim,
    next: &WorkloadProvisionDispatchClaim,
    resulting_revision: WorkloadSagaRevision,
) -> bool {
    let WorkloadProvisionDispatchAuthorization::RepublishAfterObservationAbsence(absence) =
        next.authorization()
    else {
        return false;
    };
    current.phase == WorkloadSagaPhase::Published
        && previous.attempt().step() == WorkloadProvisionStep::ObservePublication
        && next.attempt().step() == WorkloadProvisionStep::Publish
        && next.attempt().source_phase() == WorkloadSagaPhase::Published
        && next.attempt().target_phase() == WorkloadSagaPhase::Published
        && previous.provider_target() == next.provider_target()
        && previous.attempt().subjects() == next.attempt().subjects()
        && previous.dispatch_epoch().checked_next() == Some(next.dispatch_epoch())
        && next.attempt().issuing_revision() == current.revision
        && next.claimed_revision() == resulting_revision
        && absence.matches_inspection(current, previous)
}

fn reobservation_claim_follows_republication(
    current: &WorkloadSagaRecord,
    publication: &WorkloadProvisionDispatchClaim,
    observation: &WorkloadProvisionDispatchClaim,
    resulting_revision: WorkloadSagaRevision,
) -> bool {
    let lineage_matches = match (publication.authorization(), observation.authorization()) {
        (
            WorkloadProvisionDispatchAuthorization::RepublishAfterObservationAbsence(
                publication_absence,
            ),
            WorkloadProvisionDispatchAuthorization::ReobserveAfterRepublication(
                observation_absence,
            ),
        ) => publication_absence == observation_absence,
        (
            WorkloadProvisionDispatchAuthorization::RetryRepublishAfterAbsence(publication_lineage),
            WorkloadProvisionDispatchAuthorization::ReobserveAfterRetriedRepublication(
                observation_lineage,
            ),
        ) => publication_lineage == observation_lineage,
        _ => false,
    };
    lineage_matches
        && current.phase == WorkloadSagaPhase::Published
        && publication.attempt().step() == WorkloadProvisionStep::Publish
        && observation.attempt().step() == WorkloadProvisionStep::ObservePublication
        && publication.provider_target() == observation.provider_target()
        && publication.attempt().subjects() == observation.attempt().subjects()
        && publication.dispatch_epoch() == observation.dispatch_epoch()
        && observation.attempt().issuing_revision() == current.revision
        && observation.claimed_revision() == resulting_revision
}

pub(super) fn republication_evidence_refresh_is_exact(
    current: &WorkloadSagaRecord,
    candidate: &WorkloadSagaRecord,
) -> bool {
    let (
        Some(
            WorkloadProvisionDisposition::DispatchPending(publication)
            | WorkloadProvisionDisposition::InspectionRequired(publication),
        ),
        Some(WorkloadProvisionDisposition::DispatchPending(observation)),
    ) = (
        current.provision_disposition.as_ref(),
        candidate.provision_disposition.as_ref(),
    )
    else {
        return false;
    };
    if current.failure != candidate.failure
        || !reobservation_claim_follows_republication(
            current,
            publication,
            observation,
            candidate.revision,
        )
    {
        return false;
    }
    let (WorkloadPhaseDetail::Provision(previous), WorkloadPhaseDetail::Provision(next)) =
        (&current.phase_detail, &candidate.phase_detail)
    else {
        return false;
    };
    if previous.references != next.references {
        return false;
    }
    let (Some((previous_last, previous_prefix)), Some((next_last, next_prefix))) = (
        previous.observations.split_last(),
        next.observations.split_last(),
    ) else {
        return false;
    };
    previous_prefix == next_prefix
        && matches!(
            (previous_last, next_last),
            (
                WorkloadOwnerObservation::PublicationPresent {
                    reference: previous_reference,
                    ..
                },
                WorkloadOwnerObservation::PublicationPresent {
                    reference: next_reference,
                    ..
                },
            ) if previous_reference == next_reference
        )
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
        (Some(WorkloadProvisionDisposition::InspectionRequired(previous)), None)
            if candidate.phase == WorkloadSagaPhase::WithdrawalCommitted
                && previous.attempt().target_phase() == current.phase
                && candidate.teardown_disposition().is_some_and(|disposition| {
                    disposition.context().provision_absence().is_none()
                        && matches!(disposition.cause(), WorkloadTeardownCause::Successor { .. })
                }) =>
        {
            Ok(())
        }
        (Some(WorkloadProvisionDisposition::InspectionRequired(previous)), None)
            if current.phase == WorkloadSagaPhase::Published
                && candidate.phase == WorkloadSagaPhase::WithdrawalCommitted
                && previous.attempt().step() == WorkloadProvisionStep::ObservePublication
                && previous.attempt().source_phase() == WorkloadSagaPhase::Published
                && previous.attempt().target_phase() == WorkloadSagaPhase::Observed
                && candidate.teardown_disposition().is_some_and(|disposition| {
                    disposition.context().provision_absence().is_none()
                        && matches!(disposition.cause(), WorkloadTeardownCause::Successor { .. })
                }) =>
        {
            Ok(())
        }
        (Some(WorkloadProvisionDisposition::InspectionRequired(_)), None)
            if candidate.phase == WorkloadSagaPhase::WithdrawalCommitted
                && owner_reopened_attachment_fence_is_exact(current)
                && candidate.teardown_disposition().is_some_and(|disposition| {
                    disposition.context().provision_absence().is_none()
                        && matches!(disposition.cause(), WorkloadTeardownCause::Successor { .. })
                }) =>
        {
            Ok(())
        }
        (Some(WorkloadProvisionDisposition::InspectionRequired(previous)), None)
            if candidate.phase == WorkloadSagaPhase::WithdrawalCommitted
                && candidate.teardown_disposition().is_some_and(|disposition| {
                    disposition
                        .context()
                        .provision_absence()
                        .is_some_and(|absence| {
                            absence.claim() == previous
                                && absence.evidence().matches_inspection(current, previous)
                        })
                }) =>
        {
            Ok(())
        }
        (
            Some(WorkloadProvisionDisposition::DefiniteFailure {
                claim: previous_claim,
                failure: previous_failure,
            }),
            None,
        ) if candidate.phase == WorkloadSagaPhase::WithdrawalCommitted
            && matches!(
                candidate.teardown_disposition().map(WorkloadTeardownDisposition::cause),
                Some(WorkloadTeardownCause::FailedProvision { claim, failure })
                    if claim.as_ref() == previous_claim && failure == previous_failure
            ) =>
        {
            Ok(())
        }
        (Some(WorkloadProvisionDisposition::Ready), Some(WorkloadProvisionDisposition::Ready))
            if current.phase == WorkloadSagaPhase::Observed
                && candidate.phase == current.phase
                && current.restart_state() != candidate.restart_state() =>
        {
            Ok(())
        }
        (
            Some(WorkloadProvisionDisposition::Ready),
            Some(WorkloadProvisionDisposition::InspectionRequired(_)),
        ) if owner_reopened_publication_transition_is_exact(current, candidate) => Ok(()),
        (
            Some(
                WorkloadProvisionDisposition::DispatchPending(_)
                | WorkloadProvisionDisposition::InspectionRequired(_),
            ),
            Some(WorkloadProvisionDisposition::InspectionRequired(_)),
        ) if owner_reopened_publication_transition_is_exact(current, candidate) => Ok(()),
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
            Some(WorkloadProvisionDisposition::InspectionRequired(previous)),
            Some(WorkloadProvisionDisposition::InspectionRequired(next)),
        ) if candidate.phase == current.phase
            && previous == next
            && current.successor_intent != candidate.successor_intent =>
        {
            Ok(())
        }
        (
            Some(WorkloadProvisionDisposition::InspectionRequired(previous)),
            Some(WorkloadProvisionDisposition::InspectionRequired(next)),
        ) if candidate.phase == current.phase
            && previous == next
            && current.successor_intent.is_some()
            && matches!(
                previous.authorization(),
                WorkloadProvisionDispatchAuthorization::RepublishAfterObservationAbsence(_)
                    | WorkloadProvisionDispatchAuthorization::RetryRepublishAfterAbsence(_)
            ) =>
        {
            Ok(())
        }
        (
            Some(WorkloadProvisionDisposition::DefiniteFailure {
                claim: previous_claim,
                failure: previous_failure,
            }),
            Some(WorkloadProvisionDisposition::DefiniteFailure {
                claim: next_claim,
                failure: next_failure,
            }),
        ) if candidate.phase == current.phase
            && previous_claim == next_claim
            && previous_failure == next_failure
            && current.successor_intent != candidate.successor_intent =>
        {
            Ok(())
        }
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
            Some(WorkloadProvisionDisposition::InspectionRequired(next)),
        ) if candidate.phase == previous.attempt().target_phase()
            && candidate.phase != current.phase
            && current.successor_intent.is_some()
            && previous == next =>
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
            && republish_claim_follows_observation_absence(
                current,
                previous,
                next,
                candidate.revision,
            ) =>
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
            && reobservation_claim_follows_republication(
                current,
                previous,
                next,
                candidate.revision,
            ) =>
        {
            Ok(())
        }
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
