use std::path::Path;

use nimbus_core::{Error, Result};
use nimbus_runtime::RuntimeBundle;
use serde::Serialize;

use super::{
    ArtifactVerificationEvidence, ArtifactVerificationPolicy, ArtifactVerificationRequest,
    ArtifactVerificationSubject, ArtifactVerifierBackend, ArtifactVerifierError,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ArtifactAdmission {
    subject: ArtifactVerificationSubject,
    verification: ArtifactVerificationEvidence,
}

impl ArtifactAdmission {
    fn new(
        subject: ArtifactVerificationSubject,
        verification: ArtifactVerificationEvidence,
    ) -> Self {
        Self {
            subject,
            verification,
        }
    }

    pub fn subject(&self) -> &ArtifactVerificationSubject {
        &self.subject
    }

    pub fn verification(&self) -> &ArtifactVerificationEvidence {
        &self.verification
    }
}

pub fn admit_runtime_bundle_artifact(
    bundle: &RuntimeBundle,
    policy: &ArtifactVerificationPolicy,
    verifier: &dyn ArtifactVerifierBackend,
    context: &str,
) -> Result<ArtifactAdmission> {
    let expected_sha256 = bundle.identity().expected_sha256().ok_or_else(|| {
        Error::PermissionDenied(format!(
            "{context} requires runtime bundle provenance, but bundle `{}` has no immutable sha256 identity",
            bundle.entrypoint().display()
        ))
    })?;
    let expected_sha256 = normalize_sha256(expected_sha256).map_err(|error| {
        Error::InvalidInput(format!(
            "{context} runtime bundle `{}` has invalid sha256 identity: {error}",
            bundle.entrypoint().display()
        ))
    })?;
    ensure_path_matches_sha256(bundle.entrypoint(), &expected_sha256, context)?;
    admit_executable_artifact(
        ArtifactVerificationSubject::RuntimeBundle {
            path: bundle.entrypoint().to_path_buf(),
            sha256: format!("sha256:{expected_sha256}"),
        },
        policy,
        verifier,
        context,
    )
}

pub fn admit_guest_executable_artifact(
    path: impl AsRef<Path>,
    expected_sha256: impl AsRef<str>,
    policy: &ArtifactVerificationPolicy,
    verifier: &dyn ArtifactVerifierBackend,
    context: &str,
) -> Result<ArtifactAdmission> {
    let path = path.as_ref();
    let expected_sha256 = normalize_sha256(expected_sha256.as_ref()).map_err(|error| {
        Error::InvalidInput(format!(
            "{context} guest executable `{}` has invalid sha256 identity: {error}",
            path.display()
        ))
    })?;
    ensure_path_matches_sha256(path, &expected_sha256, context)?;
    admit_executable_artifact(
        ArtifactVerificationSubject::GuestExecutable {
            path: path.to_path_buf(),
            sha256: format!("sha256:{expected_sha256}"),
        },
        policy,
        verifier,
        context,
    )
}

fn admit_executable_artifact(
    subject: ArtifactVerificationSubject,
    policy: &ArtifactVerificationPolicy,
    verifier: &dyn ArtifactVerifierBackend,
    context: &str,
) -> Result<ArtifactAdmission> {
    if !policy.requires_verification() {
        return Err(Error::InvalidInput(format!(
            "{context} artifact provenance admission requires at least one signature, provenance, or SBOM policy requirement"
        )));
    }
    let request = ArtifactVerificationRequest::new(subject.clone(), policy.clone());
    let evidence = verifier.verify_artifact(&request).map_err(|error| {
        Error::PermissionDenied(format!(
            "{context} artifact verifier failed closed for `{}`: {}",
            subject.label(),
            error.redacted()
        ))
    })?;
    ensure_artifact_policy_evidence(policy, &evidence, subject.label(), context)?;
    Ok(ArtifactAdmission::new(subject, evidence))
}

fn ensure_artifact_policy_evidence(
    policy: &ArtifactVerificationPolicy,
    evidence: &ArtifactVerificationEvidence,
    subject_label: &str,
    context: &str,
) -> Result<()> {
    if let Some(signature) = policy.signature() {
        let issuer = signature.issuer();
        let subject = signature.subject();
        if !evidence.signatures().iter().any(|candidate| {
            issuer.is_none_or(|expected| candidate.issuer() == expected)
                && subject.is_none_or(|expected| candidate.subject() == expected)
        }) {
            return Err(Error::PermissionDenied(format!(
                "{context} requires matching signature evidence for {subject_label}"
            )));
        }
    }
    if let Some(provenance) = policy.provenance() {
        let Some(builder_id) = provenance.builder_id() else {
            return Err(Error::PermissionDenied(format!(
                "{context} provenance policy for {subject_label} is missing builder ID"
            )));
        };
        let source_uri = provenance.source_uri();
        if provenance.predicate_types().is_empty()
            && !evidence.attestations().iter().any(|candidate| {
                candidate.builder_id() == builder_id
                    && source_uri.is_none_or(|expected| candidate.source_uri() == Some(expected))
            })
        {
            return Err(Error::PermissionDenied(format!(
                "{context} requires provenance from builder `{builder_id}` for {subject_label}"
            )));
        }
        for predicate_type in provenance.predicate_types() {
            if !evidence.attestations().iter().any(|candidate| {
                candidate.builder_id() == builder_id
                    && source_uri.is_none_or(|expected| candidate.source_uri() == Some(expected))
                    && candidate.predicate_type() == predicate_type
            }) {
                return Err(Error::PermissionDenied(format!(
                    "{context} requires provenance predicate `{predicate_type}` from builder `{builder_id}` for {subject_label}"
                )));
            }
        }
    }
    if policy.sbom_required() && !evidence.sbom_present() {
        return Err(Error::PermissionDenied(format!(
            "{context} requires SBOM evidence for {subject_label}"
        )));
    }
    Ok(())
}

fn ensure_path_matches_sha256(path: &Path, expected_sha256: &str, context: &str) -> Result<()> {
    let actual_sha256 = RuntimeBundle::compute_sha256_for_path(path).map_err(|error| {
        Error::InvalidInput(format!(
            "{context} executable artifact `{}` could not be read before provenance admission: {error}",
            path.display()
        ))
    })?;
    if actual_sha256 == expected_sha256 {
        return Ok(());
    }
    Err(Error::PermissionDenied(format!(
        "{context} executable artifact `{}` failed immutable sha256 admission: expected {expected_sha256}, got {actual_sha256}",
        path.display()
    )))
}

fn normalize_sha256(value: &str) -> std::result::Result<String, ArtifactVerifierError> {
    let digest = value.strip_prefix("sha256:").unwrap_or(value);
    if digest.len() == 64 && digest.as_bytes().iter().all(u8::is_ascii_hexdigit) {
        Ok(digest.to_ascii_lowercase())
    } else {
        Err(ArtifactVerifierError::backend_error(format!(
            "expected sha256:<64 hex> or 64 hex chars, got `{value}`"
        )))
    }
}

#[cfg(test)]
mod tests {
    use nimbus_runtime::RuntimeBundle;

    use super::*;
    use crate::tenant::{ArtifactVerifierBackendIdentity, SLSA_PROVENANCE_V1_PREDICATE_TYPE};

    const BUILDER_ID: &str = "https://github.com/nimbus/builder";
    const SOURCE_URI: &str = "github.com/nimbus/nimbus";

    #[derive(Debug, Clone)]
    struct StaticArtifactVerifier {
        evidence: ArtifactVerificationEvidence,
    }

    impl StaticArtifactVerifier {
        fn with_attestation(builder_id: &str, predicate_type: &str) -> Self {
            Self {
                evidence: ArtifactVerificationEvidence::new(ArtifactVerifierBackendIdentity::new(
                    "fixture", "test",
                ))
                .with_attestation_from_source(
                    builder_id,
                    SOURCE_URI,
                    predicate_type,
                ),
            }
        }
    }

    impl ArtifactVerifierBackend for StaticArtifactVerifier {
        fn verify_artifact(
            &self,
            _request: &ArtifactVerificationRequest,
        ) -> super::super::ArtifactVerifierResult<ArtifactVerificationEvidence> {
            Ok(self.evidence.clone())
        }
    }

    fn policy() -> ArtifactVerificationPolicy {
        ArtifactVerificationPolicy::new().require_provenance_from_source(
            BUILDER_ID,
            SOURCE_URI,
            [SLSA_PROVENANCE_V1_PREDICATE_TYPE],
        )
    }

    fn write_bundle(contents: &str) -> (tempfile::TempDir, std::path::PathBuf, String) {
        let temp = tempfile::tempdir().expect("tempdir should create");
        let path = temp.path().join("bundle.mjs");
        std::fs::write(&path, contents).expect("bundle should write");
        let sha256 = RuntimeBundle::compute_sha256_for_path(&path).expect("bundle should hash");
        (temp, path, sha256)
    }

    #[test]
    fn runtime_bundle_artifact_admission_accepts_verified_bundle_before_invocation() {
        let (_temp, path, sha256) = write_bundle("export default 1;\n");
        let bundle = RuntimeBundle::with_expected_sha256(&path, &sha256)
            .expect("bundle identity should accept sha256");
        let verifier =
            StaticArtifactVerifier::with_attestation(BUILDER_ID, SLSA_PROVENANCE_V1_PREDICATE_TYPE);

        let admission =
            admit_runtime_bundle_artifact(&bundle, &policy(), &verifier, "runtime invocation")
                .expect("matching runtime bundle provenance should admit");

        assert!(matches!(
            admission.subject(),
            ArtifactVerificationSubject::RuntimeBundle { path: admitted_path, sha256: admitted_sha }
                if admitted_path == &path && admitted_sha == &format!("sha256:{sha256}")
        ));
        assert_eq!(admission.verification().attestations().len(), 1);
    }

    #[test]
    fn runtime_bundle_artifact_admission_rejects_missing_or_wrong_digest_before_verifier() {
        let (_temp, path, sha256) = write_bundle("export default 1;\n");
        let missing_digest_bundle = RuntimeBundle::new(&path);
        let wrong_digest_bundle = RuntimeBundle::with_expected_sha256(&path, "b".repeat(64))
            .expect("syntactically valid wrong sha should build");
        let verifier =
            StaticArtifactVerifier::with_attestation(BUILDER_ID, SLSA_PROVENANCE_V1_PREDICATE_TYPE);

        let missing_digest_error = admit_runtime_bundle_artifact(
            &missing_digest_bundle,
            &policy(),
            &verifier,
            "runtime invocation",
        )
        .expect_err("runtime bundle provenance policy should require immutable bundle identity");
        let wrong_digest_error = admit_runtime_bundle_artifact(
            &wrong_digest_bundle,
            &policy(),
            &verifier,
            "runtime invocation",
        )
        .expect_err("runtime bundle provenance policy should reject wrong digest");

        assert!(
            missing_digest_error
                .to_string()
                .contains("no immutable sha256"),
            "missing digest error should be actionable: {missing_digest_error}"
        );
        assert!(
            wrong_digest_error
                .to_string()
                .contains("failed immutable sha256 admission"),
            "wrong digest error should be actionable: {wrong_digest_error}"
        );
        assert_ne!(sha256, "b".repeat(64));
    }

    #[test]
    fn runtime_bundle_artifact_admission_rejects_wrong_builder_or_predicate() {
        let (_temp, path, sha256) = write_bundle("export default 1;\n");
        let bundle = RuntimeBundle::with_expected_sha256(&path, &sha256)
            .expect("bundle identity should accept sha256");
        let wrong_builder = StaticArtifactVerifier::with_attestation(
            "https://github.com/other/builder",
            SLSA_PROVENANCE_V1_PREDICATE_TYPE,
        );
        let wrong_predicate =
            StaticArtifactVerifier::with_attestation(BUILDER_ID, "https://example.com/not-slsa");

        let wrong_builder_error =
            admit_runtime_bundle_artifact(&bundle, &policy(), &wrong_builder, "runtime invocation")
                .expect_err("wrong builder should fail closed");
        let wrong_predicate_error = admit_runtime_bundle_artifact(
            &bundle,
            &policy(),
            &wrong_predicate,
            "runtime invocation",
        )
        .expect_err("wrong predicate should fail closed");

        assert!(
            wrong_builder_error
                .to_string()
                .contains("requires provenance")
        );
        assert!(
            wrong_predicate_error
                .to_string()
                .contains("requires provenance")
        );
    }

    #[test]
    fn guest_executable_artifact_admission_reuses_same_policy_shape() {
        let (_temp, path, sha256) = write_bundle("#!/bin/sh\nexit 0\n");
        let verifier =
            StaticArtifactVerifier::with_attestation(BUILDER_ID, SLSA_PROVENANCE_V1_PREDICATE_TYPE);

        let admission = admit_guest_executable_artifact(
            &path,
            &sha256,
            &policy(),
            &verifier,
            "sandbox guest helper launch",
        )
        .expect("matching guest executable provenance should admit");

        assert!(matches!(
            admission.subject(),
            ArtifactVerificationSubject::GuestExecutable { path: admitted_path, sha256: admitted_sha }
                if admitted_path == &path && admitted_sha == &format!("sha256:{sha256}")
        ));
    }
}
