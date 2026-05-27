use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use nimbus_core::{Error, Result};
use serde::{Deserialize, Serialize};

mod cosign;
mod sbom;
mod slsa;

pub use cosign::CosignVerifierBackend;
pub use sbom::SbomVerifierBackend;
pub use slsa::SlsaVerifierBackend;

pub(crate) use crate::tenant::{
    ArtifactVerificationEvidence, ArtifactVerificationRequest, ArtifactVerificationSubject,
    ArtifactVerificationSubjectKind, ArtifactVerifierBackend, ArtifactVerifierBackendIdentity,
    ArtifactVerifierError, ArtifactVerifierResult, SLSA_PROVENANCE_V1_PREDICATE_TYPE,
    redact_artifact_verifier_output,
};

pub const DEFAULT_ARTIFACT_VERIFIER_TIMEOUT: Duration = Duration::from_secs(10);

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

    fn validate(&self, verifier_name: &str) -> ArtifactVerifierResult<()> {
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
    #[serde(default)]
    source_uri: Option<String>,
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
        if attestation
            .source_uri
            .as_deref()
            .is_some_and(|source_uri| source_uri.trim().is_empty())
        {
            return Err(ArtifactVerifierError::malformed_output(format!(
                "artifact verifier `{}` emitted an empty attestation source URI",
                identity.name()
            )));
        }
        evidence = if let Some(source_uri) = attestation.source_uri {
            evidence.with_attestation_from_source(
                attestation.builder_id,
                source_uri,
                attestation.predicate_type,
            )
        } else {
            evidence.with_attestation(attestation.builder_id, attestation.predicate_type)
        };
    }
    if normalized.sbom_present {
        evidence = evidence.with_sbom();
    }
    Ok(evidence)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::tenant::{
        ArtifactImageVerificationProvider, ArtifactProvenanceRequirement,
        ArtifactSignatureRequirement, ArtifactVerificationPolicy, ArtifactVerifierErrorKind,
        TenantImageAdmissionSource, TenantImagePolicyDecision,
    };

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
}
