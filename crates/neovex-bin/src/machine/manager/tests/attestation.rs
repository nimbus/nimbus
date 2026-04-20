use super::*;

#[test]
fn attestation_repository_prefers_explicit_metadata() {
    assert_eq!(
        attestation_repositories_for_reference(
            "agentstation/neovex-machine-os",
            Some("agentstation/neovex")
        ),
        vec!["agentstation/neovex".to_owned()]
    );
}

#[test]
fn attestation_repository_falls_back_to_known_repo_order() {
    assert_eq!(
        attestation_repositories_for_reference("agentstation/neovex-machine-os", None),
        vec![
            "agentstation/neovex-machine-os".to_owned(),
            "agentstation/neovex".to_owned()
        ]
    );
}

#[test]
fn machine_artifact_metadata_uses_primary_then_fallback_annotations() {
    let mut primary = BTreeMap::new();
    primary.insert(
        OCI_ANNOTATION_MACHINE_ATTESTATION_REPOSITORY.to_owned(),
        "agentstation/neovex".to_owned(),
    );
    let mut fallback = BTreeMap::new();
    fallback.insert(
        OCI_ANNOTATION_SOURCE.to_owned(),
        "https://github.com/agentstation/neovex-machine-os".to_owned(),
    );
    fallback.insert(
        OCI_ANNOTATION_MACHINE_NEOVEX_VERSION.to_owned(),
        "v1.2.3".to_owned(),
    );

    let metadata = machine_artifact_metadata_from_annotations(Some(&primary), Some(&fallback));

    assert_eq!(
        metadata.attestation_repository.as_deref(),
        Some("agentstation/neovex")
    );
    assert_eq!(
        metadata.source_repository_url.as_deref(),
        Some("https://github.com/agentstation/neovex-machine-os")
    );
    assert_eq!(metadata.neovex_version.as_deref(), Some("v1.2.3"));
}
