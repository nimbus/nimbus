//! Test-only histories driven through the real pure provision reducer.

use nimbus_compute::workload_saga::{WorkloadProvisionDecision, WorkloadProvisionSymbolicAction};
use nimbus_workloads::{
    WorkloadOwnerEvidenceDigest, WorkloadProvisionAttempt, WorkloadProvisionDisposition,
    WorkloadProvisionEffectResult, WorkloadProvisionStep, WorkloadProvisionSubjects,
    WorkloadProvisionSuccessEvidence, WorkloadPublicationIntent, WorkloadSagaPhase,
    WorkloadSagaRecord,
};

fn assert_closed_fixture_action(
    source: &WorkloadSagaRecord,
    candidate: &WorkloadSagaRecord,
    action: Option<WorkloadProvisionSymbolicAction>,
) {
    match action {
        Some(WorkloadProvisionSymbolicAction::StartExactAttempt) => assert!(
            matches!(
                candidate.provision_disposition(),
                Some(WorkloadProvisionDisposition::DispatchPending(_))
            ),
            "a start action must pair with one exact pending dispatch claim"
        ),
        Some(WorkloadProvisionSymbolicAction::InspectExactAttempt) => {
            panic!("a deterministic success fixture must not enter ambiguous inspection")
        }
        None => {
            assert_eq!(
                candidate.provision_disposition(),
                Some(&WorkloadProvisionDisposition::Ready),
                "an action-free completion must carry exact ready disposition"
            );
            if let Some(
                WorkloadProvisionDisposition::DispatchPending(claim)
                | WorkloadProvisionDisposition::InspectionRequired(claim),
            ) = source.provision_disposition()
            {
                assert_eq!(
                    candidate.phase(),
                    claim.attempt().target_phase(),
                    "a confirmed provider result must advance to its exact attempted target"
                );
                return;
            }
            let resource_free_network_step = source
                .active_intent()
                .network()
                .compiled_plan()
                .content()
                .capability_selection_evidence()
                .is_none()
                && matches!(
                    (source.phase(), candidate.phase()),
                    (
                        WorkloadSagaPhase::IntentCommitted,
                        WorkloadSagaPhase::NetworkReserved
                    ) | (
                        WorkloadSagaPhase::WorkloadPrepared,
                        WorkloadSagaPhase::NetworkAttached
                    )
                );
            let withheld_observation = source.phase() == WorkloadSagaPhase::Ready
                && candidate.phase() == WorkloadSagaPhase::Observed
                && source.active_intent().publication() == WorkloadPublicationIntent::Withheld;
            assert!(
                resource_free_network_step || withheld_observation,
                "an action-free planned transition must be resource-free reserve/attach or withheld Ready-to-Observed"
            );
        }
    }
}

fn success_for(attempt: &WorkloadProvisionAttempt) -> WorkloadProvisionSuccessEvidence {
    let evidence = WorkloadOwnerEvidenceDigest::sha256(format!("{:?}", attempt.step()));
    match (attempt.step(), attempt.subjects()) {
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
        _ => panic!("fixture attempt step and typed subject must remain correlated"),
    }
}

pub(super) fn provision_candidates(record: &WorkloadSagaRecord) -> Vec<WorkloadSagaRecord> {
    let WorkloadProvisionDecision::Proposed(proposed) =
        WorkloadProvisionDecision::plan(record).expect("fixture phase should be reducible")
    else {
        panic!("fixture phase should produce a provision proposal");
    };
    assert_closed_fixture_action(
        record,
        proposed.candidate(),
        proposed.action_after_confirmation(),
    );
    let mut candidate = proposed.into_candidate();
    let mut candidates = vec![candidate.clone()];
    while let Some(WorkloadProvisionDisposition::DispatchPending(claim)) =
        candidate.provision_disposition()
    {
        let attempt = claim.attempt();
        let result = WorkloadProvisionEffectResult::Succeeded {
            attempt_id: attempt.attempt_id().clone(),
            evidence: success_for(attempt),
        };
        let WorkloadProvisionDecision::Proposed(proposed) =
            WorkloadProvisionDecision::reduce(&candidate, result)
                .expect("fixture success should reduce")
        else {
            panic!("fixture success should produce a durable candidate");
        };
        assert_closed_fixture_action(
            &candidate,
            proposed.candidate(),
            proposed.action_after_confirmation(),
        );
        candidate = proposed.into_candidate();
        candidates.push(candidate.clone());
    }
    candidates
}

pub(super) fn first_proposed_candidate(record: &WorkloadSagaRecord) -> WorkloadSagaRecord {
    provision_candidates(record)
        .into_iter()
        .next()
        .expect("fixture provision decision should produce a first candidate")
}

pub(super) fn extend_confirmed_step(history: &mut Vec<WorkloadSagaRecord>) {
    let candidates = provision_candidates(
        history
            .last()
            .expect("fixture provision history must contain a current record"),
    );
    history.extend(candidates);
}
