use super::*;

#[test]
fn inspection_success_and_failure_successors_retain_exact_confirmed_command() {
    let withdrawal = withdrawal_record(WorkloadPublicationIntent::PublishWhenReady);
    let (pending, claim) = claim_teardown_step(&withdrawal);
    let inspection = pending.teardown_dispatch_to_inspection(&claim).unwrap();
    let command_id = inspection_command_id(&inspection, &claim);
    let success = inspection
        .apply_teardown_inspection_result(
            &claim,
            WorkloadTeardownInspectionResult::Satisfied {
                attempt_id: claim.attempt().attempt_id().clone(),
                dispatch_epoch: claim.dispatch_epoch(),
                provider_target: claim.provider_target().clone(),
                inspection_command_id: command_id,
                evidence: teardown_success_evidence(&claim, "inspection-success"),
            },
        )
        .unwrap();

    let receipt = &success
        .teardown_disposition()
        .unwrap()
        .context()
        .completed()[0];
    assert!(matches!(
        receipt.confirmation(),
        WorkloadTeardownResultConfirmation::Inspection {
            inspected_revision,
            inspected_transition_id,
            inspection_command_id,
        } if *inspected_revision == inspection.revision()
            && inspected_transition_id == inspection.last_transition().transition_id()
            && *inspection_command_id == command_id
    ));

    let mut forged_dispatch = serde_json::to_value(&success).unwrap();
    forged_dispatch["teardownDisposition"]["context"]["completed"][0]["confirmation"] =
        json!({ "kind": "dispatch" });
    rehash_encoded_record(&mut forged_dispatch);
    let forged_dispatch: WorkloadSagaRecord = serde_json::from_value(forged_dispatch)
        .expect("dispatch-origin forgery should remain internally self-consistent");
    assert!(inspection.validate_successor(&forged_dispatch).is_err());

    let other = withdrawal_record_for(WorkloadPublicationIntent::PublishWhenReady, 3);
    let (other_pending, other_claim) = claim_teardown_step(&other);
    let other_inspection = other_pending
        .teardown_dispatch_to_inspection(&other_claim)
        .unwrap();
    let crossed_command = WorkloadTeardownCommandId::for_confirmed_dispatch(
        &claim,
        inspection.revision(),
        other_inspection.last_transition().transition_id(),
        WorkloadTeardownCommandMode::Inspect,
    )
    .unwrap();
    let mut crossed_transition = serde_json::to_value(&success).unwrap();
    crossed_transition["teardownDisposition"]["context"]["completed"][0]["confirmation"]["inspectedTransitionId"] =
        serde_json::to_value(other_inspection.last_transition().transition_id()).unwrap();
    crossed_transition["teardownDisposition"]["context"]["completed"][0]["confirmation"]["inspectionCommandId"] =
        serde_json::to_value(crossed_command).unwrap();
    rehash_encoded_record(&mut crossed_transition);
    let crossed_transition: WorkloadSagaRecord = serde_json::from_value(crossed_transition)
        .expect("crossed confirmation should remain internally self-consistent");
    assert!(inspection.validate_successor(&crossed_transition).is_err());

    let failure = failure("inspection-failure");
    let cleanup = inspection
        .apply_teardown_inspection_result(
            &claim,
            WorkloadTeardownInspectionResult::DefiniteFailure {
                attempt_id: claim.attempt().attempt_id().clone(),
                dispatch_epoch: claim.dispatch_epoch(),
                provider_target: claim.provider_target().clone(),
                inspection_command_id: command_id,
                failure,
            },
        )
        .unwrap();
    assert!(matches!(
        cleanup.teardown_disposition(),
        Some(WorkloadTeardownDisposition::DefiniteFailure {
            confirmation: WorkloadTeardownResultConfirmation::Inspection {
                inspected_revision,
                inspected_transition_id,
                inspection_command_id,
            },
            ..
        }) if *inspected_revision == inspection.revision()
            && inspected_transition_id == inspection.last_transition().transition_id()
            && *inspection_command_id == command_id
    ));

    let mut forged_failure = serde_json::to_value(&cleanup).unwrap();
    forged_failure["teardownDisposition"]["confirmation"] = json!({ "kind": "dispatch" });
    rehash_encoded_record(&mut forged_failure);
    let forged_failure: WorkloadSagaRecord = serde_json::from_value(forged_failure)
        .expect("failure-origin forgery should remain internally self-consistent");
    assert!(inspection.validate_successor(&forged_failure).is_err());
}
