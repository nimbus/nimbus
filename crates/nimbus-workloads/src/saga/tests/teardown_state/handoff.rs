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

fn succeed_restart_command(record: WorkloadSagaRecord, label: &str) -> WorkloadSagaRecord {
    let request_id = record
        .restart_state()
        .active()
        .expect("restart should remain active")
        .admission()
        .request_id()
        .clone();
    let pending = record
        .claim_restart_command(&request_id)
        .expect("restart command should claim");
    let claim = pending
        .restart_state()
        .active()
        .and_then(|active| active.disposition().claim())
        .expect("claimed restart should retain its exact command")
        .clone();
    pending
        .apply_restart_effect_result(
            &claim,
            WorkloadRestartEffectResult::Succeeded {
                evidence: WorkloadRestartEvidenceDigest::sha256(label),
            },
        )
        .expect("restart command should persist exact success")
}

fn pending_restart_activation() -> (WorkloadSagaRecord, WorkloadRestartCommandClaim) {
    let observed = observed_restart_record();
    let admitted = admit_restart(&observed, "issued-target-activation");
    let request_id = admitted
        .restart_state()
        .active()
        .expect("restart should remain active")
        .admission()
        .request_id()
        .clone();
    let mut record = admitted
        .advance_restart_without_effect(&request_id)
        .expect("withheld restart should enter source quiescence");
    record = succeed_restart_command(record, "target-source-quiesced");
    let due = record
        .restart_state()
        .active()
        .expect("scheduled restart should remain active")
        .admission()
        .not_before_unix_millis();
    record = record
        .advance_scheduled_restart(&request_id, due)
        .expect("scheduled restart should become due");
    for label in ["target-prepared", "target-attached", "target-prerequisites"] {
        record = succeed_restart_command(record, label);
    }
    let pending = record
        .claim_restart_command(&request_id)
        .expect("target activation should claim");
    let claim = pending
        .restart_state()
        .active()
        .and_then(|active| active.disposition().claim())
        .expect("target activation should retain its exact claim")
        .clone();
    assert_eq!(claim.step(), WorkloadRestartStep::ActivateExecution);
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

fn definite_failure_before_successor_restart(
    successor_generation: u64,
) -> (
    WorkloadSagaRecord,
    WorkloadRestartCommandClaim,
    WorkloadRestartEffectResult,
) {
    let (pending, claim) = pending_restart();
    let result = WorkloadRestartEffectResult::Failed {
        evidence: WorkloadRestartEvidenceDigest::sha256("restart-terminal-failure"),
    };
    let failed = pending
        .apply_restart_effect_result(&claim, result.clone())
        .expect("exact restart failure should persist before a successor arrives");
    let WorkloadSagaIntentUpdate::Transition(fenced) = failed
        .apply_intent(stopped_intent(successor_generation))
        .expect("successor should fence the terminal restart failure")
    else {
        panic!("successor must change the durable record");
    };
    (*fenced, claim, result)
}

fn withdrawal_with_restart_settlement(
    withdrawal: &WorkloadSagaRecord,
    settlement: &WorkloadRestartTeardownSettlement,
) -> Result<WorkloadSagaRecord, serde_json::Error> {
    let mut encoded = serde_json::to_value(withdrawal)?;
    encoded["teardownDisposition"]["context"]["restartSettlement"] =
        serde_json::to_value(settlement)?;
    rehash_encoded_record(&mut encoded);
    serde_json::from_value(encoded)
}

fn withdrawal_with_successor(
    withdrawal: &WorkloadSagaRecord,
    successor: &WorkloadSagaIntent,
) -> Result<WorkloadSagaRecord, serde_json::Error> {
    let mut encoded = serde_json::to_value(withdrawal)?;
    encoded["successorIntent"] = serde_json::to_value(successor)?;
    encoded["teardownDisposition"]["context"]["cause"]["generation"] =
        serde_json::to_value(successor.generation())?;
    encoded["teardownDisposition"]["context"]["cause"]["desiredDigest"] =
        serde_json::to_value(successor.desired_digest())?;
    encoded["teardownDisposition"]["context"]["successorFence"]["generation"] =
        serde_json::to_value(successor.generation())?;
    encoded["teardownDisposition"]["context"]["successorFence"]["desiredDigest"] =
        serde_json::to_value(successor.desired_digest())?;
    encoded["lastTransition"]["successorGeneration"] =
        serde_json::to_value(successor.generation())?;
    rehash_encoded_record(&mut encoded);
    serde_json::from_value(encoded)
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

    let (failed_before_successor, failed_claim, failed_result) =
        definite_failure_before_successor_restart(2);
    assert_eq!(failed_before_successor.phase(), WorkloadSagaPhase::Observed);
    assert!(matches!(
        failed_before_successor
            .restart_state()
            .active()
            .expect("terminal restart failure should remain active until teardown")
            .disposition(),
        WorkloadRestartDisposition::DefiniteFailure {
            claim: retained_claim,
            result: retained_result,
        } if retained_claim == &failed_claim && retained_result == &failed_result
    ));
    let failure_withdrawal = failed_before_successor
        .commit_restart_settlement_teardown()
        .expect("restart failure that preceded its successor should settle before withdrawal");
    let failure_settlement = failure_withdrawal
        .teardown_disposition()
        .expect("withdrawal should own teardown")
        .context()
        .restart_settlement()
        .expect("withdrawal should retain the exact failed restart result");
    assert_eq!(
        failure_withdrawal.phase(),
        WorkloadSagaPhase::WithdrawalCommitted
    );
    assert!(failure_withdrawal.restart_state().active().is_none());
    assert_eq!(failure_settlement.claim(), &failed_claim);
    assert_eq!(failure_settlement.result(), &failed_result);
    assert_ne!(
        failure_settlement.source_execution(),
        failure_settlement.target_execution()
    );

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
    assert_eq!(
        released.decide_teardown().unwrap(),
        WorkloadTeardownDecision::PersistCandidate(
            ProposedWorkloadTeardownTransition::RecordTerminal
        )
    );
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
    let recorded = recovered
        .record_terminal_teardown()
        .expect("exact restart settlement should be consumed into terminal evidence");
    assert_eq!(recorded.phase(), WorkloadSagaPhase::Recorded);
    assert!(recorded.teardown_disposition().is_none());
    let WorkloadPhaseDetail::Recorded(recorded_detail) = recorded.phase_detail() else {
        panic!("restart settlement teardown should finish with recorded evidence");
    };
    assert_ne!(recorded_detail.terminal_evidence_digest(), terminal_digest);
    assert_eq!(
        recorded_detail.terminal_execution_reference(),
        released_detail.terminal_execution_reference()
    );
}

#[test]
fn restart_target_effects_become_exact_teardown_subjects() {
    let (pending, claim) = pending_restart_activation();
    let WorkloadSagaIntentUpdate::Transition(fenced) = pending
        .apply_intent(stopped_intent(2))
        .expect("stopped successor should fence target activation")
    else {
        panic!("stopped successor must change the durable record");
    };
    let settled = fenced
        .apply_restart_effect_result(
            &claim,
            WorkloadRestartEffectResult::Succeeded {
                evidence: WorkloadRestartEvidenceDigest::sha256("target-activation-settled"),
            },
        )
        .expect("exact target activation result should settle after successor veto");
    let withdrawal = settled
        .commit_restart_settlement_teardown()
        .expect("settled target activation should enter canonical teardown");
    let settlement = withdrawal
        .teardown_disposition()
        .and_then(|disposition| disposition.context().restart_settlement())
        .expect("target teardown should retain exact restart settlement");
    let WorkloadPhaseDetail::Teardown(detail) = withdrawal.phase_detail() else {
        panic!("restart target teardown should retain exact phase detail");
    };
    assert_eq!(detail.origin(), WorkloadSagaPhase::WorkloadActivated);
    assert_eq!(
        detail.retained_references().execution(),
        Some(settlement.target_execution())
    );
    assert_ne!(
        detail.retained_references().execution(),
        Some(settlement.source_execution())
    );

    let released = advance_teardown(&withdrawal, WorkloadSagaPhase::NetworkReleased);
    let recorded = released
        .record_terminal_teardown()
        .expect("target teardown should consume settlement into Recorded");
    let WorkloadPhaseDetail::Recorded(recorded_detail) = recorded.phase_detail() else {
        panic!("target teardown should finish with recorded evidence");
    };
    assert_eq!(
        recorded_detail.terminal_execution_reference(),
        Some(settlement.target_execution())
    );
}

#[test]
fn restart_failure_without_successor_does_not_auto_retire() {
    let (pending, claim) = pending_restart();
    let failure = WorkloadRestartEffectResult::Failed {
        evidence: WorkloadRestartEvidenceDigest::sha256("restart-no-successor-failure"),
    };
    let failed = pending
        .apply_restart_effect_result(&claim, failure.clone())
        .expect("exact restart failure should persist");
    let before = serde_json::to_vec(&failed).expect("failed restart should encode");

    assert!(failed.commit_restart_settlement_teardown().is_err());
    assert_eq!(
        serde_json::to_vec(&failed).expect("rejected handoff should preserve the record"),
        before
    );
    assert_eq!(failed.phase(), WorkloadSagaPhase::Observed);
    assert!(failed.successor_intent().is_none());
    assert!(failed.teardown_disposition().is_none());
    assert!(matches!(
        failed.restart_state().active().unwrap().disposition(),
        WorkloadRestartDisposition::DefiniteFailure {
            claim: retained_claim,
            result: retained_result,
        } if retained_claim == &claim && retained_result == &failure
    ));
}

#[test]
fn crossed_restart_settlement_evidence_is_rejected_without_record_change() {
    let (settled, claim, result) = definite_failure_before_successor_restart(2);
    let withdrawal = settled
        .commit_restart_settlement_teardown()
        .expect("exact failed restart should enter withdrawal");
    let retained = withdrawal
        .teardown_disposition()
        .unwrap()
        .context()
        .restart_settlement()
        .expect("withdrawal should retain restart settlement");
    let before = serde_json::to_vec(&settled).expect("settled restart should encode");
    let source = retained.source_execution().clone();
    let target = retained.target_execution().clone();

    let crossed_request = WorkloadRestartRequestId::for_explicit(
        settled.saga_id(),
        settled.active_intent().source().source_generation(),
        "crossed-settlement-request",
    )
    .expect("crossed request should validate independently");
    let crossed_request_claim = WorkloadRestartCommandClaim::initial(
        crossed_request,
        claim.restart_epoch(),
        claim.attempt_id().clone(),
        claim.step(),
        claim.issuing_revision(),
    )
    .expect("crossed request claim should validate independently");
    let crossed_request_settlement = WorkloadRestartTeardownSettlement::new(
        crossed_request_claim,
        result.clone(),
        source.clone(),
        target.clone(),
        retained.owner_observations().to_vec(),
    )
    .expect("crossed request settlement should validate internally");
    let crossed_request_candidate =
        withdrawal_with_restart_settlement(&withdrawal, &crossed_request_settlement)
            .expect("crossed request candidate should validate internally");
    assert!(
        settled
            .validate_successor(&crossed_request_candidate)
            .is_err()
    );

    let crossed_restart_epoch = claim
        .restart_epoch()
        .checked_next()
        .expect("crossed restart epoch should fit");
    let crossed_epoch_target = WorkloadExecutionReference::for_restart_epoch(
        settled.active_intent(),
        crossed_restart_epoch,
    );
    let crossed_epoch_claim = WorkloadRestartCommandClaim::initial(
        claim.request_id().clone(),
        crossed_restart_epoch,
        crossed_epoch_target.attempt_id().clone(),
        claim.step(),
        claim.issuing_revision(),
    )
    .expect("crossed restart epoch claim should validate independently");
    let crossed_epoch_settlement = WorkloadRestartTeardownSettlement::new(
        crossed_epoch_claim,
        result.clone(),
        source.clone(),
        crossed_epoch_target,
        retained.owner_observations().to_vec(),
    )
    .expect("crossed restart epoch settlement should validate internally");
    let crossed_epoch_candidate =
        withdrawal_with_restart_settlement(&withdrawal, &crossed_epoch_settlement)
            .expect("crossed restart epoch candidate should validate internally");
    assert!(
        settled
            .validate_successor(&crossed_epoch_candidate)
            .is_err()
    );

    let crossed_target_epoch = crossed_restart_epoch
        .checked_next()
        .expect("crossed target epoch should fit");
    let crossed_target = WorkloadExecutionReference::for_restart_epoch(
        settled.active_intent(),
        crossed_target_epoch,
    );
    let crossed_target_claim = WorkloadRestartCommandClaim::initial(
        claim.request_id().clone(),
        claim.restart_epoch(),
        crossed_target.attempt_id().clone(),
        claim.step(),
        claim.issuing_revision(),
    )
    .expect("crossed target claim should validate independently");
    let crossed_target_settlement = WorkloadRestartTeardownSettlement::new(
        crossed_target_claim,
        result.clone(),
        source.clone(),
        crossed_target,
        retained.owner_observations().to_vec(),
    )
    .expect("crossed target settlement should validate internally");
    assert!(
        withdrawal_with_restart_settlement(&withdrawal, &crossed_target_settlement).is_err(),
        "target attempt crossed with its restart epoch must fail intrinsic record validation"
    );

    let (pending, pending_claim) = pending_restart();
    let inspection = pending
        .restart_dispatch_to_inspection(&pending_claim)
        .expect("exact pending claim should enter inspection");
    let absence = WorkloadRestartAbsenceEvidence::for_inspection(
        &inspection,
        &pending_claim,
        WorkloadRestartEvidenceDigest::sha256("crossed-dispatch-epoch-absence"),
    )
    .expect("exact inspection absence should validate");
    let crossed_dispatch_claim = WorkloadRestartCommandClaim::retry_after_absence(
        &pending_claim,
        inspection.revision(),
        absence,
    )
    .expect("next dispatch epoch should validate independently");
    let crossed_dispatch_settlement = WorkloadRestartTeardownSettlement::new(
        crossed_dispatch_claim,
        result.clone(),
        source.clone(),
        target.clone(),
        retained.owner_observations().to_vec(),
    )
    .expect("crossed dispatch settlement should validate internally");
    let crossed_dispatch_candidate =
        withdrawal_with_restart_settlement(&withdrawal, &crossed_dispatch_settlement)
            .expect("crossed dispatch candidate should validate internally");
    assert!(
        settled
            .validate_successor(&crossed_dispatch_candidate)
            .is_err()
    );

    let crossed_result_settlement = WorkloadRestartTeardownSettlement::new(
        claim.clone(),
        WorkloadRestartEffectResult::Failed {
            evidence: WorkloadRestartEvidenceDigest::sha256("crossed-settlement-result"),
        },
        source.clone(),
        target,
        retained.owner_observations().to_vec(),
    )
    .expect("crossed terminal result should validate independently");
    let crossed_result_candidate =
        withdrawal_with_restart_settlement(&withdrawal, &crossed_result_settlement)
            .expect("crossed result candidate should validate internally");
    assert!(
        settled
            .validate_successor(&crossed_result_candidate)
            .is_err()
    );

    let crossed_source = WorkloadExecutionReference::for_restart_epoch(
        settled.active_intent(),
        crossed_target_epoch,
    );
    let crossed_source_settlement = WorkloadRestartTeardownSettlement::new(
        claim,
        result,
        crossed_source,
        retained.target_execution().clone(),
        retained.owner_observations().to_vec(),
    )
    .expect("crossed source settlement should validate independently");
    assert!(
        withdrawal_with_restart_settlement(&withdrawal, &crossed_source_settlement).is_err(),
        "crossed source execution must fail intrinsic record validation"
    );

    let crossed_generation = stopped_intent(3);
    let crossed_generation_candidate = withdrawal_with_successor(&withdrawal, &crossed_generation)
        .expect("crossed successor generation should validate internally");
    assert!(
        settled
            .validate_successor(&crossed_generation_candidate)
            .is_err()
    );

    let crossed_digest = intent_with(
        "tenant-a",
        "workload-a",
        2,
        DesiredWorkloadState::Stopped,
        WorkloadActivationIntent::PrepareOnly,
        WorkloadPublicationIntent::Withheld,
        73,
    );
    let crossed_digest_candidate = withdrawal_with_successor(&withdrawal, &crossed_digest)
        .expect("crossed successor digest should validate internally");
    assert!(
        settled
            .validate_successor(&crossed_digest_candidate)
            .is_err()
    );

    assert_eq!(
        serde_json::to_vec(&settled).expect("rejected evidence should preserve the record"),
        before
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
