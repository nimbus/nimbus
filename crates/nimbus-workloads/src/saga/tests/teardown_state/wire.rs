use super::*;

#[test]
fn teardown_record_round_trips_strict_portable_wire() {
    let withdrawal = withdrawal_record(WorkloadPublicationIntent::PublishWhenReady);
    let (pending, claim) = claim_teardown_step(&withdrawal);
    let inspection = pending.teardown_dispatch_to_inspection(&claim).unwrap();
    let encoded = serde_json::to_vec(&inspection).expect("teardown record should encode");
    let decoded: WorkloadSagaRecord =
        serde_json::from_slice(&encoded).expect("strict teardown record should decode");

    assert_eq!(decoded, inspection);
    assert_eq!(decoded.format_version(), WORKLOAD_SAGA_FORMAT_VERSION);
    assert_eq!(WORKLOAD_SAGA_FORMAT_VERSION, 6);
    assert_eq!(
        serde_json::to_vec(decoded.teardown_disposition().unwrap()).unwrap(),
        serde_json::to_vec(inspection.teardown_disposition().unwrap()).unwrap()
    );
}

#[test]
fn recorded_terminal_execution_is_explicit_and_survives_successor_promotion_wire() {
    let recorded = finish_teardown(&withdrawal_record(
        WorkloadPublicationIntent::PublishWhenReady,
    ));
    let WorkloadPhaseDetail::Recorded(detail) = recorded.phase_detail() else {
        panic!("completed teardown should carry recorded detail");
    };
    let terminal_execution = detail
        .terminal_execution_reference()
        .expect("completed execution teardown should retain terminal identity")
        .clone();

    for candidate in [
        recorded.clone(),
        recorded
            .promote_successor()
            .expect("stopped successor should promote"),
    ] {
        let encoded = serde_json::to_value(&candidate).unwrap();
        assert_eq!(
            encoded["phaseDetail"]["value"]["terminalExecution"],
            serde_json::to_value(&terminal_execution).unwrap()
        );
        assert_eq!(
            serde_json::from_value::<WorkloadSagaRecord>(encoded).unwrap(),
            candidate,
            "recorded terminal identity must survive a durable reopen"
        );
    }
}

#[test]
fn recorded_terminal_execution_wire_requires_explicit_value_or_null() {
    let recorded = finish_teardown(&withdrawal_record(
        WorkloadPublicationIntent::PublishWhenReady,
    ));
    let mut missing = serde_json::to_value(&recorded).unwrap();
    missing["phaseDetail"]["value"]
        .as_object_mut()
        .unwrap()
        .remove("terminalExecution");
    assert!(
        serde_json::from_value::<WorkloadSagaRecord>(missing).is_err(),
        "missing terminal execution must not collapse into an explicit no-execution outcome"
    );

    let initial_stopped =
        WorkloadSagaRecord::new(key("tenant-a", "wire-stopped"), stopped_intent(1)).unwrap();
    let explicit_null = serde_json::to_value(&initial_stopped).unwrap();
    assert!(explicit_null["phaseDetail"]["value"]["terminalExecution"].is_null());
    assert_eq!(
        serde_json::from_value::<WorkloadSagaRecord>(explicit_null).unwrap(),
        initial_stopped,
        "explicit null is the canonical source-only no-execution outcome"
    );
}

#[test]
fn teardown_wire_rejects_unknown_missing_null_and_legacy_disposition_fields() {
    let withdrawal = withdrawal_record(WorkloadPublicationIntent::PublishWhenReady);
    let encoded = serde_json::to_value(&withdrawal).unwrap();
    let mut cases = Vec::new();

    let mut unknown = encoded.clone();
    unknown["teardownDisposition"]["unknown"] = json!(true);
    cases.push(("unknown nested field", unknown));
    let mut missing = encoded.clone();
    missing
        .as_object_mut()
        .unwrap()
        .remove("teardownDisposition");
    cases.push(("missing required disposition", missing));
    let mut null = encoded.clone();
    null["teardownDisposition"] = serde_json::Value::Null;
    cases.push(("null required disposition", null));
    let mut legacy = encoded;
    legacy["formatVersion"] = json!(5);
    cases.push(("legacy format", legacy));

    assert_eq!(cases.len(), 4);
    for (case, value) in cases {
        assert!(
            serde_json::from_value::<WorkloadSagaRecord>(value).is_err(),
            "teardown wire case {case} must fail closed"
        );
    }
}

#[test]
fn teardown_wire_rejects_tampered_attempt_claim_epoch_cause_and_digest() {
    let withdrawal = withdrawal_record(WorkloadPublicationIntent::PublishWhenReady);
    let (pending, claim) = claim_teardown_step(&withdrawal);
    let encoded = serde_json::to_value(&pending).unwrap();
    let alternate_attempt = teardown_candidate(&withdrawal_record_for(
        WorkloadPublicationIntent::PublishWhenReady,
        3,
    ))
    .0;
    let mut cases = Vec::new();

    let mut attempt = encoded.clone();
    attempt["teardownDisposition"]["claim"]["attempt"]["attemptId"] =
        json!(alternate_attempt.attempt_id().as_str());
    cases.push(("attempt id", attempt));

    let mut epoch = encoded.clone();
    epoch["teardownDisposition"]["claim"]["dispatchEpoch"] = json!("1");
    cases.push(("claim epoch", epoch));

    let mut cause = encoded.clone();
    cause["teardownDisposition"]["context"]["cause"]["generation"] = json!("3");
    cases.push(("stable cause", cause));

    let mut digest = encoded;
    digest["lastTransition"]["transitionId"] = json!(transition_id(&withdrawal).to_string());
    cases.push(("transition digest", digest));

    let advanced = pending
        .apply_teardown_effect_result(&claim, teardown_success_result(&claim, "wire-receipt"))
        .unwrap();
    let mut mismatched_observation = serde_json::to_value(&advanced).unwrap();
    mismatched_observation["teardownDisposition"]["context"]["completed"][0]["evidence"]["evidence"] =
        serde_json::to_value(evidence("crossed-receipt-observation")).unwrap();
    rehash_encoded_record(&mut mismatched_observation);
    cases.push(("receipt observation", mismatched_observation));

    let mut crossed_input = attempt_input(claim.attempt());
    crossed_input.execution_provider_id =
        WorkloadExecutionProviderId::for_registration_key("crossed-teardown-provider");
    let crossed_attempt = WorkloadTeardownAttempt::new(crossed_input).unwrap();
    let crossed_claim =
        WorkloadTeardownClaim::initial(crossed_attempt, claim.provider_target().clone()).unwrap();
    let crossed_evidence = teardown_success_evidence(&crossed_claim, "crossed-provider-receipt");
    let crossed_receipt = WorkloadTeardownReceipt::new(
        crossed_claim,
        crossed_evidence.clone(),
        WorkloadTeardownResultConfirmation::dispatch(),
    )
    .unwrap();
    let mut crossed_provider = serde_json::to_value(&advanced).unwrap();
    crossed_provider["teardownDisposition"]["context"]["completed"][0] =
        serde_json::to_value(crossed_receipt).unwrap();
    crossed_provider["phaseDetail"]["value"]["terminalObservations"][0] =
        serde_json::to_value(crossed_evidence.terminal_observation()).unwrap();
    rehash_encoded_record(&mut crossed_provider);
    cases.push(("receipt provider identity", crossed_provider));

    assert_eq!(cases.len(), 6);
    for (case, value) in cases {
        assert!(
            serde_json::from_value::<WorkloadSagaRecord>(value).is_err(),
            "tampered teardown wire case {case} must fail closed"
        );
    }
}

#[test]
fn teardown_attempt_wire_requires_digest_bound_optional_fields() {
    let withdrawal = withdrawal_record(WorkloadPublicationIntent::PublishWhenReady);
    let (_, claim) = claim_teardown_step(&withdrawal);
    let encoded = serde_json::to_value(claim.attempt()).unwrap();

    for field in ["selectionEvidence", "successorFence"] {
        let mut missing = encoded.clone();
        missing.as_object_mut().unwrap().remove(field);
        assert!(
            serde_json::from_value::<WorkloadTeardownAttempt>(missing).is_err(),
            "digest-bound field {field} must be present even when its value is null"
        );
    }

    let mut input = attempt_input(claim.attempt());
    input.selection_evidence = None;
    input.successor_fence = None;
    let without_optional_values = WorkloadTeardownAttempt::new(input).unwrap();
    let explicit_nulls = serde_json::to_value(&without_optional_values).unwrap();
    assert!(explicit_nulls["selectionEvidence"].is_null());
    assert!(explicit_nulls["successorFence"].is_null());
    assert_eq!(
        serde_json::from_value::<WorkloadTeardownAttempt>(explicit_nulls).unwrap(),
        without_optional_values,
        "explicit null remains the canonical encoding for an absent optional value"
    );
}

#[test]
fn cleanup_wire_rejects_rewritten_completed_receipt_observation() {
    let withdrawal = withdrawal_record(WorkloadPublicationIntent::PublishWhenReady);
    let (pending, claim) = claim_teardown_step(&withdrawal);
    let withdrawn = pending
        .apply_teardown_effect_result(
            &claim,
            teardown_success_result(&claim, "completed-withdraw"),
        )
        .unwrap();
    let (drain_pending, drain_claim) = claim_teardown_step(&withdrawn);
    let cleanup = drain_pending
        .apply_teardown_effect_result(
            &drain_claim,
            WorkloadTeardownEffectResult::DefiniteFailure {
                attempt_id: drain_claim.attempt().attempt_id().clone(),
                dispatch_epoch: drain_claim.dispatch_epoch(),
                provider_target: drain_claim.provider_target().clone(),
                failure: failure("drain-failed"),
            },
        )
        .unwrap();

    let mut rewritten = serde_json::to_value(&cleanup).unwrap();
    rewritten["teardownDisposition"]["context"]["completed"][0]["evidence"]["evidence"] =
        serde_json::to_value(evidence("rewritten-cleanup-receipt")).unwrap();
    rehash_encoded_record(&mut rewritten);
    assert!(
        serde_json::from_value::<WorkloadSagaRecord>(rewritten).is_err(),
        "cleanup recovery must retain an independent receipt-observation correspondence"
    );

    let replacement = evidence("coordinated-cleanup-rewrite");
    let mut coordinated = serde_json::to_value(&cleanup).unwrap();
    coordinated["teardownDisposition"]["context"]["completed"][0]["evidence"]["evidence"] =
        serde_json::to_value(replacement).unwrap();
    coordinated["teardownDisposition"]["priorTerminalObservations"][0]["evidence"] =
        serde_json::to_value(replacement).unwrap();
    rehash_encoded_record(&mut coordinated);
    let coordinated: WorkloadSagaRecord = serde_json::from_value(coordinated)
        .expect("coordinated cleanup rewrite should be internally self-consistent");
    assert!(drain_pending.validate_successor(&coordinated).is_err());
}
