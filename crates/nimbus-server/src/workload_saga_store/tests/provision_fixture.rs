//! Test-only histories driven through the real pure provision reducer.

use nimbus_compute::workload_saga::{WorkloadProvisionDecision, WorkloadProvisionSymbolicAction};
use nimbus_workloads::{
    WorkloadOwnerEvidenceDigest, WorkloadProvisionAttempt, WorkloadProvisionDisposition,
    WorkloadProvisionEffectResult, WorkloadProvisionStep, WorkloadProvisionSubjects,
    WorkloadProvisionSuccessEvidence, WorkloadSagaRecord,
};

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
    assert!(
        proposed.action_after_confirmation().is_some()
            || proposed.candidate().phase() == nimbus_workloads::WorkloadSagaPhase::Observed,
        "only the withheld Ready-to-Observed edge may omit a symbolic action"
    );
    let mut candidate = proposed.into_candidate();
    let mut candidates = vec![candidate.clone()];
    while let Some(WorkloadProvisionDisposition::AttemptPending(attempt)) =
        candidate.provision_disposition()
    {
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
        assert_ne!(
            proposed.action_after_confirmation(),
            Some(WorkloadProvisionSymbolicAction::InspectExactAttempt),
            "a deterministic success fixture must not enter ambiguous inspection"
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
