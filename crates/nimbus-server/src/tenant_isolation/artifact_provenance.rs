use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use nimbus_core::{Error, Result};
use serde::{Deserialize, Serialize};

use super::image_admission::{
    TenantImageVerificationEvidence, TenantImageVerificationProvider,
    TenantImageVerificationRequest,
};

mod admission;
mod cosign;
mod sbom;
mod slsa;

pub use admission::{
    ArtifactAdmission, admit_guest_executable_artifact, admit_runtime_bundle_artifact,
};
pub use cosign::CosignVerifierBackend;
pub use sbom::SbomVerifierBackend;
pub use slsa::{SLSA_PROVENANCE_V1_PREDICATE_TYPE, SlsaVerifierBackend};

pub const DEFAULT_ARTIFACT_VERIFIER_TIMEOUT: Duration = Duration::from_secs(10);

pub type ArtifactVerifierResult<T> = std::result::Result<T, ArtifactVerifierError>;

pub trait ArtifactVerifierBackend: Send + Sync {
    fn verify_artifact(
        &self,
        request: &ArtifactVerificationRequest,
    ) -> ArtifactVerifierResult<ArtifactVerificationEvidence>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OfflineVerificationConfig {
    trusted_root_path: PathBuf,
}

impl OfflineVerificationConfig {
    pub fn new(trusted_root_path: impl Into<PathBuf>) -> Self {
        Self {
            trusted_root_path: trusted_root_path.into(),
        }
    }

    pub fn trusted_root_path(&self) -> &Path {
        &self.trusted_root_path
    }

    pub fn validate(&self, verifier_name: &str) -> ArtifactVerifierResult<()> {
        let metadata = std::fs::metadata(&self.trusted_root_path).map_err(|error| {
            ArtifactVerifierError::backend_error(format!(
                "{verifier_name} offline verification requires readable trusted root `{}`: {error}",
                self.trusted_root_path.display()
            ))
        })?;
        if !metadata.is_file() {
            return Err(ArtifactVerifierError::backend_error(format!(
                "{verifier_name} offline verification trusted root `{}` is not a file",
                self.trusted_root_path.display()
            )));
        }
        if metadata.len() == 0 {
            return Err(ArtifactVerifierError::backend_error(format!(
                "{verifier_name} offline verification trusted root `{}` is empty",
                self.trusted_root_path.display()
            )));
        }
        Ok(())
    }
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

    pub fn from_tenant_image_request(request: &TenantImageVerificationRequest) -> Self {
        Self::oci_image(
            request.image_reference(),
            ArtifactVerificationPolicy::from(request),
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
        self.signature = Some(ArtifactSignatureRequirement {
            issuer: Some(issuer.into()),
            subject: Some(subject.into()),
        });
        self
    }

    pub fn require_provenance(
        mut self,
        builder_id: impl Into<String>,
        predicate_types: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.provenance = Some(ArtifactProvenanceRequirement {
            builder_id: Some(builder_id.into()),
            source_uri: None,
            predicate_types: predicate_types.into_iter().map(Into::into).collect(),
        });
        self
    }

    pub fn require_provenance_from_source(
        mut self,
        builder_id: impl Into<String>,
        source_uri: impl Into<String>,
        predicate_types: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.provenance = Some(ArtifactProvenanceRequirement {
            builder_id: Some(builder_id.into()),
            source_uri: Some(source_uri.into()),
            predicate_types: predicate_types.into_iter().map(Into::into).collect(),
        });
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

impl From<&TenantImageVerificationRequest> for ArtifactVerificationPolicy {
    fn from(request: &TenantImageVerificationRequest) -> Self {
        Self {
            signature: request
                .signature()
                .map(|signature| ArtifactSignatureRequirement {
                    issuer: signature.issuer().map(str::to_string),
                    subject: signature.subject().map(str::to_string),
                }),
            provenance: request
                .provenance()
                .map(|provenance| ArtifactProvenanceRequirement {
                    builder_id: provenance.builder_id().map(str::to_string),
                    source_uri: provenance.source_uri().map(str::to_string),
                    predicate_types: provenance.predicate_types().to_vec(),
                }),
            sbom_required: request.sbom_required(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactSignatureRequirement {
    issuer: Option<String>,
    subject: Option<String>,
}

impl ArtifactSignatureRequirement {
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
        self.attestations.push(ArtifactAttestationEvidence {
            builder_id: builder_id.into(),
            predicate_type: predicate_type.into(),
        });
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

    pub fn to_tenant_image_evidence(&self) -> TenantImageVerificationEvidence {
        let mut evidence = TenantImageVerificationEvidence::new();
        for signature in &self.signatures {
            evidence = evidence.with_signature(signature.issuer(), signature.subject());
        }
        for attestation in &self.attestations {
            evidence =
                evidence.with_attestation(attestation.builder_id(), attestation.predicate_type());
        }
        if self.sbom_present {
            evidence = evidence.with_sbom();
        }
        evidence
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
    predicate_type: String,
}

impl ArtifactAttestationEvidence {
    pub fn builder_id(&self) -> &str {
        &self.builder_id
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

    fn validate(&self) -> ArtifactVerifierResult<()> {
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
        let artifact_request = ArtifactVerificationRequest::from_tenant_image_request(request);
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
        Ok(evidence.to_tenant_image_evidence())
    }
}

pub struct ArtifactVerifierCommandBackend {
    identity: ArtifactVerifierBackendIdentity,
    program: String,
    args: Vec<String>,
    supported_subjects: BTreeSet<ArtifactVerificationSubjectKind>,
    timeout: Duration,
    runner: Arc<dyn ArtifactVerifierCommandRunner>,
}

impl ArtifactVerifierCommandBackend {
    pub fn new(identity: ArtifactVerifierBackendIdentity, program: impl Into<String>) -> Self {
        Self {
            identity,
            program: program.into(),
            args: Vec::new(),
            supported_subjects: BTreeSet::from([ArtifactVerificationSubjectKind::OciImage]),
            timeout: DEFAULT_ARTIFACT_VERIFIER_TIMEOUT,
            runner: Arc::new(ProcessArtifactVerifierCommandRunner),
        }
    }

    pub fn with_args(mut self, args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.args = args.into_iter().map(Into::into).collect();
        self
    }

    pub fn with_supported_subjects(
        mut self,
        subjects: impl IntoIterator<Item = ArtifactVerificationSubjectKind>,
    ) -> Result<Self> {
        self.supported_subjects = subjects.into_iter().collect();
        if self.supported_subjects.is_empty() {
            return Err(Error::InvalidInput(
                "artifact verifier command backend must support at least one artifact class"
                    .to_string(),
            ));
        }
        Ok(self)
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Result<Self> {
        if timeout.is_zero() {
            return Err(Error::InvalidInput(
                "artifact verifier timeout must be greater than 0".to_string(),
            ));
        }
        self.timeout = timeout;
        Ok(self)
    }

    pub fn with_runner(mut self, runner: Arc<dyn ArtifactVerifierCommandRunner>) -> Self {
        self.runner = runner;
        self
    }
}

impl std::fmt::Debug for ArtifactVerifierCommandBackend {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ArtifactVerifierCommandBackend")
            .field("identity", &self.identity)
            .field("program", &self.program)
            .field("args", &self.args)
            .field("supported_subjects", &self.supported_subjects)
            .field("timeout", &self.timeout)
            .finish_non_exhaustive()
    }
}

impl ArtifactVerifierBackend for ArtifactVerifierCommandBackend {
    fn verify_artifact(
        &self,
        request: &ArtifactVerificationRequest,
    ) -> ArtifactVerifierResult<ArtifactVerificationEvidence> {
        self.identity.validate()?;
        let subject_kind = request.subject().kind();
        if !self.supported_subjects.contains(&subject_kind) {
            return Err(ArtifactVerifierError::unsupported_artifact(format!(
                "artifact verifier `{}` does not support `{}` artifacts",
                self.identity.name(),
                subject_kind.label()
            )));
        }
        let stdin = serde_json::to_string(request).map_err(|error| {
            ArtifactVerifierError::backend_error(format!(
                "failed to serialize artifact verifier request: {error}"
            ))
        })?;
        let invocation = ArtifactVerifierCommandInvocation {
            program: self.program.clone(),
            args: self.args.clone(),
            timeout: self.timeout,
            stdin: Some(stdin),
        };
        let output = self
            .runner
            .run(&invocation)
            .map_err(ArtifactVerifierError::redacted)?;
        if !output.is_success() {
            return Err(ArtifactVerifierError::command_failure(format!(
                "artifact verifier `{}` exited with status {}: stdout: {}; stderr: {}",
                self.identity.name(),
                output.status_label(),
                redact_artifact_verifier_output(&output.stdout),
                redact_artifact_verifier_output(&output.stderr)
            )));
        }
        parse_normalized_evidence(&self.identity, &output.stdout)
    }
}

pub trait ArtifactVerifierCommandRunner: Send + Sync {
    fn run(
        &self,
        invocation: &ArtifactVerifierCommandInvocation,
    ) -> ArtifactVerifierResult<ArtifactVerifierCommandOutput>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactVerifierCommandInvocation {
    program: String,
    args: Vec<String>,
    timeout: Duration,
    stdin: Option<String>,
}

impl ArtifactVerifierCommandInvocation {
    pub fn program(&self) -> &str {
        &self.program
    }

    pub fn args(&self) -> &[String] {
        &self.args
    }

    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    pub fn stdin(&self) -> Option<&str> {
        self.stdin.as_deref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactVerifierCommandOutput {
    status_code: Option<i32>,
    stdout: String,
    stderr: String,
}

impl ArtifactVerifierCommandOutput {
    pub fn success(stdout: impl Into<String>) -> Self {
        Self {
            status_code: Some(0),
            stdout: stdout.into(),
            stderr: String::new(),
        }
    }

    pub fn failure(status_code: i32, stdout: impl Into<String>, stderr: impl Into<String>) -> Self {
        Self {
            status_code: Some(status_code),
            stdout: stdout.into(),
            stderr: stderr.into(),
        }
    }

    fn is_success(&self) -> bool {
        self.status_code == Some(0)
    }

    fn status_label(&self) -> String {
        self.status_code
            .map(|code| code.to_string())
            .unwrap_or_else(|| "unknown".to_string())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ProcessArtifactVerifierCommandRunner;

impl ArtifactVerifierCommandRunner for ProcessArtifactVerifierCommandRunner {
    fn run(
        &self,
        invocation: &ArtifactVerifierCommandInvocation,
    ) -> ArtifactVerifierResult<ArtifactVerifierCommandOutput> {
        let mut command = Command::new(invocation.program());
        command
            .args(invocation.args())
            .stdin(if invocation.stdin().is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().map_err(|error| {
            ArtifactVerifierError::unavailable(format!(
                "failed to start artifact verifier `{}`: {error}",
                invocation.program()
            ))
        })?;
        if let Some(stdin) = invocation.stdin() {
            let mut child_stdin = child.stdin.take().ok_or_else(|| {
                ArtifactVerifierError::unavailable(format!(
                    "failed to open stdin for artifact verifier `{}`",
                    invocation.program()
                ))
            })?;
            child_stdin.write_all(stdin.as_bytes()).map_err(|error| {
                ArtifactVerifierError::unavailable(format!(
                    "failed to write request to artifact verifier `{}`: {error}",
                    invocation.program()
                ))
            })?;
        }
        let deadline = Instant::now() + invocation.timeout();
        loop {
            match child.try_wait() {
                Ok(Some(_status)) => {
                    let output = child.wait_with_output().map_err(|error| {
                        ArtifactVerifierError::unavailable(format!(
                            "failed to collect artifact verifier `{}` output: {error}",
                            invocation.program()
                        ))
                    })?;
                    return Ok(ArtifactVerifierCommandOutput {
                        status_code: output.status.code(),
                        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
                    });
                }
                Ok(None) if Instant::now() >= deadline => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(ArtifactVerifierError::timeout(format!(
                        "artifact verifier `{}` exceeded {}ms",
                        invocation.program(),
                        invocation.timeout().as_millis()
                    )));
                }
                Ok(None) => thread::sleep(Duration::from_millis(10)),
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(ArtifactVerifierError::unavailable(format!(
                        "failed to observe artifact verifier `{}`: {error}",
                        invocation.program()
                    )));
                }
            }
        }
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

#[derive(Debug, Deserialize)]
struct NormalizedVerifierOutput {
    #[serde(default)]
    signatures: Vec<NormalizedSignatureEvidence>,
    #[serde(default)]
    attestations: Vec<NormalizedAttestationEvidence>,
    #[serde(default)]
    sbom_present: bool,
}

#[derive(Debug, Deserialize)]
struct NormalizedSignatureEvidence {
    issuer: String,
    subject: String,
}

#[derive(Debug, Deserialize)]
struct NormalizedAttestationEvidence {
    builder_id: String,
    predicate_type: String,
}

fn parse_normalized_evidence(
    identity: &ArtifactVerifierBackendIdentity,
    stdout: &str,
) -> ArtifactVerifierResult<ArtifactVerificationEvidence> {
    let normalized: NormalizedVerifierOutput = serde_json::from_str(stdout).map_err(|error| {
        ArtifactVerifierError::malformed_output(format!(
            "artifact verifier `{}` emitted malformed normalized evidence: {error}; stdout: {}",
            identity.name(),
            redact_artifact_verifier_output(stdout)
        ))
    })?;
    let mut evidence = ArtifactVerificationEvidence::new(identity.clone());
    for signature in normalized.signatures {
        if signature.issuer.trim().is_empty() || signature.subject.trim().is_empty() {
            return Err(ArtifactVerifierError::malformed_output(format!(
                "artifact verifier `{}` emitted an empty signature issuer or subject",
                identity.name()
            )));
        }
        evidence = evidence.with_signature(signature.issuer, signature.subject);
    }
    for attestation in normalized.attestations {
        if attestation.builder_id.trim().is_empty() || attestation.predicate_type.trim().is_empty()
        {
            return Err(ArtifactVerifierError::malformed_output(format!(
                "artifact verifier `{}` emitted an empty attestation builder ID or predicate type",
                identity.name()
            )));
        }
        evidence = evidence.with_attestation(attestation.builder_id, attestation.predicate_type);
    }
    if normalized.sbom_present {
        evidence = evidence.with_sbom();
    }
    Ok(evidence)
}

pub fn redact_artifact_verifier_output(output: &str) -> String {
    if output.is_empty() {
        return String::new();
    }
    output
        .lines()
        .map(|line| {
            let normalized = line.to_ascii_lowercase();
            if SENSITIVE_OUTPUT_FRAGMENTS
                .iter()
                .any(|fragment| normalized.contains(fragment))
            {
                "[redacted verifier output]".to_string()
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

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

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::tenant_isolation::{TenantImageAdmissionSource, TenantImagePolicyDecision};

    const DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const IMAGE: &str = "registry.example.com/nimbus/api@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    #[derive(Debug)]
    struct StaticCommandRunner {
        result: ArtifactVerifierResult<ArtifactVerifierCommandOutput>,
        invocations: Mutex<Vec<ArtifactVerifierCommandInvocation>>,
    }

    impl StaticCommandRunner {
        fn success(stdout: impl Into<String>) -> Self {
            Self {
                result: Ok(ArtifactVerifierCommandOutput::success(stdout)),
                invocations: Mutex::new(Vec::new()),
            }
        }

        fn failure(stdout: impl Into<String>, stderr: impl Into<String>) -> Self {
            Self {
                result: Ok(ArtifactVerifierCommandOutput::failure(1, stdout, stderr)),
                invocations: Mutex::new(Vec::new()),
            }
        }

        fn error(error: ArtifactVerifierError) -> Self {
            Self {
                result: Err(error),
                invocations: Mutex::new(Vec::new()),
            }
        }

        fn invocations(&self) -> Vec<ArtifactVerifierCommandInvocation> {
            self.invocations
                .lock()
                .expect("invocation list should not be poisoned")
                .clone()
        }
    }

    impl ArtifactVerifierCommandRunner for StaticCommandRunner {
        fn run(
            &self,
            invocation: &ArtifactVerifierCommandInvocation,
        ) -> ArtifactVerifierResult<ArtifactVerifierCommandOutput> {
            self.invocations
                .lock()
                .expect("invocation list should not be poisoned")
                .push(invocation.clone());
            self.result.clone()
        }
    }

    fn command_backend(runner: Arc<StaticCommandRunner>) -> ArtifactVerifierCommandBackend {
        ArtifactVerifierCommandBackend::new(
            ArtifactVerifierBackendIdentity::new("fixture-verifier", "1.0.0"),
            "fixture-verifier",
        )
        .with_args(["verify", "--normalized-json"])
        .with_runner(runner)
    }

    #[test]
    fn command_backend_success_normalizes_evidence_and_policy_request() {
        let runner = Arc::new(StaticCommandRunner::success(
            r#"{
                "signatures": [
                    {
                        "issuer": "https://issuer.example.com",
                        "subject": "repo:nimbus/api"
                    }
                ],
                "attestations": [
                    {
                        "builder_id": "https://github.com/nimbus/builder",
                        "predicate_type": "https://slsa.dev/provenance/v1"
                    }
                ],
                "sbom_present": true
            }"#,
        ));
        let provider = ArtifactImageVerificationProvider::new(command_backend(Arc::clone(&runner)));
        let policy = TenantImagePolicyDecision::digest_pinned(IMAGE)
            .require_signature("https://issuer.example.com", "repo:nimbus/api")
            .require_provenance(
                "https://github.com/nimbus/builder",
                ["https://slsa.dev/provenance/v1"],
            )
            .require_sbom();

        let admission = policy
            .admit_image(TenantImageAdmissionSource::registry(IMAGE), &provider)
            .expect("matching verifier evidence should admit the image");

        assert_eq!(admission.verification().signatures().len(), 1);
        assert_eq!(admission.verification().attestations().len(), 1);
        assert!(admission.verification().sbom_present());
        let invocations = runner.invocations();
        assert_eq!(invocations.len(), 1);
        assert_eq!(invocations[0].program(), "fixture-verifier");
        assert_eq!(invocations[0].args(), ["verify", "--normalized-json"]);
        let request: ArtifactVerificationRequest = serde_json::from_str(
            invocations[0]
                .stdin()
                .expect("command adapter should pass normalized request JSON on stdin"),
        )
        .expect("artifact verifier request should deserialize");
        assert!(matches!(
            request.subject(),
            ArtifactVerificationSubject::OciImage { reference } if reference == IMAGE
        ));
        assert_eq!(
            request
                .policy()
                .signature()
                .and_then(ArtifactSignatureRequirement::issuer),
            Some("https://issuer.example.com")
        );
        assert_eq!(
            request
                .policy()
                .provenance()
                .and_then(ArtifactProvenanceRequirement::builder_id),
            Some("https://github.com/nimbus/builder")
        );
        assert!(request.policy().sbom_required());
    }

    #[test]
    fn command_backend_non_zero_exit_fails_closed_and_redacts_output() {
        let runner = Arc::new(StaticCommandRunner::failure(
            "authorization: Bearer do-not-log-token",
            "registry_auth={\"password\":\"do-not-log-password\"}",
        ));
        let backend = command_backend(runner);
        let request =
            ArtifactVerificationRequest::oci_image(IMAGE, ArtifactVerificationPolicy::new());

        let error = backend
            .verify_artifact(&request)
            .expect_err("non-zero verifier exit should fail closed");

        assert_eq!(error.kind, ArtifactVerifierErrorKind::CommandFailure);
        let rendered = error.to_string();
        assert!(rendered.contains("[redacted verifier output]"));
        for secret in ["do-not-log-token", "do-not-log-password"] {
            assert!(
                !rendered.contains(secret),
                "verifier failure should not leak {secret}: {rendered}"
            );
        }
    }

    #[test]
    fn command_backend_missing_executable_fails_closed() {
        let backend = ArtifactVerifierCommandBackend::new(
            ArtifactVerifierBackendIdentity::new("missing-verifier", "1.0.0"),
            "nimbus-artifact-verifier-definitely-missing",
        );
        let request =
            ArtifactVerificationRequest::oci_image(IMAGE, ArtifactVerificationPolicy::new());

        let error = backend
            .verify_artifact(&request)
            .expect_err("missing verifier binary should fail closed");

        assert_eq!(error.kind, ArtifactVerifierErrorKind::Unavailable);
        assert!(
            error
                .to_string()
                .contains("failed to start artifact verifier"),
            "missing-tool error should be actionable: {error}"
        );
    }

    #[test]
    fn command_backend_malformed_output_fails_closed() {
        let runner = Arc::new(StaticCommandRunner::success("not-json"));
        let backend = command_backend(runner);
        let request =
            ArtifactVerificationRequest::oci_image(IMAGE, ArtifactVerificationPolicy::new());

        let error = backend
            .verify_artifact(&request)
            .expect_err("malformed verifier output should fail closed");

        assert_eq!(error.kind, ArtifactVerifierErrorKind::MalformedOutput);
        assert!(
            error.to_string().contains("malformed normalized evidence"),
            "malformed output should name the parser boundary: {error}"
        );
    }

    #[test]
    fn command_backend_timeout_fails_closed_and_redacts_runner_error() {
        let runner = Arc::new(StaticCommandRunner::error(ArtifactVerifierError::timeout(
            "deadline exceeded while using token=do-not-log-timeout-token",
        )));
        let backend = command_backend(runner);
        let request =
            ArtifactVerificationRequest::oci_image(IMAGE, ArtifactVerificationPolicy::new());

        let error = backend
            .verify_artifact(&request)
            .expect_err("timeout should fail closed");

        assert_eq!(error.kind, ArtifactVerifierErrorKind::Timeout);
        let rendered = error.to_string();
        assert!(rendered.contains("[redacted verifier output]"));
        assert!(!rendered.contains("do-not-log-timeout-token"));
    }

    #[test]
    fn command_backend_unsupported_artifact_class_fails_closed() {
        let runner = Arc::new(StaticCommandRunner::success("{}"));
        let backend = command_backend(runner);
        let request = ArtifactVerificationRequest::new(
            ArtifactVerificationSubject::RuntimeBundle {
                path: PathBuf::from("/srv/nimbus/functions/bundle.mjs"),
                sha256: format!("sha256:{DIGEST}"),
            },
            ArtifactVerificationPolicy::new(),
        );

        let error = backend
            .verify_artifact(&request)
            .expect_err("unsupported artifact class should fail closed");

        assert_eq!(error.kind, ArtifactVerifierErrorKind::UnsupportedArtifact);
        assert!(
            error.to_string().contains("runtime_bundle"),
            "unsupported artifact error should name the artifact class: {error}"
        );
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
}
