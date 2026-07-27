use super::*;

#[test]
fn manifest_deserialization_requires_explicit_launch_authority_phase() {
    let mut wire = serde_json::to_value(sample_manifest(sample_spec(), KrunStartMode::Execute))
        .expect("fixture manifest should serialize");
    wire.as_object_mut()
        .expect("manifest wire should be an object")
        .remove("launch_authority");

    let error = serde_json::from_value::<KrunSandboxManifest>(wire)
        .expect_err("omitted launch authority must not infer provider ownership");
    assert!(
        error.to_string().contains("launch_authority"),
        "the missing required authority field must be explicit: {error}"
    );
}

#[test]
fn manifest_deserialization_requires_explicit_creator_handoff_phase() {
    let mut wire = serde_json::to_value(sample_manifest(sample_spec(), KrunStartMode::Execute))
        .expect("fixture manifest should serialize");
    wire.as_object_mut()
        .expect("manifest wire should be an object")
        .remove("creator_handoff");

    let error = serde_json::from_value::<KrunSandboxManifest>(wire)
        .expect_err("omitted creator handoff must not infer quiescence");
    assert!(
        error.to_string().contains("creator_handoff"),
        "the missing creator authority field must be explicit: {error}"
    );
}

#[test]
fn manifest_deserialization_requires_explicit_provider_failure_cleanup_phase() {
    let mut wire = serde_json::to_value(sample_manifest(sample_spec(), KrunStartMode::Execute))
        .expect("fixture manifest should serialize");
    wire.as_object_mut()
        .expect("manifest wire should be an object")
        .remove("provider_failure_cleanup");

    let error = serde_json::from_value::<KrunSandboxManifest>(wire)
        .expect_err("omitted provider-failure progress must not infer inactive cleanup");
    assert!(
        error.to_string().contains("provider_failure_cleanup"),
        "the missing required cleanup-progress field must be explicit: {error}"
    );
}

#[test]
fn manifest_deserialization_rejects_unknown_launch_authority_phase() {
    let mut wire = serde_json::to_value(sample_manifest(sample_spec(), KrunStartMode::Execute))
        .expect("fixture manifest should serialize");
    wire.as_object_mut()
        .expect("manifest wire should be an object")
        .insert(
            "launch_authority".to_owned(),
            serde_json::json!({"phase": "guessed_from_provider_state"}),
        );

    let error = serde_json::from_value::<KrunSandboxManifest>(wire)
        .expect_err("unknown authority phases must fail closed");
    assert!(
        error
            .to_string()
            .contains("unknown variant `guessed_from_provider_state`"),
        "the invalid phase must be explicit: {error}"
    );
}
