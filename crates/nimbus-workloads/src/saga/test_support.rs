//! Test-only builders for exact confirmed provision histories.

use super::*;

pub(crate) fn provision_attempt(
    record: &WorkloadSagaRecord,
    step: WorkloadProvisionStep,
    target_phase: WorkloadSagaPhase,
    subjects: WorkloadProvisionSubjects,
    prerequisite: Option<WorkloadProvisionPrerequisiteEvidence>,
) -> WorkloadProvisionAttempt {
    let intent = record.active_intent();
    WorkloadProvisionAttempt::new(WorkloadProvisionAttemptInput {
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
        subjects,
        prerequisite,
    })
    .expect("fixture provision attempt should validate")
}

pub(crate) fn persist_attempt(
    record: &WorkloadSagaRecord,
    attempt: WorkloadProvisionAttempt,
) -> WorkloadSagaRecord {
    let provider_target = WorkloadProvisionProviderTarget::for_attempt(&attempt)
        .expect("fixture provider target should validate")
        .expect("effectful fixture attempt requires a provider target");
    if attempt.step() == WorkloadProvisionStep::ActivateWorkload {
        record
            .dispatch_to_activation(attempt, provider_target)
            .expect("fixture activation claim should follow prerequisite inspection")
    } else {
        record
            .ready_to_initial_dispatch(attempt, provider_target)
            .expect("fixture attempt should persist before completion")
    }
}

pub(crate) fn provision_candidates(
    record: &WorkloadSagaRecord,
    target_phase: WorkloadSagaPhase,
    detail: WorkloadPhaseDetail,
) -> Vec<WorkloadSagaRecord> {
    if record.phase() == WorkloadSagaPhase::Ready
        && target_phase == WorkloadSagaPhase::Observed
        && record.active_intent().publication() == WorkloadPublicationIntent::Withheld
    {
        return vec![
            record
                .advance(target_phase, detail, None)
                .expect("withheld publication should observe without an effect"),
        ];
    }

    let intent = record.active_intent();
    let network = WorkloadNetworkReference::for_intent(intent);
    let execution = WorkloadExecutionReference::for_intent(intent);
    if record.phase() == WorkloadSagaPhase::NetworkAttached
        && target_phase == WorkloadSagaPhase::WorkloadActivated
    {
        let inspection = provision_attempt(
            record,
            WorkloadProvisionStep::InspectActivationPrerequisites,
            WorkloadSagaPhase::NetworkAttached,
            WorkloadProvisionSubjects::Readiness {
                network: network.clone(),
                execution: execution.clone(),
            },
            None,
        );
        let inspection_pending = persist_attempt(record, inspection.clone());
        let prerequisite = WorkloadProvisionPrerequisiteEvidence::new(
            inspection.attempt_id().clone(),
            WorkloadProvisionSuccessEvidence::ActivationPrerequisitesReady {
                network,
                execution: execution.clone(),
                evidence: WorkloadOwnerEvidenceDigest::sha256("activation-prerequisites-ready"),
            },
        )
        .expect("fixture prerequisite should validate");
        let activation = provision_attempt(
            &inspection_pending,
            WorkloadProvisionStep::ActivateWorkload,
            target_phase,
            WorkloadProvisionSubjects::Execution(execution),
            Some(prerequisite),
        );
        let activation_pending = persist_attempt(&inspection_pending, activation);
        let completed = activation_pending
            .dispatch_to_success(target_phase, detail)
            .expect("confirmed activation fixture should complete");
        return vec![inspection_pending, activation_pending, completed];
    }

    let (step, subjects) = match (record.phase(), target_phase) {
        (WorkloadSagaPhase::IntentCommitted, WorkloadSagaPhase::NetworkReserved) => (
            WorkloadProvisionStep::ReserveNetwork,
            WorkloadProvisionSubjects::Network(network),
        ),
        (WorkloadSagaPhase::NetworkReserved, WorkloadSagaPhase::WorkloadPrepared) => (
            WorkloadProvisionStep::PrepareWorkload,
            WorkloadProvisionSubjects::Execution(execution),
        ),
        (WorkloadSagaPhase::WorkloadPrepared, WorkloadSagaPhase::NetworkAttached) => (
            WorkloadProvisionStep::AttachNetwork,
            WorkloadProvisionSubjects::Network(network),
        ),
        (WorkloadSagaPhase::WorkloadActivated, WorkloadSagaPhase::Ready) => (
            WorkloadProvisionStep::InspectWorkloadReadiness,
            WorkloadProvisionSubjects::Readiness { network, execution },
        ),
        (WorkloadSagaPhase::Ready, WorkloadSagaPhase::Published) => (
            WorkloadProvisionStep::Publish,
            WorkloadProvisionSubjects::Publication(
                record
                    .phase_detail()
                    .references()
                    .publication()
                    .expect("ready publication fixture should retain a reference")
                    .clone(),
            ),
        ),
        (WorkloadSagaPhase::Published, WorkloadSagaPhase::Observed) => (
            WorkloadProvisionStep::ObservePublication,
            WorkloadProvisionSubjects::Publication(
                record
                    .phase_detail()
                    .references()
                    .publication()
                    .expect("published fixture should retain a reference")
                    .clone(),
            ),
        ),
        edge => panic!("unsupported confirmed provision fixture edge {edge:?}"),
    };
    let attempt = provision_attempt(record, step, target_phase, subjects, None);
    if WorkloadProvisionProviderTarget::for_attempt(&attempt)
        .expect("fixture provider target should validate")
        .is_none()
    {
        return vec![
            record
                .record_resource_free_network_step(step, target_phase, detail)
                .expect("resource-free network fixture should complete without a command"),
        ];
    }
    let pending = persist_attempt(record, attempt);
    let completed = pending
        .dispatch_to_success(target_phase, detail)
        .expect("confirmed provision fixture should complete");
    vec![pending, completed]
}

pub(crate) fn confirmed_provision(
    record: &WorkloadSagaRecord,
    target_phase: WorkloadSagaPhase,
    detail: WorkloadPhaseDetail,
) -> WorkloadSagaRecord {
    provision_candidates(record, target_phase, detail)
        .pop()
        .expect("a fixture provision edge should produce a candidate")
}
