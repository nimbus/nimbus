//! Test-only histories driven through the real pure provision reducer.

use nimbus_workloads::{
    WorkloadOwnerEvidenceDigest, WorkloadProvisionAttempt, WorkloadProvisionDisposition,
    WorkloadProvisionEffectResult, WorkloadProvisionStep, WorkloadProvisionSubjects,
    WorkloadProvisionSuccessEvidence, WorkloadSagaRecord,
};

use super::WorkloadProvisionDecision;

pub(crate) fn success_for(attempt: &WorkloadProvisionAttempt) -> WorkloadProvisionSuccessEvidence {
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

pub(crate) fn provision_candidates(record: &WorkloadSagaRecord) -> Vec<WorkloadSagaRecord> {
    let WorkloadProvisionDecision::Proposed(proposed) =
        WorkloadProvisionDecision::plan(record).expect("fixture phase should be reducible")
    else {
        panic!("fixture phase should produce a provision proposal");
    };
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
        candidate = proposed.into_candidate();
        candidates.push(candidate.clone());
    }
    candidates
}

pub(crate) fn confirmed_provision(record: &WorkloadSagaRecord) -> WorkloadSagaRecord {
    provision_candidates(record)
        .pop()
        .expect("fixture provision decision should produce a candidate")
}

pub(crate) fn first_proposed_candidate(record: &WorkloadSagaRecord) -> WorkloadSagaRecord {
    provision_candidates(record)
        .into_iter()
        .next()
        .expect("fixture provision decision should produce a first candidate")
}
