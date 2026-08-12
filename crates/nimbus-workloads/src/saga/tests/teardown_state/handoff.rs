use super::*;

fn pending_reservation() -> (WorkloadSagaRecord, WorkloadProvisionDispatchClaim) {
    let record = WorkloadSagaRecord::new(
        key("tenant-a", "handoff-provision"),
        running_intent(1, WorkloadPublicationIntent::PublishWhenReady),
    )
    .expect("provision handoff fixture should validate");
    let attempt = provision_attempt_fixture(
        &record,
        WorkloadProvisionStep::ReserveNetwork,
        WorkloadSagaPhase::NetworkReserved,
        WorkloadProvisionSubjects::Network(WorkloadNetworkReference::for_intent(
            record.active_intent(),
        )),
        None,
    );
    let pending = persist_attempt_fixture(&record, attempt);
    let claim = pending
        .provision_disposition()
        .and_then(WorkloadProvisionDisposition::claim)
        .expect("pending provision should retain its exact claim")
        .clone();
    (pending, claim)
}

fn fence_pending_provision(
    pending: &WorkloadSagaRecord,
) -> (WorkloadSagaRecord, WorkloadProvisionDispatchClaim) {
    let claim = pending
        .provision_disposition()
        .and_then(WorkloadProvisionDisposition::claim)
        .expect("pending provision should retain its claim")
        .clone();
    let WorkloadSagaIntentUpdate::Transition(fenced) = pending
        .apply_intent(stopped_intent(2))
        .expect("stopped successor should fence pending provision")
    else {
        panic!("stopped successor must change the durable record");
    };
    (*fenced, claim)
}

fn observed_restart_record() -> WorkloadSagaRecord {
    let intent = intent_with_restart_policy(
        "tenant-a",
        "handoff-restart",
        1,
        DesiredWorkloadState::Running,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::Withheld,
        7,
        WorkloadRestartPolicy::Always { max_restarts: 2 },
    );
    let mut record = WorkloadSagaRecord::new(key("tenant-a", "handoff-restart"), intent)
        .expect("restart handoff fixture should validate");
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

fn admit_restart(record: &WorkloadSagaRecord, request_key: &str) -> WorkloadSagaRecord {
    let input = WorkloadRestartAdmissionInput {
        expected_revision: record.revision(),
        trigger: WorkloadRestartTrigger::Explicit,
        inspection_version: None,
        request_id: WorkloadRestartRequestId::for_explicit(
            record.saga_id(),
            record.active_intent().source().source_generation(),
            request_key,
        )
        .expect("restart request identity should validate"),
        not_before_unix_millis: WorkloadRestartNotBeforeUnixMillis::new(0),
    };
    let WorkloadRestartAdmissionUpdate::Transition(admitted) = record
        .admit_restart(input)
        .expect("explicit restart should admit")
    else {
        panic!("new restart admission must change the durable record");
    };
    *admitted
}

fn pending_restart() -> (WorkloadSagaRecord, WorkloadRestartCommandClaim) {
    let observed = observed_restart_record();
    let admitted = admit_restart(&observed, "issued-handoff");
    let request_id = admitted
        .restart_state()
        .active()
        .expect("restart should remain active")
        .admission()
        .request_id()
        .clone();
    let quiescence = admitted
        .advance_restart_without_effect(&request_id)
        .expect("requested restart should enter its first command phase");
    let pending = quiescence
        .claim_restart_command(&request_id)
        .expect("restart command claim should persist");
    let claim = pending
        .restart_state()
        .active()
        .expect("restart should remain active")
        .disposition()
        .claim()
        .expect("pending restart should retain its exact claim")
        .clone();
    (pending, claim)
}

fn successor_vetoed_restart(
    successor_generation: u64,
) -> (
    WorkloadSagaRecord,
    WorkloadRestartCommandClaim,
    WorkloadRestartEffectResult,
) {
    let (pending, claim) = pending_restart();
    let WorkloadSagaIntentUpdate::Transition(fenced) = pending
        .apply_intent(stopped_intent(successor_generation))
        .expect("successor should fence the issued restart")
    else {
        panic!("successor must change the durable record");
    };
    let result = WorkloadRestartEffectResult::Succeeded {
        evidence: WorkloadRestartEvidenceDigest::sha256("restart-terminal-result"),
    };
    let settled = fenced
        .apply_restart_effect_result(&claim, result.clone())
        .expect("exact terminal result should settle the successor-vetoed restart");
    (settled, claim, result)
}

#[test]
fn pending_provision_successor_converts_dispatch_to_inspection_before_teardown() {
    let (pending, claim) = pending_reservation();
    let (fenced, retained_claim) = fence_pending_provision(&pending);

    assert_eq!(retained_claim, claim);
    assert_eq!(fenced.phase(), pending.phase());
    assert!(fenced.teardown_disposition().is_none());
    assert!(matches!(
        fenced.provision_disposition(),
        Some(WorkloadProvisionDisposition::InspectionRequired(retained)) if retained == &claim
    ));
    assert!(fenced.commit_queued_successor_teardown().is_err());
}

#[test]
fn provision_inspection_success_retains_effect_then_commits_withdrawal() {
    let (pending, _) = pending_reservation();
    let (fenced, claim) = fence_pending_provision(&pending);
    let reserved = fenced
        .dispatch_to_success(
            WorkloadSagaPhase::NetworkReserved,
            provision_detail(
                WorkloadSagaPhase::NetworkReserved,
                fenced.active_intent(),
                None,
            ),
        )
        .expect("exact inspected success should retain the established effect");
    let established = reserved.phase_detail().references();
    assert!(matches!(
        reserved.provision_disposition(),
        Some(WorkloadProvisionDisposition::InspectionRequired(retained)) if retained == &claim
    ));
    let withdrawal = reserved
        .commit_queued_successor_teardown()
        .expect("settled provision success should commit withdrawal once");

    assert_eq!(withdrawal.phase(), WorkloadSagaPhase::WithdrawalCommitted);
    assert_eq!(withdrawal.phase_detail().references(), established);
}

#[test]
fn provision_inspection_absence_never_retries_after_stopped_successor() {
    let (pending, _) = pending_reservation();
    let (fenced, claim) = fence_pending_provision(&pending);
    let absence = WorkloadProvisionAbsenceEvidence::for_inspection(
        &fenced,
        &claim,
        evidence("fenced-provision-absent"),
    )
    .expect("absence should bind the fenced provision inspection");

    assert!(
        fenced
            .inspection_to_retry_dispatch(absence.clone())
            .is_err()
    );
    let withdrawal = fenced
        .provision_inspection_absence_to_teardown(absence)
        .expect("exact absence should hand off directly to teardown");
    let context = withdrawal.teardown_disposition().unwrap().context();
    assert_eq!(
        context
            .provision_absence()
            .expect("teardown should retain exact provision absence")
            .claim(),
        &claim
    );
}

#[test]
fn provision_definite_failure_starts_compensation_from_exact_retained_references() {
    let initial = WorkloadSagaRecord::new(
        key("tenant-a", "handoff-compensation"),
        running_intent(1, WorkloadPublicationIntent::Withheld),
    )
    .unwrap();
    let reserved = advance_provision(&initial, WorkloadSagaPhase::NetworkReserved, None);
    let attempt = provision_attempt_fixture(
        &reserved,
        WorkloadProvisionStep::PrepareWorkload,
        WorkloadSagaPhase::WorkloadPrepared,
        WorkloadProvisionSubjects::Execution(WorkloadExecutionReference::for_intent(
            reserved.active_intent(),
        )),
        None,
    );
    let pending = persist_attempt_fixture(&reserved, attempt);
    let claim = pending
        .provision_disposition()
        .and_then(WorkloadProvisionDisposition::claim)
        .unwrap()
        .clone();
    let failure = failure("handoff-provision-failed");
    let failed = pending
        .dispatch_to_definite_failure(failure.clone())
        .expect("definite provision failure should persist");
    let retained = failed.phase_detail().references();
    let withdrawal = failed
        .commit_teardown_cause(WorkloadTeardownCause::FailedProvision {
            claim: Box::new(claim.clone()),
            failure: failure.clone(),
        })
        .expect("exact failure should enter compensation");

    assert_eq!(withdrawal.phase_detail().references(), retained);
    assert!(matches!(
        withdrawal.teardown_disposition().unwrap().cause(),
        WorkloadTeardownCause::FailedProvision {
            claim: retained_claim,
            failure: retained_failure,
        } if retained_claim.as_ref() == &claim && retained_failure == &failure
    ));
}

#[test]
fn unissued_restart_is_cleared_before_withdrawal() {
    let observed = observed_restart_record();
    let admitted = admit_restart(&observed, "unissued-handoff");
    let WorkloadSagaIntentUpdate::Transition(withdrawal) = admitted
        .apply_intent(stopped_intent(2))
        .expect("successor should clear unissued restart and begin teardown")
    else {
        panic!("successor must change the durable record");
    };

    assert_eq!(withdrawal.phase(), WorkloadSagaPhase::WithdrawalCommitted);
    assert!(withdrawal.restart_state().active().is_none());
}

#[test]
fn issued_restart_successor_waits_for_exact_terminal_inspection() {
    let (pending, claim) = pending_restart();
    let WorkloadSagaIntentUpdate::Transition(fenced) = pending
        .apply_intent(stopped_intent(2))
        .expect("successor should fence issued restart")
    else {
        panic!("successor must change the durable record");
    };

    assert_eq!(fenced.phase(), WorkloadSagaPhase::Observed);
    assert!(fenced.teardown_disposition().is_none());
    assert!(matches!(
        fenced.restart_state().active().unwrap().disposition(),
        WorkloadRestartDisposition::InspectionRequired { claim: retained } if retained == &claim
    ));
    assert!(fenced.commit_restart_settlement_teardown().is_err());
}

#[test]
fn restart_result_is_settled_before_withdrawal_committed() {
    let (settled, claim, result) = successor_vetoed_restart(2);
    assert_eq!(settled.phase(), WorkloadSagaPhase::Observed);
    let withdrawal = settled
        .commit_restart_settlement_teardown()
        .expect("terminal restart result should settle before withdrawal");
    let settlement = withdrawal
        .teardown_disposition()
        .unwrap()
        .context()
        .restart_settlement()
        .expect("withdrawal should retain exact restart settlement");

    assert_eq!(withdrawal.phase(), WorkloadSagaPhase::WithdrawalCommitted);
    assert!(withdrawal.restart_state().active().is_none());
    assert_eq!(settlement.claim(), &claim);
    assert_eq!(settlement.result(), &result);
    assert_ne!(settlement.source_execution(), settlement.target_execution());
    assert!(withdrawal.commit_restart_settlement_teardown().is_err());

    let withdrawn = advance_teardown(&withdrawal, WorkloadSagaPhase::Withdrawn);
    let (pending, drain_claim) = claim_teardown_step(&withdrawn);
    let drained = pending
        .apply_teardown_effect_result(
            &drain_claim,
            teardown_success_result(&drain_claim, "restart-source-drained"),
        )
        .expect("source drain should preserve the restart settlement");
    let mut dropped_settlement = serde_json::to_value(&drained).unwrap();
    dropped_settlement["teardownDisposition"]["context"]["restartSettlement"] =
        serde_json::Value::Null;
    rehash_encoded_record(&mut dropped_settlement);
    let dropped_settlement: WorkloadSagaRecord = serde_json::from_value(dropped_settlement)
        .expect("the forged candidate should remain internally self-consistent");
    assert!(pending.validate_successor(&dropped_settlement).is_err());

    let released = advance_teardown(&drained, WorkloadSagaPhase::NetworkReleased);
    assert!(matches!(
        released.decide_teardown().unwrap(),
        WorkloadTeardownDecision::RestartSettlementPending(retained)
            if retained.as_ref() == settlement
    ));
    assert!(released.record_terminal_teardown().is_err());
    let WorkloadPhaseDetail::Teardown(released_detail) = released.phase_detail() else {
        panic!("released teardown should retain terminal observations");
    };
    let terminal_digest =
        WorkloadTerminalEvidenceDigest::for_observations(released_detail.terminal_observations())
            .unwrap();
    let mut forged_terminal = serde_json::to_value(&released).unwrap();
    let next_revision = released.revision().checked_next().unwrap();
    forged_terminal["revision"] = serde_json::to_value(next_revision).unwrap();
    forged_terminal["phase"] = serde_json::to_value(WorkloadSagaPhase::Recorded).unwrap();
    forged_terminal["phaseDetail"] = serde_json::to_value(WorkloadPhaseDetail::recorded(
        released.active_intent(),
        terminal_digest,
        released_detail.terminal_execution_reference().cloned(),
    ))
    .unwrap();
    forged_terminal
        .as_object_mut()
        .unwrap()
        .remove("teardownDisposition");
    forged_terminal["lastTransition"]["sourcePhase"] =
        serde_json::to_value(WorkloadSagaPhase::NetworkReleased).unwrap();
    forged_terminal["lastTransition"]["targetPhase"] =
        serde_json::to_value(WorkloadSagaPhase::Recorded).unwrap();
    forged_terminal["lastTransition"]["resultingRevision"] =
        serde_json::to_value(next_revision).unwrap();
    rehash_encoded_record(&mut forged_terminal);
    let forged_terminal: WorkloadSagaRecord = serde_json::from_value(forged_terminal)
        .expect("forged terminal candidate should remain internally self-consistent");
    assert!(released.validate_successor(&forged_terminal).is_err());

    let recovered: WorkloadSagaRecord =
        serde_json::from_value(serde_json::to_value(&released).unwrap()).unwrap();
    assert!(
        recovered
            .teardown_disposition()
            .unwrap()
            .context()
            .restart_settlement()
            .is_some()
    );
}

#[test]
fn later_successor_rebinds_restart_and_teardown_fences_before_effect() {
    let (pending, claim) = pending_restart();
    let WorkloadSagaIntentUpdate::Transition(first) = pending
        .apply_intent(stopped_intent(2))
        .expect("first successor should fence issued restart")
    else {
        panic!("first successor must change the durable record");
    };
    let WorkloadSagaIntentUpdate::Transition(latest) = first
        .apply_intent(stopped_intent(3))
        .expect("later successor should advance the restart fence")
    else {
        panic!("later successor must change the durable record");
    };
    assert_eq!(
        latest
            .restart_state()
            .active()
            .unwrap()
            .successor_veto_generation(),
        Some(WorkloadGeneration::new(3))
    );
    assert!(matches!(
        latest.restart_state().active().unwrap().disposition(),
        WorkloadRestartDisposition::InspectionRequired { claim: retained } if retained == &claim
    ));

    let settled = latest
        .apply_restart_effect_result(
            &claim,
            WorkloadRestartEffectResult::Succeeded {
                evidence: WorkloadRestartEvidenceDigest::sha256("latest-successor-settlement"),
            },
        )
        .expect("exact result should settle against the latest successor fence");
    let withdrawal = settled
        .commit_restart_settlement_teardown()
        .expect("latest restart fence should hand off to teardown");
    let context = withdrawal.teardown_disposition().unwrap().context();
    assert!(matches!(
        context.cause(),
        WorkloadTeardownCause::Successor { generation, .. }
            if *generation == WorkloadGeneration::new(3)
    ));
    assert_eq!(
        context.successor_fence().unwrap().generation(),
        WorkloadGeneration::new(3)
    );
}
