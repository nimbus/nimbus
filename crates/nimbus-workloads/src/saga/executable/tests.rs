use serde_json::json;

use super::*;

fn executable(content: &str) -> WorkloadExecutableIntent {
    WorkloadExecutableIntent::new(
        WorkloadExecutableEncoding::SandboxSpecCanonicalJsonV1,
        content,
    )
    .expect("fixture executable should validate")
}

#[test]
fn executable_envelope_round_trip_is_exact() {
    let value = executable(r#"{"env":["TOKEN=secret"],"process":["serve"]}"#);
    let wire = serde_json::to_vec(&value).expect("carrier should serialize");
    let decoded: WorkloadExecutableIntent =
        serde_json::from_slice(&wire).expect("carrier should deserialize");
    assert_eq!(decoded, value);
    assert_eq!(decoded.format_version(), WORKLOAD_EXECUTABLE_FORMAT_VERSION);
    assert_eq!(
        decoded.encoding(),
        WorkloadExecutableEncoding::SandboxSpecCanonicalJsonV1
    );
}

#[test]
fn missing_executable_field_is_rejected() {
    let mut wire = serde_json::to_value(executable("{}")).unwrap();
    wire.as_object_mut().unwrap().remove("content");
    assert!(serde_json::from_value::<WorkloadExecutableIntent>(wire).is_err());
}

#[test]
fn unknown_executable_field_is_rejected() {
    let mut wire = serde_json::to_value(executable("{}")).unwrap();
    wire["cacheKey"] = json!("forbidden");
    assert!(serde_json::from_value::<WorkloadExecutableIntent>(wire).is_err());
}

#[test]
fn duplicate_executable_field_is_rejected() {
    let value = executable("{}");
    let duplicate = format!(
        r#"{{"formatVersion":1,"formatVersion":1,"encoding":"sandbox_spec_canonical_json_v1","content":"{{}}","contentDigest":"{}"}}"#,
        value.content_digest()
    );
    assert!(serde_json::from_str::<WorkloadExecutableIntent>(&duplicate).is_err());
}

#[test]
fn crossed_content_digest_is_rejected() {
    let mut wire = serde_json::to_value(executable("{}")).unwrap();
    wire["content"] = json!(r#"{"different":true}"#);
    assert!(serde_json::from_value::<WorkloadExecutableIntent>(wire).is_err());
}

#[test]
fn unsupported_format_and_encoding_are_rejected() {
    let mut format = serde_json::to_value(executable("{}")).unwrap();
    format["formatVersion"] = json!(2);
    assert!(serde_json::from_value::<WorkloadExecutableIntent>(format).is_err());

    let mut encoding = serde_json::to_value(executable("{}")).unwrap();
    encoding["encoding"] = json!("unknown_v9");
    assert!(serde_json::from_value::<WorkloadExecutableIntent>(encoding).is_err());
}

#[test]
fn oversized_executable_content_is_rejected() {
    assert!(
        WorkloadExecutableIntent::new(
            WorkloadExecutableEncoding::SandboxSpecCanonicalJsonV1,
            "x".repeat(MAX_WORKLOAD_EXECUTABLE_CONTENT_BYTES + 1),
        )
        .is_err()
    );
    assert!(
        WorkloadExecutableIntent::new(
            WorkloadExecutableEncoding::SandboxSpecCanonicalJsonV1,
            String::new(),
        )
        .is_err()
    );
}

#[test]
fn debug_redacts_executable_content() {
    let value = executable(r#"{"env":["NNC63A_SECRET=must-not-leak"]}"#);
    let rendered = format!("{value:?}");
    assert!(rendered.contains("content_bytes"));
    assert!(rendered.contains(&value.content_digest().to_string()));
    assert!(!rendered.contains("NNC63A_SECRET"));
    assert!(!rendered.contains("must-not-leak"));
    assert!(!rendered.contains(value.canonical_content()));
}
