use nimbus_core::{Error, Result};
use oci_client::Reference;
use serde::Serialize;

use super::TenantImagePolicyDecision;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "source")]
pub enum TenantImageAdmissionSource {
    RegistryImage { image_reference: String },
    LocalBuild { image_name: String },
}

impl TenantImageAdmissionSource {
    pub fn registry(image_reference: impl Into<String>) -> Self {
        Self::RegistryImage {
            image_reference: image_reference.into(),
        }
    }

    pub fn local_build(image_name: impl Into<String>) -> Self {
        Self::LocalBuild {
            image_name: image_name.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TenantImageAdmission {
    source: TenantImageAdmissionSource,
    verification: TenantImageVerificationEvidence,
}

impl TenantImageAdmission {
    pub fn source(&self) -> &TenantImageAdmissionSource {
        &self.source
    }

    pub fn verification(&self) -> &TenantImageVerificationEvidence {
        &self.verification
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TenantImageVerificationEvidence {
    signatures: Vec<TenantImageSignatureEvidence>,
    attestations: Vec<TenantImageAttestationEvidence>,
    sbom_present: bool,
}

impl TenantImageVerificationEvidence {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_signature(mut self, issuer: impl Into<String>, subject: impl Into<String>) -> Self {
        self.signatures.push(TenantImageSignatureEvidence {
            issuer: issuer.into(),
            subject: subject.into(),
        });
        self
    }

    pub fn with_attestation(
        mut self,
        builder_id: impl Into<String>,
        predicate_type: impl Into<String>,
    ) -> Self {
        self.attestations.push(TenantImageAttestationEvidence {
            builder_id: builder_id.into(),
            source_uri: None,
            predicate_type: predicate_type.into(),
        });
        self
    }

    pub fn with_attestation_from_source(
        mut self,
        builder_id: impl Into<String>,
        source_uri: impl Into<String>,
        predicate_type: impl Into<String>,
    ) -> Self {
        self.attestations.push(TenantImageAttestationEvidence {
            builder_id: builder_id.into(),
            source_uri: Some(source_uri.into()),
            predicate_type: predicate_type.into(),
        });
        self
    }

    pub fn with_sbom(mut self) -> Self {
        self.sbom_present = true;
        self
    }

    pub fn signatures(&self) -> &[TenantImageSignatureEvidence] {
        &self.signatures
    }

    pub fn attestations(&self) -> &[TenantImageAttestationEvidence] {
        &self.attestations
    }

    pub fn sbom_present(&self) -> bool {
        self.sbom_present
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TenantImageSignatureEvidence {
    issuer: String,
    subject: String,
}

impl TenantImageSignatureEvidence {
    pub fn issuer(&self) -> &str {
        &self.issuer
    }

    pub fn subject(&self) -> &str {
        &self.subject
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TenantImageAttestationEvidence {
    builder_id: String,
    source_uri: Option<String>,
    predicate_type: String,
}

impl TenantImageAttestationEvidence {
    pub fn builder_id(&self) -> &str {
        &self.builder_id
    }

    pub fn source_uri(&self) -> Option<&str> {
        self.source_uri.as_deref()
    }

    pub fn predicate_type(&self) -> &str {
        &self.predicate_type
    }
}

pub trait TenantImageVerificationProvider: Send + Sync {
    fn verify_registry_image(
        &self,
        request: &TenantImageVerificationRequest,
    ) -> Result<TenantImageVerificationEvidence>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TenantImageVerificationRequest {
    image_reference: String,
    signature: Option<TenantImageSignatureRequirement>,
    provenance: Option<TenantImageProvenanceRequirement>,
    sbom_required: bool,
}

impl TenantImageVerificationRequest {
    fn from_policy(image_reference: impl Into<String>, policy: &TenantImagePolicyDecision) -> Self {
        Self {
            image_reference: image_reference.into(),
            signature: policy
                .signature_required
                .then(|| TenantImageSignatureRequirement {
                    issuer: policy.allowed_signature_issuer.clone(),
                    subject: policy.allowed_signature_subject.clone(),
                }),
            provenance: policy
                .provenance_required
                .then(|| TenantImageProvenanceRequirement {
                    builder_id: policy.allowed_builder_id.clone(),
                    source_uri: policy.allowed_source_uri.clone(),
                    predicate_types: policy.required_attestation_predicates.clone(),
                }),
            sbom_required: policy.sbom_required,
        }
    }

    pub fn image_reference(&self) -> &str {
        &self.image_reference
    }

    pub fn signature(&self) -> Option<&TenantImageSignatureRequirement> {
        self.signature.as_ref()
    }

    pub fn provenance(&self) -> Option<&TenantImageProvenanceRequirement> {
        self.provenance.as_ref()
    }

    pub fn sbom_required(&self) -> bool {
        self.sbom_required
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TenantImageSignatureRequirement {
    issuer: Option<String>,
    subject: Option<String>,
}

impl TenantImageSignatureRequirement {
    pub fn issuer(&self) -> Option<&str> {
        self.issuer.as_deref()
    }

    pub fn subject(&self) -> Option<&str> {
        self.subject.as_deref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TenantImageProvenanceRequirement {
    builder_id: Option<String>,
    source_uri: Option<String>,
    predicate_types: Vec<String>,
}

impl TenantImageProvenanceRequirement {
    pub fn builder_id(&self) -> Option<&str> {
        self.builder_id.as_deref()
    }

    pub fn source_uri(&self) -> Option<&str> {
        self.source_uri.as_deref()
    }

    pub fn predicate_types(&self) -> &[String] {
        &self.predicate_types
    }
}

impl TenantImagePolicyDecision {
    pub fn require_digest_reference(mut self) -> Self {
        self.digest_required = true;
        self
    }

    pub fn with_image_reference(mut self, image_reference: impl Into<String>) -> Self {
        self.image_reference = Some(image_reference.into());
        self.digest_required = true;
        self
    }

    pub fn with_allowed_registry(mut self, registry: impl Into<String>) -> Self {
        self.allowed_registries.push(registry.into());
        self
    }

    pub fn require_signature(
        mut self,
        issuer: impl Into<String>,
        subject: impl Into<String>,
    ) -> Self {
        self.signature_required = true;
        self.allowed_signature_issuer = Some(issuer.into());
        self.allowed_signature_subject = Some(subject.into());
        self
    }

    pub fn require_provenance(
        mut self,
        builder_id: impl Into<String>,
        predicate_types: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.provenance_required = true;
        self.allowed_builder_id = Some(builder_id.into());
        self.allowed_source_uri = None;
        self.required_attestation_predicates =
            predicate_types.into_iter().map(Into::into).collect();
        self
    }

    pub fn require_provenance_from_source(
        mut self,
        builder_id: impl Into<String>,
        source_uri: impl Into<String>,
        predicate_types: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.provenance_required = true;
        self.allowed_builder_id = Some(builder_id.into());
        self.allowed_source_uri = Some(source_uri.into());
        self.required_attestation_predicates =
            predicate_types.into_iter().map(Into::into).collect();
        self
    }

    pub fn require_sbom(mut self) -> Self {
        self.sbom_required = true;
        self
    }

    pub fn allow_local_build(mut self) -> Self {
        self.local_build_allowed = true;
        self
    }

    pub fn admit_image(
        &self,
        source: TenantImageAdmissionSource,
        provider: &(impl TenantImageVerificationProvider + ?Sized),
    ) -> Result<TenantImageAdmission> {
        match &source {
            TenantImageAdmissionSource::LocalBuild { image_name } => {
                self.admit_local_build(image_name)?;
                Ok(TenantImageAdmission {
                    source,
                    verification: TenantImageVerificationEvidence::default(),
                })
            }
            TenantImageAdmissionSource::RegistryImage { image_reference } => {
                let parsed_reference = self.admit_registry_image_reference(image_reference)?;
                let canonical_image_reference = parsed_reference.whole();
                let verification = if self.requires_provider_verification() {
                    let request = TenantImageVerificationRequest::from_policy(
                        &canonical_image_reference,
                        self,
                    );
                    provider.verify_registry_image(&request)?
                } else {
                    TenantImageVerificationEvidence::default()
                };
                self.ensure_signature_policy(&verification, &canonical_image_reference)?;
                self.ensure_provenance_policy(&verification, &canonical_image_reference)?;
                self.ensure_sbom_policy(&verification, &canonical_image_reference)?;
                Ok(TenantImageAdmission {
                    source,
                    verification,
                })
            }
        }
    }

    fn admit_local_build(&self, image_name: &str) -> Result<()> {
        if self.local_build_allowed {
            return Ok(());
        }
        Err(Error::PermissionDenied(format!(
            "tenant image policy does not authorize local build image `{image_name}`; production launches require an admitted registry image or explicit local-build policy"
        )))
    }

    fn admit_registry_image_reference(&self, image_reference: &str) -> Result<Reference> {
        let requested = parse_oci_image_reference(image_reference)?;
        if let Some(expected) = self.image_reference.as_deref() {
            let expected = parse_oci_image_reference(expected)?;
            if expected.whole() != requested.whole() {
                return Err(Error::PermissionDenied(format!(
                    "tenant image policy authorized image `{}`, but launch requested `{}`",
                    expected.whole(),
                    requested.whole()
                )));
            }
        }
        if self.digest_required && !has_sha256_digest(&requested) {
            return Err(Error::PermissionDenied(format!(
                "tenant image policy requires an immutable sha256 digest reference, but `{image_reference}` is tag-only or missing a valid digest"
            )));
        }
        if !self.allowed_registries.is_empty() {
            let registry = requested.registry();
            if !self
                .allowed_registries
                .iter()
                .any(|allowed| allowed == registry)
            {
                return Err(Error::PermissionDenied(format!(
                    "tenant image policy allowed registries {:?}, but `{image_reference}` resolves to registry `{registry}`",
                    self.allowed_registries
                )));
            }
        }
        Ok(requested)
    }

    fn requires_provider_verification(&self) -> bool {
        self.signature_required || self.provenance_required || self.sbom_required
    }

    fn ensure_signature_policy(
        &self,
        evidence: &TenantImageVerificationEvidence,
        image_reference: &str,
    ) -> Result<()> {
        if !self.signature_required {
            return Ok(());
        }
        let issuer = self.allowed_signature_issuer.as_deref();
        let subject = self.allowed_signature_subject.as_deref();
        if evidence.signatures.iter().any(|signature| {
            issuer.is_none_or(|expected| signature.issuer == expected)
                && subject.is_none_or(|expected| signature.subject == expected)
        }) {
            return Ok(());
        }
        Err(Error::PermissionDenied(format!(
            "tenant image policy requires a matching signature for `{image_reference}`"
        )))
    }

    fn ensure_provenance_policy(
        &self,
        evidence: &TenantImageVerificationEvidence,
        image_reference: &str,
    ) -> Result<()> {
        if !self.provenance_required {
            return Ok(());
        }
        let Some(builder_id) = self.allowed_builder_id.as_deref() else {
            return Err(Error::PermissionDenied(format!(
                "tenant image policy requires provenance for `{image_reference}`, but no builder ID is configured"
            )));
        };
        let source_uri = self.allowed_source_uri.as_deref();
        if self.required_attestation_predicates.is_empty()
            && !evidence.attestations.iter().any(|attestation| {
                attestation.builder_id == builder_id
                    && source_uri
                        .is_none_or(|expected| attestation.source_uri.as_deref() == Some(expected))
            })
        {
            return Err(Error::PermissionDenied(format!(
                "tenant image policy requires provenance from builder `{builder_id}` for `{image_reference}`"
            )));
        }
        for predicate_type in &self.required_attestation_predicates {
            if !evidence.attestations.iter().any(|attestation| {
                attestation.builder_id == builder_id
                    && source_uri
                        .is_none_or(|expected| attestation.source_uri.as_deref() == Some(expected))
                    && attestation.predicate_type == *predicate_type
            }) {
                return Err(Error::PermissionDenied(format!(
                    "tenant image policy requires provenance predicate `{predicate_type}` from builder `{builder_id}` for `{image_reference}`"
                )));
            }
        }
        Ok(())
    }

    fn ensure_sbom_policy(
        &self,
        evidence: &TenantImageVerificationEvidence,
        image_reference: &str,
    ) -> Result<()> {
        if !self.sbom_required || evidence.sbom_present {
            return Ok(());
        }
        Err(Error::PermissionDenied(format!(
            "tenant image policy requires SBOM evidence for `{image_reference}`"
        )))
    }
}

pub fn parse_oci_image_reference(image_reference: &str) -> Result<Reference> {
    let stripped = image_reference
        .strip_prefix("docker://")
        .unwrap_or(image_reference);
    Reference::try_from(stripped).map_err(|error| {
        Error::InvalidInput(format!(
            "invalid OCI image reference `{image_reference}`: {error}"
        ))
    })
}

pub fn has_sha256_digest(reference: &Reference) -> bool {
    reference
        .digest()
        .is_some_and(|digest| digest.strip_prefix("sha256:").is_some_and(is_sha256_hex))
}

fn is_sha256_hex(digest: &str) -> bool {
    digest.len() == 64 && digest.as_bytes().iter().all(u8::is_ascii_hexdigit)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    const DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const IMAGE: &str = "registry.example.com/nimbus/api@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    #[derive(Default)]
    struct StaticImageVerifier {
        evidence: TenantImageVerificationEvidence,
        calls: AtomicUsize,
        seen_references: Mutex<Vec<String>>,
    }

    impl StaticImageVerifier {
        fn with_evidence(evidence: TenantImageVerificationEvidence) -> Self {
            Self {
                evidence,
                calls: AtomicUsize::new(0),
                seen_references: Mutex::new(Vec::new()),
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }

        fn seen_references(&self) -> Vec<String> {
            self.seen_references
                .lock()
                .expect("seen reference list should not be poisoned")
                .clone()
        }
    }

    impl TenantImageVerificationProvider for StaticImageVerifier {
        fn verify_registry_image(
            &self,
            request: &TenantImageVerificationRequest,
        ) -> Result<TenantImageVerificationEvidence> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.seen_references
                .lock()
                .expect("seen reference list should not be poisoned")
                .push(request.image_reference().to_string());
            Ok(self.evidence.clone())
        }
    }

    #[test]
    fn image_admission_allows_digest_only_floor_without_provider_call() {
        let policy = TenantImagePolicyDecision::digest_pinned(IMAGE)
            .with_allowed_registry("registry.example.com");
        let verifier = StaticImageVerifier::default();

        let admission = policy
            .admit_image(TenantImageAdmissionSource::registry(IMAGE), &verifier)
            .expect("digest-pinned registry image should pass the production floor");

        assert!(matches!(
            admission.source(),
            TenantImageAdmissionSource::RegistryImage { image_reference } if image_reference == IMAGE
        ));
        assert_eq!(
            verifier.calls(),
            0,
            "digest-only policy should not require a signature provider call"
        );
    }

    #[test]
    fn image_admission_rejects_tag_only_reference_when_digest_is_required() {
        let image = "registry.example.com/nimbus/api:latest";
        let policy = TenantImagePolicyDecision::digest_pinned(image);
        let verifier = StaticImageVerifier::default();

        let error = policy
            .admit_image(TenantImageAdmissionSource::registry(image), &verifier)
            .expect_err("tag-only production image should be denied");

        assert!(
            error
                .to_string()
                .contains("requires an immutable sha256 digest"),
            "error should name the digest floor: {error}"
        );
    }

    #[test]
    fn image_admission_uses_canonical_oci_parser_for_registry_and_digest_policy() {
        let digest = format!("sha256:{DIGEST}");
        let docker_hub_image = format!("busybox@{digest}");
        let localhost_image = format!("localhost:5000/nimbus/api:stable@{digest}");
        let tag_and_digest_image = format!("registry.example.com/nimbus/api:v1@{digest}");
        let verifier = StaticImageVerifier::default();

        TenantImagePolicyDecision::default()
            .require_digest_reference()
            .with_allowed_registry("docker.io")
            .admit_image(
                TenantImageAdmissionSource::registry(docker_hub_image),
                &verifier,
            )
            .expect("Docker Hub short names should normalize to docker.io/library with digest");
        TenantImagePolicyDecision::default()
            .require_digest_reference()
            .with_allowed_registry("localhost:5000")
            .admit_image(
                TenantImageAdmissionSource::registry(localhost_image),
                &verifier,
            )
            .expect("localhost registry with a port should parse and match policy");
        TenantImagePolicyDecision::default()
            .require_digest_reference()
            .with_allowed_registry("registry.example.com")
            .admit_image(
                TenantImageAdmissionSource::registry(tag_and_digest_image),
                &verifier,
            )
            .expect("tag plus digest references should remain immutable");
    }

    #[test]
    fn image_admission_rejects_invalid_oci_references_before_provider_calls() {
        let policy = TenantImagePolicyDecision::default()
            .require_digest_reference()
            .require_signature("https://issuer.example.com", "repo:nimbus/api");
        let verifier = StaticImageVerifier::default();

        let error = policy
            .admit_image(TenantImageAdmissionSource::registry(":justtag"), &verifier)
            .expect_err("invalid OCI reference should fail closed before verification");

        assert_eq!(
            verifier.calls(),
            0,
            "invalid OCI references should not call the verification provider"
        );
        assert!(
            error.to_string().contains("invalid OCI image reference"),
            "error should name the parser failure: {error}"
        );
    }

    #[test]
    fn image_admission_rejects_unsigned_image_when_signature_is_required() {
        let policy = TenantImagePolicyDecision::digest_pinned(IMAGE)
            .require_signature("https://issuer.example.com", "repo:nimbus/api");
        let verifier = StaticImageVerifier::default();

        let error = policy
            .admit_image(TenantImageAdmissionSource::registry(IMAGE), &verifier)
            .expect_err("missing signature should be denied");

        assert_eq!(verifier.calls(), 1);
        assert!(
            error.to_string().contains("requires a matching signature"),
            "error should name the signature requirement: {error}"
        );
    }

    #[test]
    fn image_verification_provider_receives_canonical_oci_reference() {
        let image = format!("docker://busybox@sha256:{DIGEST}");
        let policy = TenantImagePolicyDecision::default()
            .require_digest_reference()
            .with_allowed_registry("docker.io")
            .require_signature("https://issuer.example.com", "repo:nimbus/api");
        let verifier = StaticImageVerifier::with_evidence(
            TenantImageVerificationEvidence::new()
                .with_signature("https://issuer.example.com", "repo:nimbus/api"),
        );

        policy
            .admit_image(TenantImageAdmissionSource::registry(image), &verifier)
            .expect("canonicalized Docker Hub digest image with matching signature should pass");

        assert_eq!(
            verifier.seen_references(),
            vec![format!("docker.io/library/busybox@sha256:{DIGEST}")],
            "provider should receive canonical OCI reference without transport prefix"
        );
    }

    #[test]
    fn image_admission_compares_expected_references_after_oci_normalization() {
        let requested = format!("docker.io/library/busybox@sha256:{DIGEST}");
        let policy = TenantImagePolicyDecision::digest_pinned(format!("busybox@sha256:{DIGEST}"));
        let verifier = StaticImageVerifier::default();

        policy
            .admit_image(TenantImageAdmissionSource::registry(requested), &verifier)
            .expect(
                "expected and requested image references should compare after OCI normalization",
            );
    }

    #[test]
    fn image_admission_rejects_registry_mismatch_after_oci_normalization() {
        let image = format!("ghcr.io/nimbus/api@sha256:{DIGEST}");
        let policy = TenantImagePolicyDecision::default()
            .require_digest_reference()
            .with_allowed_registry("docker.io");
        let verifier = StaticImageVerifier::default();

        let error = policy
            .admit_image(TenantImageAdmissionSource::registry(image), &verifier)
            .expect_err("registry outside the allowlist should fail closed");

        assert!(
            error.to_string().contains("resolves to registry `ghcr.io`"),
            "error should name the normalized OCI registry: {error}"
        );
    }

    #[test]
    fn image_admission_rejects_wrong_signature_identity() {
        let policy = TenantImagePolicyDecision::digest_pinned(IMAGE)
            .require_signature("https://issuer.example.com", "repo:nimbus/api");
        let verifier = StaticImageVerifier::with_evidence(
            TenantImageVerificationEvidence::new()
                .with_signature("https://issuer.example.com", "repo:other/api"),
        );

        let error = policy
            .admit_image(TenantImageAdmissionSource::registry(IMAGE), &verifier)
            .expect_err("wrong signature subject should be denied");

        assert!(
            error.to_string().contains("requires a matching signature"),
            "error should name the signature identity mismatch: {error}"
        );
    }

    #[test]
    fn image_admission_rejects_wrong_provenance_builder_or_missing_predicate() {
        let policy = TenantImagePolicyDecision::digest_pinned(IMAGE).require_provenance(
            "https://github.com/nimbus/nimbus/.github/workflows/release.yml",
            ["https://slsa.dev/provenance/v1"],
        );
        let verifier = StaticImageVerifier::with_evidence(
            TenantImageVerificationEvidence::new().with_attestation(
                "https://github.com/other/project/.github/workflows/release.yml",
                "https://slsa.dev/provenance/v1",
            ),
        );

        let error = policy
            .admit_image(TenantImageAdmissionSource::registry(IMAGE), &verifier)
            .expect_err("wrong provenance builder should be denied");

        assert!(
            error.to_string().contains("requires provenance predicate"),
            "error should name the provenance requirement: {error}"
        );
    }

    #[test]
    fn image_admission_rejects_wrong_provenance_source_uri() {
        let policy = TenantImagePolicyDecision::digest_pinned(IMAGE)
            .require_provenance_from_source(
                "https://github.com/nimbus/nimbus/.github/workflows/release.yml",
                "github.com/nimbus/nimbus",
                ["https://slsa.dev/provenance/v1"],
            );
        let verifier = StaticImageVerifier::with_evidence(
            TenantImageVerificationEvidence::new().with_attestation_from_source(
                "https://github.com/nimbus/nimbus/.github/workflows/release.yml",
                "github.com/other/project",
                "https://slsa.dev/provenance/v1",
            ),
        );

        let error = policy
            .admit_image(TenantImageAdmissionSource::registry(IMAGE), &verifier)
            .expect_err("wrong provenance source should be denied");

        assert!(
            error.to_string().contains("requires provenance predicate"),
            "error should name the source-scoped provenance requirement: {error}"
        );
    }

    #[test]
    fn image_admission_accepts_matching_signature_provenance_and_sbom() {
        let policy = TenantImagePolicyDecision::digest_pinned(IMAGE)
            .require_signature("https://issuer.example.com", "repo:nimbus/api")
            .require_provenance_from_source(
                "https://github.com/nimbus/nimbus/.github/workflows/release.yml",
                "github.com/nimbus/nimbus",
                ["https://slsa.dev/provenance/v1"],
            )
            .require_sbom();
        let verifier = StaticImageVerifier::with_evidence(
            TenantImageVerificationEvidence::new()
                .with_signature("https://issuer.example.com", "repo:nimbus/api")
                .with_attestation_from_source(
                    "https://github.com/nimbus/nimbus/.github/workflows/release.yml",
                    "github.com/nimbus/nimbus",
                    "https://slsa.dev/provenance/v1",
                )
                .with_sbom(),
        );

        let admission = policy
            .admit_image(TenantImageAdmissionSource::registry(IMAGE), &verifier)
            .expect("matching signature, provenance, and SBOM should admit");

        assert_eq!(verifier.calls(), 1);
        assert_eq!(admission.verification().signatures().len(), 1);
        assert_eq!(admission.verification().attestations().len(), 1);
        assert!(admission.verification().sbom_present());
    }

    #[test]
    fn image_admission_rejects_local_build_without_explicit_policy() {
        let policy = TenantImagePolicyDecision::digest_pinned(format!(
            "registry.example.com/nimbus/api@sha256:{DIGEST}"
        ));
        let verifier = StaticImageVerifier::default();

        let error = policy
            .admit_image(
                TenantImageAdmissionSource::local_build("localhost/nimbus-api"),
                &verifier,
            )
            .expect_err("local build should be denied without explicit policy");

        assert!(
            error.to_string().contains("does not authorize local build"),
            "error should name the local-build policy gate: {error}"
        );
    }
}
