use nimbus_core::{Error, Result};
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

pub fn admit_artifact_subject(
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
    let subject = normalize_artifact_subject(subject, context)?;
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

fn normalize_artifact_subject(
    subject: ArtifactVerificationSubject,
    context: &str,
) -> Result<ArtifactVerificationSubject> {
    match subject {
        ArtifactVerificationSubject::RuntimeBundle { path, sha256 } => {
            let sha256 = normalize_artifact_sha256(&sha256).map_err(|error| {
                Error::InvalidInput(format!(
                    "{context} runtime bundle `{}` has invalid sha256 identity: {error}",
                    path.display()
                ))
            })?;
            Ok(ArtifactVerificationSubject::RuntimeBundle {
                path,
                sha256: format!("sha256:{sha256}"),
            })
        }
        ArtifactVerificationSubject::GuestExecutable { path, sha256 } => {
            let sha256 = normalize_artifact_sha256(&sha256).map_err(|error| {
                Error::InvalidInput(format!(
                    "{context} guest executable `{}` has invalid sha256 identity: {error}",
                    path.display()
                ))
            })?;
            Ok(ArtifactVerificationSubject::GuestExecutable {
                path,
                sha256: format!("sha256:{sha256}"),
            })
        }
        ArtifactVerificationSubject::File {
            path,
            sha256: Some(sha256),
        } => {
            let sha256 = normalize_artifact_sha256(&sha256).map_err(|error| {
                Error::InvalidInput(format!(
                    "{context} file artifact `{}` has invalid sha256 identity: {error}",
                    path.display()
                ))
            })?;
            Ok(ArtifactVerificationSubject::File {
                path,
                sha256: Some(format!("sha256:{sha256}")),
            })
        }
        other => Ok(other),
    }
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
        let matching_attestations = evidence
            .attestations()
            .iter()
            .filter(|candidate| {
                candidate.builder_id() == builder_id
                    && source_uri.is_none_or(|expected| candidate.source_uri() == Some(expected))
            })
            .collect::<Vec<_>>();
        if matching_attestations.is_empty() {
            return Err(Error::PermissionDenied(format!(
                "{context} requires provenance from builder `{builder_id}` for {subject_label}"
            )));
        }
        for predicate_type in provenance.predicate_types() {
            if !matching_attestations
                .iter()
                .any(|candidate| candidate.predicate_type() == predicate_type)
            {
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

pub fn normalize_artifact_sha256(
    value: &str,
) -> std::result::Result<String, ArtifactVerifierError> {
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
    use super::*;
    use crate::{ArtifactVerifierBackendIdentity, SLSA_PROVENANCE_V1_PREDICATE_TYPE};

    const BUILDER_ID: &str = "https://github.com/nimbus/builder";
    const SOURCE_URI: &str = "github.com/nimbus/nimbus";
    const DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    #[derive(Debug, Clone)]
    struct StaticArtifactVerifier {
        evidence: ArtifactVerificationEvidence,
    }

    impl StaticArtifactVerifier {
        fn with_evidence(evidence: ArtifactVerificationEvidence) -> Self {
            Self { evidence }
        }

        fn with_attestation(builder_id: &str, predicate_type: &str) -> Self {
            Self::with_evidence(
                ArtifactVerificationEvidence::new(ArtifactVerifierBackendIdentity::new(
                    "fixture", "test",
                ))
                .with_attestation_from_source(
                    builder_id,
                    SOURCE_URI,
                    predicate_type,
                ),
            )
        }
    }

    impl ArtifactVerifierBackend for StaticArtifactVerifier {
        fn verify_artifact(
            &self,
            _request: &ArtifactVerificationRequest,
        ) -> crate::ArtifactVerifierResult<ArtifactVerificationEvidence> {
            Ok(self.evidence.clone())
        }
    }

    #[derive(Debug, Clone)]
    struct FailingArtifactVerifier {
        error: ArtifactVerifierError,
    }

    impl FailingArtifactVerifier {
        fn new(error: ArtifactVerifierError) -> Self {
            Self { error }
        }
    }

    impl ArtifactVerifierBackend for FailingArtifactVerifier {
        fn verify_artifact(
            &self,
            _request: &ArtifactVerificationRequest,
        ) -> crate::ArtifactVerifierResult<ArtifactVerificationEvidence> {
            Err(self.error.clone())
        }
    }

    fn policy() -> ArtifactVerificationPolicy {
        ArtifactVerificationPolicy::new().require_provenance_from_source(
            BUILDER_ID,
            SOURCE_URI,
            [SLSA_PROVENANCE_V1_PREDICATE_TYPE],
        )
    }

    fn runtime_subject(sha256: impl Into<String>) -> ArtifactVerificationSubject {
        ArtifactVerificationSubject::RuntimeBundle {
            path: "/srv/nimbus/functions/bundle.mjs".into(),
            sha256: sha256.into(),
        }
    }

    fn guest_subject(sha256: impl Into<String>) -> ArtifactVerificationSubject {
        ArtifactVerificationSubject::GuestExecutable {
            path: "/usr/libexec/nimbus/helper".into(),
            sha256: sha256.into(),
        }
    }

    #[test]
    fn runtime_bundle_artifact_admission_accepts_verified_subject_before_invocation() {
        let verifier =
            StaticArtifactVerifier::with_attestation(BUILDER_ID, SLSA_PROVENANCE_V1_PREDICATE_TYPE);

        let admission = admit_artifact_subject(
            runtime_subject(DIGEST),
            &policy(),
            &verifier,
            "runtime invocation",
        )
        .expect("matching runtime bundle provenance should admit");

        assert!(matches!(
            admission.subject(),
            ArtifactVerificationSubject::RuntimeBundle { path: admitted_path, sha256: admitted_sha }
                if admitted_path == &std::path::PathBuf::from("/srv/nimbus/functions/bundle.mjs")
                    && admitted_sha == &format!("sha256:{DIGEST}")
        ));
        assert_eq!(admission.verification().attestations().len(), 1);
    }

    #[test]
    fn runtime_bundle_artifact_admission_rejects_invalid_digest_before_verifier() {
        let verifier =
            StaticArtifactVerifier::with_attestation(BUILDER_ID, SLSA_PROVENANCE_V1_PREDICATE_TYPE);

        let invalid_digest_error = admit_artifact_subject(
            runtime_subject("latest"),
            &policy(),
            &verifier,
            "runtime invocation",
        )
        .expect_err("runtime bundle provenance policy should require immutable bundle identity");

        assert!(
            invalid_digest_error
                .to_string()
                .contains("invalid sha256 identity"),
            "invalid digest error should be actionable: {invalid_digest_error}"
        );
    }

    #[test]
    fn runtime_bundle_artifact_admission_fails_closed_when_verifier_errors() {
        let verifier = FailingArtifactVerifier::new(ArtifactVerifierError::command_failure(
            "cosign verify-attestation exited with status 1",
        ));

        let error = admit_artifact_subject(
            runtime_subject(DIGEST),
            &policy(),
            &verifier,
            "runtime invocation",
        )
        .expect_err("a failing verifier backend must fail closed rather than admit the artifact");

        assert!(
            matches!(error, Error::PermissionDenied(_)),
            "verifier failure must map to PermissionDenied, got {error:?}"
        );
        assert!(
            error
                .to_string()
                .contains("artifact verifier failed closed"),
            "verifier failure error should be actionable: {error}"
        );
    }

    #[test]
    fn runtime_bundle_artifact_admission_rejects_wrong_builder_or_predicate() {
        let wrong_builder = StaticArtifactVerifier::with_attestation(
            "https://github.com/other/builder",
            SLSA_PROVENANCE_V1_PREDICATE_TYPE,
        );
        let wrong_predicate =
            StaticArtifactVerifier::with_attestation(BUILDER_ID, "https://example.com/not-slsa");

        let wrong_builder_error = admit_artifact_subject(
            runtime_subject(DIGEST),
            &policy(),
            &wrong_builder,
            "runtime invocation",
        )
        .expect_err("wrong builder should fail closed");
        let wrong_predicate_error = admit_artifact_subject(
            runtime_subject(DIGEST),
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
    fn runtime_bundle_artifact_admission_rejects_predicate_from_wrong_attestation_scope() {
        let evidence = ArtifactVerificationEvidence::new(ArtifactVerifierBackendIdentity::new(
            "fixture", "test",
        ))
        .with_attestation_from_source(BUILDER_ID, SOURCE_URI, "https://example.com/not-slsa")
        .with_attestation_from_source(
            "https://github.com/other/builder",
            SOURCE_URI,
            SLSA_PROVENANCE_V1_PREDICATE_TYPE,
        )
        .with_attestation_from_source(
            BUILDER_ID,
            "github.com/nimbus/other",
            SLSA_PROVENANCE_V1_PREDICATE_TYPE,
        );
        let verifier = StaticArtifactVerifier::with_evidence(evidence);

        let error = admit_artifact_subject(
            runtime_subject(DIGEST),
            &policy(),
            &verifier,
            "runtime invocation",
        )
        .expect_err("required predicate must be present on matching builder/source evidence");

        assert!(
            error.to_string().contains(
                "requires provenance predicate `https://slsa.dev/provenance/v1` from builder"
            ),
            "wrong-scope predicate error should be precise: {error}"
        );
    }

    #[test]
    fn guest_executable_artifact_admission_reuses_same_policy_shape() {
        let verifier =
            StaticArtifactVerifier::with_attestation(BUILDER_ID, SLSA_PROVENANCE_V1_PREDICATE_TYPE);

        let admission = admit_artifact_subject(
            guest_subject(DIGEST),
            &policy(),
            &verifier,
            "sandbox guest helper launch",
        )
        .expect("matching guest executable provenance should admit");

        assert!(matches!(
            admission.subject(),
            ArtifactVerificationSubject::GuestExecutable { path: admitted_path, sha256: admitted_sha }
                if admitted_path == &std::path::PathBuf::from("/usr/libexec/nimbus/helper")
                    && admitted_sha == &format!("sha256:{DIGEST}")
        ));
    }
}
