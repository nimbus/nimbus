use nimbus_core::{Error, Result};
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
    predicate_type: String,
}

impl TenantImageAttestationEvidence {
    pub fn builder_id(&self) -> &str {
        &self.builder_id
    }

    pub fn predicate_type(&self) -> &str {
        &self.predicate_type
    }
}

pub trait TenantImageVerificationProvider {
    fn verify_registry_image(
        &self,
        image_reference: &str,
    ) -> Result<TenantImageVerificationEvidence>;
}

impl TenantImagePolicyDecision {
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
        provider: &impl TenantImageVerificationProvider,
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
                self.admit_registry_image_reference(image_reference)?;
                let verification = if self.requires_provider_verification() {
                    provider.verify_registry_image(image_reference)?
                } else {
                    TenantImageVerificationEvidence::default()
                };
                self.ensure_signature_policy(&verification, image_reference)?;
                self.ensure_provenance_policy(&verification, image_reference)?;
                self.ensure_sbom_policy(&verification, image_reference)?;
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

    fn admit_registry_image_reference(&self, image_reference: &str) -> Result<()> {
        if let Some(expected) = self.image_reference.as_deref()
            && expected != image_reference
        {
            return Err(Error::PermissionDenied(format!(
                "tenant image policy authorized image `{expected}`, but launch requested `{image_reference}`"
            )));
        }
        if self.digest_required && !is_sha256_digest_pinned(image_reference) {
            return Err(Error::PermissionDenied(format!(
                "tenant image policy requires an immutable sha256 digest reference, but `{image_reference}` is tag-only or missing a valid digest"
            )));
        }
        if !self.allowed_registries.is_empty() {
            let registry = image_registry(image_reference);
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
        Ok(())
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
            return Ok(());
        };
        for predicate_type in &self.required_attestation_predicates {
            if !evidence.attestations.iter().any(|attestation| {
                attestation.builder_id == builder_id
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

fn is_sha256_digest_pinned(image_reference: &str) -> bool {
    let Some((_, digest)) = image_reference.rsplit_once("@sha256:") else {
        return false;
    };
    digest.len() == 64 && digest.as_bytes().iter().all(u8::is_ascii_hexdigit)
}

fn image_registry(image_reference: &str) -> &str {
    let image_reference = image_reference
        .strip_prefix("docker://")
        .unwrap_or(image_reference);
    let first_segment = image_reference
        .split_once('/')
        .map(|(registry, _)| registry)
        .unwrap_or(image_reference);
    if first_segment == "localhost" || first_segment.contains('.') || first_segment.contains(':') {
        first_segment
    } else {
        "docker.io"
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    const DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const IMAGE: &str = "registry.example.com/nimbus/api@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    #[derive(Default)]
    struct StaticImageVerifier {
        evidence: TenantImageVerificationEvidence,
        calls: AtomicUsize,
    }

    impl StaticImageVerifier {
        fn with_evidence(evidence: TenantImageVerificationEvidence) -> Self {
            Self {
                evidence,
                calls: AtomicUsize::new(0),
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl TenantImageVerificationProvider for StaticImageVerifier {
        fn verify_registry_image(
            &self,
            _image_reference: &str,
        ) -> Result<TenantImageVerificationEvidence> {
            self.calls.fetch_add(1, Ordering::SeqCst);
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
    fn image_admission_accepts_matching_signature_provenance_and_sbom() {
        let policy = TenantImagePolicyDecision::digest_pinned(IMAGE)
            .require_signature("https://issuer.example.com", "repo:nimbus/api")
            .require_provenance(
                "https://github.com/nimbus/nimbus/.github/workflows/release.yml",
                ["https://slsa.dev/provenance/v1"],
            )
            .require_sbom();
        let verifier = StaticImageVerifier::with_evidence(
            TenantImageVerificationEvidence::new()
                .with_signature("https://issuer.example.com", "repo:nimbus/api")
                .with_attestation(
                    "https://github.com/nimbus/nimbus/.github/workflows/release.yml",
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
