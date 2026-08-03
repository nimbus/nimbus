//! Behavioral proofs for the pure provision reducer.

use nimbus_workloads::{
    WorkloadActivationIntent, WorkloadFailureEvidence, WorkloadOwnerEvidenceDigest,
    WorkloadPhaseDetail, WorkloadProvisionDisposition, WorkloadProvisionEffectResult,
    WorkloadProvisionStep, WorkloadProvisionSubjects, WorkloadProvisionSuccessEvidence,
    WorkloadPublicationIntent, WorkloadSagaPhase, WorkloadSagaRecord,
};

use super::*;
use crate::workload_saga::recovery::tests::provision_record;

fn record(phase: WorkloadSagaPhase) -> WorkloadSagaRecord {
    provision_record(
        &format!("decision-{phase:?}").to_ascii_lowercase(),
        phase,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
    )
}

fn proposed(record: &WorkloadSagaRecord) -> ProposedWorkloadProvisionTransition {
    let WorkloadProvisionDecision::Proposed(proposed) =
        WorkloadProvisionDecision::plan(record).expect("phase should produce a proposal")
    else {
        panic!("phase should produce a proposal");
    };
    proposed
}

fn pending_attempt(record: &WorkloadSagaRecord) -> &WorkloadProvisionAttempt {
    let Some(WorkloadProvisionDisposition::AttemptPending(attempt)) =
        record.provision_disposition()
    else {
        panic!("candidate should retain an exact pending attempt");
    };
    attempt
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
        _ => panic!("attempt step and typed subject are correlated"),
    }
}

fn failure_for(attempt: &WorkloadProvisionAttempt) -> WorkloadFailureEvidence {
    WorkloadFailureEvidence::new(
        format!("{:?}_failed", attempt.step()).to_ascii_lowercase(),
        WorkloadOwnerEvidenceDigest::sha256(format!("{:?}-failed", attempt.step())),
    )
    .expect("fixture failure should validate")
}

fn assert_effect_result_matrix(
    pending: &WorkloadSagaRecord,
) -> ProposedWorkloadProvisionTransition {
    let attempt = pending_attempt(pending).clone();
    assert_eq!(
        WorkloadProvisionDecision::plan(pending).expect("pending state should reopen"),
        WorkloadProvisionDecision::InspectExact(Box::new(attempt.clone())),
        "a process reopening a pending attempt may only inspect it"
    );

    let WorkloadProvisionDecision::Proposed(inspection) = WorkloadProvisionDecision::reduce(
        pending,
        WorkloadProvisionEffectResult::Ambiguous {
            attempt_id: attempt.attempt_id().clone(),
        },
    )
    .expect("ambiguity should persist exact inspection state") else {
        panic!("first ambiguity should propose one durable inspection state");
    };
    assert_eq!(inspection.candidate().phase(), pending.phase());
    assert_eq!(
        inspection.candidate().revision().as_u64(),
        pending.revision().as_u64() + 1
    );
    assert_eq!(
        inspection.action_after_confirmation(),
        Some(WorkloadProvisionSymbolicAction::InspectExactAttempt)
    );
    assert!(matches!(
        inspection.candidate().provision_disposition(),
        Some(WorkloadProvisionDisposition::InspectionRequired(retained)) if retained == &attempt
    ));
    assert_eq!(
        WorkloadProvisionDecision::plan(inspection.candidate())
            .expect("inspection state should reopen"),
        WorkloadProvisionDecision::InspectExact(Box::new(attempt.clone()))
    );
    assert_eq!(
        WorkloadProvisionDecision::reduce(
            inspection.candidate(),
            WorkloadProvisionEffectResult::Ambiguous {
                attempt_id: attempt.attempt_id().clone(),
            },
        )
        .expect("repeated ambiguity should remain inspect-only"),
        WorkloadProvisionDecision::InspectExact(Box::new(attempt.clone()))
    );

    for unresolved in [pending, inspection.candidate()] {
        let WorkloadProvisionDecision::Proposed(failed) = WorkloadProvisionDecision::reduce(
            unresolved,
            WorkloadProvisionEffectResult::DefiniteFailure {
                attempt_id: attempt.attempt_id().clone(),
                failure: failure_for(&attempt),
            },
        )
        .expect("definite failure should persist") else {
            panic!("definite failure should propose one durable terminal disposition");
        };
        assert_eq!(failed.candidate().phase(), pending.phase());
        assert_eq!(
            failed.candidate().revision().as_u64(),
            unresolved.revision().as_u64() + 1
        );
        assert!(failed.action_after_confirmation().is_none());
        assert!(!failed.candidate().requires_recovery());
        assert_eq!(
            WorkloadProvisionDecision::plan(failed.candidate())
                .expect("definite failure should reopen"),
            WorkloadProvisionDecision::DefiniteFailure,
            "a halted generation must never emit a later provision action"
        );
    }

    let WorkloadProvisionDecision::Proposed(succeeded) = WorkloadProvisionDecision::reduce(
        pending,
        WorkloadProvisionEffectResult::Succeeded {
            attempt_id: attempt.attempt_id().clone(),
            evidence: success_for(&attempt),
        },
    )
    .expect("exact success should reduce") else {
        panic!("exact success should propose one durable candidate");
    };
    assert_eq!(
        succeeded.candidate().revision().as_u64(),
        pending.revision().as_u64() + 1
    );
    if attempt.step() == WorkloadProvisionStep::InspectActivationPrerequisites {
        let activation = pending_attempt(succeeded.candidate());
        assert_eq!(succeeded.candidate().phase(), pending.phase());
        assert_eq!(activation.step(), WorkloadProvisionStep::ActivateWorkload);
        assert_eq!(
            activation
                .prerequisite()
                .expect("activation retains prerequisite evidence")
                .attempt_id(),
            attempt.attempt_id()
        );
        assert_eq!(
            succeeded.action_after_confirmation(),
            Some(WorkloadProvisionSymbolicAction::StartExactAttempt)
        );
    } else {
        assert_eq!(succeeded.candidate().phase(), attempt.target_phase());
        assert_eq!(
            succeeded.candidate().provision_disposition(),
            Some(&WorkloadProvisionDisposition::Ready)
        );
        assert!(succeeded.action_after_confirmation().is_none());
    }
    succeeded
}

#[test]
fn activation_prerequisite_success_prepares_activation_attempt() {
    let attached = record(WorkloadSagaPhase::NetworkAttached);
    let prerequisite_candidate = proposed(&attached).into_candidate();
    let prerequisite_attempt = pending_attempt(&prerequisite_candidate).clone();
    assert_eq!(
        prerequisite_attempt.step(),
        WorkloadProvisionStep::InspectActivationPrerequisites
    );

    let WorkloadProvisionDecision::Proposed(activation) = WorkloadProvisionDecision::reduce(
        &prerequisite_candidate,
        WorkloadProvisionEffectResult::Succeeded {
            attempt_id: prerequisite_attempt.attempt_id().clone(),
            evidence: success_for(&prerequisite_attempt),
        },
    )
    .expect("prerequisite success should propose activation") else {
        panic!("prerequisite success should produce one activation proposal");
    };
    let activation_attempt = pending_attempt(activation.candidate());
    assert_eq!(
        activation.candidate().phase(),
        WorkloadSagaPhase::NetworkAttached
    );
    assert_eq!(
        activation_attempt.step(),
        WorkloadProvisionStep::ActivateWorkload
    );
    assert_eq!(
        activation_attempt
            .prerequisite()
            .expect("activation retains prerequisite evidence")
            .attempt_id(),
        prerequisite_attempt.attempt_id()
    );
}

#[test]
fn definite_failure_retains_completed_phase() {
    let completed = record(WorkloadSagaPhase::WorkloadPrepared);
    let candidate = proposed(&completed).into_candidate();
    let attempt = pending_attempt(&candidate).clone();
    let failure = WorkloadFailureEvidence::new(
        "attach_failed",
        WorkloadOwnerEvidenceDigest::sha256("attach-failed"),
    )
    .expect("failure should validate");
    let WorkloadProvisionDecision::Proposed(failed) = WorkloadProvisionDecision::reduce(
        &candidate,
        WorkloadProvisionEffectResult::DefiniteFailure {
            attempt_id: attempt.attempt_id().clone(),
            failure,
        },
    )
    .expect("failure should be retained") else {
        panic!("failure should produce one durable candidate");
    };

    assert_eq!(failed.candidate().phase(), completed.phase());
    assert!(!failed.candidate().requires_recovery());
    assert_eq!(
        WorkloadProvisionDecision::plan(failed.candidate()).expect("failed state should reopen"),
        WorkloadProvisionDecision::DefiniteFailure,
        "definite failure permits no later provision command"
    );
}

#[test]
fn ambiguous_result_requires_exact_inspection() {
    let candidate = proposed(&record(WorkloadSagaPhase::IntentCommitted)).into_candidate();
    let attempt = pending_attempt(&candidate).clone();
    let WorkloadProvisionDecision::Proposed(inspection) = WorkloadProvisionDecision::reduce(
        &candidate,
        WorkloadProvisionEffectResult::Ambiguous {
            attempt_id: attempt.attempt_id().clone(),
        },
    )
    .expect("ambiguity should become durable inspection state") else {
        panic!("first ambiguity should produce one inspection candidate");
    };
    assert_eq!(
        inspection.action_after_confirmation(),
        Some(WorkloadProvisionSymbolicAction::InspectExactAttempt)
    );
    assert_eq!(
        WorkloadProvisionDecision::plan(inspection.candidate())
            .expect("ambiguous state should reopen"),
        WorkloadProvisionDecision::InspectExact(Box::new(attempt))
    );
}

#[test]
fn crossed_attempt_id_rejects_without_candidate_or_command() {
    let first = proposed(&record(WorkloadSagaPhase::IntentCommitted)).into_candidate();
    let second = proposed(&provision_record(
        "crossed-attempt",
        WorkloadSagaPhase::IntentCommitted,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::Withheld,
    ))
    .into_candidate();
    let crossed_id = pending_attempt(&second).attempt_id().clone();
    assert!(matches!(
        WorkloadProvisionDecision::reduce(
            &first,
            WorkloadProvisionEffectResult::Ambiguous {
                attempt_id: crossed_id,
            },
        ),
        Err(WorkloadSagaError::InvalidEvidence(_))
    ));
}

#[test]
fn wrong_success_evidence_rejects_without_state_change() {
    let candidate = proposed(&record(WorkloadSagaPhase::IntentCommitted)).into_candidate();
    let attempt = pending_attempt(&candidate);
    let WorkloadProvisionSubjects::Network(reference) = attempt.subjects() else {
        panic!("reserve attempt should carry network subject");
    };
    let wrong = WorkloadProvisionSuccessEvidence::NetworkAttached {
        reference: reference.clone(),
        evidence: WorkloadOwnerEvidenceDigest::sha256("wrong-step"),
    };
    assert!(matches!(
        WorkloadProvisionDecision::reduce(
            &candidate,
            WorkloadProvisionEffectResult::Succeeded {
                attempt_id: attempt.attempt_id().clone(),
                evidence: wrong,
            },
        ),
        Err(WorkloadSagaError::InvalidEvidence(_))
    ));
}

#[test]
fn publication_requires_workload_readiness() {
    for phase in [
        WorkloadSagaPhase::IntentCommitted,
        WorkloadSagaPhase::NetworkReserved,
        WorkloadSagaPhase::WorkloadPrepared,
        WorkloadSagaPhase::NetworkAttached,
        WorkloadSagaPhase::WorkloadActivated,
    ] {
        let proposal = proposed(&record(phase));
        assert_ne!(
            pending_attempt(proposal.candidate()).step(),
            WorkloadProvisionStep::Publish,
            "publication is unreachable before workload readiness"
        );
    }
    let ready = record(WorkloadSagaPhase::Ready);
    assert_eq!(
        pending_attempt(proposed(&ready).candidate()).step(),
        WorkloadProvisionStep::Publish
    );
}

#[test]
fn definite_failure_reopen_emits_no_later_command() {
    definite_failure_retains_completed_phase();
}

#[test]
fn ambiguous_result_emits_exact_inspection_only() {
    ambiguous_result_requires_exact_inspection();
}

#[test]
fn ambiguous_reopen_retains_exact_attempt_correlation() {
    ambiguous_result_requires_exact_inspection();
}

#[test]
fn publication_is_unreachable_before_workload_readiness() {
    publication_requires_workload_readiness();
}

#[test]
fn resource_free_attempt_has_no_selection_evidence() {
    let candidate = proposed(&provision_record(
        "decision-resource-free",
        WorkloadSagaPhase::IntentCommitted,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::Withheld,
    ))
    .into_candidate();
    assert!(pending_attempt(&candidate).selection_evidence().is_none());
}

#[test]
fn every_provision_phase_and_result_is_exhaustive() {
    let cases = [
        (
            WorkloadSagaPhase::IntentCommitted,
            WorkloadProvisionStep::ReserveNetwork,
            WorkloadSagaPhase::NetworkReserved,
        ),
        (
            WorkloadSagaPhase::NetworkReserved,
            WorkloadProvisionStep::PrepareWorkload,
            WorkloadSagaPhase::WorkloadPrepared,
        ),
        (
            WorkloadSagaPhase::WorkloadPrepared,
            WorkloadProvisionStep::AttachNetwork,
            WorkloadSagaPhase::NetworkAttached,
        ),
        (
            WorkloadSagaPhase::NetworkAttached,
            WorkloadProvisionStep::InspectActivationPrerequisites,
            WorkloadSagaPhase::NetworkAttached,
        ),
        (
            WorkloadSagaPhase::WorkloadActivated,
            WorkloadProvisionStep::InspectWorkloadReadiness,
            WorkloadSagaPhase::Ready,
        ),
        (
            WorkloadSagaPhase::Ready,
            WorkloadProvisionStep::Publish,
            WorkloadSagaPhase::Published,
        ),
        (
            WorkloadSagaPhase::Published,
            WorkloadProvisionStep::ObservePublication,
            WorkloadSagaPhase::Observed,
        ),
    ];
    let mut observed_steps = Vec::new();
    for (phase, expected_step, expected_target) in cases {
        let current = record(phase);
        let proposal = proposed(&current);
        assert_eq!(proposal.candidate().phase(), phase);
        assert_eq!(
            proposal.candidate().revision().as_u64(),
            current.revision().as_u64() + 1
        );
        assert_eq!(
            proposal.action_after_confirmation(),
            Some(WorkloadProvisionSymbolicAction::StartExactAttempt)
        );
        let attempt = pending_attempt(proposal.candidate());
        assert_eq!(attempt.step(), expected_step);
        assert_eq!(attempt.source_phase(), phase);
        assert_eq!(attempt.target_phase(), expected_target);
        assert_eq!(attempt.issuing_revision(), current.revision());
        assert_eq!(attempt.key(), current.key());
        assert_eq!(attempt.saga_id(), current.saga_id());
        assert_eq!(
            attempt.required_node(),
            current.active_intent().admission().assigned_node()
        );
        assert_eq!(
            attempt.selection_evidence(),
            current
                .active_intent()
                .network()
                .compiled_plan()
                .content()
                .capability_selection_evidence()
        );
        observed_steps.push(expected_step);

        let succeeded = assert_effect_result_matrix(proposal.candidate());
        if expected_step == WorkloadProvisionStep::InspectActivationPrerequisites {
            observed_steps.push(WorkloadProvisionStep::ActivateWorkload);
            let activated = assert_effect_result_matrix(succeeded.candidate());
            assert_eq!(
                activated.candidate().phase(),
                WorkloadSagaPhase::WorkloadActivated
            );
        }
    }

    let expected_steps = [
        WorkloadProvisionStep::ReserveNetwork,
        WorkloadProvisionStep::PrepareWorkload,
        WorkloadProvisionStep::AttachNetwork,
        WorkloadProvisionStep::InspectActivationPrerequisites,
        WorkloadProvisionStep::ActivateWorkload,
        WorkloadProvisionStep::InspectWorkloadReadiness,
        WorkloadProvisionStep::Publish,
        WorkloadProvisionStep::ObservePublication,
    ];
    assert_eq!(observed_steps.len(), expected_steps.len());
    for expected in expected_steps {
        assert!(observed_steps.contains(&expected), "missing {expected:?}");
    }

    let prepare_only = provision_record(
        "decision-prepare-only",
        WorkloadSagaPhase::NetworkAttached,
        WorkloadActivationIntent::PrepareOnly,
        WorkloadPublicationIntent::Withheld,
    );
    assert_eq!(
        WorkloadProvisionDecision::plan(&prepare_only).expect("prepare-only should reduce"),
        WorkloadProvisionDecision::Wait
    );

    let ready_withheld = provision_record(
        "decision-ready-withheld",
        WorkloadSagaPhase::Ready,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::Withheld,
    );
    let WorkloadProvisionDecision::Proposed(observed) =
        WorkloadProvisionDecision::plan(&ready_withheld)
            .expect("withheld publication should reduce")
    else {
        panic!("withheld publication should propose the pure observed edge");
    };
    assert_eq!(observed.candidate().phase(), WorkloadSagaPhase::Observed);
    assert_eq!(
        observed.candidate().provision_disposition(),
        Some(&WorkloadProvisionDisposition::Ready)
    );
    assert!(observed.action_after_confirmation().is_none());
    assert_eq!(
        WorkloadProvisionDecision::plan(observed.candidate())
            .expect("observed state should reduce"),
        WorkloadProvisionDecision::Wait
    );

    let arbitrary_proposal = proposed(&record(WorkloadSagaPhase::IntentCommitted));
    let arbitrary_attempt = pending_attempt(arbitrary_proposal.candidate());
    for quiescent in [&prepare_only, observed.candidate()] {
        assert!(
            WorkloadProvisionDecision::reduce(
                quiescent,
                WorkloadProvisionEffectResult::Ambiguous {
                    attempt_id: arbitrary_attempt.attempt_id().clone(),
                },
            )
            .is_err()
        );
    }
}

#[test]
fn crossed_same_variant_subject_rejects_without_candidate_or_command() {
    let first = proposed(&record(WorkloadSagaPhase::IntentCommitted)).into_candidate();
    let second = proposed(&provision_record(
        "decision-crossed-subject",
        WorkloadSagaPhase::IntentCommitted,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
    ))
    .into_candidate();
    let first_attempt = pending_attempt(&first);
    let crossed_evidence = success_for(pending_attempt(&second));
    assert_eq!(crossed_evidence.step(), first_attempt.step());
    assert!(matches!(
        WorkloadProvisionDecision::reduce(
            &first,
            WorkloadProvisionEffectResult::Succeeded {
                attempt_id: first_attempt.attempt_id().clone(),
                evidence: crossed_evidence,
            },
        ),
        Err(WorkloadSagaError::InvalidEvidence(_))
    ));
}

#[test]
fn publication_observed_success_retains_exact_durable_observation_evidence() {
    let published = record(WorkloadSagaPhase::Published);
    let pending = proposed(&published).into_candidate();
    let attempt = pending_attempt(&pending).clone();
    let WorkloadProvisionSubjects::Publication(reference) = attempt.subjects() else {
        panic!("publication observation should retain its exact publication reference");
    };
    let reference = reference.clone();
    let first_evidence = WorkloadOwnerEvidenceDigest::sha256("publication-observed-first");
    let second_evidence = WorkloadOwnerEvidenceDigest::sha256("publication-observed-second");
    let reduce = |evidence| {
        let WorkloadProvisionDecision::Proposed(proposed) = WorkloadProvisionDecision::reduce(
            &pending,
            WorkloadProvisionEffectResult::Succeeded {
                attempt_id: attempt.attempt_id().clone(),
                evidence: WorkloadProvisionSuccessEvidence::PublicationObserved {
                    reference: reference.clone(),
                    evidence,
                },
            },
        )
        .expect("publication observation should reduce") else {
            panic!("publication observation should propose an observed record");
        };
        proposed.into_candidate()
    };
    let first = reduce(first_evidence);
    let second = reduce(second_evidence);
    let WorkloadPhaseDetail::Provision(first_detail) = first.phase_detail() else {
        panic!("observed record should carry provision detail");
    };

    assert_eq!(first.phase(), WorkloadSagaPhase::Observed);
    assert_eq!(
        first_detail.observations().len(),
        match published.phase_detail() {
            WorkloadPhaseDetail::Provision(detail) => detail.observations().len() + 1,
            _ => panic!("published record should carry provision detail"),
        }
    );
    let retained = serde_json::to_value(
        first_detail
            .observations()
            .last()
            .expect("observed record should retain publication observation"),
    )
    .expect("observation should encode");
    assert_eq!(retained["kind"], "publication_observed");
    assert_eq!(
        retained["reference"],
        serde_json::to_value(reference).unwrap()
    );
    assert_eq!(retained["evidence"], first_evidence.to_string());
    assert_ne!(
        first.last_transition().transition_id(),
        second.last_transition().transition_id(),
        "publication observation evidence must bind durable transition identity"
    );
}

#[test]
fn ingress_and_recovery_delegate_to_same_provision_reducer() {
    let record = record(WorkloadSagaPhase::IntentCommitted);
    let expected = WorkloadProvisionDecision::plan(&record).expect("record should be reducible");
    let recovery = crate::workload_saga::WorkloadSagaDecision::for_record(&record)
        .expect("recovery should delegate");
    assert_eq!(
        recovery.action(),
        &crate::workload_saga::WorkloadSagaAction::Provision(expected)
    );
}
