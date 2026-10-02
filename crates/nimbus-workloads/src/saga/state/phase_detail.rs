//! Phase graph and phase-detail invariants for workload-saga records.
//!
//! Each phase carries one exact detail shape. This module builds the initial
//! and promoted details, owns the legal phase edges, and validates each detail.

use super::*;

pub(super) fn initial_phase_detail(
    intent: &WorkloadSagaIntent,
) -> Result<(WorkloadSagaPhase, WorkloadPhaseDetail), WorkloadSagaError> {
    match intent.desired_state {
        DesiredWorkloadState::Running => Ok((
            WorkloadSagaPhase::IntentCommitted,
            WorkloadPhaseDetail::Intent,
        )),
        DesiredWorkloadState::Stopped => Ok((
            WorkloadSagaPhase::Recorded,
            WorkloadPhaseDetail::recorded(
                intent,
                WorkloadTerminalEvidenceDigest::for_observations(&[])?,
                None,
            ),
        )),
    }
}

pub(super) fn promoted_phase_detail(
    current: &WorkloadSagaRecord,
    intent: &WorkloadSagaIntent,
) -> Result<(WorkloadSagaPhase, WorkloadPhaseDetail), WorkloadSagaError> {
    if intent.desired_state != DesiredWorkloadState::Stopped {
        return initial_phase_detail(intent);
    }
    let WorkloadPhaseDetail::Recorded(previous) = &current.phase_detail else {
        return Err(WorkloadSagaError::InvalidTransition(
            "stopped successor promotion requires recorded predecessor evidence",
        ));
    };
    Ok((
        WorkloadSagaPhase::Recorded,
        WorkloadPhaseDetail::recorded(
            intent,
            previous.terminal_evidence_digest,
            previous.terminal_execution.clone(),
        ),
    ))
}

pub(super) fn legal_phase_edge(
    source: WorkloadSagaPhase,
    target: WorkloadSagaPhase,
    publication: WorkloadPublicationIntent,
) -> bool {
    if target == WorkloadSagaPhase::CleanupPending {
        return source != WorkloadSagaPhase::IntentCommitted
            && source != WorkloadSagaPhase::Recorded
            && source != WorkloadSagaPhase::CleanupPending;
    }
    if target == WorkloadSagaPhase::WithdrawalCommitted && source.is_provision() {
        return true;
    }
    matches!(
        (source, target),
        (
            WorkloadSagaPhase::IntentCommitted,
            WorkloadSagaPhase::NetworkReserved
        ) | (
            WorkloadSagaPhase::NetworkReserved,
            WorkloadSagaPhase::WorkloadPrepared
        ) | (
            WorkloadSagaPhase::WorkloadPrepared,
            WorkloadSagaPhase::NetworkAttached
        ) | (
            WorkloadSagaPhase::NetworkAttached,
            WorkloadSagaPhase::WorkloadActivated
        ) | (
            WorkloadSagaPhase::WorkloadActivated,
            WorkloadSagaPhase::Ready
        ) | (WorkloadSagaPhase::Ready, WorkloadSagaPhase::Published)
            | (WorkloadSagaPhase::Published, WorkloadSagaPhase::Observed)
            | (
                WorkloadSagaPhase::WithdrawalCommitted,
                WorkloadSagaPhase::Withdrawn
            )
            | (WorkloadSagaPhase::Withdrawn, WorkloadSagaPhase::Drained)
            | (
                WorkloadSagaPhase::Drained,
                WorkloadSagaPhase::WorkloadStopped
            )
            | (
                WorkloadSagaPhase::WorkloadStopped,
                WorkloadSagaPhase::NetworkDetached
            )
            | (
                WorkloadSagaPhase::NetworkDetached,
                WorkloadSagaPhase::NetworkReleased
            )
            | (
                WorkloadSagaPhase::NetworkReleased,
                WorkloadSagaPhase::Recorded
            )
    ) || (source == WorkloadSagaPhase::Ready
        && target == WorkloadSagaPhase::Observed
        && publication == WorkloadPublicationIntent::Withheld)
}

pub(in crate::saga) fn validate_phase_detail(
    phase: WorkloadSagaPhase,
    intent: &WorkloadSagaIntent,
    detail: &WorkloadPhaseDetail,
) -> Result<(), WorkloadSagaError> {
    match (phase, detail) {
        (WorkloadSagaPhase::IntentCommitted, WorkloadPhaseDetail::Intent)
            if intent.desired_state == DesiredWorkloadState::Running =>
        {
            Ok(())
        }
        (phase, WorkloadPhaseDetail::Provision(detail)) if phase.is_provision() => {
            validate_provision_detail(phase, intent, detail)
        }
        (phase, WorkloadPhaseDetail::Teardown(detail)) if phase.is_teardown() => {
            validate_teardown_detail(phase, intent, detail)
        }
        (WorkloadSagaPhase::CleanupPending, WorkloadPhaseDetail::CleanupPending(detail)) => {
            validate_cleanup_detail(intent, detail)
        }
        (WorkloadSagaPhase::Recorded, WorkloadPhaseDetail::Recorded(detail)) => {
            if detail.completed_generation != intent.generation
                || detail.desired_digest != intent.desired_digest
            {
                return Err(WorkloadSagaError::InvalidEvidence(
                    "recorded detail is crossed with another desired generation",
                ));
            }
            if let Some(execution) = &detail.terminal_execution {
                execution.validate_intrinsic()?;
                if execution.generation == intent.generation {
                    execution.validate_for(intent)?;
                } else if execution.generation > intent.generation
                    || intent.desired_state != DesiredWorkloadState::Stopped
                {
                    return Err(WorkloadSagaError::InvalidEvidence(
                        "recorded terminal execution is crossed or stale",
                    ));
                }
            }
            Ok(())
        }
        _ => Err(WorkloadSagaError::InvalidEvidence(
            "phase detail tag is not valid for the workload saga phase",
        )),
    }
}

fn validate_provision_detail(
    phase: WorkloadSagaPhase,
    intent: &WorkloadSagaIntent,
    detail: &WorkloadProvisionDetail,
) -> Result<(), WorkloadSagaError> {
    if matches!(phase, WorkloadSagaPhase::IntentCommitted) {
        return Err(WorkloadSagaError::InvalidEvidence(
            "intent committed cannot carry provision evidence",
        ));
    }
    if intent.desired_state != DesiredWorkloadState::Running {
        return Err(WorkloadSagaError::InvalidIntent(
            "stopped intent cannot enter provision",
        ));
    }
    if intent.activation == WorkloadActivationIntent::PrepareOnly
        && matches!(
            phase,
            WorkloadSagaPhase::WorkloadActivated
                | WorkloadSagaPhase::Ready
                | WorkloadSagaPhase::Published
                | WorkloadSagaPhase::Observed
        )
    {
        return Err(WorkloadSagaError::InvalidEvidence(
            "prepare-only intent cannot carry activated evidence",
        ));
    }
    detail.references.validate_for(intent)?;
    if detail.references.network.is_none() || detail.references.execution.is_none() {
        return Err(WorkloadSagaError::InvalidEvidence(
            "provision phase requires network and execution references",
        ));
    }

    let publication_required = matches!(
        phase,
        WorkloadSagaPhase::Ready | WorkloadSagaPhase::Published | WorkloadSagaPhase::Observed
    ) && (intent.publication
        == WorkloadPublicationIntent::PublishWhenReady
        || (intent.publication == WorkloadPublicationIntent::Withheld
            && intent
                .network()
                .compiled_plan()
                .content()
                .listeners()
                .is_empty()));
    if detail.references.publication.is_some() != publication_required {
        return Err(WorkloadSagaError::InvalidEvidence(
            "publication reference presence does not match phase and publication intent",
        ));
    }
    if phase == WorkloadSagaPhase::Published
        && intent.publication != WorkloadPublicationIntent::PublishWhenReady
    {
        return Err(WorkloadSagaError::InvalidEvidence(
            "published phase requires publish-when-ready intent",
        ));
    }

    let expected = expected_owner_observations(phase, intent.publication)?;
    if detail.observations.len() != expected.len()
        || detail
            .observations
            .iter()
            .zip(expected)
            .any(|(observation, expected)| {
                observation.kind() != expected || !observation.matches(&detail.references)
            })
    {
        return Err(WorkloadSagaError::InvalidEvidence(
            "provision observations are missing, extra, duplicated, crossed, or out of order",
        ));
    }
    Ok(())
}

fn expected_owner_observations(
    phase: WorkloadSagaPhase,
    publication: WorkloadPublicationIntent,
) -> Result<Vec<OwnerObservationKind>, WorkloadSagaError> {
    let mut expected = Vec::new();
    let rank = match phase {
        WorkloadSagaPhase::NetworkReserved => 1,
        WorkloadSagaPhase::WorkloadPrepared => 2,
        WorkloadSagaPhase::NetworkAttached => 3,
        WorkloadSagaPhase::WorkloadActivated => 4,
        WorkloadSagaPhase::Ready => 5,
        WorkloadSagaPhase::Published => 6,
        WorkloadSagaPhase::Observed => match publication {
            WorkloadPublicationIntent::Withheld => 5,
            WorkloadPublicationIntent::PublishWhenReady => 7,
        },
        _ => {
            return Err(WorkloadSagaError::InvalidEvidence(
                "phase has no provision observation matrix",
            ));
        }
    };
    if rank >= 1 {
        expected.push(OwnerObservationKind::NetworkReserved);
    }
    if rank >= 2 {
        expected.push(OwnerObservationKind::ExecutionPrepared);
    }
    if rank >= 3 {
        expected.push(OwnerObservationKind::NetworkAttached);
    }
    if rank >= 4 {
        expected.push(OwnerObservationKind::ExecutionActivated);
    }
    if rank >= 5 {
        expected.push(OwnerObservationKind::Ready);
    }
    if rank >= 6 {
        expected.push(OwnerObservationKind::PublicationPresent);
    }
    if rank >= 7 {
        expected.push(OwnerObservationKind::PublicationObserved);
    }
    Ok(expected)
}

fn validate_teardown_detail(
    phase: WorkloadSagaPhase,
    intent: &WorkloadSagaIntent,
    detail: &WorkloadTeardownDetail,
) -> Result<(), WorkloadSagaError> {
    if !detail.origin.is_provision() {
        return Err(WorkloadSagaError::InvalidEvidence(
            "teardown origin must be a provision phase",
        ));
    }
    detail.retained_references.validate_for(intent)?;
    validate_origin_references(detail.origin, intent, &detail.retained_references)?;
    let expected =
        expected_terminal_observations(phase, intent, detail.origin, &detail.retained_references);
    if detail.terminal_observations.len() != expected.len()
        || detail
            .terminal_observations
            .iter()
            .zip(expected)
            .any(|(observation, expected)| {
                observation.kind() != expected || !observation.matches(&detail.retained_references)
            })
    {
        return Err(WorkloadSagaError::InvalidEvidence(
            "teardown observations are missing, extra, duplicated, crossed, or out of order",
        ));
    }
    Ok(())
}

fn validate_origin_references(
    origin: WorkloadSagaPhase,
    intent: &WorkloadSagaIntent,
    references: &WorkloadEffectReferences,
) -> Result<(), WorkloadSagaError> {
    if origin == WorkloadSagaPhase::IntentCommitted {
        return if references.is_empty() {
            Ok(())
        } else {
            Err(WorkloadSagaError::InvalidEvidence(
                "intent origin cannot retain effect references",
            ))
        };
    }
    if references.network.is_none() || references.execution.is_none() {
        return Err(WorkloadSagaError::InvalidEvidence(
            "effect-bearing origin must retain network and execution references",
        ));
    }
    let publication_required = matches!(
        origin,
        WorkloadSagaPhase::Ready | WorkloadSagaPhase::Published | WorkloadSagaPhase::Observed
    ) && (intent.publication
        == WorkloadPublicationIntent::PublishWhenReady
        || (intent.publication == WorkloadPublicationIntent::Withheld
            && intent
                .network()
                .compiled_plan()
                .content()
                .listeners()
                .is_empty()));
    if references.publication.is_some() != publication_required {
        return Err(WorkloadSagaError::InvalidEvidence(
            "teardown retained publication reference does not match its origin",
        ));
    }
    Ok(())
}

fn expected_terminal_observations(
    phase: WorkloadSagaPhase,
    intent: &WorkloadSagaIntent,
    origin: WorkloadSagaPhase,
    references: &WorkloadEffectReferences,
) -> Vec<TerminalObservationKind> {
    let rank = match phase {
        WorkloadSagaPhase::WithdrawalCommitted => 0,
        WorkloadSagaPhase::Withdrawn => 1,
        WorkloadSagaPhase::Drained => 2,
        WorkloadSagaPhase::WorkloadStopped => 3,
        WorkloadSagaPhase::NetworkDetached => 4,
        WorkloadSagaPhase::NetworkReleased => 5,
        _ => 0,
    };
    let origin_rank = origin.recovery_order();
    let provider_managed_network = intent
        .network()
        .compiled_plan()
        .content()
        .capability_selection_evidence()
        .is_some();
    let mut expected = Vec::new();
    if rank >= 1
        && references.publication.is_some()
        && provider_managed_network
        && origin_rank >= WorkloadSagaPhase::Published.recovery_order()
    {
        expected.push(TerminalObservationKind::PublicationAbsent);
    }
    if rank >= 2
        && references.execution.is_some()
        && origin_rank >= WorkloadSagaPhase::WorkloadActivated.recovery_order()
    {
        expected.push(TerminalObservationKind::ExecutionDrained);
    }
    if rank >= 3
        && references.execution.is_some()
        && origin_rank >= WorkloadSagaPhase::WorkloadPrepared.recovery_order()
    {
        expected.push(TerminalObservationKind::ExecutionStopped);
    }
    if rank >= 4
        && references.network.is_some()
        && provider_managed_network
        && origin_rank >= WorkloadSagaPhase::NetworkAttached.recovery_order()
    {
        expected.push(TerminalObservationKind::NetworkDetached);
    }
    if rank >= 5
        && references.network.is_some()
        && provider_managed_network
        && origin_rank >= WorkloadSagaPhase::NetworkReserved.recovery_order()
    {
        expected.push(TerminalObservationKind::NetworkReleased);
    }
    expected
}

fn validate_cleanup_detail(
    intent: &WorkloadSagaIntent,
    detail: &WorkloadCleanupPendingDetail,
) -> Result<(), WorkloadSagaError> {
    if matches!(
        detail.last_safe_phase,
        WorkloadSagaPhase::IntentCommitted
            | WorkloadSagaPhase::Recorded
            | WorkloadSagaPhase::CleanupPending
    ) || detail.retained_references.is_empty()
    {
        return Err(WorkloadSagaError::InvalidEvidence(
            "cleanup pending requires an effect-bearing last-safe phase and retained reference",
        ));
    }
    detail.retained_references.validate_for(intent)?;
    if detail.inspections.len() != detail.retained_references.len()
        || detail
            .inspections
            .iter()
            .enumerate()
            .any(|(index, inspection)| {
                !inspection.matches_index(index, &detail.retained_references)
                    || match inspection {
                        WorkloadInspectionRequirement::Network { expected_phase, .. }
                        | WorkloadInspectionRequirement::Execution { expected_phase, .. }
                        | WorkloadInspectionRequirement::Publication { expected_phase, .. } => {
                            *expected_phase != detail.last_safe_phase
                        }
                    }
            })
    {
        return Err(WorkloadSagaError::InvalidEvidence(
            "cleanup inspection set must match every retained subject exactly once in N/E/P order",
        ));
    }
    Ok(())
}
