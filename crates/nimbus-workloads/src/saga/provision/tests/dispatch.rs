use nimbus_network::{NetworkCapabilityRole, NetworkCapabilitySourceDigest};

use super::*;

fn initial_activation_claim(label: &str) -> WorkloadProvisionDispatchClaim {
    let attempt = activation_attempt(
        attempt_key(label),
        prerequisite(0x61),
        Some(selection_evidence()),
    );
    let target = WorkloadProvisionProviderTarget::for_attempt(&attempt)
        .expect("provider target should validate")
        .expect("activation requires an execution provider");
    WorkloadProvisionDispatchClaim::initial(attempt, target)
        .expect("initial dispatch claim should validate")
}

fn absence_for_claim(
    claim: &WorkloadProvisionDispatchClaim,
    confirmed_revision: u64,
    label: &str,
) -> WorkloadProvisionAbsenceEvidence {
    serde_json::from_value(json!({
        "attemptId": claim.attempt().attempt_id(),
        "dispatchEpoch": claim.dispatch_epoch().to_string(),
        "confirmedRevision": confirmed_revision.to_string(),
        "transitionId": format!("wst_{}", "7".repeat(64)),
        "providerTarget": claim.provider_target(),
        "step": claim.attempt().step(),
        "evidence": WorkloadOwnerEvidenceDigest::sha256(label),
    }))
    .expect("absence fixture should decode")
}

#[test]
fn provider_target_uses_execution_identity_without_network_role() {
    let claim = initial_activation_claim("execution-target");
    assert_eq!(
        claim.provider_target(),
        &WorkloadProvisionProviderTarget::Execution {
            provider_id: WorkloadExecutionProviderId::for_registration_key("execution-a"),
            provider_source_digest: WorkloadProvisionSourceDigest::sha256("source"),
        }
    );
}

#[test]
fn network_target_binds_selected_attachment_role_provider_and_digest() {
    let selection = selection_evidence();
    let mut input = activation_attempt_input(
        attempt_key("network-target"),
        prerequisite(0x62),
        Some(selection.clone()),
    );
    input.issuing_revision = WorkloadSagaRevision::new(0);
    input.source_phase = WorkloadSagaPhase::IntentCommitted;
    input.target_phase = WorkloadSagaPhase::NetworkReserved;
    input.step = WorkloadProvisionStep::ReserveNetwork;
    input.subjects = WorkloadProvisionSubjects::Network(network_reference());
    input.prerequisite = None;
    let attempt = WorkloadProvisionAttempt::new(input).expect("network attempt should validate");
    assert_eq!(
        WorkloadProvisionProviderTarget::for_attempt(&attempt)
            .expect("target should validate")
            .expect("selected network attempt requires a target"),
        WorkloadProvisionProviderTarget::Network {
            role: NetworkCapabilityRole::Attachment,
            provider_id: selection.selection().attachment_provider_id().clone(),
            provider_source_digest: selection.source_digest(),
        }
    );
}

#[test]
fn resource_free_network_attempt_has_no_provider_target() {
    let mut input =
        activation_attempt_input(attempt_key("resource-free"), prerequisite(0x63), None);
    input.issuing_revision = WorkloadSagaRevision::new(0);
    input.source_phase = WorkloadSagaPhase::IntentCommitted;
    input.target_phase = WorkloadSagaPhase::NetworkReserved;
    input.step = WorkloadProvisionStep::ReserveNetwork;
    input.subjects = WorkloadProvisionSubjects::Network(network_reference());
    input.prerequisite = None;
    let attempt = WorkloadProvisionAttempt::new(input).expect("network attempt should validate");
    assert_eq!(
        WorkloadProvisionProviderTarget::for_attempt(&attempt)
            .expect("resource-free target decision should validate"),
        None
    );
}

#[test]
fn dispatch_claim_wire_rejects_crossed_epoch_revision_and_provider() {
    let claim = initial_activation_claim("wire");
    let exact = serde_json::to_value(&claim).expect("claim should encode");
    assert_eq!(
        serde_json::from_value::<WorkloadProvisionDispatchClaim>(exact.clone())
            .expect("exact claim should decode"),
        claim
    );

    let mut wrong_epoch = exact.clone();
    wrong_epoch["dispatchEpoch"] = json!("1");
    assert!(serde_json::from_value::<WorkloadProvisionDispatchClaim>(wrong_epoch).is_err());

    let mut wrong_revision = exact.clone();
    wrong_revision["claimedRevision"] = json!("9");
    assert!(serde_json::from_value::<WorkloadProvisionDispatchClaim>(wrong_revision).is_err());

    let mut wrong_provider = exact;
    wrong_provider["providerTarget"]["providerId"] = json!(
        WorkloadExecutionProviderId::for_registration_key("crossed-execution")
    );
    assert!(serde_json::from_value::<WorkloadProvisionDispatchClaim>(wrong_provider).is_err());
}

#[test]
fn dispatch_epoch_and_inspection_wire_reject_unknown_noncanonical_values() {
    let claim = initial_activation_claim("strict-wire");
    let exact_claim = serde_json::to_value(&claim).expect("claim should encode");

    for invalid_epoch in [json!(0), json!(""), json!("00"), json!("+0"), json!(" 0")] {
        let mut invalid = exact_claim.clone();
        invalid["dispatchEpoch"] = invalid_epoch;
        assert!(serde_json::from_value::<WorkloadProvisionDispatchClaim>(invalid).is_err());
    }

    let exact_result = serde_json::to_value(WorkloadProvisionInspectionResult::Ambiguous {
        attempt_id: claim.attempt().attempt_id().clone(),
        dispatch_epoch: claim.dispatch_epoch(),
        provider_target: claim.provider_target().clone(),
    })
    .expect("inspection result should encode");
    assert!(
        serde_json::from_value::<WorkloadProvisionInspectionResult>(exact_result.clone()).is_ok()
    );

    let mut unknown_kind = exact_result.clone();
    unknown_kind["kind"] = json!("unrecognized");
    assert!(serde_json::from_value::<WorkloadProvisionInspectionResult>(unknown_kind).is_err());

    let mut unknown_field = exact_result;
    unknown_field["unexpected"] = json!(true);
    assert!(serde_json::from_value::<WorkloadProvisionInspectionResult>(unknown_field).is_err());
}

#[test]
fn exact_absence_authorizes_same_attempt_at_next_epoch_only() {
    let initial = initial_activation_claim("retry");
    let absence = absence_for_claim(&initial, 6, "absent");
    let retry = WorkloadProvisionDispatchClaim::retry_after_absence(
        &initial,
        WorkloadSagaRevision::new(7),
        absence.clone(),
    )
    .expect("exact absence should authorize retry");

    assert_eq!(retry.attempt().attempt_id(), initial.attempt().attempt_id());
    assert_eq!(
        retry.dispatch_epoch(),
        WorkloadProvisionDispatchEpoch::new(1)
    );
    assert!(matches!(
        retry.authorization(),
        WorkloadProvisionDispatchAuthorization::RetryAfterAbsence(retained)
            if retained == &absence
    ));

    let crossed = initial_activation_claim("retry-crossed");
    let crossed_absence = absence_for_claim(&crossed, 6, "crossed-absence");
    assert!(
        WorkloadProvisionDispatchClaim::retry_after_absence(
            &initial,
            WorkloadSagaRevision::new(7),
            crossed_absence,
        )
        .is_err()
    );
}

#[test]
fn retry_authorization_wire_rejects_crossed_absence_revision() {
    let initial = initial_activation_claim("retry-wire-revision");
    let absence = absence_for_claim(&initial, 6, "retry-wire-absent");
    let retry = WorkloadProvisionDispatchClaim::retry_after_absence(
        &initial,
        WorkloadSagaRevision::new(7),
        absence,
    )
    .expect("exact absence should authorize retry");
    let mut crossed_revision = serde_json::to_value(&retry).expect("retry claim should encode");
    crossed_revision["authorization"]["evidence"]["confirmedRevision"] = json!("5");
    assert!(
        serde_json::from_value::<WorkloadProvisionDispatchClaim>(crossed_revision).is_err(),
        "retry wire must bind the absence revision immediately preceding the claim"
    );
}

#[test]
fn inspection_result_rejects_crossed_attempt_epoch_and_provider() {
    let claim = initial_activation_claim("inspection");
    let exact = WorkloadProvisionInspectionResult::InProgress {
        attempt_id: claim.attempt().attempt_id().clone(),
        dispatch_epoch: claim.dispatch_epoch(),
        provider_target: claim.provider_target().clone(),
        evidence: WorkloadOwnerEvidenceDigest::sha256("still-starting"),
    };
    exact
        .validate_for_claim(&claim)
        .expect("exact inspection should validate");

    let crossed_epoch = WorkloadProvisionInspectionResult::Ambiguous {
        attempt_id: claim.attempt().attempt_id().clone(),
        dispatch_epoch: WorkloadProvisionDispatchEpoch::new(1),
        provider_target: claim.provider_target().clone(),
    };
    assert!(crossed_epoch.validate_for_claim(&claim).is_err());

    let crossed_provider = WorkloadProvisionInspectionResult::DefiniteFailure {
        attempt_id: claim.attempt().attempt_id().clone(),
        dispatch_epoch: claim.dispatch_epoch(),
        provider_target: WorkloadProvisionProviderTarget::Network {
            role: NetworkCapabilityRole::Attachment,
            provider_id: provider("crossed-attachment"),
            provider_source_digest: NetworkCapabilitySourceDigest::from_bytes([0x91; 32]),
        },
        failure: WorkloadFailureEvidence::new(
            "crossed_provider",
            WorkloadOwnerEvidenceDigest::sha256("crossed-provider"),
        )
        .expect("failure should validate"),
    };
    assert!(crossed_provider.validate_for_claim(&claim).is_err());
}
