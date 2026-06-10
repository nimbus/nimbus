use std::path::PathBuf;
use std::sync::Arc;

use nimbus_core::{Error, Result};
use oci_client::Reference;
use serde::{Deserialize, Serialize};

mod admission;

pub use admission::{ArtifactAdmission, admit_artifact_subject, normalize_artifact_sha256};

pub const SLSA_PROVENANCE_V1_PREDICATE_TYPE: &str = "https://slsa.dev/provenance/v1";

pub type ArtifactVerifierResult<T> = std::result::Result<T, ArtifactVerifierError>;

pub trait ArtifactVerifierBackend: Send + Sync {
    fn verify_artifact(
        &self,
        request: &ArtifactVerificationRequest,
    ) -> ArtifactVerifierResult<ArtifactVerificationEvidence>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactVerificationRequest {
    subject: ArtifactVerificationSubject,
    policy: ArtifactVerificationPolicy,
}

impl ArtifactVerificationRequest {
    pub fn new(subject: ArtifactVerificationSubject, policy: ArtifactVerificationPolicy) -> Self {
        Self { subject, policy }
    }

    pub fn oci_image(reference: impl Into<String>, policy: ArtifactVerificationPolicy) -> Self {
        Self::new(
            ArtifactVerificationSubject::OciImage {
                reference: reference.into(),
            },
            policy,
        )
    }

    pub fn subject(&self) -> &ArtifactVerificationSubject {
        &self.subject
    }

    pub fn policy(&self) -> &ArtifactVerificationPolicy {
        &self.policy
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ArtifactVerificationSubject {
    OciImage {
        reference: String,
    },
    RuntimeBundle {
        path: PathBuf,
        sha256: String,
    },
    File {
        path: PathBuf,
        sha256: Option<String>,
    },
    MachineImage {
        reference: String,
    },
    GuestExecutable {
        path: PathBuf,
        sha256: String,
    },
}

impl ArtifactVerificationSubject {
    pub fn kind(&self) -> ArtifactVerificationSubjectKind {
        match self {
            Self::OciImage { .. } => ArtifactVerificationSubjectKind::OciImage,
            Self::RuntimeBundle { .. } => ArtifactVerificationSubjectKind::RuntimeBundle,
            Self::File { .. } => ArtifactVerificationSubjectKind::File,
            Self::MachineImage { .. } => ArtifactVerificationSubjectKind::MachineImage,
            Self::GuestExecutable { .. } => ArtifactVerificationSubjectKind::GuestExecutable,
        }
    }

    pub fn label(&self) -> &'static str {
        self.kind().label()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactVerificationSubjectKind {
    OciImage,
    RuntimeBundle,
    File,
    MachineImage,
    GuestExecutable,
}

impl ArtifactVerificationSubjectKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::OciImage => "oci_image",
            Self::RuntimeBundle => "runtime_bundle",
            Self::File => "file",
            Self::MachineImage => "machine_image",
            Self::GuestExecutable => "guest_executable",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactVerificationPolicy {
    signature: Option<ArtifactSignatureRequirement>,
    provenance: Option<ArtifactProvenanceRequirement>,
    sbom_required: bool,
}

impl ArtifactVerificationPolicy {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn require_signature(
        mut self,
        issuer: impl Into<String>,
        subject: impl Into<String>,
    ) -> Self {
        self.signature = Some(ArtifactSignatureRequirement::new(
            Some(issuer.into()),
            Some(subject.into()),
        ));
        self
    }

    pub fn with_signature_requirement(mut self, signature: ArtifactSignatureRequirement) -> Self {
        self.signature = Some(signature);
        self
    }

    pub fn require_provenance(
        mut self,
        builder_id: impl Into<String>,
        predicate_types: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.provenance = Some(ArtifactProvenanceRequirement::new(
            Some(builder_id.into()),
            None,
            predicate_types.into_iter().map(Into::into).collect(),
        ));
        self
    }

    pub fn require_provenance_from_source(
        mut self,
        builder_id: impl Into<String>,
        source_uri: impl Into<String>,
        predicate_types: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.provenance = Some(ArtifactProvenanceRequirement::new(
            Some(builder_id.into()),
            Some(source_uri.into()),
            predicate_types.into_iter().map(Into::into).collect(),
        ));
        self
    }

    pub fn with_provenance_requirement(
        mut self,
        provenance: ArtifactProvenanceRequirement,
    ) -> Self {
        self.provenance = Some(provenance);
        self
    }

    pub fn require_sbom(mut self) -> Self {
        self.sbom_required = true;
        self
    }

    pub fn signature(&self) -> Option<&ArtifactSignatureRequirement> {
        self.signature.as_ref()
    }

    pub fn provenance(&self) -> Option<&ArtifactProvenanceRequirement> {
        self.provenance.as_ref()
    }

    pub fn sbom_required(&self) -> bool {
        self.sbom_required
    }

    pub fn requires_verification(&self) -> bool {
        self.signature.is_some() || self.provenance.is_some() || self.sbom_required
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactSignatureRequirement {
    issuer: Option<String>,
    subject: Option<String>,
}

impl ArtifactSignatureRequirement {
    pub fn new(issuer: Option<String>, subject: Option<String>) -> Self {
        Self { issuer, subject }
    }

    pub fn issuer(&self) -> Option<&str> {
        self.issuer.as_deref()
    }

    pub fn subject(&self) -> Option<&str> {
        self.subject.as_deref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactProvenanceRequirement {
    builder_id: Option<String>,
    source_uri: Option<String>,
    predicate_types: Vec<String>,
}

impl ArtifactProvenanceRequirement {
    pub fn new(
        builder_id: Option<String>,
        source_uri: Option<String>,
        predicate_types: Vec<String>,
    ) -> Self {
        Self {
            builder_id,
            source_uri,
            predicate_types,
        }
    }

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ArtifactVerificationEvidence {
    backend: ArtifactVerifierBackendIdentity,
    signatures: Vec<ArtifactSignatureEvidence>,
    attestations: Vec<ArtifactAttestationEvidence>,
    sbom_present: bool,
}

impl ArtifactVerificationEvidence {
    pub fn new(backend: ArtifactVerifierBackendIdentity) -> Self {
        Self {
            backend,
            signatures: Vec::new(),
            attestations: Vec::new(),
            sbom_present: false,
        }
    }

    pub fn with_signature(mut self, issuer: impl Into<String>, subject: impl Into<String>) -> Self {
        self.signatures.push(ArtifactSignatureEvidence {
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
        self.attestations.push(ArtifactAttestationEvidence::new(
            builder_id.into(),
            None,
            predicate_type.into(),
        ));
        self
    }

    pub fn with_attestation_from_source(
        mut self,
        builder_id: impl Into<String>,
        source_uri: impl Into<String>,
        predicate_type: impl Into<String>,
    ) -> Self {
        self.attestations.push(ArtifactAttestationEvidence::new(
            builder_id.into(),
            Some(source_uri.into()),
            predicate_type.into(),
        ));
        self
    }

    pub fn with_sbom(mut self) -> Self {
        self.sbom_present = true;
        self
    }

    pub fn backend(&self) -> &ArtifactVerifierBackendIdentity {
        &self.backend
    }

    pub fn signatures(&self) -> &[ArtifactSignatureEvidence] {
        &self.signatures
    }

    pub fn attestations(&self) -> &[ArtifactAttestationEvidence] {
        &self.attestations
    }

    pub fn sbom_present(&self) -> bool {
        self.sbom_present
    }

    fn merge(mut self, other: ArtifactVerificationEvidence) -> Self {
        self.signatures.extend(other.signatures);
        self.attestations.extend(other.attestations);
        self.sbom_present |= other.sbom_present;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ArtifactSignatureEvidence {
    issuer: String,
    subject: String,
}

impl ArtifactSignatureEvidence {
    pub fn issuer(&self) -> &str {
        &self.issuer
    }

    pub fn subject(&self) -> &str {
        &self.subject
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ArtifactAttestationEvidence {
    builder_id: String,
    source_uri: Option<String>,
    predicate_type: String,
}

impl ArtifactAttestationEvidence {
    fn new(builder_id: String, source_uri: Option<String>, predicate_type: String) -> Self {
        Self {
            builder_id,
            source_uri,
            predicate_type,
        }
    }

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ArtifactVerifierBackendIdentity {
    name: String,
    version: String,
}

impl ArtifactVerifierBackendIdentity {
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn validate(&self) -> ArtifactVerifierResult<()> {
        if self.name.trim().is_empty() {
            return Err(ArtifactVerifierError::malformed_output(
                "artifact verifier backend name cannot be empty",
            ));
        }
        if self.version.trim().is_empty() {
            return Err(ArtifactVerifierError::malformed_output(
                "artifact verifier backend version cannot be empty",
            ));
        }
        Ok(())
    }
}

pub struct CompositeArtifactVerifierBackend {
    identity: ArtifactVerifierBackendIdentity,
    backends: Vec<Arc<dyn ArtifactVerifierBackend>>,
}

impl CompositeArtifactVerifierBackend {
    pub fn new(
        backends: impl IntoIterator<Item = Arc<dyn ArtifactVerifierBackend>>,
    ) -> Result<Self> {
        Self::with_identity(
            ArtifactVerifierBackendIdentity::new("composite-artifact-verifier", "cli-chain"),
            backends,
        )
    }

    pub fn with_identity(
        identity: ArtifactVerifierBackendIdentity,
        backends: impl IntoIterator<Item = Arc<dyn ArtifactVerifierBackend>>,
    ) -> Result<Self> {
        let backends = backends.into_iter().collect::<Vec<_>>();
        if backends.is_empty() {
            return Err(Error::InvalidInput(
                "composite artifact verifier requires at least one backend".to_string(),
            ));
        }
        Ok(Self { identity, backends })
    }
}

impl std::fmt::Debug for CompositeArtifactVerifierBackend {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CompositeArtifactVerifierBackend")
            .field("identity", &self.identity)
            .field("backend_count", &self.backends.len())
            .finish_non_exhaustive()
    }
}

impl ArtifactVerifierBackend for CompositeArtifactVerifierBackend {
    fn verify_artifact(
        &self,
        request: &ArtifactVerificationRequest,
    ) -> ArtifactVerifierResult<ArtifactVerificationEvidence> {
        self.identity.validate()?;
        let mut merged = ArtifactVerificationEvidence::new(self.identity.clone());
        for backend in &self.backends {
            let evidence = backend
                .verify_artifact(request)
                .map_err(ArtifactVerifierError::redacted)?;
            merged = merged.merge(evidence);
        }
        Ok(merged)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactVerifierError {
    pub kind: ArtifactVerifierErrorKind,
    pub message: String,
}

impl ArtifactVerifierError {
    pub fn backend_error(message: impl Into<String>) -> Self {
        Self {
            kind: ArtifactVerifierErrorKind::BackendError,
            message: message.into(),
        }
    }

    pub fn command_failure(message: impl Into<String>) -> Self {
        Self {
            kind: ArtifactVerifierErrorKind::CommandFailure,
            message: message.into(),
        }
    }

    pub fn malformed_output(message: impl Into<String>) -> Self {
        Self {
            kind: ArtifactVerifierErrorKind::MalformedOutput,
            message: message.into(),
        }
    }

    pub fn timeout(message: impl Into<String>) -> Self {
        Self {
            kind: ArtifactVerifierErrorKind::Timeout,
            message: message.into(),
        }
    }

    pub fn unavailable(message: impl Into<String>) -> Self {
        Self {
            kind: ArtifactVerifierErrorKind::Unavailable,
            message: message.into(),
        }
    }

    pub fn unsupported_artifact(message: impl Into<String>) -> Self {
        Self {
            kind: ArtifactVerifierErrorKind::UnsupportedArtifact,
            message: message.into(),
        }
    }

    pub fn redacted(mut self) -> Self {
        self.message = redact_artifact_verifier_output(&self.message);
        self
    }
}

impl std::fmt::Display for ArtifactVerifierError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.kind.label(), self.message)
    }
}

impl std::error::Error for ArtifactVerifierError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactVerifierErrorKind {
    BackendError,
    CommandFailure,
    MalformedOutput,
    Timeout,
    Unavailable,
    UnsupportedArtifact,
}

impl ArtifactVerifierErrorKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::BackendError => "backend_error",
            Self::CommandFailure => "command_failure",
            Self::MalformedOutput => "malformed_output",
            Self::Timeout => "timeout",
            Self::Unavailable => "unavailable",
            Self::UnsupportedArtifact => "unsupported_artifact",
        }
    }
}

/// Redacts verifier subprocess diagnostics before they are exposed to callers.
///
/// This is a best-effort line redactor: any line containing a sensitive keyword
/// or common unlabeled secret shape is replaced wholesale. The typed verifier
/// result remains authoritative; this helper favors hiding suspicious output
/// over preserving every benign diagnostic byte.
pub fn redact_artifact_verifier_output(output: &str) -> String {
    if output.is_empty() {
        return String::new();
    }
    output
        .lines()
        .map(|line| {
            if line_contains_sensitive_artifact_verifier_output(line) {
                REDACTED_ARTIFACT_VERIFIER_OUTPUT.to_string()
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn line_contains_sensitive_artifact_verifier_output(line: &str) -> bool {
    let normalized = line.to_ascii_lowercase();
    SENSITIVE_OUTPUT_FRAGMENTS
        .iter()
        .any(|fragment| normalized.contains(fragment))
        || line_contains_sensitive_value_shape(line)
}

fn line_contains_sensitive_value_shape(line: &str) -> bool {
    line.split(|ch: char| !is_secret_shape_char(ch))
        .any(token_has_sensitive_value_shape)
}

fn token_has_sensitive_value_shape(token: &str) -> bool {
    is_jwt_like(token)
        || is_aws_access_key_id(token)
        || is_long_hex_secret(token)
        || is_long_base64_secret(token)
}

fn is_jwt_like(token: &str) -> bool {
    if !token.starts_with("eyJ") {
        return false;
    }
    let mut segments = token.split('.');
    matches!(
        (segments.next(), segments.next(), segments.next(), segments.next()),
        (Some(header), Some(claims), Some(signature), None)
            if [header, claims, signature]
                .iter()
                .all(|segment| !segment.is_empty() && segment.chars().all(is_base64url_char))
    )
}

fn is_aws_access_key_id(token: &str) -> bool {
    token.len() == 20
        && (token.starts_with("AKIA") || token.starts_with("ASIA"))
        && token
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
}

fn is_long_hex_secret(token: &str) -> bool {
    token.len() >= 40 && token.as_bytes().iter().all(u8::is_ascii_hexdigit)
}

fn is_long_base64_secret(token: &str) -> bool {
    let has_uppercase = token.chars().any(|ch| ch.is_ascii_uppercase());
    let has_lowercase = token.chars().any(|ch| ch.is_ascii_lowercase());
    let has_strong_base64_marker = token
        .chars()
        .any(|ch| ch.is_ascii_digit() || matches!(ch, '+' | '/' | '=' | '_'));
    token.len() >= 40
        && token.chars().all(is_base64_or_base64url_char)
        && has_uppercase
        && has_lowercase
        && has_strong_base64_marker
}

fn is_secret_shape_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '+' | '/' | '=' | '_' | '-' | '.')
}

fn is_base64url_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_')
}

fn is_base64_or_base64url_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '+' | '/' | '=' | '_' | '-')
}

const REDACTED_ARTIFACT_VERIFIER_OUTPUT: &str = "[redacted verifier output]";

const SENSITIVE_OUTPUT_FRAGMENTS: &[&str] = &[
    "authorization",
    "bearer",
    "client_secret",
    "cookie",
    "credential",
    "password",
    "private key",
    "private_key",
    "registry auth",
    "registry_auth",
    "secret",
    "token",
    "x-amz-security-token",
];

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
    use super::*;

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
    fn composite_backend_merges_signature_provenance_and_sbom_evidence() {
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
        let backend = CompositeArtifactVerifierBackend::new(backends)
            .expect("composite verifier should build");
        let request = ArtifactVerificationRequest::oci_image(
            IMAGE,
            ArtifactVerificationPolicy::new()
                .require_signature("https://issuer.example.com", "repo:nimbus/api")
                .require_provenance_from_source(
                    "https://github.com/nimbus/builder",
                    "github.com/nimbus/nimbus",
                    ["https://slsa.dev/provenance/v1"],
                )
                .require_sbom(),
        );

        let evidence = backend
            .verify_artifact(&request)
            .expect("composite evidence should satisfy all artifact policy requirements");

        assert_eq!(evidence.signatures().len(), 1);
        assert_eq!(evidence.attestations().len(), 1);
        assert_eq!(
            evidence.attestations()[0].source_uri(),
            Some("github.com/nimbus/nimbus")
        );
        assert!(evidence.sbom_present());
    }

    #[test]
    fn redaction_covers_tokens_credentials_secret_handles_and_registry_auth() {
        let redacted = redact_artifact_verifier_output(
            "token=do-not-log-token\n\
             credential=do-not-log-credential\n\
             secret_handle=nimbus://secret/prod/db/password\n\
             registry_auth={\"auth\":\"do-not-log-registry-auth\"}\n\
             ordinary diagnostic line",
        );

        assert!(redacted.contains("ordinary diagnostic line"));
        assert_eq!(redacted.matches("[redacted verifier output]").count(), 4);
        for secret in [
            "do-not-log-token",
            "do-not-log-credential",
            "nimbus://secret/prod/db/password",
            "do-not-log-registry-auth",
        ] {
            assert!(
                !redacted.contains(secret),
                "redacted output should not leak {secret}: {redacted}"
            );
        }
    }

    #[test]
    fn redaction_covers_unlabeled_secret_value_shapes() {
        let jwt = "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiJuaW1idXMifQ.signature";
        let registry_auth_blob = "c2VjcmV0LXJlZ2lzdHJ5LWF1dGgtcGxhaW50ZXh0LWJsb2I=";
        let hex_secret = "0123456789abcdef0123456789abcdef01234567";
        let aws_access_key = "AKIAIOSFODNN7EXAMPLE";

        let redacted = redact_artifact_verifier_output(&format!(
            "verifier stderr: {jwt}\n\
             verifier stderr: {registry_auth_blob}\n\
             verifier stderr: {hex_secret}\n\
             verifier stderr: {aws_access_key}\n\
             ordinary diagnostic line"
        ));

        assert!(redacted.contains("ordinary diagnostic line"));
        assert_eq!(
            redacted.matches(REDACTED_ARTIFACT_VERIFIER_OUTPUT).count(),
            4
        );
        for secret in [jwt, registry_auth_blob, hex_secret, aws_access_key] {
            assert!(
                !redacted.contains(secret),
                "redacted output should not leak {secret}: {redacted}"
            );
        }
    }

    #[test]
    fn redaction_preserves_actionable_missing_tool_diagnostics() {
        let diagnostic = "failed to start artifact verifier `nimbus-artifact-verifier-definitely-missing`: \
             No such file or directory (os error 2)";

        let redacted = redact_artifact_verifier_output(diagnostic);

        assert!(redacted.contains("failed to start artifact verifier"));
        assert!(redacted.contains("nimbus-artifact-verifier-definitely-missing"));
        assert!(!redacted.contains("[redacted verifier output]"));
    }
}
