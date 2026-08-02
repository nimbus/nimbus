use nimbus_workloads::WorkloadSagaStoreError;
use serde_json::{Value, json};

use super::super::codec::{decode_workload_saga_record, encode_workload_saga_record};
use super::{document_for, initial_record, initial_record_with_counters};

#[test]
fn strict_codec_round_trips_exact_physical_shape_and_max_counters() {
    let record = initial_record_with_counters("codec-max", u64::MAX, u64::MAX);
    let fields = encode_workload_saga_record(&record).expect("record should encode");

    assert_eq!(fields.len(), 19);
    assert_eq!(
        fields.get("desiredGeneration"),
        Some(&json!(u64::MAX.to_string()))
    );
    assert_eq!(
        fields.get("networkGeneration"),
        Some(&json!(u64::MAX.to_string()))
    );
    assert_eq!(fields.get("recoveryEligible"), Some(&Value::Bool(true)));
    assert!(!fields.contains_key("successorIntent"));
    assert!(!fields.contains_key("failure"));
    assert_eq!(
        decode_workload_saga_record(&document_for(&record)),
        Ok(record)
    );
}

#[test]
fn strict_codec_rejects_unknown_crossed_and_noncanonical_fields() {
    let record = initial_record("codec-corrupt");
    let cases = [
        ("unknown", json!(true)),
        ("sagaRevision", json!("00")),
        ("networkGeneration", json!("18446744073709551616")),
        ("desiredDigest", json!("ABC")),
        ("recoveryEligible", json!(false)),
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
fn strict_codec_rejects_missing_and_null_optional_shapes() {
    let record = initial_record("codec-missing");
    let mut missing = document_for(&record);
    missing.fields.remove("phaseDetail");
    assert_eq!(
        decode_workload_saga_record(&missing),
        Err(WorkloadSagaStoreError::Corrupt)
    );

    for optional in ["successorIntent", "failure"] {
        let mut null = document_for(&record);
        null.fields.insert(optional.to_owned(), Value::Null);
        assert_eq!(
            decode_workload_saga_record(&null),
            Err(WorkloadSagaStoreError::Corrupt)
        );
    }
}
