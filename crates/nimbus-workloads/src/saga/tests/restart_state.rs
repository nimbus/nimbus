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

fn advance_to_observation(mut record: WorkloadSagaRecord) -> WorkloadSagaRecord {
    let request_id = active_request_id(&record);
    for phase in [
        WorkloadRestartPhase::PublicationWithdrawalPending,
        WorkloadRestartPhase::ExecutionQuiescencePending,
        WorkloadRestartPhase::Scheduled,
    ] {
        record = record
            .advance_restart_phase(&request_id, phase)
            .expect("restart phase should advance");
    }
    let due = record
        .restart_state()
        .active()
        .expect("restart should be active")
        .admission()
        .not_before_unix_millis();
    record = record
        .advance_scheduled_restart(&request_id, due)
        .expect("due restart should advance");
    for phase in [
        WorkloadRestartPhase::AttachmentPending,
        WorkloadRestartPhase::ActivationPrerequisitePending,
        WorkloadRestartPhase::ActivationPending,
        WorkloadRestartPhase::ReadinessPending,
        WorkloadRestartPhase::PublicationPending,
        WorkloadRestartPhase::ObservationPending,
    ] {
        record = record
            .advance_restart_phase(&request_id, phase)
            .expect("restart phase should advance");
    }
    record
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
    record
        .complete_restart(
            &request_id,
            observed_detail_for_active_attempt(record),
            WorkloadRestartEvidenceDigest::sha256("restart-observed"),
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
            .advance_restart_phase(&request_id, WorkloadRestartPhase::Scheduled)
            .is_err()
    );
    let crossed = WorkloadRestartRequestId::for_explicit(
        admitted.saga_id(),
        admitted.active_intent().source().source_generation(),
        "crossed",
    )
    .unwrap();
    assert!(
        admitted
            .advance_restart_phase(&crossed, WorkloadRestartPhase::PublicationWithdrawalPending)
            .is_err()
    );
}

#[test]
fn restart_recovery_eligibility_is_exhaustive() {
    let record = observed_record(WorkloadRestartPolicy::Always { max_restarts: 2 });
    assert!(!record.requires_recovery());
    let mut active = admitted(&record, explicit_input(&record, "recovery", 100));
    assert!(active.requires_recovery());
    let request_id = active_request_id(&active);
    for phase in [
        WorkloadRestartPhase::PublicationWithdrawalPending,
        WorkloadRestartPhase::ExecutionQuiescencePending,
        WorkloadRestartPhase::Scheduled,
    ] {
        active = active.advance_restart_phase(&request_id, phase).unwrap();
        assert!(active.requires_recovery());
    }
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
    for phase in [
        WorkloadRestartPhase::PublicationWithdrawalPending,
        WorkloadRestartPhase::ExecutionQuiescencePending,
        WorkloadRestartPhase::Scheduled,
    ] {
        active = active.advance_restart_phase(&request_id, phase).unwrap();
    }
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
