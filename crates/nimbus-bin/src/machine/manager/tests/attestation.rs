use super::*;

#[test]
fn attestation_repository_prefers_explicit_metadata() {
    assert_eq!(
        attestation_repositories_for_reference("nimbus/nimbus-machine-os", Some("nimbus/nimbus")),
        vec!["nimbus/nimbus".to_owned()]
    );
}

#[test]
fn attestation_repository_falls_back_to_known_repo_order() {
    assert_eq!(
        attestation_repositories_for_reference("nimbus/nimbus-machine-os", None),
        vec![
            "nimbus/nimbus-machine-os".to_owned(),
            "nimbus/nimbus".to_owned()
        ]
    );
}

#[test]
fn machine_artifact_metadata_uses_primary_then_fallback_annotations() {
    let mut primary = BTreeMap::new();
    primary.insert(
        OCI_ANNOTATION_MACHINE_ATTESTATION_REPOSITORY.to_owned(),
        "nimbus/nimbus".to_owned(),
    );
    let mut fallback = BTreeMap::new();
    fallback.insert(
        OCI_ANNOTATION_SOURCE.to_owned(),
        "https://github.com/nimbus/nimbus-machine-os".to_owned(),
    );
    fallback.insert(
        OCI_ANNOTATION_MACHINE_NIMBUS_VERSION.to_owned(),
        "v1.2.3".to_owned(),
    );

    let metadata = machine_artifact_metadata_from_annotations(Some(&primary), Some(&fallback));

    assert_eq!(
        metadata.attestation_repository.as_deref(),
        Some("nimbus/nimbus")
    );
    assert_eq!(
        metadata.source_repository_url.as_deref(),
        Some("https://github.com/nimbus/nimbus-machine-os")
    );
    assert_eq!(metadata.nimbus_version.as_deref(), Some("v1.2.3"));
}
