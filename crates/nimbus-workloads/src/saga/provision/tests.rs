use nimbus_core::{TenantId, WorkloadId};
use nimbus_network::{
    NetworkCapabilitySelectionEvidence, NetworkPlanDigest, NetworkPlanId, NetworkProviderId,
};
use serde_json::{Value, json};

use super::*;

fn digest(byte: u8) -> WorkloadExecutableContentDigest {
    WorkloadExecutableContentDigest::from_bytes([byte; 32])
}

fn provider(label: &str) -> NetworkProviderId {
    NetworkProviderId::for_registration_key(label)
}

fn attempt_key(label: &str) -> WorkloadSagaKey {
    WorkloadSagaKey::new(
        TenantId::new(format!("tenant-{label}")).expect("tenant should validate"),
        WorkloadId::new(format!("workload-{label}")).expect("workload should validate"),
    )
}

fn execution_reference() -> WorkloadExecutionReference {
    let workload_uid: TenantWorkloadUid = format!("twu_{}", "22".repeat(32))
        .try_into()
        .expect("workload UID should validate");
    let node_identity = NodeIdentity::new("node-a").expect("node should validate");
    let generation = WorkloadGeneration::new(1);
    let execution_id =
        WorkloadExecutionId::for_execution(&workload_uid, &node_identity, generation);
    serde_json::from_value(json!({
        "workloadUid": workload_uid,
        "nodeIdentity": node_identity,
        "executionId": execution_id,
        "generation": "1",
        "desiredDigest": "44".repeat(32)
    }))
    .expect("exact execution reference should decode")
}

fn network_reference() -> WorkloadNetworkReference {
    serde_json::from_value(json!({
        "planId": NetworkPlanId::generate(),
        "generation": "1",
        "digest": NetworkPlanDigest::from_bytes([0x31; 32])
    }))
    .expect("exact network reference should decode")
}

fn selection_evidence() -> NetworkCapabilitySelectionEvidence {
    serde_json::from_value(json!({
        "selection": {
            "attachment_provider_id": provider("attachment-a"),
            "ingress_provider_id": provider("ingress-a")
        },
        "source_digest": "55".repeat(32)
    }))
    .expect("selection evidence should decode")
}

fn activation_attempt(
    key: WorkloadSagaKey,
    prerequisite: WorkloadProvisionPrerequisiteEvidence,
    selection_evidence: Option<NetworkCapabilitySelectionEvidence>,
) -> WorkloadProvisionAttempt {
    WorkloadProvisionAttempt::new(activation_attempt_input(
        key,
        prerequisite,
        selection_evidence,
    ))
    .expect("activation attempt should validate")
}

fn activation_attempt_input(
    key: WorkloadSagaKey,
    prerequisite: WorkloadProvisionPrerequisiteEvidence,
    selection_evidence: Option<NetworkCapabilitySelectionEvidence>,
) -> WorkloadProvisionAttemptInput {
    WorkloadProvisionAttemptInput {
        saga_id: key.saga_id(),
        key,
        issuing_revision: WorkloadSagaRevision::new(4),
        generation: WorkloadGeneration::new(1),
        desired_digest: WorkloadDesiredDigest::sha256("desired"),
        required_node: NodeIdentity::new("node-a").expect("node should validate"),
        source_digest: WorkloadProvisionSourceDigest::sha256("source"),
        network_plan_digest: NetworkPlanDigest::from_bytes([0x32; 32]),
        selection_evidence,
        source_phase: WorkloadSagaPhase::NetworkAttached,
        target_phase: WorkloadSagaPhase::WorkloadActivated,
        step: WorkloadProvisionStep::ActivateWorkload,
        subjects: WorkloadProvisionSubjects::Execution(execution_reference()),
        prerequisite: Some(prerequisite),
    }
}

fn prerequisite(byte: u8) -> WorkloadProvisionPrerequisiteEvidence {
    WorkloadProvisionPrerequisiteEvidence::new(
        format!("wpa_{}", format!("{byte:02x}").repeat(32))
            .parse()
            .expect("attempt ID should validate"),
        WorkloadProvisionSuccessEvidence::ActivationPrerequisitesReady {
            network: network_reference(),
            execution: execution_reference(),
            evidence: WorkloadOwnerEvidenceDigest::sha256(format!("prerequisite-{byte}")),
        },
    )
    .expect("prerequisite should validate")
}

#[test]
fn source_identity_is_closed_and_strict() {
    let standalone = WorkloadProvisionSourceIdentity::standalone_sandbox("sandbox-a", "profile-a")
        .expect("standalone identity should validate");
    let service = WorkloadProvisionSourceIdentity::sandbox_backed_service("service-a")
        .expect("service identity should validate");

    assert_eq!(
        standalone.kind(),
        WorkloadProvisionSourceKind::StandaloneSandbox
    );
    assert_eq!(standalone.profile(), Some("profile-a"));
    assert_eq!(
        service.kind(),
        WorkloadProvisionSourceKind::SandboxBackedService
    );
    assert_eq!(service.profile(), None);

    assert!(WorkloadProvisionSourceIdentity::standalone_sandbox("sandbox-a", "").is_err());
    assert!(WorkloadProvisionSourceIdentity::sandbox_backed_service(" service-a").is_err());
    assert!(
        serde_json::from_value::<WorkloadProvisionSourceIdentity>(json!({
            "kind": "sandbox_backed_service",
            "stableName": "service-a",
            "profile": "forbidden"
        }))
        .is_err()
    );
}

#[test]
fn source_evidence_binds_independent_generation_version_executable_and_provider() {
    let identity = WorkloadProvisionSourceIdentity::standalone_sandbox("sandbox-a", "profile-a")
        .expect("source identity should validate");
    let evidence = WorkloadProvisionSourceEvidence::standalone_sandbox(
        identity,
        WorkloadProvisionSourceGeneration::new(17),
        WorkloadProvisionSourceResourceVersion::new("resource-v4")
            .expect("resource version should validate"),
        digest(0x41),
        provider("attachment-a"),
    )
    .expect("source evidence should validate");

    assert_eq!(evidence.source_generation().as_u64(), 17);
    assert_eq!(evidence.resource_version().as_str(), "resource-v4");
    assert_eq!(evidence.attachment_provider_id(), &provider("attachment-a"));
    evidence
        .validate(digest(0x41))
        .expect("matching executable should validate");
    assert!(evidence.validate(digest(0x42)).is_err());

    let changed_generation = WorkloadProvisionSourceEvidence::standalone_sandbox(
        evidence.source_identity().clone(),
        WorkloadProvisionSourceGeneration::new(18),
        evidence.resource_version().clone(),
        digest(0x41),
        provider("attachment-a"),
    )
    .expect("changed source should validate");
    assert_ne!(evidence.source_digest(), changed_generation.source_digest());
}

#[test]
fn source_evidence_wire_rejects_crossed_digest_and_unknown_fields() {
    let evidence = WorkloadProvisionSourceEvidence::sandbox_backed_service(
        WorkloadProvisionSourceIdentity::sandbox_backed_service("service-a")
            .expect("source identity should validate"),
        WorkloadProvisionSourceGeneration::new(3),
        WorkloadProvisionSourceResourceVersion::new("etag-3")
            .expect("resource version should validate"),
        digest(0x51),
        provider("attachment-a"),
    )
    .expect("source evidence should validate");
    let exact = serde_json::to_value(&evidence).expect("source evidence should serialize");

    assert_eq!(
        serde_json::from_value::<WorkloadProvisionSourceEvidence>(exact.clone())
            .expect("exact source wire should decode"),
        evidence
    );

    let mut unknown = exact.clone();
    unknown["unexpected"] = json!(true);
    assert!(serde_json::from_value::<WorkloadProvisionSourceEvidence>(unknown).is_err());

    let mut crossed = exact;
    crossed["sourceDigest"] = json!("00".repeat(32));
    let decoded = serde_json::from_value::<WorkloadProvisionSourceEvidence>(crossed)
        .expect("source evidence defers executable-bound validation to its containing intent");
    assert_ne!(decoded.source_digest(), evidence.source_digest());
}

#[test]
fn prerequisite_accepts_only_activation_readiness() {
    let attempt_id: WorkloadProvisionAttemptId = format!("wpa_{}", "11".repeat(32))
        .parse()
        .expect("attempt ID should validate");
    let workload_uid: TenantWorkloadUid = format!("twu_{}", "22".repeat(32))
        .try_into()
        .expect("workload UID should validate");
    let node_identity = NodeIdentity::new("node-a").expect("node should validate");
    let generation = WorkloadGeneration::new(1);
    let execution_id =
        WorkloadExecutionId::for_execution(&workload_uid, &node_identity, generation);
    let reference = serde_json::from_value(json!({
        "workloadUid": workload_uid,
        "nodeIdentity": node_identity,
        "executionId": execution_id,
        "generation": "1",
        "desiredDigest": "44".repeat(32)
    }))
    .expect("exact execution reference should decode");
    let rejected = WorkloadProvisionSuccessEvidence::WorkloadPrepared {
        reference,
        evidence: WorkloadOwnerEvidenceDigest::sha256("prepared"),
    };

    assert!(WorkloadProvisionPrerequisiteEvidence::new(attempt_id, rejected).is_err());
}

#[test]
fn unknown_effect_result_variant_is_rejected() {
    for unknown in [
        json!({"kind": "retry"}),
        json!({"kind": "succeeded", "attemptId": "wpa_deadbeef", "unknown": true}),
        json!({"kind": "definite_failure"}),
        json!({"kind": "ambiguous"}),
    ] {
        assert!(serde_json::from_value::<WorkloadProvisionEffectResult>(unknown).is_err());
    }
}

#[test]
fn attempt_identity_binds_saga_key_and_prerequisite() {
    let base = activation_attempt(attempt_key("base"), prerequisite(0x11), None);
    let changed_key = activation_attempt(attempt_key("other"), prerequisite(0x11), None);
    let changed_prerequisite = activation_attempt(attempt_key("base"), prerequisite(0x12), None);

    assert_ne!(base.attempt_id(), changed_key.attempt_id());
    assert_ne!(base.attempt_id(), changed_prerequisite.attempt_id());
    assert_eq!(base.key(), &attempt_key("base"));
    assert_eq!(
        base.prerequisite()
            .expect("activation retains prerequisite")
            .attempt_id(),
        prerequisite(0x11).attempt_id()
    );
}

#[test]
fn attempt_identity_binds_every_named_fence_and_rejects_forged_wire() {
    let input = activation_attempt_input(
        attempt_key("all-fences"),
        prerequisite(0x31),
        Some(selection_evidence()),
    );
    let base = WorkloadProvisionAttempt::new(input.clone()).expect("base attempt should validate");

    let mut valid_mutations = Vec::new();
    let mut changed = input.clone();
    changed.key = attempt_key("changed-key");
    changed.saga_id = changed.key.saga_id();
    valid_mutations.push(changed);
    let mut changed = input.clone();
    changed.issuing_revision = WorkloadSagaRevision::new(5);
    valid_mutations.push(changed);
    let mut changed = input.clone();
    changed.generation = WorkloadGeneration::new(2);
    valid_mutations.push(changed);
    let mut changed = input.clone();
    changed.desired_digest = WorkloadDesiredDigest::sha256("changed-desired");
    valid_mutations.push(changed);
    let mut changed = input.clone();
    changed.required_node = NodeIdentity::new("node-b").expect("node should validate");
    valid_mutations.push(changed);
    let mut changed = input.clone();
    changed.source_digest = WorkloadProvisionSourceDigest::sha256("changed-source");
    valid_mutations.push(changed);
    let mut changed = input.clone();
    changed.network_plan_digest = NetworkPlanDigest::from_bytes([0x91; 32]);
    valid_mutations.push(changed);
    let mut changed = input.clone();
    changed.selection_evidence = None;
    valid_mutations.push(changed);
    let mut changed = input.clone();
    changed.prerequisite = Some(prerequisite(0x32));
    valid_mutations.push(changed);
    let mut changed = input;
    changed.source_phase = WorkloadSagaPhase::WorkloadActivated;
    changed.target_phase = WorkloadSagaPhase::Ready;
    changed.step = WorkloadProvisionStep::InspectWorkloadReadiness;
    changed.subjects = WorkloadProvisionSubjects::Readiness {
        network: network_reference(),
        execution: execution_reference(),
    };
    changed.prerequisite = None;
    valid_mutations.push(changed);

    for changed in valid_mutations {
        let changed = WorkloadProvisionAttempt::new(changed)
            .expect("semantically valid changed attempt should validate");
        assert_ne!(
            changed.attempt_id(),
            base.attempt_id(),
            "every semantically changeable fence must alter attempt identity"
        );
    }

    let exact = serde_json::to_value(&base).expect("attempt should serialize");
    assert_eq!(
        serde_json::from_value::<WorkloadProvisionAttempt>(exact.clone())
            .expect("exact attempt should decode"),
        base
    );
    let forgeries = [
        ("/key/workloadId", json!("workload-forged")),
        ("/sagaId", json!(attempt_key("forged").saga_id())),
        ("/issuingRevision", json!("5")),
        ("/generation", json!("2")),
        ("/desiredDigest", json!("66".repeat(32))),
        ("/requiredNode", json!("node-b")),
        ("/sourceDigest", json!("77".repeat(32))),
        ("/networkPlanDigest", json!("88".repeat(32))),
        ("/selectionEvidence/source_digest", json!("99".repeat(32))),
        ("/sourcePhase", json!("workload_activated")),
        ("/targetPhase", json!("ready")),
        ("/step", json!("inspect_workload_readiness")),
        ("/subjects/kind", json!("readiness")),
        (
            "/prerequisite/attemptId",
            json!(format!("wpa_{}", "aa".repeat(32))),
        ),
    ];
    for (pointer, replacement) in forgeries {
        let mut forged = exact.clone();
        *forged
            .pointer_mut(pointer)
            .unwrap_or_else(|| panic!("fixture pointer {pointer} must exist")) = replacement;
        assert!(
            serde_json::from_value::<WorkloadProvisionAttempt>(forged).is_err(),
            "forged attempt field {pointer} must fail closed"
        );
    }
    let mut unknown = exact;
    unknown["providerHandle"] = json!("forbidden");
    assert!(serde_json::from_value::<WorkloadProvisionAttempt>(unknown).is_err());

    let wire = serde_json::to_string(&base).expect("attempt should serialize");
    for forbidden_identity_field in ["ipAddress", "assignedPort", "providerHandle"] {
        assert!(
            !wire.contains(forbidden_identity_field),
            "attempt identity must not contain {forbidden_identity_field}"
        );
    }
}

#[test]
fn effect_result_round_trips_exactly_three_strict_variants() {
    let attempt = activation_attempt(
        attempt_key("result-wire"),
        prerequisite(0x41),
        Some(selection_evidence()),
    );
    let results = [
        WorkloadProvisionEffectResult::Succeeded {
            attempt_id: attempt.attempt_id().clone(),
            evidence: WorkloadProvisionSuccessEvidence::WorkloadActivated {
                reference: execution_reference(),
                evidence: WorkloadOwnerEvidenceDigest::sha256("activated"),
            },
        },
        WorkloadProvisionEffectResult::DefiniteFailure {
            attempt_id: attempt.attempt_id().clone(),
            failure: WorkloadFailureEvidence::new(
                "activation_failed",
                WorkloadOwnerEvidenceDigest::sha256("activation-failed"),
            )
            .expect("failure should validate"),
        },
        WorkloadProvisionEffectResult::Ambiguous {
            attempt_id: attempt.attempt_id().clone(),
        },
    ];
    for result in results {
        let wire = serde_json::to_value(&result).expect("result should serialize");
        assert_eq!(
            serde_json::from_value::<WorkloadProvisionEffectResult>(wire)
                .expect("exact result should decode"),
            result
        );
    }
}

#[test]
fn connected_attempt_requires_selection_evidence() {
    let evidence = selection_evidence();
    let attempt = activation_attempt(
        attempt_key("connected"),
        prerequisite(0x21),
        Some(evidence.clone()),
    );
    assert_eq!(attempt.selection_evidence(), Some(&evidence));

    let mut wire = serde_json::to_value(&attempt).expect("attempt should encode");
    wire["selectionEvidence"] = Value::Null;
    assert!(
        serde_json::from_value::<WorkloadProvisionAttempt>(wire).is_err(),
        "removing connected selection evidence must invalidate the derived attempt ID"
    );
}
