use std::any::TypeId;

use super::*;

#[test]
fn counters_have_distinct_lossless_wire_types() {
    assert_ne!(
        TypeId::of::<WorkloadGeneration>(),
        TypeId::of::<WorkloadSagaRevision>()
    );
}

#[test]
fn decimal_counters_round_trip_losslessly_at_boundaries() {
    for value in [0, TWO_TO_53, u64::MAX] {
        let generation = WorkloadGeneration::new(value);
        let revision = WorkloadSagaRevision::new(value);
        assert_eq!(
            serde_json::to_string(&generation).unwrap(),
            format!("\"{value}\"")
        );
        assert_eq!(
            serde_json::to_string(&revision).unwrap(),
            format!("\"{value}\"")
        );
        assert_eq!(
            serde_json::from_str::<WorkloadGeneration>(&format!("\"{value}\"")).unwrap(),
            generation
        );
        assert_eq!(
            serde_json::from_str::<WorkloadSagaRevision>(&format!("\"{value}\"")).unwrap(),
            revision
        );
    }
    assert_eq!(WorkloadGeneration::new(u64::MAX).checked_next(), None);
    assert_eq!(WorkloadSagaRevision::new(u64::MAX).checked_next(), None);
}

#[test]
fn decimal_counters_reject_noncanonical_or_lossy_forms() {
    for malformed in [
        json!(0),
        json!(1.0),
        json!(""),
        json!("00"),
        json!("01"),
        json!("-1"),
        json!("+1"),
        json!(" 1"),
        json!("1.0"),
        json!("١"),
        json!("18446744073709551616"),
    ] {
        assert!(serde_json::from_value::<WorkloadGeneration>(malformed.clone()).is_err());
        assert!(serde_json::from_value::<WorkloadSagaRevision>(malformed).is_err());
    }
}

#[test]
fn nested_counter_wire_is_decimal_text_and_strict() {
    let record = WorkloadSagaRecord::new(key("tenant-a", "workload-a"), stopped_intent(TWO_TO_53))
        .expect("stopped record should initialize");
    let value = serde_json::to_value(&record).unwrap();
    assert_eq!(
        value["activeIntent"]["generation"],
        json!(TWO_TO_53.to_string())
    );
    assert!(value["activeIntent"]["network"]["plan"]["generation"].is_string());
    assert!(value["activeIntent"]["network"]["content"]["identity"]["generation"].is_string());
    assert_eq!(value["revision"], json!("0"));
    assert_eq!(value["lastTransition"]["resultingRevision"], json!("0"));
    assert_eq!(
        value["phaseDetail"]["value"]["completedGeneration"],
        json!(TWO_TO_53.to_string())
    );

    for path in [
        &["activeIntent", "generation"][..],
        &["activeIntent", "network", "plan", "generation"][..],
        &[
            "activeIntent",
            "network",
            "content",
            "identity",
            "generation",
        ][..],
        &["revision"][..],
        &["lastTransition", "resultingRevision"][..],
        &["phaseDetail", "value", "completedGeneration"][..],
    ] {
        let mut malformed = value.clone();
        let mut slot = &mut malformed;
        for component in path {
            slot = &mut slot[*component];
        }
        *slot = json!(TWO_TO_53);
        assert!(serde_json::from_value::<WorkloadSagaRecord>(malformed).is_err());
    }
}

#[test]
fn stable_ids_are_deterministic_domain_separated_and_length_framed() {
    let first = key("tenant-a", "workload-a");
    assert_eq!(first.saga_id(), first.saga_id());
    assert_ne!(first.saga_id(), key("tenant-b", "workload-a").saga_id());
    assert_ne!(first.saga_id(), key("tenant-a", "workload-b").saga_id());
    assert_ne!(key("a", "bc").saga_id(), key("ab", "c").saga_id());

    let uid = workload_uid(1);
    let node = NodeIdentity::new("node-a").unwrap();
    let execution = WorkloadExecutionId::for_execution(&uid, &node, WorkloadGeneration::new(1));
    assert_eq!(
        execution,
        WorkloadExecutionId::for_execution(&uid, &node, WorkloadGeneration::new(1))
    );
    assert_ne!(
        execution,
        WorkloadExecutionId::for_execution(&workload_uid(2), &node, WorkloadGeneration::new(1))
    );
    assert_ne!(
        execution,
        WorkloadExecutionId::for_execution(
            &uid,
            &NodeIdentity::new("node-b").unwrap(),
            WorkloadGeneration::new(1),
        )
    );
    assert_ne!(
        execution,
        WorkloadExecutionId::for_execution(&uid, &node, WorkloadGeneration::new(2))
    );
    assert_ne!(first.saga_id().as_str(), execution.as_str());
}

#[test]
fn stable_id_and_admission_identity_decoders_reject_malformed_text() {
    for malformed in [
        "bad_0000000000000000000000000000000000000000000000000000000000000000",
        "wsg_00",
        "wsg_G000000000000000000000000000000000000000000000000000000000000000",
        "wsg_z000000000000000000000000000000000000000000000000000000000000000",
    ] {
        assert!(malformed.parse::<WorkloadSagaId>().is_err());
    }
    for malformed in [
        "bad_0000000000000000000000000000000000000000000000000000000000000000",
        "wst_00",
        "wst_F000000000000000000000000000000000000000000000000000000000000000",
        "wst_z000000000000000000000000000000000000000000000000000000000000000",
    ] {
        assert!(serde_json::from_value::<WorkloadSagaTransitionId>(json!(malformed)).is_err());
    }
    assert!(serde_json::from_value::<NodeIdentity>(json!("")).is_err());
    assert!(serde_json::from_value::<TenantWorkloadUid>(json!("twu_00")).is_err());
    assert!(
        serde_json::from_value::<TenantWorkloadUid>(json!(format!("twu_{}", "A".repeat(64))))
            .is_err()
    );
    assert!(serde_json::from_value::<TenantIsolationDecisionId>(json!("tid_00")).is_err());
}

#[test]
fn digest_decoders_reject_wrong_length_uppercase_and_nonhex() {
    for malformed in ["00".to_string(), "A".repeat(64), "z".repeat(64)] {
        let value = json!(malformed);
        assert!(serde_json::from_value::<WorkloadDesiredDigest>(value.clone()).is_err());
        assert!(serde_json::from_value::<WorkloadOwnerEvidenceDigest>(value.clone()).is_err());
        assert!(serde_json::from_value::<WorkloadTerminalEvidenceDigest>(value).is_err());
    }
}
