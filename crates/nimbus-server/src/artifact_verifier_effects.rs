#[cfg(test)]
use std::collections::BTreeSet;
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;
#[cfg(test)]
use std::sync::Arc;
#[cfg(test)]
use std::time::Duration;

use nimbus_core::{Error, Result};
use nimbus_runtime::RuntimeBundle;
#[cfg(test)]
use serde::{Deserialize, Serialize};

#[cfg(test)]
mod cosign;
#[cfg(test)]
mod process;
#[cfg(test)]
mod sbom;
#[cfg(test)]
mod slsa;

#[cfg(test)]
pub use process::ProcessArtifactVerifierCommandRunner;

pub(crate) use nimbus_artifacts::{
    ArtifactAdmission, ArtifactVerificationPolicy, ArtifactVerificationSubject,
    ArtifactVerifierBackend, admit_artifact_subject, normalize_artifact_sha256,
};
#[cfg(test)]
pub(crate) use nimbus_artifacts::{
    ArtifactVerificationEvidence, ArtifactVerificationRequest, ArtifactVerificationSubjectKind,
    ArtifactVerifierBackendIdentity, ArtifactVerifierError, ArtifactVerifierResult,
    SLSA_PROVENANCE_V1_PREDICATE_TYPE, redact_artifact_verifier_output,
};

#[cfg(test)]
pub const DEFAULT_ARTIFACT_VERIFIER_TIMEOUT: Duration = Duration::from_secs(10);

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OfflineVerificationConfig {
    trusted_root_path: PathBuf,
}

#[cfg(test)]
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
    let expected_sha256 = normalize_artifact_sha256(expected_sha256).map_err(|error| {
        Error::InvalidInput(format!(
            "{context} runtime bundle `{}` has invalid sha256 identity: {error}",
            bundle.entrypoint().display()
        ))
    })?;
    ensure_path_matches_sha256(bundle.entrypoint(), &expected_sha256, context)?;
    admit_artifact_subject(
        ArtifactVerificationSubject::RuntimeBundle {
            path: bundle.entrypoint().to_path_buf(),
            sha256: format!("sha256:{expected_sha256}"),
        },
        policy,
        verifier,
        context,
    )
}

#[cfg(test)]
pub fn admit_guest_executable_artifact(
    path: impl AsRef<Path>,
    expected_sha256: impl AsRef<str>,
    policy: &ArtifactVerificationPolicy,
    verifier: &dyn ArtifactVerifierBackend,
    context: &str,
) -> Result<ArtifactAdmission> {
    let path = path.as_ref();
    let expected_sha256 = normalize_artifact_sha256(expected_sha256.as_ref()).map_err(|error| {
        Error::InvalidInput(format!(
            "{context} guest executable `{}` has invalid sha256 identity: {error}",
            path.display()
        ))
    })?;
    ensure_path_matches_sha256(path, &expected_sha256, context)?;
    admit_artifact_subject(
        ArtifactVerificationSubject::GuestExecutable {
            path: path.to_path_buf(),
            sha256: format!("sha256:{expected_sha256}"),
        },
        policy,
        verifier,
        context,
    )
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

#[cfg(test)]
pub struct ArtifactVerifierCommandBackend {
    identity: ArtifactVerifierBackendIdentity,
    program: String,
    args: Vec<String>,
    supported_subjects: BTreeSet<ArtifactVerificationSubjectKind>,
    timeout: Duration,
    runner: Arc<dyn ArtifactVerifierCommandRunner>,
}

#[cfg(test)]
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

#[cfg(test)]
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

#[cfg(test)]
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

#[cfg(test)]
pub trait ArtifactVerifierCommandRunner: Send + Sync {
    fn run(
        &self,
        invocation: &ArtifactVerifierCommandInvocation,
    ) -> ArtifactVerifierResult<ArtifactVerifierCommandOutput>;
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactVerifierCommandInvocation {
    program: String,
    args: Vec<String>,
    timeout: Duration,
    stdin: Option<String>,
}

#[cfg(test)]
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

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactVerifierCommandOutput {
    status_code: Option<i32>,
    stdout: String,
    stderr: String,
}

#[cfg(test)]
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

#[cfg(test)]
#[derive(Debug, Deserialize)]
struct NormalizedVerifierOutput {
    #[serde(default)]
    signatures: Vec<NormalizedSignatureEvidence>,
    #[serde(default)]
    attestations: Vec<NormalizedAttestationEvidence>,
    #[serde(default)]
    sbom_present: bool,
}

#[cfg(test)]
#[derive(Debug, Deserialize)]
struct NormalizedSignatureEvidence {
    issuer: String,
    subject: String,
}

#[cfg(test)]
#[derive(Debug, Deserialize)]
struct NormalizedAttestationEvidence {
    builder_id: String,
    #[serde(default)]
    source_uri: Option<String>,
    predicate_type: String,
}

#[cfg(test)]
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
    use nimbus_artifacts::{
        ArtifactProvenanceRequirement, ArtifactSignatureRequirement, ArtifactVerificationPolicy,
        ArtifactVerifierErrorKind,
    };
    use nimbus_tenant::{
        ArtifactImageVerificationProvider, TenantImageAdmissionSource, TenantImagePolicyDecision,
    };

    const DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const IMAGE: &str = "registry.example.com/nimbus/api@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const BUILDER_ID: &str = "https://github.com/nimbus/builder";
    const SOURCE_URI: &str = "github.com/nimbus/nimbus";

    #[derive(Debug, Clone)]
    struct StaticArtifactVerifier {
        evidence: ArtifactVerificationEvidence,
    }

    impl StaticArtifactVerifier {
        fn slsa() -> Self {
            Self {
                evidence: ArtifactVerificationEvidence::new(ArtifactVerifierBackendIdentity::new(
                    "fixture", "test",
                ))
                .with_attestation_from_source(
                    BUILDER_ID,
                    SOURCE_URI,
                    SLSA_PROVENANCE_V1_PREDICATE_TYPE,
                ),
            }
        }
    }

    impl ArtifactVerifierBackend for StaticArtifactVerifier {
        fn verify_artifact(
            &self,
            _request: &ArtifactVerificationRequest,
        ) -> ArtifactVerifierResult<ArtifactVerificationEvidence> {
            Ok(self.evidence.clone())
        }
    }

    #[derive(Debug)]
    struct PanicArtifactVerifier;

    impl ArtifactVerifierBackend for PanicArtifactVerifier {
        fn verify_artifact(
            &self,
            _request: &ArtifactVerificationRequest,
        ) -> ArtifactVerifierResult<ArtifactVerificationEvidence> {
            panic!("host hash failures must not call artifact verifier")
        }
    }

    fn executable_policy() -> ArtifactVerificationPolicy {
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
    fn runtime_bundle_artifact_effect_hashes_path_before_pure_admission() {
        let (_temp, path, sha256) = write_bundle("export default 1;\n");
        let bundle = RuntimeBundle::with_expected_sha256(&path, &sha256)
            .expect("bundle identity should accept sha256");
        let verifier = StaticArtifactVerifier::slsa();

        let admission = admit_runtime_bundle_artifact(
            &bundle,
            &executable_policy(),
            &verifier,
            "runtime invocation",
        )
        .expect("matching runtime bundle provenance should admit");

        assert!(matches!(
            admission.subject(),
            ArtifactVerificationSubject::RuntimeBundle { path: admitted_path, sha256: admitted_sha }
                if admitted_path == &path && admitted_sha == &format!("sha256:{sha256}")
        ));
        assert_eq!(admission.verification().attestations().len(), 1);
    }

    #[test]
    fn runtime_bundle_artifact_effect_rejects_missing_or_wrong_digest_before_verifier() {
        let (_temp, path, _sha256) = write_bundle("export default 1;\n");
        let missing_digest_bundle = RuntimeBundle::new(&path);
        let wrong_digest_bundle = RuntimeBundle::with_expected_sha256(&path, "b".repeat(64))
            .expect("syntactically valid wrong sha should build");

        let missing_digest_error = admit_runtime_bundle_artifact(
            &missing_digest_bundle,
            &executable_policy(),
            &PanicArtifactVerifier,
            "runtime invocation",
        )
        .expect_err("runtime bundle provenance policy should require immutable bundle identity");
        let wrong_digest_error = admit_runtime_bundle_artifact(
            &wrong_digest_bundle,
            &executable_policy(),
            &PanicArtifactVerifier,
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
    }

    #[test]
    fn guest_executable_artifact_effect_reuses_same_hash_and_policy_shape() {
        let (_temp, path, sha256) = write_bundle("#!/bin/sh\nexit 0\n");
        let verifier = StaticArtifactVerifier::slsa();

        let admission = admit_guest_executable_artifact(
            &path,
            &sha256,
            &executable_policy(),
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
    fn command_backend_builder_validation_rejects_empty_subjects_and_zero_timeout() {
        let backend = ArtifactVerifierCommandBackend::new(
            ArtifactVerifierBackendIdentity::new("fixture-verifier", "1.0.0"),
            "fixture-verifier",
        );

        let empty_subjects = backend
            .with_supported_subjects([])
            .expect_err("verifier must declare at least one supported artifact class");
        assert!(
            empty_subjects.to_string().contains("at least one"),
            "empty-subject error should be actionable: {empty_subjects}"
        );

        let zero_timeout = ArtifactVerifierCommandBackend::new(
            ArtifactVerifierBackendIdentity::new("fixture-verifier", "1.0.0"),
            "fixture-verifier",
        )
        .with_timeout(Duration::ZERO)
        .expect_err("zero verifier timeout must fail closed");
        assert!(
            zero_timeout.to_string().contains("greater than 0"),
            "zero-timeout error should be actionable: {zero_timeout}"
        );
    }

    #[test]
    fn command_backend_custom_supported_subjects_are_used_for_admission() {
        let runner = Arc::new(StaticCommandRunner::success("{}"));
        let backend = command_backend(Arc::clone(&runner))
            .with_supported_subjects([ArtifactVerificationSubjectKind::RuntimeBundle])
            .expect("runtime bundle support should configure");
        let request = ArtifactVerificationRequest::new(
            ArtifactVerificationSubject::RuntimeBundle {
                path: PathBuf::from("/srv/nimbus/functions/bundle.mjs"),
                sha256: format!("sha256:{DIGEST}"),
            },
            ArtifactVerificationPolicy::new(),
        );

        backend
            .verify_artifact(&request)
            .expect("custom-supported runtime bundle should reach command runner");

        assert_eq!(runner.invocations().len(), 1);
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
