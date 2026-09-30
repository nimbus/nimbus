use super::*;

#[test]
fn attestation_repository_prefers_explicit_metadata() {
    assert_eq!(
        attestation_repositories_for_reference("nimbus/machine-os", Some("nimbus/nimbus")),
        vec!["nimbus/nimbus".to_owned()]
    );
}

#[test]
fn attestation_repository_falls_back_to_known_repo_order() {
    assert_eq!(
        attestation_repositories_for_reference("nimbus/machine-os", None),
        vec!["nimbus/machine-os".to_owned(), "nimbus/nimbus".to_owned()]
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
        "https://github.com/nimbus/machine-os".to_owned(),
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
        Some("https://github.com/nimbus/machine-os")
    );
    assert_eq!(metadata.nimbus_version.as_deref(), Some("v1.2.3"));
}

#[test]
fn build_attestation_missing_is_error() {
    let repos = attestation_repositories_for_reference("nimbus/machine-os", None);
    let reference = "docker://ghcr.io/nimbus/machine-os:v0.0.0";
    let digest = "sha256:0000000000000000000000000000000000000000000000000000000000000000";

    let absent = require_build_attestation(reference, digest, &repos, |_| Ok(0))
        .expect_err("an absent attestation must fail");
    assert!(
        absent.to_string().contains("no valid build attestation"),
        "{absent}"
    );

    let lookup_failure =
        require_build_attestation(reference, digest, &repos, |_| Err("HTTP 404".to_owned()))
            .expect_err("a failed attestation lookup must fail");
    assert!(
        lookup_failure.to_string().contains("HTTP 404"),
        "{lookup_failure}"
    );
}

#[test]
fn build_attestation_found_in_fallback_repository_passes() {
    let repos = attestation_repositories_for_reference("nimbus/machine-os", None);

    let found = require_build_attestation(
        "docker://ghcr.io/nimbus/machine-os:v0.0.0",
        "sha256:abc",
        &repos,
        |repo| {
            if repo == "nimbus/nimbus" {
                Ok(2)
            } else {
                Err("HTTP 404".to_owned())
            }
        },
    )
    .expect("an attestation in a fallback repository should pass");

    assert_eq!(found, ("nimbus/nimbus".to_owned(), 2));
}

#[test]
fn build_attestation_skips_registries_without_an_attestation_source() {
    // Only ghcr.io images have a GitHub attestation source. Other registries
    // keep the OCI blob digest check and skip the attestation lookup.
    for reference in [
        "docker://quay.io/podman/machine-os:5.5",
        "docker://127.0.0.1:5000/nimbus/machine-os:v0.0.0",
    ] {
        check_build_attestation(
            reference,
            "sha256:0000000000000000000000000000000000000000000000000000000000000000",
            None,
        )
        .unwrap_or_else(|error| panic!("{reference} should skip the attestation step: {error}"));
    }
}
