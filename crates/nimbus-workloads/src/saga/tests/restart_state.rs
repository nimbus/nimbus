use super::*;

fn observed_record(policy: WorkloadRestartPolicy) -> WorkloadSagaRecord {
    let intent = intent_with_restart_policy(
        "tenant-a",
        "workload-a",
        1,
        DesiredWorkloadState::Running,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::Withheld,
        1,
        policy,
    );
    let mut record = WorkloadSagaRecord::new(key("tenant-a", "workload-a"), intent)
        .expect("restart fixture should initialize");
    for phase in [
        WorkloadSagaPhase::NetworkReserved,
        WorkloadSagaPhase::WorkloadPrepared,
        WorkloadSagaPhase::NetworkAttached,
        WorkloadSagaPhase::WorkloadActivated,
        WorkloadSagaPhase::Ready,
        WorkloadSagaPhase::Observed,
    ] {
        record = advance_provision(&record, phase, None);
    }
    record
}

fn explicit_input(
    record: &WorkloadSagaRecord,
    key: &str,
    not_before: u64,
) -> WorkloadRestartAdmissionInput {
    WorkloadRestartAdmissionInput {
        expected_revision: record.revision(),
        trigger: WorkloadRestartTrigger::Explicit,
        inspection_version: None,
        request_id: WorkloadRestartRequestId::for_explicit(
            record.saga_id(),
            record.active_intent().source().source_generation(),
            key,
        )
        .expect("explicit request ID should validate"),
        not_before_unix_millis: WorkloadRestartNotBeforeUnixMillis::new(not_before),
    }
}

fn automatic_input(
    record: &WorkloadSagaRecord,
    exit_code: i32,
    not_before: u64,
    byte: u8,
) -> WorkloadRestartAdmissionInput {
    let inspection_version = WorkloadInspectionVersion::from_bytes([byte; 32]);
    WorkloadRestartAdmissionInput {
        expected_revision: record.revision(),
        trigger: WorkloadRestartTrigger::Automatic { exit_code },
        inspection_version: Some(inspection_version),
        request_id: WorkloadRestartRequestId::for_automatic(record.saga_id(), inspection_version),
        not_before_unix_millis: WorkloadRestartNotBeforeUnixMillis::new(not_before),
    }
}

fn admitted(
    record: &WorkloadSagaRecord,
    input: WorkloadRestartAdmissionInput,
) -> WorkloadSagaRecord {
    let WorkloadRestartAdmissionUpdate::Transition(record) =
        record.admit_restart(input).expect("restart should admit")
    else {
        panic!("new restart request must create a transition");
    };
    *record
}

fn active_request_id(record: &WorkloadSagaRecord) -> WorkloadRestartRequestId {
    record
        .restart_state()
        .active()
        .expect("restart should be active")
        .admission()
        .request_id()
        .clone()
}

fn active_claim(record: &WorkloadSagaRecord) -> WorkloadRestartCommandClaim {
    record
        .restart_state()
        .active()
        .expect("restart should be active")
        .disposition()
        .claim()
        .expect("restart command should be claimed")
        .clone()
}

fn pending_withdrawal_command(
    policy: WorkloadRestartPolicy,
    request_key: &str,
) -> (WorkloadSagaRecord, WorkloadRestartCommandClaim) {
    let record = observed_record(policy);
    let admitted = admitted(&record, explicit_input(&record, request_key, 0));
    let request_id = active_request_id(&admitted);
    let withdrawal = admitted
        .advance_restart_without_effect(&request_id)
        .expect("requested restart should enter withdrawal");
    let pending = withdrawal
        .claim_restart_command(&request_id)
        .expect("withdrawal command should claim");
    let claim = active_claim(&pending);
    (pending, claim)
}

fn succeed_current_command(record: WorkloadSagaRecord, label: &str) -> WorkloadSagaRecord {
    let request_id = active_request_id(&record);
    let claimed = record
        .claim_restart_command(&request_id)
        .expect("restart command should claim");
    let claim = active_claim(&claimed);
    claimed
        .apply_restart_effect_result(
            &claim,
            WorkloadRestartEffectResult::Succeeded {
                evidence: WorkloadRestartEvidenceDigest::sha256(label),
            },
            None,
        )
        .expect("restart command should succeed")
}

fn advance_to_observation(mut record: WorkloadSagaRecord) -> WorkloadSagaRecord {
    let request_id = active_request_id(&record);
    record = record
        .advance_restart_without_effect(&request_id)
        .expect("requested restart should enter withdrawal");
    record = succeed_current_command(record, "withdrawn");
    record = succeed_current_command(record, "quiesced");
    let due = record
        .restart_state()
        .active()
        .expect("restart should be active")
        .admission()
        .not_before_unix_millis();
    record = record
        .advance_scheduled_restart(&request_id, due)
        .expect("due restart should advance");
    for label in [
        "prepared",
        "attached",
        "prerequisites",
        "activated",
        "ready",
    ] {
        record = succeed_current_command(record, label);
    }
    record
        .advance_restart_without_effect(&request_id)
        .expect("withheld publication should advance without an ingress effect")
}

fn observed_detail_for_active_attempt(record: &WorkloadSagaRecord) -> WorkloadPhaseDetail {
    let intent = record.active_intent();
    let active = record
        .restart_state()
        .active()
        .expect("restart should be active");
    let execution =
        WorkloadExecutionReference::for_restart_epoch(intent, active.admission().restart_epoch());
    let network = WorkloadNetworkReference::for_intent(intent);
    let references =
        WorkloadEffectReferences::new(Some(network.clone()), Some(execution.clone()), None);
    WorkloadPhaseDetail::provision(
        WorkloadSagaPhase::Observed,
        intent,
        references,
        vec![
            WorkloadOwnerObservation::NetworkReserved {
                reference: network.clone(),
                evidence: evidence("restart-network-reserved"),
            },
            WorkloadOwnerObservation::ExecutionPrepared {
                reference: execution.clone(),
                evidence: evidence("restart-execution-prepared"),
            },
            WorkloadOwnerObservation::NetworkAttached {
                reference: network.clone(),
                evidence: evidence("restart-network-attached"),
            },
            WorkloadOwnerObservation::ExecutionActivated {
                reference: execution.clone(),
                evidence: evidence("restart-execution-activated"),
            },
            WorkloadOwnerObservation::Ready {
                network,
                execution,
                evidence: evidence("restart-ready"),
            },
        ],
    )
    .expect("new-attempt observed detail should validate")
}

fn complete(record: &WorkloadSagaRecord) -> WorkloadSagaRecord {
    let request_id = active_request_id(record);
    let claimed = record
        .claim_restart_command(&request_id)
        .expect("observation command should claim");
    let claim = active_claim(&claimed);
    claimed
        .apply_restart_effect_result(
            &claim,
            WorkloadRestartEffectResult::Succeeded {
                evidence: WorkloadRestartEvidenceDigest::sha256("restart-observed"),
            },
            Some(observed_detail_for_active_attempt(&claimed)),
        )
        .expect("restart should complete")
}

#[test]
fn restart_trigger_request_epoch_and_attempt_ids_round_trip() {
    let record = observed_record(WorkloadRestartPolicy::Always { max_restarts: 2 });
    let admitted = admitted(&record, automatic_input(&record, 0, u64::MAX, 0x41));
    let encoded = serde_json::to_value(&admitted).expect("record should encode");
    let decoded: WorkloadSagaRecord =
        serde_json::from_value(encoded).expect("record should decode");
    assert_eq!(decoded, admitted);
    let active = decoded.restart_state().active().unwrap();
    assert_eq!(active.admission().restart_epoch().as_u64(), 1);
    assert_eq!(
        active.admission().not_before_unix_millis().as_u64(),
        u64::MAX
    );
}

#[test]
fn restart_admission_binds_every_portable_fence() {
    let record = observed_record(WorkloadRestartPolicy::Always { max_restarts: 2 });
    let admitted = admitted(&record, automatic_input(&record, 17, 50, 0x42));
    let admission = admitted.restart_state().active().unwrap().admission();
    assert_eq!(admission.saga_id(), record.saga_id());
    assert_eq!(admission.source(), record.active_intent().source());
    assert_eq!(admission.generation(), record.active_intent().generation());
    assert_eq!(
        admission.desired_digest(),
        record.active_intent().desired_digest()
    );
    assert_eq!(admission.revision(), record.revision());
    assert_eq!(
        admission.provider_selection(),
        record.active_intent().source().execution_provider_id()
    );
    assert_eq!(admission.policy_attempt_count(), 1);
    assert_eq!(
        admission.source_attempt_id(),
        record.restart_state().current_execution_attempt_id()
    );
}

#[test]
fn restart_state_rejects_unknown_or_partial_shapes() {
    let record = observed_record(WorkloadRestartPolicy::Always { max_restarts: 2 });
    for mutation in ["unknown", "missing", "null"] {
        let mut wire = serde_json::to_value(&record).unwrap();
        match mutation {
            "unknown" => {
                wire["restart"]["unknown"] = json!(true);
            }
            "missing" => {
                wire.as_object_mut().unwrap().remove("restart");
            }
            "null" => wire["restart"] = serde_json::Value::Null,
            _ => unreachable!(),
        }
        assert!(
            serde_json::from_value::<WorkloadSagaRecord>(wire).is_err(),
            "{mutation} restart state must fail closed"
        );
    }
}

#[test]
fn same_generation_restart_keeps_desired_generation() {
    let record = observed_record(WorkloadRestartPolicy::Always { max_restarts: 2 });
    let admitted = admitted(&record, explicit_input(&record, "same-generation", 0));
    assert_eq!(
        admitted.active_intent().generation(),
        record.active_intent().generation()
    );
    assert_eq!(
        admitted.active_intent().network().generation(),
        record.active_intent().network().generation()
    );
    assert_ne!(
        admitted
            .restart_state()
            .active()
            .unwrap()
            .admission()
            .attempt_id(),
        record.restart_state().current_execution_attempt_id()
    );
}

#[test]
fn restart_epoch_and_attempt_id_prevent_same_generation_aba() {
    let execution = WorkloadExecutionId::for_execution(
        &workload_uid(0x55),
        &NodeIdentity::new("node-aba").unwrap(),
        WorkloadGeneration::new(9),
    );
    let first = WorkloadExecutionAttemptId::for_execution(&execution, WorkloadRestartEpoch::new(0));
    let second =
        WorkloadExecutionAttemptId::for_execution(&execution, WorkloadRestartEpoch::new(1));
    assert_ne!(first, second);
    assert!(
        !serde_json::to_string(&second)
            .unwrap()
            .contains("127.0.0.1")
    );
}

#[test]
fn restart_transition_id_covers_complete_restart_state() {
    let record = observed_record(WorkloadRestartPolicy::Always { max_restarts: 2 });
    let admitted = admitted(&record, explicit_input(&record, "transition-digest", 7));
    let mut forged = serde_json::to_value(&admitted).unwrap();
    forged["restart"]["active"]["admission"]["notBeforeUnixMillis"] = json!("8");
    assert!(serde_json::from_value::<WorkloadSagaRecord>(forged).is_err());
}

#[test]
fn restart_legal_transition_matrix_is_exhaustive() {
    let record = observed_record(WorkloadRestartPolicy::Always { max_restarts: 2 });
    let admitted = admitted(&record, explicit_input(&record, "phase-matrix", 10));
    let observed = advance_to_observation(admitted);
    let completed_admission = observed
        .restart_state()
        .active()
        .expect("restart should be active")
        .admission()
        .clone();
    let completed = complete(&observed);
    assert_eq!(
        completed.restart_state().phase(),
        WorkloadRestartPhase::Idle
    );
    assert_eq!(
        completed.restart_state().completed_restart_epoch().as_u64(),
        1
    );
    assert_eq!(
        completed
            .current_execution_reference()
            .restart_epoch()
            .as_u64(),
        1
    );
    let history = completed
        .restart_state()
        .last_completed()
        .expect("completed restart should retain exact history");
    assert_eq!(history.admission(), &completed_admission);
    assert_eq!(history.restart_epoch(), WorkloadRestartEpoch::new(1));
    assert_eq!(history.trigger(), WorkloadRestartTrigger::Explicit);
    assert_eq!(
        history.attempt_id(),
        completed.restart_state().current_execution_attempt_id()
    );
    assert_eq!(history.completed_automatic_restart_count(), 0);
    assert_eq!(history.not_before_unix_millis().as_u64(), 10);
    assert_eq!(
        history.evidence(),
        WorkloadRestartEvidenceDigest::sha256("restart-observed")
    );
}

#[test]
fn restart_skipped_backward_and_crossed_transitions_fail_closed() {
    let record = observed_record(WorkloadRestartPolicy::Always { max_restarts: 2 });
    let admitted = admitted(&record, explicit_input(&record, "legal-edges", 0));
    let request_id = active_request_id(&admitted);
    assert!(
        admitted
            .advance_scheduled_restart(
                &request_id,
                WorkloadRestartNotBeforeUnixMillis::new(u64::MAX),
            )
            .is_err()
    );
    assert!(admitted.claim_restart_command(&request_id).is_err());
    let crossed = WorkloadRestartRequestId::for_explicit(
        admitted.saga_id(),
        admitted.active_intent().source().source_generation(),
        "crossed",
    )
    .unwrap();
    assert!(admitted.advance_restart_without_effect(&crossed).is_err());
}

#[test]
fn restart_recovery_eligibility_is_exhaustive() {
    let record = observed_record(WorkloadRestartPolicy::Always { max_restarts: 2 });
    assert!(!record.requires_recovery());
    let mut active = admitted(&record, explicit_input(&record, "recovery", 100));
    assert!(active.requires_recovery());
    let request_id = active_request_id(&active);
    active = active.advance_restart_without_effect(&request_id).unwrap();
    assert!(active.requires_recovery());
    active = succeed_current_command(active, "recovery-withdrawal");
    assert!(active.requires_recovery());
    active = succeed_current_command(active, "recovery-quiescence");
    assert!(active.requires_recovery());
    assert_eq!(
        active.restart_recovery_decision(WorkloadRestartNotBeforeUnixMillis::new(99)),
        WorkloadRestartRecoveryDecision::WaitingUntil(WorkloadRestartNotBeforeUnixMillis::new(100))
    );
}

#[test]
fn explicit_restart_does_not_consume_automatic_count() {
    let record = observed_record(WorkloadRestartPolicy::Always { max_restarts: 1 });
    let admitted = admitted(&record, explicit_input(&record, "explicit-count", 0));
    assert_eq!(
        admitted.restart_state().completed_automatic_restart_count(),
        0
    );
    assert_eq!(
        admitted
            .restart_state()
            .active()
            .unwrap()
            .admission()
            .policy_attempt_count(),
        0
    );
}

#[test]
fn automatic_restart_count_increments_once_and_exhausts() {
    let record = observed_record(WorkloadRestartPolicy::Always { max_restarts: 1 });
    let input = automatic_input(&record, 0, 0, 0x43);
    let admitted = admitted(&record, input.clone());
    assert_eq!(
        admitted.restart_state().completed_automatic_restart_count(),
        1
    );
    assert!(matches!(
        admitted.admit_restart(input),
        Ok(WorkloadRestartAdmissionUpdate::Unchanged)
    ));
    let completed = complete(&advance_to_observation(admitted));
    assert!(
        completed
            .admit_restart(automatic_input(&completed, 0, 0, 0x44))
            .is_err()
    );
}

#[test]
fn duplicate_restart_request_requires_exact_admission_content() {
    let record = observed_record(WorkloadRestartPolicy::Always { max_restarts: 2 });
    let original = explicit_input(&record, "duplicate-content", 100);
    let admitted = admitted(&record, original.clone());
    assert!(matches!(
        admitted.admit_restart(original.clone()),
        Ok(WorkloadRestartAdmissionUpdate::Unchanged)
    ));

    let mut crossed_deadline = original.clone();
    crossed_deadline.not_before_unix_millis = WorkloadRestartNotBeforeUnixMillis::new(101);
    assert!(admitted.admit_restart(crossed_deadline).is_err());

    let inspection = WorkloadInspectionVersion::from_bytes([0x61; 32]);
    let crossed_trigger = WorkloadRestartAdmissionInput {
        trigger: WorkloadRestartTrigger::Automatic { exit_code: 9 },
        inspection_version: Some(inspection),
        ..original
    };
    assert!(admitted.admit_restart(crossed_trigger).is_err());
}

#[test]
fn automatic_request_id_must_bind_the_inspection_version() {
    let record = observed_record(WorkloadRestartPolicy::Always { max_restarts: 2 });
    let mut input = automatic_input(&record, 7, 0, 0x62);
    input.request_id = WorkloadRestartRequestId::for_automatic(
        record.saga_id(),
        WorkloadInspectionVersion::from_bytes([0x63; 32]),
    );
    assert!(record.admit_restart(input).is_err());
}

#[test]
fn deadline_survives_clock_rollback_without_early_admission() {
    let record = observed_record(WorkloadRestartPolicy::Always { max_restarts: 1 });
    let mut active = admitted(&record, explicit_input(&record, "clock", 100));
    let request_id = active_request_id(&active);
    active = active.advance_restart_without_effect(&request_id).unwrap();
    active = succeed_current_command(active, "clock-withdrawal");
    active = succeed_current_command(active, "clock-quiescence");
    assert!(
        active
            .advance_scheduled_restart(&request_id, WorkloadRestartNotBeforeUnixMillis::new(99))
            .is_err()
    );
    let reopened: WorkloadSagaRecord =
        serde_json::from_value(serde_json::to_value(&active).unwrap()).unwrap();
    assert!(
        reopened
            .advance_scheduled_restart(&request_id, WorkloadRestartNotBeforeUnixMillis::new(99))
            .is_err()
    );
    assert!(
        reopened
            .advance_scheduled_restart(&request_id, WorkloadRestartNotBeforeUnixMillis::new(100))
            .is_ok()
    );
}

#[test]
fn withdrawal_vetoes_unissued_restart() {
    let record = observed_record(WorkloadRestartPolicy::Always { max_restarts: 2 });
    let admitted = admitted(&record, explicit_input(&record, "withdrawal", 0));
    let WorkloadSagaIntentUpdate::Transition(withdrawal) = admitted
        .apply_intent(stopped_intent(2))
        .expect("successor should win")
    else {
        panic!("successor should transition");
    };
    assert_eq!(withdrawal.phase(), WorkloadSagaPhase::WithdrawalCommitted);
    assert!(withdrawal.restart_state().active().is_none());
}

#[test]
fn successor_vetoes_restart_before_admission() {
    let record = observed_record(WorkloadRestartPolicy::Always { max_restarts: 2 });
    let WorkloadSagaIntentUpdate::Transition(withdrawal) = record
        .apply_intent(stopped_intent(2))
        .expect("successor should queue")
    else {
        panic!("successor should transition");
    };
    assert!(
        withdrawal
            .admit_restart(explicit_input(&withdrawal, "too-late", 0))
            .is_err()
    );
}

#[test]
fn deadline_and_count_survive_strict_serialization_round_trip() {
    let record = observed_record(WorkloadRestartPolicy::Always { max_restarts: 2 });
    let admitted = admitted(&record, automatic_input(&record, 9, 987_654, 0x45));
    let reopened: WorkloadSagaRecord =
        serde_json::from_slice(&serde_json::to_vec(&admitted).unwrap()).unwrap();
    assert_eq!(
        reopened.restart_state().completed_automatic_restart_count(),
        1
    );
    assert_eq!(
        reopened
            .restart_state()
            .active()
            .unwrap()
            .admission()
            .not_before_unix_millis(),
        WorkloadRestartNotBeforeUnixMillis::new(987_654)
    );
}

#[test]
fn restart_command_claim_binds_the_exact_pending_transition() {
    let (pending, claim) = pending_withdrawal_command(
        WorkloadRestartPolicy::Always { max_restarts: 2 },
        "command-identity",
    );
    let active = pending.restart_state().active().unwrap();
    assert_eq!(claim.request_id(), active.admission().request_id());
    assert_eq!(claim.restart_epoch(), active.admission().restart_epoch());
    assert_eq!(claim.attempt_id(), active.admission().attempt_id());
    assert_eq!(claim.step(), WorkloadRestartStep::WithdrawPublication);
    assert_eq!(claim.dispatch_epoch(), WorkloadRestartDispatchEpoch::new(0));
    assert_eq!(
        claim.issuing_revision().checked_next(),
        Some(pending.revision())
    );
    assert!(matches!(
        claim.authorization(),
        WorkloadRestartDispatchAuthorization::Initial
    ));

    let request_id = active_request_id(&pending);
    assert!(pending.claim_restart_command(&request_id).is_err());
    assert!(pending.advance_restart_without_effect(&request_id).is_err());

    let succeeded = pending
        .apply_restart_effect_result(
            &claim,
            WorkloadRestartEffectResult::Succeeded {
                evidence: WorkloadRestartEvidenceDigest::sha256("withdrawal-success"),
            },
            None,
        )
        .expect("exact pending command should succeed");
    let receipt = succeeded
        .restart_state()
        .active()
        .unwrap()
        .disposition()
        .receipt()
        .expect("phase advance should retain the exact success receipt");
    assert_eq!(receipt.claim(), &claim);
    assert_eq!(
        receipt.result().evidence(),
        WorkloadRestartEvidenceDigest::sha256("withdrawal-success")
    );
}

#[test]
fn restart_ambiguity_requires_exact_absence_before_same_attempt_retry() {
    let (pending, claim) = pending_withdrawal_command(
        WorkloadRestartPolicy::Always { max_restarts: 2 },
        "ambiguous-command",
    );
    let inspection = pending
        .restart_dispatch_to_inspection(&claim)
        .expect("uncertain dispatch should require inspection");
    let absence = WorkloadRestartAbsenceEvidence::for_inspection(
        &inspection,
        &claim,
        WorkloadRestartEvidenceDigest::sha256("authenticated-absence"),
    )
    .expect("exact inspection should authenticate absence");
    let retry = inspection
        .restart_inspection_to_retry(&claim, absence.clone())
        .expect("exact absence should authorize one retry");
    let retry_claim = active_claim(&retry);
    assert_eq!(retry_claim.request_id(), claim.request_id());
    assert_eq!(retry_claim.restart_epoch(), claim.restart_epoch());
    assert_eq!(retry_claim.attempt_id(), claim.attempt_id());
    assert_eq!(retry_claim.step(), claim.step());
    assert_eq!(
        retry_claim.dispatch_epoch(),
        claim.dispatch_epoch().checked_next().unwrap()
    );
    assert_eq!(retry_claim.issuing_revision(), inspection.revision());
    assert!(matches!(
        retry_claim.authorization(),
        WorkloadRestartDispatchAuthorization::RetryAfterAbsence(retained)
            if retained == &absence
    ));

    assert!(
        retry
            .apply_restart_effect_result(
                &claim,
                WorkloadRestartEffectResult::Succeeded {
                    evidence: WorkloadRestartEvidenceDigest::sha256("stale-success"),
                },
                None,
            )
            .is_err()
    );
    assert!(
        retry
            .restart_inspection_to_retry(&retry_claim, absence.clone())
            .is_err()
    );
    let next_inspection = retry
        .restart_dispatch_to_inspection(&retry_claim)
        .expect("retry ambiguity should return to inspection");
    assert!(
        WorkloadRestartAbsenceEvidence::for_inspection(
            &next_inspection,
            &claim,
            WorkloadRestartEvidenceDigest::sha256("crossed-absence"),
        )
        .is_err()
    );
    assert!(
        next_inspection
            .restart_inspection_to_retry(&retry_claim, absence)
            .is_err()
    );
}

#[test]
fn authenticated_absence_cannot_be_recorded_as_a_direct_effect_result() {
    let (pending, claim) = pending_withdrawal_command(
        WorkloadRestartPolicy::Always { max_restarts: 2 },
        "absence-result",
    );
    assert!(
        pending
            .apply_restart_effect_result(
                &claim,
                WorkloadRestartEffectResult::AuthenticatedAbsent {
                    evidence: WorkloadRestartEvidenceDigest::sha256("absence-result"),
                },
                None,
            )
            .is_err()
    );
}

#[test]
fn definite_restart_failure_is_terminal_for_the_active_command() {
    let (pending, claim) = pending_withdrawal_command(
        WorkloadRestartPolicy::Always { max_restarts: 2 },
        "definite-failure",
    );
    let failed = pending
        .apply_restart_effect_result(
            &claim,
            WorkloadRestartEffectResult::Failed {
                evidence: WorkloadRestartEvidenceDigest::sha256("definite-failure"),
            },
            None,
        )
        .expect("definite failure should persist");
    assert!(matches!(
        failed.restart_state().active().unwrap().disposition(),
        WorkloadRestartDisposition::DefiniteFailure {
            claim: retained,
            result: WorkloadRestartEffectResult::Failed { .. },
        } if retained == &claim
    ));
    let request_id = active_request_id(&failed);
    assert!(failed.claim_restart_command(&request_id).is_err());
    assert!(failed.advance_restart_without_effect(&request_id).is_err());
    assert!(failed.restart_dispatch_to_inspection(&claim).is_err());
    assert!(
        failed
            .apply_restart_effect_result(
                &claim,
                WorkloadRestartEffectResult::Succeeded {
                    evidence: WorkloadRestartEvidenceDigest::sha256("late-success"),
                },
                None,
            )
            .is_err()
    );
}

#[test]
fn restart_command_wire_rejects_tampered_claim_authorization_and_receipt() {
    let (pending, claim) = pending_withdrawal_command(
        WorkloadRestartPolicy::Always { max_restarts: 2 },
        "strict-command-wire",
    );
    let mut crossed_epoch = serde_json::to_value(&claim).unwrap();
    crossed_epoch["dispatchEpoch"] = json!("1");
    assert!(serde_json::from_value::<WorkloadRestartCommandClaim>(crossed_epoch).is_err());

    let mut unknown_claim_field = serde_json::to_value(&claim).unwrap();
    unknown_claim_field["unknown"] = json!(true);
    assert!(serde_json::from_value::<WorkloadRestartCommandClaim>(unknown_claim_field).is_err());

    let succeeded = pending
        .apply_restart_effect_result(
            &claim,
            WorkloadRestartEffectResult::Succeeded {
                evidence: WorkloadRestartEvidenceDigest::sha256("strict-success"),
            },
            None,
        )
        .unwrap();
    let receipt = succeeded
        .restart_state()
        .active()
        .unwrap()
        .disposition()
        .receipt()
        .unwrap();
    let mut failed_receipt = serde_json::to_value(receipt).unwrap();
    failed_receipt["result"] = json!({
        "result": "failed",
        "evidence": WorkloadRestartEvidenceDigest::sha256("forged-failure"),
    });
    assert!(serde_json::from_value::<WorkloadRestartCommandReceipt>(failed_receipt).is_err());

    let mut succeeded_failure = json!({
        "disposition": "definite_failure",
        "claim": claim,
        "result": {
            "result": "succeeded",
            "evidence": WorkloadRestartEvidenceDigest::sha256("forged-success"),
        },
    });
    assert!(
        serde_json::from_value::<WorkloadRestartDisposition>(succeeded_failure.clone()).is_err()
    );
    succeeded_failure["unknown"] = json!(true);
    assert!(serde_json::from_value::<WorkloadRestartDisposition>(succeeded_failure).is_err());
}

#[test]
fn restart_watch_candidate_predicate_is_clock_free_and_exhaustive() {
    let eligible = observed_record(WorkloadRestartPolicy::Always { max_restarts: 2 });
    assert!(eligible.requires_restart_watch());
    assert!(!observed_record(WorkloadRestartPolicy::Never).requires_restart_watch());

    let never = observed_record(WorkloadRestartPolicy::Never);
    let active_never = admitted(&never, explicit_input(&never, "active-never", u64::MAX));
    assert!(active_never.requires_restart_watch());

    let WorkloadSagaIntentUpdate::Transition(withdrawal) = eligible
        .apply_intent(stopped_intent(2))
        .expect("successor should queue")
    else {
        panic!("successor should transition");
    };
    assert!(!withdrawal.requires_restart_watch());

    let stopped = WorkloadSagaRecord::new(
        key("tenant-a", "stopped-watch"),
        intent_with_restart_policy(
            "tenant-a",
            "stopped-watch",
            1,
            DesiredWorkloadState::Stopped,
            WorkloadActivationIntent::PrepareOnly,
            WorkloadPublicationIntent::Withheld,
            1,
            WorkloadRestartPolicy::Always { max_restarts: 2 },
        ),
    )
    .expect("stopped record should initialize");
    assert!(!stopped.requires_restart_watch());
}
