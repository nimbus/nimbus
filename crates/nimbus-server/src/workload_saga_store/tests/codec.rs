use nimbus_network::NetworkProviderId;
use nimbus_workloads::{
    DesiredWorkloadKind, DesiredWorkloadState, ProposedWorkloadTeardownTransition,
    WORKLOAD_SAGA_FORMAT_VERSION, WorkloadActivationIntent, WorkloadGeneration,
    WorkloadNetworkIntent, WorkloadPublicationIntent, WorkloadSagaIntent, WorkloadSagaIntentUpdate,
    WorkloadSagaPhase, WorkloadSagaRecord, WorkloadSagaStoreError, WorkloadTeardownDecision,
    WorkloadTeardownDisposition, WorkloadTeardownStep,
};
use serde_json::{Map, Value, json};

use super::super::codec::{decode_workload_saga_record, encode_workload_saga_record};
use super::{
    compiled_network_plan, document_for, initial_record, initial_record_with_counters,
    provision_fixture, provision_source,
};

#[test]
fn provision_source_round_trips_through_physical_codec() {
    let record = initial_record_with_counters("codec-max", u64::MAX, u64::MAX);
    let fields = encode_workload_saga_record(&record).expect("record should encode");

    assert_eq!(WORKLOAD_SAGA_FORMAT_VERSION, 6);
    assert_eq!(fields.len(), 23);
    assert_eq!(
        fields.get("desiredGeneration"),
        Some(&json!(u64::MAX.to_string()))
    );
    assert_eq!(
        fields
            .get("compiledNetworkPlan")
            .and_then(|value| value.pointer("/plan/generation")),
        Some(&json!(u64::MAX.to_string()))
    );
    assert_eq!(
        fields
            .get("compiledNetworkPlan")
            .and_then(|value| value.pointer("/content/identity/generation")),
        Some(&json!(u64::MAX.to_string()))
    );
    assert_eq!(fields.get("recoveryEligible"), Some(&Value::Bool(true)));
    assert_eq!(
        fields.get("restartWatchCandidate"),
        Some(&Value::Bool(record.requires_restart_watch()))
    );
    assert_eq!(
        fields.get("executable"),
        Some(
            &serde_json::to_value(record.active_intent().executable())
                .expect("portable executable should encode")
        )
    );
    assert_eq!(
        fields.get("source"),
        Some(
            &serde_json::to_value(record.active_intent().source())
                .expect("portable source should encode")
        )
    );
    assert_eq!(
        fields.get("provisionDisposition"),
        Some(
            &serde_json::to_value(record.provision_disposition())
                .expect("portable provision disposition should encode")
        )
    );
    assert!(!fields.contains_key("successorIntent"));
    assert!(!fields.contains_key("teardownDisposition"));
    assert!(!fields.contains_key("failure"));
    for legacy in ["networkPlanId", "networkGeneration", "networkPlanDigest"] {
        assert!(!fields.contains_key(legacy));
    }

    let physical = fields
        .get("compiledNetworkPlan")
        .expect("compiled network plan is required");
    let portable = serde_json::to_value(record.active_intent().network())
        .expect("portable network intent should encode");
    assert_eq!(physical, &portable);
    assert_eq!(
        serde_json::to_vec(physical).expect("physical plan should encode"),
        serde_json::to_vec(&portable).expect("portable plan should encode"),
        "the physical field must preserve the exact compiled plan and resource bytes"
    );
    assert_eq!(
        decode_workload_saga_record(&document_for(&record)),
        Ok(record)
    );
}

#[test]
fn strict_codec_rejects_unknown_crossed_and_noncanonical_record_fields() {
    let record = initial_record("codec-corrupt");
    let cases = [
        ("unknown", json!(true)),
        ("sagaRevision", json!("00")),
        ("desiredDigest", json!("ABC")),
        ("recoveryEligible", json!(false)),
        ("restartWatchCandidate", json!(true)),
    ];

    for (field, value) in cases {
        let mut document = document_for(&record);
        document.fields.insert(field.to_owned(), value);
        assert_eq!(
            decode_workload_saga_record(&document),
            Err(WorkloadSagaStoreError::Corrupt),
            "{field} corruption must fail closed"
        );
    }

    let mut crossed = document_for(&record);
    crossed.id = nimbus_core::DocumentId::from_key("crossed").expect("fixture id is valid");
    assert_eq!(
        decode_workload_saga_record(&crossed),
        Err(WorkloadSagaStoreError::Corrupt)
    );
}

#[test]
fn strict_codec_rejects_missing_null_legacy_and_partial_compiled_plan_shapes() {
    let record = initial_record("codec-missing");

    for required in ["phaseDetail", "executable", "source", "compiledNetworkPlan"] {
        let mut missing = document_for(&record);
        missing.fields.remove(required);
        assert_eq!(
            decode_workload_saga_record(&missing),
            Err(WorkloadSagaStoreError::Corrupt),
            "missing {required} must fail closed"
        );
    }

    let mut null_plan = document_for(&record);
    null_plan
        .fields
        .insert("compiledNetworkPlan".to_owned(), Value::Null);
    assert_eq!(
        decode_workload_saga_record(&null_plan),
        Err(WorkloadSagaStoreError::Corrupt)
    );

    let complete = document_for(&record)
        .fields
        .get("compiledNetworkPlan")
        .and_then(Value::as_object)
        .expect("fixture compiled plan should be an object")
        .clone();
    let partials = [
        json!({"plan": complete.get("plan").expect("plan is required")}),
        json!({"content": complete.get("content").expect("content is required")}),
        json!({"planId": "digest-only", "generation": "1", "digest": "00"}),
    ];
    for partial in partials {
        let mut document = document_for(&record);
        document
            .fields
            .insert("compiledNetworkPlan".to_owned(), partial);
        assert_eq!(
            decode_workload_saga_record(&document),
            Err(WorkloadSagaStoreError::Corrupt),
            "partial or digest-only plan must fail closed"
        );
    }

    let mut legacy = document_for(&record);
    legacy.fields.remove("compiledNetworkPlan");
    legacy
        .fields
        .insert("networkPlanId".to_owned(), json!("legacy"));
    legacy
        .fields
        .insert("networkGeneration".to_owned(), json!("1"));
    legacy
        .fields
        .insert("networkPlanDigest".to_owned(), json!("00"));
    assert_eq!(
        decode_workload_saga_record(&legacy),
        Err(WorkloadSagaStoreError::Corrupt)
    );

    for optional in [
        "successorIntent",
        "provisionDisposition",
        "teardownDisposition",
        "failure",
    ] {
        let mut null = document_for(&record);
        null.fields.insert(optional.to_owned(), Value::Null);
        assert_eq!(
            decode_workload_saga_record(&null),
            Err(WorkloadSagaStoreError::Corrupt)
        );
    }
}

#[test]
fn strict_codec_rejects_crossed_and_unknown_compiled_plan_content() {
    let first = initial_record("codec-plan-first");
    let second = initial_record("codec-plan-second");
    let second_compiled = document_for(&second)
        .fields
        .get("compiledNetworkPlan")
        .expect("second compiled plan is required")
        .clone();

    let mut crossed_record = document_for(&first);
    crossed_record
        .fields
        .insert("compiledNetworkPlan".to_owned(), second_compiled.clone());
    assert_eq!(
        decode_workload_saga_record(&crossed_record),
        Err(WorkloadSagaStoreError::Corrupt),
        "a valid plan crossed with another transition must fail closed"
    );

    let second_plan = second_compiled
        .get("plan")
        .expect("second plan envelope is required")
        .clone();
    let mut crossed_envelope = document_for(&first);
    compiled_object_mut(&mut crossed_envelope.fields).insert("plan".to_owned(), second_plan);
    assert_eq!(
        decode_workload_saga_record(&crossed_envelope),
        Err(WorkloadSagaStoreError::Corrupt),
        "an envelope crossed with different retained content must fail closed"
    );

    let second_content = second_compiled
        .get("content")
        .expect("second retained content is required")
        .clone();
    let mut crossed_content = document_for(&first);
    compiled_object_mut(&mut crossed_content.fields).insert("content".to_owned(), second_content);
    assert_eq!(
        decode_workload_saga_record(&crossed_content),
        Err(WorkloadSagaStoreError::Corrupt),
        "retained content crossed with a different envelope must fail closed"
    );

    for inner in ["plan", "content"] {
        let mut unknown = document_for(&first);
        compiled_object_mut(&mut unknown.fields)
            .get_mut(inner)
            .and_then(Value::as_object_mut)
            .expect("compiled plan member should be an object")
            .insert("unknown".to_owned(), json!(true));
        assert_eq!(
            decode_workload_saga_record(&unknown),
            Err(WorkloadSagaStoreError::Corrupt),
            "unknown {inner} content must fail closed"
        );
    }
}

#[test]
fn strict_codec_rejects_legacy_unknown_saga_and_inner_plan_versions() {
    let record = initial_record("codec-versions");
    for candidate in [1, 2, 3, 4, 7] {
        let mut document = document_for(&record);
        document
            .fields
            .insert("formatVersion".to_owned(), json!(candidate));
        assert_eq!(
            decode_workload_saga_record(&document),
            Err(WorkloadSagaStoreError::Corrupt),
            "saga format version {candidate} must fail closed"
        );
    }

    for candidate in [0, 1, 3] {
        let mut document = document_for(&record);
        compiled_object_mut(&mut document.fields)
            .get_mut("content")
            .and_then(Value::as_object_mut)
            .expect("compiled content should be an object")
            .insert("formatVersion".to_owned(), json!(candidate));
        assert_eq!(
            decode_workload_saga_record(&document),
            Err(WorkloadSagaStoreError::Corrupt),
            "inner plan format version {candidate} must fail closed"
        );
    }
}

#[test]
fn provision_disposition_round_trips_through_physical_codec() {
    let record = initial_record("codec-provision-disposition");
    let document = document_for(&record);

    assert_eq!(
        document.fields.get("provisionDisposition"),
        Some(
            &serde_json::to_value(record.provision_disposition())
                .expect("portable provision disposition should encode")
        )
    );
    assert_eq!(decode_workload_saga_record(&document), Ok(record));
}

#[test]
fn teardown_disposition_round_trips_through_physical_codec() {
    let record = teardown_ready_record("codec-teardown-round-trip");
    let fields = encode_workload_saga_record(&record).expect("teardown record should encode");
    let portable = serde_json::to_value(
        record
            .teardown_disposition()
            .expect("teardown record should retain a disposition"),
    )
    .expect("portable teardown disposition should encode");

    assert_eq!(fields.len(), 24);
    assert_eq!(fields.get("teardownDisposition"), Some(&portable));
    assert_eq!(
        decode_workload_saga_record(&document_for(&record)),
        Ok(record)
    );
}

#[test]
fn strict_codec_rejects_missing_null_unknown_and_crossed_teardown_disposition() {
    let first = teardown_ready_record("codec-teardown-shape-first");

    let mut missing = document_for(&first);
    missing.fields.remove("teardownDisposition");
    assert_eq!(
        decode_workload_saga_record(&missing),
        Err(WorkloadSagaStoreError::Corrupt),
        "teardown-owned state without its disposition must fail closed"
    );

    let mut null = document_for(&first);
    null.fields
        .insert("teardownDisposition".to_owned(), Value::Null);
    assert_eq!(
        decode_workload_saga_record(&null),
        Err(WorkloadSagaStoreError::Corrupt),
        "a present-null teardown disposition must fail closed"
    );

    let mut unknown = document_for(&first);
    teardown_object_mut(&mut unknown.fields).insert("unknown".to_owned(), json!(true));
    assert_eq!(
        decode_workload_saga_record(&unknown),
        Err(WorkloadSagaStoreError::Corrupt),
        "unknown teardown disposition content must fail closed"
    );

    let second = teardown_ready_record("codec-teardown-shape-second");
    let crossed_disposition = document_for(&second)
        .fields
        .get("teardownDisposition")
        .expect("second teardown disposition is required")
        .clone();
    let mut crossed = document_for(&first);
    crossed.fields.insert(
        "teardownDisposition".to_owned(),
        crossed_disposition.clone(),
    );
    assert_eq!(
        decode_workload_saga_record(&crossed),
        Err(WorkloadSagaStoreError::Corrupt),
        "a teardown disposition crossed with another saga must fail closed"
    );

    let mut crossed_phase = document_for(&initial_record("codec-teardown-crossed-phase"));
    crossed_phase
        .fields
        .insert("teardownDisposition".to_owned(), crossed_disposition);
    assert_eq!(
        decode_workload_saga_record(&crossed_phase),
        Err(WorkloadSagaStoreError::Corrupt),
        "non-teardown state must reject a teardown disposition"
    );
}

#[test]
fn teardown_codec_preserves_exact_attempt_claim_and_transition_bytes() {
    let record = claimed_teardown_record("codec-teardown-claim");
    let disposition = record
        .teardown_disposition()
        .expect("claimed teardown record should retain a disposition");
    let claim = disposition
        .claim()
        .expect("claimed teardown record should retain an exact claim");
    let portable =
        serde_json::to_value(disposition).expect("portable teardown disposition should encode");
    let fields = encode_workload_saga_record(&record).expect("claimed record should encode");
    let physical = fields
        .get("teardownDisposition")
        .expect("physical teardown disposition is required");

    assert_eq!(physical, &portable);
    assert_eq!(
        serde_json::to_vec(physical).expect("physical disposition should encode"),
        serde_json::to_vec(&portable).expect("portable disposition should encode")
    );
    assert_eq!(
        physical.pointer("/claim/attempt/attemptId"),
        Some(&json!(claim.attempt().attempt_id().as_str()))
    );
    assert_eq!(
        physical.pointer("/claim/attempt/issuingTransitionId"),
        Some(&json!(claim.attempt().issuing_transition_id().as_str()))
    );
    assert_eq!(
        physical.pointer("/claim/attempt/issuingRevision"),
        Some(&json!(
            claim.attempt().issuing_revision().as_u64().to_string()
        ))
    );
    assert_eq!(
        physical.pointer("/claim/claimedRevision"),
        Some(&json!(claim.claimed_revision().as_u64().to_string()))
    );
    assert_eq!(
        physical.pointer("/claim/dispatchEpoch"),
        Some(&json!(claim.dispatch_epoch().as_u64().to_string()))
    );

    let decoded = decode_workload_saga_record(&document_for(&record))
        .expect("claimed teardown record should decode");
    let decoded_claim = decoded
        .teardown_disposition()
        .and_then(WorkloadTeardownDisposition::claim)
        .expect("decoded record should retain the claim");
    assert_eq!(
        decoded_claim.attempt().attempt_id(),
        claim.attempt().attempt_id()
    );
    assert_eq!(
        decoded_claim.attempt().issuing_transition_id(),
        claim.attempt().issuing_transition_id()
    );
    assert_eq!(decoded_claim.claimed_revision(), claim.claimed_revision());
    assert_eq!(decoded_claim.dispatch_epoch(), claim.dispatch_epoch());
}

#[test]
fn legacy_format_without_teardown_disposition_fails_closed() {
    let mut legacy_teardown = document_for(&teardown_ready_record("codec-teardown-legacy"));
    legacy_teardown
        .fields
        .insert("formatVersion".to_owned(), json!(5));
    legacy_teardown.fields.remove("teardownDisposition");
    assert_eq!(
        decode_workload_saga_record(&legacy_teardown),
        Err(WorkloadSagaStoreError::Corrupt),
        "the prior format must not bypass required teardown state"
    );

    let mut legacy_non_teardown = document_for(&initial_record("codec-non-teardown-legacy"));
    legacy_non_teardown
        .fields
        .insert("formatVersion".to_owned(), json!(5));
    assert_eq!(
        decode_workload_saga_record(&legacy_non_teardown),
        Err(WorkloadSagaStoreError::Corrupt),
        "the prior format must fail even where teardown state is absent"
    );
}

fn teardown_ready_record(label: &str) -> WorkloadSagaRecord {
    let active = observed_record(label);
    let WorkloadSagaIntentUpdate::Transition(withdrawal) = active
        .apply_intent(stopped_successor(&active, label))
        .expect("stopped successor should commit teardown")
    else {
        panic!("stopped successor must create one durable transition");
    };
    *withdrawal
}

fn claimed_teardown_record(label: &str) -> WorkloadSagaRecord {
    let ready = teardown_ready_record(label)
        .record_resource_free_teardown_step(WorkloadTeardownStep::WithdrawPublication)
        .expect("withheld publication should advance without a provider claim");
    let WorkloadTeardownDecision::PersistCandidate(ProposedWorkloadTeardownTransition::Claim {
        attempt,
        provider_target,
    }) = ready
        .decide_teardown()
        .expect("execution drain should produce one exact claim")
    else {
        panic!("execution drain must require one exact claim");
    };
    ready
        .claim_teardown(*attempt, provider_target)
        .expect("the exact reducer candidate should claim")
}

fn observed_record(label: &str) -> WorkloadSagaRecord {
    let mut history = vec![initial_record(label)];
    while history
        .last()
        .expect("provision history is nonempty")
        .phase()
        != WorkloadSagaPhase::Observed
    {
        provision_fixture::extend_confirmed_step(&mut history);
    }
    history.pop().expect("provision history is nonempty")
}

fn stopped_successor(record: &WorkloadSagaRecord, label: &str) -> WorkloadSagaIntent {
    let generation = WorkloadGeneration::new(2);
    let executable = record.active_intent().executable().clone();
    let source = provision_source(
        &executable,
        &format!("{label}-successor"),
        generation.as_u64(),
        NetworkProviderId::for_registration_key("fixture-attachment"),
    );
    WorkloadSagaIntent::new_without_automatic_restart(
        DesiredWorkloadKind::Sandbox,
        DesiredWorkloadState::Stopped,
        generation,
        executable,
        source,
        WorkloadNetworkIntent::new(compiled_network_plan(
            record.key().tenant_id(),
            &format!("{label}-successor"),
            generation.as_u64(),
            WorkloadActivationIntent::PrepareOnly,
            WorkloadPublicationIntent::Withheld,
        )),
        WorkloadActivationIntent::PrepareOnly,
        WorkloadPublicationIntent::Withheld,
        record.active_intent().admission().clone(),
    )
    .expect("stopped successor should validate")
}

fn compiled_object_mut(fields: &mut Map<String, Value>) -> &mut Map<String, Value> {
    fields
        .get_mut("compiledNetworkPlan")
        .and_then(Value::as_object_mut)
        .expect("fixture compiled plan should be an object")
}

fn teardown_object_mut(fields: &mut Map<String, Value>) -> &mut Map<String, Value> {
    fields
        .get_mut("teardownDisposition")
        .and_then(Value::as_object_mut)
        .expect("fixture teardown disposition should be an object")
}
