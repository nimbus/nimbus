use super::*;

#[path = "teardown_state/handoff.rs"]
mod handoff;
#[path = "teardown_state/inspection.rs"]
mod inspection;
#[path = "teardown_state/receipt_prefix.rs"]
mod receipt_prefix;
#[path = "teardown_state/wire.rs"]
mod wire;

fn withdrawal_record(publication: WorkloadPublicationIntent) -> WorkloadSagaRecord {
    withdrawal_record_for(publication, 2)
}

fn withdrawal_record_for(
    publication: WorkloadPublicationIntent,
    successor_generation: u64,
) -> WorkloadSagaRecord {
    let active = established_record(publication);
    let WorkloadSagaIntentUpdate::Transition(record) = active
        .apply_intent(stopped_intent(successor_generation))
        .expect("a stopped successor should commit withdrawal")
    else {
        panic!("a stopped successor must change the durable record");
    };
    *record
}

fn established_record(publication: WorkloadPublicationIntent) -> WorkloadSagaRecord {
    let active = record_at_ready(publication);
    if publication == WorkloadPublicationIntent::PublishWhenReady {
        let publication = active
            .phase_detail()
            .references()
            .publication()
            .expect("publish-when-ready fixture should retain publication identity")
            .clone();
        advance_provision(&active, WorkloadSagaPhase::Published, Some(&publication))
    } else {
        active
    }
}

fn teardown_candidate(
    record: &WorkloadSagaRecord,
) -> (WorkloadTeardownAttempt, WorkloadTeardownProviderTarget) {
    let WorkloadTeardownDecision::PersistCandidate(ProposedWorkloadTeardownTransition::Claim {
        attempt,
        provider_target,
    }) = record
        .decide_teardown()
        .expect("teardown decision should validate")
    else {
        panic!("fixture state should propose an exact teardown claim");
    };
    (*attempt, provider_target)
}

fn claim_teardown_step(record: &WorkloadSagaRecord) -> (WorkloadSagaRecord, WorkloadTeardownClaim) {
    let (attempt, provider_target) = teardown_candidate(record);
    let claimed = record
        .claim_teardown(attempt, provider_target)
        .expect("exact teardown claim should persist");
    let claim = claimed
        .teardown_disposition()
        .and_then(WorkloadTeardownDisposition::claim)
        .expect("claimed teardown record should retain the claim")
        .clone();
    (claimed, claim)
}

fn teardown_success_evidence(
    claim: &WorkloadTeardownClaim,
    label: &str,
) -> WorkloadTeardownSuccessEvidence {
    let evidence = evidence(label);
    match (claim.attempt().step(), claim.attempt().subjects()) {
        (
            WorkloadTeardownStep::WithdrawPublication,
            WorkloadTeardownSubjects::Publication(reference),
        ) => WorkloadTeardownSuccessEvidence::PublicationAbsent {
            reference: reference.clone(),
            evidence,
        },
        (WorkloadTeardownStep::DrainExecution, WorkloadTeardownSubjects::Execution(reference)) => {
            WorkloadTeardownSuccessEvidence::ExecutionDrained {
                reference: reference.clone(),
                evidence,
            }
        }
        (WorkloadTeardownStep::StopExecution, WorkloadTeardownSubjects::Execution(reference)) => {
            WorkloadTeardownSuccessEvidence::ExecutionStopped {
                reference: reference.clone(),
                evidence,
            }
        }
        (WorkloadTeardownStep::DetachNetwork, WorkloadTeardownSubjects::Network(reference)) => {
            WorkloadTeardownSuccessEvidence::NetworkDetached {
                reference: reference.clone(),
                evidence,
            }
        }
        (WorkloadTeardownStep::ReleaseNetwork, WorkloadTeardownSubjects::Network(reference)) => {
            WorkloadTeardownSuccessEvidence::NetworkReleased {
                reference: reference.clone(),
                evidence,
            }
        }
        _ => panic!("validated teardown claim has a crossed step and subject"),
    }
}

fn teardown_success_result(
    claim: &WorkloadTeardownClaim,
    label: &str,
) -> WorkloadTeardownEffectResult {
    WorkloadTeardownEffectResult::Succeeded {
        attempt_id: claim.attempt().attempt_id().clone(),
        dispatch_epoch: claim.dispatch_epoch(),
        provider_target: claim.provider_target().clone(),
        evidence: Box::new(teardown_success_evidence(claim, label)),
    }
}

fn inspection_command_id(
    record: &WorkloadSagaRecord,
    claim: &WorkloadTeardownClaim,
) -> WorkloadTeardownCommandId {
    WorkloadTeardownCommandId::for_confirmed_dispatch(
        claim,
        record.revision(),
        record.last_transition().transition_id(),
        WorkloadTeardownCommandMode::Inspect,
    )
    .expect("inspection command identity should derive")
}

fn complete_effectful_teardown_step(
    record: &WorkloadSagaRecord,
    label: &str,
) -> WorkloadSagaRecord {
    let (claimed, claim) = claim_teardown_step(record);
    claimed
        .apply_teardown_effect_result(&claim, teardown_success_result(&claim, label))
        .expect("exact teardown success should advance one phase")
}

fn attempt_input(attempt: &WorkloadTeardownAttempt) -> WorkloadTeardownAttemptInput {
    WorkloadTeardownAttemptInput {
        key: attempt.key().clone(),
        saga_id: attempt.saga_id().clone(),
        issuing_revision: attempt.issuing_revision(),
        issuing_transition_id: attempt.issuing_transition_id().clone(),
        generation: attempt.generation(),
        desired_digest: attempt.desired_digest(),
        required_node: attempt.required_node().clone(),
        source_digest: attempt.source_digest(),
        execution_provider_id: attempt.execution_provider_id().clone(),
        network_plan_digest: attempt.network_plan_digest(),
        selection_evidence: attempt.selection_evidence().cloned(),
        cause: attempt.cause().clone(),
        successor_fence: attempt.successor_fence(),
        source_phase: attempt.source_phase(),
        target_phase: attempt.target_phase(),
        step: attempt.step(),
        subjects: attempt.subjects().clone(),
    }
}

fn failure(label: &str) -> WorkloadFailureEvidence {
    WorkloadFailureEvidence::new("provider_failed", evidence(label))
        .expect("fixture failure should validate")
}

#[test]
fn teardown_successor_commits_withdrawal_before_first_claim() {
    let active = record_at_ready(WorkloadPublicationIntent::PublishWhenReady);
    let successor = stopped_intent(2);
    let successor_generation = successor.generation();
    let successor_digest = successor.desired_digest();

    let WorkloadSagaIntentUpdate::Transition(withdrawal) = active
        .apply_intent(successor)
        .expect("a stopped successor should commit withdrawal")
    else {
        panic!("a stopped successor must produce one durable transition");
    };

    assert_eq!(withdrawal.phase(), WorkloadSagaPhase::WithdrawalCommitted);
    let disposition = withdrawal
        .teardown_disposition()
        .expect("withdrawal should retain exact teardown state");
    assert!(matches!(
        disposition,
        WorkloadTeardownDisposition::Ready { .. }
    ));
    assert!(matches!(
        disposition.cause(),
        WorkloadTeardownCause::Successor {
            generation,
            desired_digest,
        } if *generation == successor_generation && *desired_digest == successor_digest
    ));
    assert!(
        withdrawal
            .teardown_disposition()
            .and_then(WorkloadTeardownDisposition::claim)
            .is_none()
    );
    assert_eq!(
        withdrawal.last_transition().target_phase(),
        WorkloadSagaPhase::WithdrawalCommitted
    );
}

#[test]
fn teardown_claim_binds_complete_active_and_successor_identity() {
    let withdrawal = withdrawal_record(WorkloadPublicationIntent::PublishWhenReady);
    let issuing_transition = withdrawal.last_transition().transition_id().clone();
    let (attempt, target) = teardown_candidate(&withdrawal);

    assert_eq!(attempt.key(), withdrawal.key());
    assert_eq!(attempt.saga_id(), withdrawal.saga_id());
    assert_eq!(attempt.issuing_revision(), withdrawal.revision());
    assert_eq!(attempt.issuing_transition_id(), &issuing_transition);
    assert_eq!(
        attempt.generation(),
        withdrawal.active_intent().generation()
    );
    assert_eq!(
        attempt.desired_digest(),
        withdrawal.active_intent().desired_digest()
    );
    assert_eq!(
        attempt.required_node(),
        withdrawal.active_intent().admission().assigned_node()
    );
    assert_eq!(
        attempt.source_digest(),
        withdrawal.active_intent().source().source_digest()
    );
    assert_eq!(
        attempt.execution_provider_id(),
        withdrawal.active_intent().source().execution_provider_id()
    );
    assert_eq!(
        attempt.network_plan_digest(),
        withdrawal.active_intent().network().digest()
    );
    assert_eq!(
        attempt.source_phase(),
        WorkloadSagaPhase::WithdrawalCommitted
    );
    assert_eq!(attempt.target_phase(), WorkloadSagaPhase::Withdrawn);
    assert_eq!(attempt.step(), WorkloadTeardownStep::WithdrawPublication);
    assert_eq!(
        WorkloadTeardownProviderTarget::for_attempt(&attempt).unwrap(),
        Some(target.clone())
    );

    let claimed = withdrawal
        .claim_teardown(attempt.clone(), target)
        .expect("candidate should persist as a claim");
    let claim = claimed
        .teardown_disposition()
        .and_then(WorkloadTeardownDisposition::claim)
        .expect("claim should be durable");
    assert_eq!(claim.attempt(), &attempt);
    assert_eq!(claim.claimed_revision(), claimed.revision());
    assert_eq!(
        claim.dispatch_epoch(),
        WorkloadTeardownDispatchEpoch::new(0)
    );
    assert!(matches!(
        claim.authorization(),
        WorkloadTeardownDispatchAuthorization::Initial
    ));

    let command = WorkloadTeardownCommandId::for_confirmed_dispatch(
        claim,
        claimed.revision(),
        claimed.last_transition().transition_id(),
        WorkloadTeardownCommandMode::Execute,
    )
    .expect("confirmed command identity should derive");
    assert_eq!(
        command,
        WorkloadTeardownCommandId::for_confirmed_dispatch(
            claim,
            claimed.revision(),
            claimed.last_transition().transition_id(),
            WorkloadTeardownCommandMode::Execute,
        )
        .unwrap()
    );
    assert_ne!(
        command,
        WorkloadTeardownCommandId::for_confirmed_dispatch(
            claim,
            claimed.revision(),
            claimed.last_transition().transition_id(),
            WorkloadTeardownCommandMode::Inspect,
        )
        .unwrap()
    );
    assert_ne!(
        attempt.issuing_transition_id(),
        claimed.last_transition().transition_id()
    );
}

#[test]
fn teardown_happy_path_orders_withdraw_drain_stop_detach_release_record() {
    let mut record = withdrawal_record(WorkloadPublicationIntent::PublishWhenReady);
    let terminal_execution = record.current_execution_reference();
    let expected = [
        (
            WorkloadTeardownStep::WithdrawPublication,
            WorkloadSagaPhase::Withdrawn,
        ),
        (
            WorkloadTeardownStep::DrainExecution,
            WorkloadSagaPhase::Drained,
        ),
        (
            WorkloadTeardownStep::StopExecution,
            WorkloadSagaPhase::WorkloadStopped,
        ),
        (
            WorkloadTeardownStep::DetachNetwork,
            WorkloadSagaPhase::NetworkDetached,
        ),
        (
            WorkloadTeardownStep::ReleaseNetwork,
            WorkloadSagaPhase::NetworkReleased,
        ),
    ];

    for (index, (step, phase)) in expected.into_iter().enumerate() {
        let (attempt, _) = teardown_candidate(&record);
        assert_eq!(attempt.step(), step);
        record = complete_effectful_teardown_step(&record, &format!("success-{index}"));
        assert_eq!(record.phase(), phase);
        let completed = record
            .teardown_disposition()
            .expect("teardown state should remain durable")
            .context()
            .completed();
        assert_eq!(completed.len(), index + 1);
        assert_eq!(completed[index].claim().attempt().step(), step);
    }

    assert!(matches!(
        record.decide_teardown().unwrap(),
        WorkloadTeardownDecision::PersistCandidate(
            ProposedWorkloadTeardownTransition::RecordTerminal
        )
    ));
    let recorded = record
        .record_terminal_teardown()
        .expect("terminal evidence should record");
    assert_eq!(recorded.phase(), WorkloadSagaPhase::Recorded);
    assert!(recorded.teardown_disposition().is_none());
    assert_eq!(
        serde_json::to_value(&recorded).unwrap()["phaseDetail"]["value"]["terminalExecution"],
        serde_json::to_value(terminal_execution).unwrap(),
        "Recorded durable truth must retain the exact execution that teardown stopped"
    );
}

#[test]
fn resource_free_teardown_advances_without_claim_or_terminal_observation() {
    let active = WorkloadSagaRecord::new(
        key("tenant-a", "workload-a"),
        running_intent(1, WorkloadPublicationIntent::Withheld),
    )
    .expect("record should initialize");
    let WorkloadSagaIntentUpdate::Transition(record) = active
        .apply_intent(stopped_intent(2))
        .expect("successor should commit withdrawal")
    else {
        panic!("stopped successor should change the record");
    };
    let mut record = *record;

    for (step, phase) in [
        (
            WorkloadTeardownStep::WithdrawPublication,
            WorkloadSagaPhase::Withdrawn,
        ),
        (
            WorkloadTeardownStep::DrainExecution,
            WorkloadSagaPhase::Drained,
        ),
        (
            WorkloadTeardownStep::StopExecution,
            WorkloadSagaPhase::WorkloadStopped,
        ),
        (
            WorkloadTeardownStep::DetachNetwork,
            WorkloadSagaPhase::NetworkDetached,
        ),
        (
            WorkloadTeardownStep::ReleaseNetwork,
            WorkloadSagaPhase::NetworkReleased,
        ),
    ] {
        assert_eq!(
            record.decide_teardown().unwrap(),
            WorkloadTeardownDecision::PersistCandidate(
                ProposedWorkloadTeardownTransition::ResourceFree {
                    step,
                    target_phase: phase,
                }
            )
        );
        record = record
            .record_resource_free_teardown_step(step)
            .expect("resource-free step should advance");
        assert_eq!(record.phase(), phase);
        assert!(
            record
                .teardown_disposition()
                .expect("teardown state should remain durable")
                .context()
                .completed()
                .is_empty()
        );
        let WorkloadPhaseDetail::Teardown(detail) = record.phase_detail() else {
            panic!("teardown phase should retain teardown detail");
        };
        assert!(detail.terminal_observations().is_empty());
    }
}

#[test]
fn teardown_step_requires_exact_phase_and_subject() {
    let withdrawal = withdrawal_record(WorkloadPublicationIntent::PublishWhenReady);
    let (attempt, _) = teardown_candidate(&withdrawal);

    let mut wrong_phase = attempt_input(&attempt);
    wrong_phase.target_phase = WorkloadSagaPhase::Drained;
    assert!(matches!(
        WorkloadTeardownAttempt::new(wrong_phase),
        Err(WorkloadSagaError::InvalidTransition(_))
    ));

    let mut wrong_subject = attempt_input(&attempt);
    wrong_subject.subjects = WorkloadTeardownSubjects::Execution(
        withdrawal
            .phase_detail()
            .references()
            .execution()
            .expect("ready fixture retains execution")
            .clone(),
    );
    assert!(matches!(
        WorkloadTeardownAttempt::new(wrong_subject),
        Err(WorkloadSagaError::InvalidEvidence(_))
    ));
}

#[test]
fn teardown_provider_target_matches_exact_step_role() {
    let mut record = withdrawal_record(WorkloadPublicationIntent::PublishWhenReady);
    for expected_role in [
        Some(nimbus_network::NetworkCapabilityRole::Ingress),
        None,
        None,
        Some(nimbus_network::NetworkCapabilityRole::Attachment),
        Some(nimbus_network::NetworkCapabilityRole::Attachment),
    ] {
        let (attempt, target) = teardown_candidate(&record);
        assert_eq!(target.network_role(), expected_role);
        assert_eq!(
            WorkloadTeardownProviderTarget::for_attempt(&attempt).unwrap(),
            Some(target)
        );
        record = complete_effectful_teardown_step(&record, "provider-role");
    }
}

#[test]
fn teardown_counter_boundaries_round_trip_canonical_decimal() {
    for value in [0, TWO_TO_53 - 1, TWO_TO_53, u64::MAX] {
        let revision = WorkloadSagaRevision::new(value);
        let epoch = WorkloadTeardownDispatchEpoch::new(value);
        assert_eq!(
            serde_json::to_value(revision).unwrap(),
            json!(value.to_string())
        );
        assert_eq!(
            serde_json::to_value(epoch).unwrap(),
            json!(value.to_string())
        );
        assert_eq!(
            serde_json::from_value::<WorkloadSagaRevision>(json!(value.to_string())).unwrap(),
            revision
        );
        assert_eq!(
            serde_json::from_value::<WorkloadTeardownDispatchEpoch>(json!(value.to_string()))
                .unwrap(),
            epoch
        );
    }

    // Six noncanonical or non-string cases must all fail closed for both counters.
    let rejected = [
        json!(0),
        json!(-1),
        json!(""),
        json!("00"),
        json!("01"),
        json!("+1"),
    ];
    assert_eq!(rejected.len(), 6);
    for value in rejected {
        assert!(serde_json::from_value::<WorkloadSagaRevision>(value.clone()).is_err());
        assert!(serde_json::from_value::<WorkloadTeardownDispatchEpoch>(value).is_err());
    }
}

#[test]
fn teardown_dispatch_epoch_overflow_fails_closed() {
    let withdrawal = withdrawal_record(WorkloadPublicationIntent::PublishWhenReady);
    let (pending, claim) = claim_teardown_step(&withdrawal);
    let inspection = pending.teardown_dispatch_to_inspection(&claim).unwrap();
    let first_retry =
        WorkloadTeardownRetryEvidence::for_inspection(&inspection, &claim, evidence("first-retry"))
            .unwrap();
    let retried = inspection
        .teardown_inspection_to_retry(&claim, first_retry)
        .unwrap();
    let next_claim = retried
        .teardown_disposition()
        .unwrap()
        .claim()
        .unwrap()
        .clone();
    let next_inspection = retried
        .teardown_dispatch_to_inspection(&next_claim)
        .unwrap();
    let next_not_completed = WorkloadTeardownRetryEvidence::for_inspection(
        &next_inspection,
        &next_claim,
        evidence("max-epoch-not-completed"),
    )
    .unwrap();

    let mut encoded_claim = serde_json::to_value(&next_claim).unwrap();
    encoded_claim["dispatchEpoch"] = json!(u64::MAX.to_string());
    encoded_claim["authorization"]["evidence"]["dispatchEpoch"] = json!((u64::MAX - 1).to_string());
    let max_claim: WorkloadTeardownClaim =
        serde_json::from_value(encoded_claim).expect("max-epoch claim should remain valid");
    let mut encoded_evidence = serde_json::to_value(&next_not_completed).unwrap();
    encoded_evidence["dispatchEpoch"] = json!(u64::MAX.to_string());
    let max_not_completed: WorkloadTeardownRetryEvidence =
        serde_json::from_value(encoded_evidence).expect("max-epoch evidence should decode");
    let next_revision = max_not_completed
        .inspected_revision()
        .checked_next()
        .unwrap();
    let inspected_revision = max_not_completed.inspected_revision();

    assert_eq!(
        WorkloadTeardownDispatchEpoch::new(u64::MAX).checked_next(),
        None
    );
    assert!(matches!(
        WorkloadTeardownClaim::retry_after_not_completed(
            &max_claim,
            next_revision,
            max_not_completed,
        ),
        Err(WorkloadSagaError::InvalidCounter(_))
    ));
    assert_eq!(next_inspection.revision(), inspected_revision);
}

#[test]
fn teardown_revision_overflow_fails_closed() {
    let source = withdrawal_record(WorkloadPublicationIntent::PublishWhenReady);
    let (attempt, _) = teardown_candidate(&source);
    let mut input = attempt_input(&attempt);
    input.issuing_revision = WorkloadSagaRevision::new(u64::MAX);
    let max_attempt = WorkloadTeardownAttempt::new(input).unwrap();
    let target = WorkloadTeardownProviderTarget::for_attempt(&max_attempt)
        .unwrap()
        .unwrap();

    assert_eq!(WorkloadSagaRevision::new(u64::MAX).checked_next(), None);
    assert!(matches!(
        WorkloadTeardownClaim::initial(max_attempt, target),
        Err(WorkloadSagaError::RevisionOverflow)
    ));
    assert!(source.teardown_disposition().unwrap().claim().is_none());
}

#[test]
fn teardown_attempt_id_rejects_each_tampered_identity_field() {
    let withdrawal = withdrawal_record(WorkloadPublicationIntent::PublishWhenReady);
    let (attempt, _) = teardown_candidate(&withdrawal);
    let encoded = serde_json::to_value(&attempt).expect("attempt should encode");
    let alternate_intent = running_intent(7, WorkloadPublicationIntent::PublishWhenReady);
    let alternate_key = key("tenant-b", "workload-b");

    let replacements = [
        ("key", serde_json::to_value(&alternate_key).unwrap()),
        (
            "sagaId",
            serde_json::to_value(alternate_key.saga_id()).unwrap(),
        ),
        ("issuingRevision", json!("999")),
        (
            "issuingTransitionId",
            serde_json::to_value(transition_id(&record_at_ready(
                WorkloadPublicationIntent::Withheld,
            )))
            .unwrap(),
        ),
        (
            "generation",
            serde_json::to_value(WorkloadGeneration::new(7)).unwrap(),
        ),
        (
            "desiredDigest",
            serde_json::to_value(alternate_intent.desired_digest()).unwrap(),
        ),
        (
            "requiredNode",
            serde_json::to_value(NodeIdentity::new("node-other").unwrap()).unwrap(),
        ),
        (
            "sourceDigest",
            serde_json::to_value(alternate_intent.source().source_digest()).unwrap(),
        ),
        (
            "executionProviderId",
            serde_json::to_value(alternate_intent.source().execution_provider_id()).unwrap(),
        ),
        (
            "networkPlanDigest",
            serde_json::to_value(alternate_intent.network().digest()).unwrap(),
        ),
        ("selectionEvidence", serde_json::Value::Null),
        (
            "cause",
            serde_json::to_value(WorkloadTeardownCause::Successor {
                generation: WorkloadGeneration::new(3),
                desired_digest: stopped_intent(3).desired_digest(),
            })
            .unwrap(),
        ),
        (
            "successorFence",
            serde_json::to_value(WorkloadTeardownSuccessorFence::new(
                WorkloadGeneration::new(3),
                stopped_intent(3).desired_digest(),
            ))
            .unwrap(),
        ),
        ("sourcePhase", json!("withdrawn")),
        ("targetPhase", json!("drained")),
        ("step", json!("drain_execution")),
        (
            "subjects",
            serde_json::to_value(WorkloadTeardownSubjects::Execution(
                withdrawal
                    .phase_detail()
                    .references()
                    .execution()
                    .unwrap()
                    .clone(),
            ))
            .unwrap(),
        ),
    ];

    // All 17 semantic fields outside attemptId are changed one at a time.
    assert_eq!(replacements.len(), 17);
    for (field, replacement) in replacements {
        let mut tampered = encoded.clone();
        assert_ne!(
            tampered[field], replacement,
            "case {field} must change its field"
        );
        tampered[field] = replacement;
        assert!(
            serde_json::from_value::<WorkloadTeardownAttempt>(tampered).is_err(),
            "tampered attempt field {field} must fail closed"
        );
    }
}

#[test]
fn teardown_claim_rejects_unknown_and_noncanonical_wire_fields() {
    let withdrawal = withdrawal_record(WorkloadPublicationIntent::PublishWhenReady);
    let (claimed, _) = claim_teardown_step(&withdrawal);
    let claim = claimed.teardown_disposition().unwrap().claim().unwrap();
    let encoded = serde_json::to_value(claim).expect("claim should encode");
    let mut cases = Vec::new();

    let mut unknown = encoded.clone();
    unknown["unknown"] = json!(true);
    cases.push(("unknown", unknown));
    let mut numeric_epoch = encoded.clone();
    numeric_epoch["dispatchEpoch"] = json!(0);
    cases.push(("numeric epoch", numeric_epoch));
    let mut leading_zero_epoch = encoded.clone();
    leading_zero_epoch["dispatchEpoch"] = json!("00");
    cases.push(("leading-zero epoch", leading_zero_epoch));
    let mut numeric_revision = encoded;
    numeric_revision["claimedRevision"] = json!(claimed.revision().as_u64());
    cases.push(("numeric revision", numeric_revision));

    assert_eq!(cases.len(), 4);
    for (case, value) in cases {
        assert!(
            serde_json::from_value::<WorkloadTeardownClaim>(value).is_err(),
            "claim case {case} must fail closed"
        );
    }
}

#[test]
fn teardown_transition_digest_binds_disposition_and_cause() {
    let source = established_record(WorkloadPublicationIntent::PublishWhenReady);
    let WorkloadSagaIntentUpdate::Transition(first) =
        source.apply_intent(stopped_intent(2)).unwrap()
    else {
        panic!("first successor should transition");
    };
    let WorkloadSagaIntentUpdate::Transition(other_cause) =
        source.apply_intent(stopped_intent(3)).unwrap()
    else {
        panic!("second successor should transition");
    };
    assert_eq!(first.phase(), other_cause.phase());
    assert_ne!(
        first.last_transition().transition_id(),
        other_cause.last_transition().transition_id()
    );

    let (claimed, _) = claim_teardown_step(&first);
    assert_eq!(first.phase(), claimed.phase());
    assert_ne!(
        first.last_transition().transition_id(),
        claimed.last_transition().transition_id()
    );
}

#[test]
fn teardown_claim_rejects_stale_generation_revision_and_successor() {
    let withdrawal = withdrawal_record(WorkloadPublicationIntent::PublishWhenReady);
    let source_revision = withdrawal.revision();
    let (attempt, target) = teardown_candidate(&withdrawal);

    let WorkloadSagaIntentUpdate::Transition(newer) = withdrawal
        .apply_intent(stopped_intent(3))
        .expect("later successor should advance the fence")
    else {
        panic!("later successor should change teardown state");
    };
    assert_eq!(withdrawal.revision(), source_revision);
    assert!(
        newer
            .claim_teardown(attempt.clone(), target.clone())
            .is_err()
    );

    let (other_attempt, other_target) = teardown_candidate(&withdrawal_record_for(
        WorkloadPublicationIntent::PublishWhenReady,
        3,
    ));
    assert!(
        withdrawal
            .claim_teardown(other_attempt, other_target)
            .is_err()
    );

    let (fresh_attempt, fresh_target) = teardown_candidate(&newer);
    assert_ne!(fresh_attempt.attempt_id(), attempt.attempt_id());
    assert!(newer.claim_teardown(fresh_attempt, fresh_target).is_ok());
}

#[test]
fn teardown_result_rejects_crossed_attempt_epoch_target_and_step() {
    let withdrawal = withdrawal_record(WorkloadPublicationIntent::PublishWhenReady);
    let (claimed, claim) = claim_teardown_step(&withdrawal);
    let baseline_revision = claimed.revision();
    let alternate_attempt = teardown_candidate(&withdrawal_record_for(
        WorkloadPublicationIntent::PublishWhenReady,
        3,
    ))
    .0;
    let execution_target = WorkloadTeardownProviderTarget::Execution {
        provider_id: claim.attempt().execution_provider_id().clone(),
        provider_source_digest: claim.attempt().source_digest(),
    };

    let cases = [
        WorkloadTeardownEffectResult::Succeeded {
            attempt_id: alternate_attempt.attempt_id().clone(),
            dispatch_epoch: claim.dispatch_epoch(),
            provider_target: claim.provider_target().clone(),
            evidence: Box::new(teardown_success_evidence(&claim, "cross-attempt")),
        },
        WorkloadTeardownEffectResult::Succeeded {
            attempt_id: claim.attempt().attempt_id().clone(),
            dispatch_epoch: WorkloadTeardownDispatchEpoch::new(1),
            provider_target: claim.provider_target().clone(),
            evidence: Box::new(teardown_success_evidence(&claim, "cross-epoch")),
        },
        WorkloadTeardownEffectResult::Succeeded {
            attempt_id: claim.attempt().attempt_id().clone(),
            dispatch_epoch: claim.dispatch_epoch(),
            provider_target: execution_target,
            evidence: Box::new(teardown_success_evidence(&claim, "cross-target")),
        },
        WorkloadTeardownEffectResult::Succeeded {
            attempt_id: claim.attempt().attempt_id().clone(),
            dispatch_epoch: claim.dispatch_epoch(),
            provider_target: claim.provider_target().clone(),
            evidence: Box::new(WorkloadTeardownSuccessEvidence::ExecutionStopped {
                reference: claimed
                    .phase_detail()
                    .references()
                    .execution()
                    .unwrap()
                    .clone(),
                evidence: evidence("cross-step"),
            }),
        },
    ];

    assert_eq!(cases.len(), 4);
    for result in cases {
        assert!(
            claimed
                .apply_teardown_effect_result(&claim, result)
                .is_err()
        );
        assert_eq!(claimed.revision(), baseline_revision);
    }
}

#[test]
fn teardown_inspection_rejects_crossed_transition_and_provider_target() {
    let withdrawal = withdrawal_record(WorkloadPublicationIntent::PublishWhenReady);
    let (pending, claim) = claim_teardown_step(&withdrawal);
    let inspection = pending
        .teardown_dispatch_to_inspection(&claim)
        .expect("pending claim should become inspection-only");
    let exact = WorkloadTeardownRetryEvidence::for_inspection(
        &inspection,
        &claim,
        evidence("not-completed"),
    )
    .unwrap();

    let mut crossed_transition = serde_json::to_value(&exact).unwrap();
    crossed_transition["inspectedTransitionId"] =
        serde_json::to_value(withdrawal.last_transition().transition_id()).unwrap();
    let crossed_transition =
        serde_json::from_value::<WorkloadTeardownRetryEvidence>(crossed_transition).unwrap();
    assert!(
        inspection
            .apply_teardown_inspection_result(
                &claim,
                WorkloadTeardownInspectionResult::NotCompleted {
                    evidence: crossed_transition,
                },
            )
            .is_err()
    );

    let crossed_target = WorkloadTeardownProviderTarget::Execution {
        provider_id: claim.attempt().execution_provider_id().clone(),
        provider_source_digest: claim.attempt().source_digest(),
    };
    assert!(
        inspection
            .apply_teardown_inspection_result(
                &claim,
                WorkloadTeardownInspectionResult::Ambiguous {
                    attempt_id: claim.attempt().attempt_id().clone(),
                    dispatch_epoch: claim.dispatch_epoch(),
                    provider_target: crossed_target,
                    inspection_command_id: inspection_command_id(&inspection, &claim),
                },
            )
            .is_err()
    );

    let stale_satisfied = WorkloadTeardownInspectionResult::Satisfied {
        attempt_id: claim.attempt().attempt_id().clone(),
        dispatch_epoch: claim.dispatch_epoch(),
        provider_target: claim.provider_target().clone(),
        inspection_command_id: inspection_command_id(&inspection, &claim),
        evidence: teardown_success_evidence(&claim, "stale-satisfied"),
    };
    let WorkloadSagaIntentUpdate::Transition(refenced) =
        inspection.apply_intent(stopped_intent(3)).unwrap()
    else {
        panic!("later successor should advance the inspection fence");
    };
    assert!(
        refenced
            .apply_teardown_inspection_result(&claim, stale_satisfied)
            .is_err()
    );
}

#[test]
fn later_successor_vetoes_pending_teardown_execute_and_requires_inspection() {
    let withdrawal = withdrawal_record(WorkloadPublicationIntent::PublishWhenReady);
    let initiating_cause = withdrawal.teardown_disposition().unwrap().cause().clone();
    let (stale_attempt, stale_target) = teardown_candidate(&withdrawal);
    let WorkloadSagaIntentUpdate::Transition(fenced) =
        withdrawal.apply_intent(stopped_intent(3)).unwrap()
    else {
        panic!("later successor should advance the teardown fence");
    };
    assert_eq!(
        fenced.teardown_disposition().unwrap().cause(),
        &initiating_cause
    );
    assert_eq!(
        fenced
            .teardown_disposition()
            .unwrap()
            .context()
            .successor_fence()
            .unwrap()
            .generation(),
        WorkloadGeneration::new(3)
    );
    assert!(fenced.claim_teardown(stale_attempt, stale_target).is_err());

    let (pending, claim) = claim_teardown_step(&fenced);
    let WorkloadSagaIntentUpdate::Transition(refenced) =
        pending.apply_intent(stopped_intent(4)).unwrap()
    else {
        panic!("newest successor should fence the issued claim");
    };
    assert!(matches!(
        refenced.teardown_disposition(),
        Some(WorkloadTeardownDisposition::InspectionRequired { claim: retained, .. })
            if retained == &claim
    ));
    assert_eq!(
        refenced.teardown_disposition().unwrap().cause(),
        &initiating_cause
    );
    assert_eq!(
        refenced.decide_teardown().unwrap(),
        WorkloadTeardownDecision::InspectExact(claim)
    );
}

#[test]
fn replayed_teardown_claim_is_inspection_only() {
    let withdrawal = withdrawal_record(WorkloadPublicationIntent::PublishWhenReady);
    let (pending, claim) = claim_teardown_step(&withdrawal);
    assert_eq!(
        pending.decide_teardown().unwrap(),
        WorkloadTeardownDecision::InspectExact(claim)
    );
}

#[test]
fn duplicate_teardown_success_is_rejected_without_revision_change() {
    let withdrawal = withdrawal_record(WorkloadPublicationIntent::PublishWhenReady);
    let (pending, claim) = claim_teardown_step(&withdrawal);
    let result = teardown_success_result(&claim, "once");
    let advanced = pending
        .apply_teardown_effect_result(&claim, result.clone())
        .expect("first exact success should advance");
    let revision = advanced.revision();
    assert!(
        advanced
            .apply_teardown_effect_result(&claim, result)
            .is_err()
    );
    assert_eq!(advanced.revision(), revision);

    let (next_pending, next_claim) = claim_teardown_step(&advanced);
    let next_advanced = next_pending
        .apply_teardown_effect_result(&next_claim, teardown_success_result(&next_claim, "second"))
        .unwrap();
    let rewritten = evidence("rewritten-receipt-prefix");
    let mut rewritten_prefix = serde_json::to_value(&next_advanced).unwrap();
    rewritten_prefix["teardownDisposition"]["context"]["completed"][0]["evidence"]["evidence"] =
        serde_json::to_value(rewritten).unwrap();
    rewritten_prefix["phaseDetail"]["value"]["terminalObservations"][0]["evidence"] =
        serde_json::to_value(rewritten).unwrap();
    rehash_encoded_record(&mut rewritten_prefix);
    let rewritten_prefix: WorkloadSagaRecord = serde_json::from_value(rewritten_prefix)
        .expect("rewritten history should remain internally self-consistent");
    assert!(next_pending.validate_successor(&rewritten_prefix).is_err());
}

#[test]
fn reused_teardown_retry_evidence_is_rejected() {
    let withdrawal = withdrawal_record(WorkloadPublicationIntent::PublishWhenReady);
    let (pending, claim) = claim_teardown_step(&withdrawal);
    let inspection = pending.teardown_dispatch_to_inspection(&claim).unwrap();
    let first = WorkloadTeardownRetryEvidence::for_inspection(
        &inspection,
        &claim,
        evidence("first-not-completed"),
    )
    .unwrap();
    let retried = inspection
        .apply_teardown_inspection_result(
            &claim,
            WorkloadTeardownInspectionResult::NotCompleted {
                evidence: first.clone(),
            },
        )
        .unwrap();
    let next_claim = retried
        .teardown_disposition()
        .unwrap()
        .claim()
        .unwrap()
        .clone();
    let next_inspection = retried
        .teardown_dispatch_to_inspection(&next_claim)
        .unwrap();
    let revision = next_inspection.revision();
    assert!(
        next_inspection
            .teardown_inspection_to_retry(&next_claim, first)
            .is_err()
    );
    assert_eq!(next_inspection.revision(), revision);
}

#[test]
fn reordered_or_skipped_teardown_phase_is_rejected() {
    let withdrawal = withdrawal_record(WorkloadPublicationIntent::PublishWhenReady);
    let revision = withdrawal.revision();
    for wrong_step in [
        WorkloadTeardownStep::DrainExecution,
        WorkloadTeardownStep::StopExecution,
        WorkloadTeardownStep::DetachNetwork,
        WorkloadTeardownStep::ReleaseNetwork,
    ] {
        assert!(
            withdrawal
                .record_resource_free_teardown_step(wrong_step)
                .is_err()
        );
        assert_eq!(withdrawal.revision(), revision);
    }
}

#[test]
fn ambiguous_teardown_effect_persists_inspection_required() {
    let withdrawal = withdrawal_record(WorkloadPublicationIntent::PublishWhenReady);
    let (pending, claim) = claim_teardown_step(&withdrawal);
    let inspection = pending
        .apply_teardown_effect_result(
            &claim,
            WorkloadTeardownEffectResult::Ambiguous {
                attempt_id: claim.attempt().attempt_id().clone(),
                dispatch_epoch: claim.dispatch_epoch(),
                provider_target: claim.provider_target().clone(),
            },
        )
        .expect("ambiguous outcome should persist inspection state");
    assert!(matches!(
        inspection.teardown_disposition(),
        Some(WorkloadTeardownDisposition::InspectionRequired { claim: retained, .. })
            if retained == &claim
    ));
    assert_eq!(
        inspection.decide_teardown().unwrap(),
        WorkloadTeardownDecision::InspectExact(claim)
    );
}

#[test]
fn ambiguous_teardown_inspection_stays_inspection_required() {
    let withdrawal = withdrawal_record(WorkloadPublicationIntent::PublishWhenReady);
    let (pending, claim) = claim_teardown_step(&withdrawal);
    let inspection = pending.teardown_dispatch_to_inspection(&claim).unwrap();
    let unchanged = inspection
        .apply_teardown_inspection_result(
            &claim,
            WorkloadTeardownInspectionResult::Ambiguous {
                attempt_id: claim.attempt().attempt_id().clone(),
                dispatch_epoch: claim.dispatch_epoch(),
                provider_target: claim.provider_target().clone(),
                inspection_command_id: inspection_command_id(&inspection, &claim),
            },
        )
        .unwrap();
    assert_eq!(unchanged, inspection);
    assert_eq!(
        unchanged.decide_teardown().unwrap(),
        WorkloadTeardownDecision::InspectExact(claim)
    );
}

#[test]
fn in_progress_teardown_inspection_stays_inspection_required() {
    let withdrawal = withdrawal_record(WorkloadPublicationIntent::PublishWhenReady);
    let (pending, claim) = claim_teardown_step(&withdrawal);
    let inspection = pending.teardown_dispatch_to_inspection(&claim).unwrap();
    let unchanged = inspection
        .apply_teardown_inspection_result(
            &claim,
            WorkloadTeardownInspectionResult::InProgress {
                attempt_id: claim.attempt().attempt_id().clone(),
                dispatch_epoch: claim.dispatch_epoch(),
                provider_target: claim.provider_target().clone(),
                inspection_command_id: inspection_command_id(&inspection, &claim),
                evidence: evidence("still-running"),
            },
        )
        .unwrap();
    assert_eq!(unchanged, inspection);
    assert_eq!(
        unchanged.decide_teardown().unwrap(),
        WorkloadTeardownDecision::InspectExact(claim)
    );
}

#[test]
fn teardown_inspection_not_completed_authorizes_same_attempt_next_epoch_once() {
    let withdrawal = withdrawal_record(WorkloadPublicationIntent::PublishWhenReady);
    let (pending, claim) = claim_teardown_step(&withdrawal);
    let inspection = pending.teardown_dispatch_to_inspection(&claim).unwrap();
    let evidence = WorkloadTeardownRetryEvidence::for_inspection(
        &inspection,
        &claim,
        evidence("not-completed"),
    )
    .unwrap();
    let retried = inspection
        .apply_teardown_inspection_result(
            &claim,
            WorkloadTeardownInspectionResult::NotCompleted {
                evidence: evidence.clone(),
            },
        )
        .unwrap();
    let next = retried.teardown_disposition().unwrap().claim().unwrap();
    assert_eq!(next.attempt(), claim.attempt());
    assert_eq!(
        next.dispatch_epoch(),
        claim.dispatch_epoch().checked_next().unwrap()
    );
    assert!(matches!(
        next.authorization(),
        WorkloadTeardownDispatchAuthorization::RetryAfterNotCompleted(retained)
            if retained == &evidence
    ));

    let other_withdrawal = withdrawal_record_for(WorkloadPublicationIntent::PublishWhenReady, 3);
    let (other_pending, other_claim) = claim_teardown_step(&other_withdrawal);
    let other_inspection = other_pending
        .teardown_dispatch_to_inspection(&other_claim)
        .unwrap();
    assert_eq!(other_inspection.revision(), inspection.revision());
    let wrong_transition = other_inspection.last_transition().transition_id().clone();
    let wrong_command = WorkloadTeardownCommandId::for_confirmed_dispatch(
        &claim,
        inspection.revision(),
        &wrong_transition,
        WorkloadTeardownCommandMode::Inspect,
    )
    .unwrap();
    let mut forged_retry = serde_json::to_value(&retried).unwrap();
    forged_retry["teardownDisposition"]["claim"]["authorization"]["evidence"]["inspectedTransitionId"] =
        serde_json::to_value(wrong_transition).unwrap();
    forged_retry["teardownDisposition"]["claim"]["authorization"]["evidence"]["inspectionCommandId"] =
        serde_json::to_value(wrong_command).unwrap();
    rehash_encoded_record(&mut forged_retry);
    let forged_retry: WorkloadSagaRecord = serde_json::from_value(forged_retry)
        .expect("forged retry should remain internally self-consistent");
    assert!(inspection.validate_successor(&forged_retry).is_err());

    assert!(
        retried
            .teardown_inspection_to_retry(&claim, evidence)
            .is_err()
    );
}

#[test]
fn teardown_inspection_satisfied_advances_without_retry() {
    let withdrawal = withdrawal_record(WorkloadPublicationIntent::PublishWhenReady);
    let (pending, claim) = claim_teardown_step(&withdrawal);
    let inspection = pending.teardown_dispatch_to_inspection(&claim).unwrap();
    let advanced = inspection
        .apply_teardown_inspection_result(
            &claim,
            WorkloadTeardownInspectionResult::Satisfied {
                attempt_id: claim.attempt().attempt_id().clone(),
                dispatch_epoch: claim.dispatch_epoch(),
                provider_target: claim.provider_target().clone(),
                inspection_command_id: inspection_command_id(&inspection, &claim),
                evidence: teardown_success_evidence(&claim, "observed-satisfied"),
            },
        )
        .unwrap();
    assert_eq!(advanced.phase(), WorkloadSagaPhase::Withdrawn);
    assert!(advanced.teardown_disposition().unwrap().claim().is_none());
    assert_eq!(
        advanced
            .teardown_disposition()
            .unwrap()
            .context()
            .completed()
            .len(),
        1
    );
    let encoded = serde_json::to_value(&advanced).unwrap();
    assert_eq!(
        encoded["teardownDisposition"]["context"]["completed"][0]["confirmation"]["kind"],
        json!("inspection")
    );
    assert_eq!(
        encoded["teardownDisposition"]["context"]["completed"][0]["confirmation"]["inspectionCommandId"],
        serde_json::to_value(inspection_command_id(&inspection, &claim)).unwrap()
    );
}

#[test]
fn teardown_inspection_definite_failure_enters_cleanup_pending() {
    let withdrawal = withdrawal_record(WorkloadPublicationIntent::PublishWhenReady);
    let (pending, claim) = claim_teardown_step(&withdrawal);
    let inspection = pending.teardown_dispatch_to_inspection(&claim).unwrap();
    let failure = failure("inspected-failure");
    let cleanup = inspection
        .apply_teardown_inspection_result(
            &claim,
            WorkloadTeardownInspectionResult::DefiniteFailure {
                attempt_id: claim.attempt().attempt_id().clone(),
                dispatch_epoch: claim.dispatch_epoch(),
                provider_target: claim.provider_target().clone(),
                inspection_command_id: inspection_command_id(&inspection, &claim),
                failure: failure.clone(),
            },
        )
        .unwrap();
    assert_eq!(cleanup.phase(), WorkloadSagaPhase::CleanupPending);
    assert_eq!(cleanup.failure(), Some(&failure));
    assert!(matches!(
        cleanup.teardown_disposition(),
        Some(WorkloadTeardownDisposition::DefiniteFailure {
            claim: retained,
            failure: retained_failure,
            ..
        }) if retained == &claim && retained_failure == &failure
    ));
    let encoded = serde_json::to_value(&cleanup).unwrap();
    assert_eq!(
        encoded["teardownDisposition"]["confirmation"]["kind"],
        json!("inspection")
    );
    assert_eq!(
        encoded["teardownDisposition"]["confirmation"]["inspectionCommandId"],
        serde_json::to_value(inspection_command_id(&inspection, &claim)).unwrap()
    );
}

#[test]
fn cancel_before_teardown_claim_leaves_source_record_unchanged() {
    let withdrawal = withdrawal_record(WorkloadPublicationIntent::PublishWhenReady);
    let snapshot = withdrawal.clone();
    let _abandoned_candidate = teardown_candidate(&withdrawal);
    assert_eq!(withdrawal, snapshot);
    assert!(withdrawal.teardown_disposition().unwrap().claim().is_none());
}

#[test]
fn cancel_after_teardown_claim_reopens_as_exact_inspection() {
    let withdrawal = withdrawal_record(WorkloadPublicationIntent::PublishWhenReady);
    let (pending, claim) = claim_teardown_step(&withdrawal);
    let inspection = pending
        .teardown_dispatch_to_inspection(&claim)
        .expect("persisted claim must recover through exact inspection");
    assert_eq!(
        inspection.decide_teardown().unwrap(),
        WorkloadTeardownDecision::InspectExact(claim)
    );
}

#[test]
fn teardown_failure_retains_exact_claim_failure_references_and_inspections() {
    let withdrawal = withdrawal_record(WorkloadPublicationIntent::PublishWhenReady);
    let retained_references = withdrawal.phase_detail().references();
    let (pending, claim) = claim_teardown_step(&withdrawal);
    let failure = failure("direct-failure");
    let cleanup = pending
        .apply_teardown_effect_result(
            &claim,
            WorkloadTeardownEffectResult::DefiniteFailure {
                attempt_id: claim.attempt().attempt_id().clone(),
                dispatch_epoch: claim.dispatch_epoch(),
                provider_target: claim.provider_target().clone(),
                failure: failure.clone(),
            },
        )
        .expect("definite teardown failure should enter cleanup");

    assert_eq!(cleanup.phase(), WorkloadSagaPhase::CleanupPending);
    assert_eq!(cleanup.failure(), Some(&failure));
    assert!(matches!(
        cleanup.teardown_disposition(),
        Some(WorkloadTeardownDisposition::DefiniteFailure {
            claim: retained_claim,
            failure: retained_failure,
            ..
        }) if retained_claim == &claim && retained_failure == &failure
    ));
    let WorkloadPhaseDetail::CleanupPending(detail) = cleanup.phase_detail() else {
        panic!("teardown failure should retain cleanup detail");
    };
    assert_eq!(detail.retained_references(), &retained_references);
    assert_eq!(detail.inspections().len(), retained_references.len());
}

#[test]
fn cleanup_pending_rejects_successor_replacement_claim_rewrite_and_reuse() {
    let withdrawal = withdrawal_record(WorkloadPublicationIntent::PublishWhenReady);
    let (pending, claim) = claim_teardown_step(&withdrawal);
    let failure = failure("cleanup-fence");
    let cleanup = pending
        .apply_teardown_effect_result(
            &claim,
            WorkloadTeardownEffectResult::DefiniteFailure {
                attempt_id: claim.attempt().attempt_id().clone(),
                dispatch_epoch: claim.dispatch_epoch(),
                provider_target: claim.provider_target().clone(),
                failure: failure.clone(),
            },
        )
        .unwrap();
    let revision = cleanup.revision();

    assert!(cleanup.apply_intent(stopped_intent(3)).is_err());
    assert!(
        cleanup
            .claim_teardown(claim.attempt().clone(), claim.provider_target().clone())
            .is_err()
    );
    assert!(
        cleanup
            .apply_teardown_effect_result(&claim, teardown_success_result(&claim, "late-success"))
            .is_err()
    );
    assert_eq!(
        cleanup.decide_teardown().unwrap(),
        WorkloadTeardownDecision::CleanupPending { claim, failure }
    );
    assert_eq!(cleanup.revision(), revision);
}

#[test]
fn failed_provision_compensation_releases_only_observed_resources_in_reverse_order() {
    let initial = WorkloadSagaRecord::new(
        key("tenant-a", "workload-a"),
        running_intent(1, WorkloadPublicationIntent::PublishWhenReady),
    )
    .unwrap();
    let reserved = advance_provision(&initial, WorkloadSagaPhase::NetworkReserved, None);
    let prepared_detail = provision_detail(
        WorkloadSagaPhase::WorkloadPrepared,
        reserved.active_intent(),
        None,
    );
    let pending = super::test_support::provision_candidates(
        &reserved,
        WorkloadSagaPhase::WorkloadPrepared,
        prepared_detail,
    )
    .into_iter()
    .next()
    .expect("prepare-workload edge should first persist a claim");
    let provision_claim = pending
        .provision_disposition()
        .and_then(WorkloadProvisionDisposition::claim)
        .expect("pending provision should retain its claim")
        .clone();
    let provision_failure = failure("prepare-failed");
    let failed = pending
        .dispatch_to_definite_failure(provision_failure.clone())
        .expect("definite provision failure should persist");
    let withdrawal = failed
        .commit_teardown_cause(WorkloadTeardownCause::FailedProvision {
            claim: Box::new(provision_claim.clone()),
            failure: provision_failure.clone(),
        })
        .expect("exact provision failure should start compensation");
    assert!(matches!(
        withdrawal.teardown_disposition().unwrap().cause(),
        WorkloadTeardownCause::FailedProvision { claim, failure }
            if claim.as_ref() == &provision_claim && failure == &provision_failure
    ));

    let mut record = withdrawal;
    for (step, target_phase) in [
        (
            WorkloadTeardownStep::WithdrawPublication,
            WorkloadSagaPhase::Withdrawn,
        ),
        (
            WorkloadTeardownStep::DrainExecution,
            WorkloadSagaPhase::Drained,
        ),
        (
            WorkloadTeardownStep::StopExecution,
            WorkloadSagaPhase::WorkloadStopped,
        ),
        (
            WorkloadTeardownStep::DetachNetwork,
            WorkloadSagaPhase::NetworkDetached,
        ),
    ] {
        assert_eq!(
            record.decide_teardown().unwrap(),
            WorkloadTeardownDecision::PersistCandidate(
                ProposedWorkloadTeardownTransition::ResourceFree { step, target_phase }
            ),
            "unobserved compensation step {step:?} must not fabricate an effect"
        );
        record = record.record_resource_free_teardown_step(step).unwrap();
    }

    let (release, target) = teardown_candidate(&record);
    assert_eq!(release.step(), WorkloadTeardownStep::ReleaseNetwork);
    assert!(matches!(
        target,
        WorkloadTeardownProviderTarget::Attachment { .. }
    ));
}
