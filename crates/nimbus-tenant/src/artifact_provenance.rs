use std::sync::Arc;

use nimbus_core::{Error, Result};

use super::image_admission::{
    TenantImageVerificationEvidence, TenantImageVerificationProvider,
    TenantImageVerificationRequest,
};

pub use nimbus_artifacts::{
    ArtifactAdmission, ArtifactAttestationEvidence, ArtifactProvenanceRequirement,
    ArtifactSignatureEvidence, ArtifactSignatureRequirement, ArtifactVerificationEvidence,
    ArtifactVerificationPolicy, ArtifactVerificationRequest, ArtifactVerificationSubject,
    ArtifactVerificationSubjectKind, ArtifactVerifierBackend, ArtifactVerifierBackendIdentity,
    ArtifactVerifierError, ArtifactVerifierErrorKind, ArtifactVerifierResult,
    CompositeArtifactVerifierBackend, SLSA_PROVENANCE_V1_PREDICATE_TYPE, admit_artifact_subject,
    normalize_artifact_sha256, redact_artifact_verifier_output,
};

pub struct ArtifactImageVerificationProvider {
    backend: Arc<dyn ArtifactVerifierBackend>,
}

impl ArtifactImageVerificationProvider {
    pub fn new(backend: impl ArtifactVerifierBackend + 'static) -> Self {
        Self::from_arc(Arc::new(backend))
    }

    pub fn from_arc(backend: Arc<dyn ArtifactVerifierBackend>) -> Self {
        Self { backend }
    }
}

impl std::fmt::Debug for ArtifactImageVerificationProvider {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ArtifactImageVerificationProvider")
            .finish_non_exhaustive()
    }
}

impl TenantImageVerificationProvider for ArtifactImageVerificationProvider {
    fn verify_registry_image(
        &self,
        request: &TenantImageVerificationRequest,
    ) -> Result<TenantImageVerificationEvidence> {
        let artifact_request = ArtifactVerificationRequest::oci_image(
            request.image_reference(),
            artifact_policy_from_tenant_image_request(request),
        );
        let evidence = self
            .backend
            .verify_artifact(&artifact_request)
            .map_err(|error| {
                Error::PermissionDenied(format!(
                    "artifact verifier failed closed for OCI image `{}`: {}",
                    request.image_reference(),
                    error.redacted()
                ))
            })?;
        Ok(to_tenant_image_evidence(&evidence))
    }
}

fn artifact_policy_from_tenant_image_request(
    request: &TenantImageVerificationRequest,
) -> ArtifactVerificationPolicy {
    let mut policy = ArtifactVerificationPolicy::new();
    if let Some(signature) = request.signature() {
        policy = policy.with_signature_requirement(ArtifactSignatureRequirement::new(
            signature.issuer().map(str::to_string),
            signature.subject().map(str::to_string),
        ));
    }
    if let Some(provenance) = request.provenance() {
        policy = policy.with_provenance_requirement(ArtifactProvenanceRequirement::new(
            provenance.builder_id().map(str::to_string),
            provenance.source_uri().map(str::to_string),
            provenance.predicate_types().to_vec(),
        ));
    }
    if request.sbom_required() {
        policy = policy.require_sbom();
    }
    policy
}

fn to_tenant_image_evidence(
    artifact_evidence: &ArtifactVerificationEvidence,
) -> TenantImageVerificationEvidence {
    let mut evidence = TenantImageVerificationEvidence::new();
    for signature in artifact_evidence.signatures() {
        evidence = evidence.with_signature(signature.issuer(), signature.subject());
    }
    for attestation in artifact_evidence.attestations() {
        evidence = if let Some(source_uri) = attestation.source_uri() {
            evidence.with_attestation_from_source(
                attestation.builder_id(),
                source_uri,
                attestation.predicate_type(),
            )
        } else {
            evidence.with_attestation(attestation.builder_id(), attestation.predicate_type())
        };
    }
    if artifact_evidence.sbom_present() {
        evidence = evidence.with_sbom();
    }
    evidence
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TenantImageAdmissionSource, TenantImagePolicyDecision};
    use nimbus_artifacts::{
        ArtifactVerifierBackendIdentity, ArtifactVerifierError, ArtifactVerifierResult,
        CompositeArtifactVerifierBackend,
    };

    const IMAGE: &str = "registry.example.com/nimbus/api@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    #[derive(Debug, Clone)]
    struct StaticArtifactBackend {
        evidence: ArtifactVerificationEvidence,
    }

    impl StaticArtifactBackend {
        fn new(evidence: ArtifactVerificationEvidence) -> Self {
            Self { evidence }
        }
    }

    impl ArtifactVerifierBackend for StaticArtifactBackend {
        fn verify_artifact(
            &self,
            _request: &ArtifactVerificationRequest,
        ) -> ArtifactVerifierResult<ArtifactVerificationEvidence> {
            Ok(self.evidence.clone())
        }
    }

    #[test]
    fn artifact_provider_translates_composite_evidence_to_tenant_image_evidence() {
        let signature = Arc::new(StaticArtifactBackend::new(
            ArtifactVerificationEvidence::new(ArtifactVerifierBackendIdentity::new(
                "signature-fixture",
                "test",
            ))
            .with_signature("https://issuer.example.com", "repo:nimbus/api"),
        ));
        let provenance = Arc::new(StaticArtifactBackend::new(
            ArtifactVerificationEvidence::new(ArtifactVerifierBackendIdentity::new(
                "provenance-fixture",
                "test",
            ))
            .with_attestation_from_source(
                "https://github.com/nimbus/builder",
                "github.com/nimbus/nimbus",
                "https://slsa.dev/provenance/v1",
            ),
        ));
        let sbom = Arc::new(StaticArtifactBackend::new(
            ArtifactVerificationEvidence::new(ArtifactVerifierBackendIdentity::new(
                "sbom-fixture",
                "test",
            ))
            .with_sbom(),
        ));
        let backends: Vec<Arc<dyn ArtifactVerifierBackend>> = vec![signature, provenance, sbom];
        let provider = ArtifactImageVerificationProvider::new(
            CompositeArtifactVerifierBackend::new(backends)
                .expect("composite verifier should build"),
        );
        let policy = TenantImagePolicyDecision::digest_pinned(IMAGE)
            .require_signature("https://issuer.example.com", "repo:nimbus/api")
            .require_provenance_from_source(
                "https://github.com/nimbus/builder",
                "github.com/nimbus/nimbus",
                ["https://slsa.dev/provenance/v1"],
            )
            .require_sbom();

        let admission = policy
            .admit_image(TenantImageAdmissionSource::registry(IMAGE), &provider)
            .expect("composite evidence should satisfy all image policy requirements");

        assert_eq!(admission.verification().signatures().len(), 1);
        assert_eq!(admission.verification().attestations().len(), 1);
        assert_eq!(
            admission.verification().attestations()[0].source_uri(),
            Some("github.com/nimbus/nimbus")
        );
        assert!(admission.verification().sbom_present());
    }

    #[derive(Debug)]
    struct FailingLibraryBackend;

    impl ArtifactVerifierBackend for FailingLibraryBackend {
        fn verify_artifact(
            &self,
            _request: &ArtifactVerificationRequest,
        ) -> ArtifactVerifierResult<ArtifactVerificationEvidence> {
            Err(ArtifactVerifierError::backend_error(
                "library verifier failed with credential=do-not-log-library-secret",
            ))
        }
    }

    #[test]
    fn image_provider_library_error_fails_closed_and_redacts_output() {
        let provider = ArtifactImageVerificationProvider::new(FailingLibraryBackend);
        let policy = TenantImagePolicyDecision::digest_pinned(IMAGE)
            .require_signature("https://issuer.example.com", "repo:nimbus/api");

        let error = policy
            .admit_image(TenantImageAdmissionSource::registry(IMAGE), &provider)
            .expect_err("library verifier error should deny image admission");

        let rendered = error.to_string();
        assert!(rendered.contains("artifact verifier failed closed"));
        assert!(rendered.contains("[redacted verifier output]"));
        assert!(!rendered.contains("do-not-log-library-secret"));
    }
}
