use std::sync::Arc;
use std::time::Duration;

use nimbus_core::{Error, Result};
use serde::Deserialize;

use super::{
    ArtifactVerificationEvidence, ArtifactVerificationRequest, ArtifactVerificationSubject,
    ArtifactVerifierBackend, ArtifactVerifierBackendIdentity, ArtifactVerifierCommandInvocation,
    ArtifactVerifierCommandRunner, ArtifactVerifierError, ArtifactVerifierResult,
    DEFAULT_ARTIFACT_VERIFIER_TIMEOUT, OfflineVerificationConfig,
    ProcessArtifactVerifierCommandRunner, redact_artifact_verifier_output,
};
use crate::tenant::{has_sha256_digest, parse_oci_image_reference};

pub struct CosignVerifierBackend {
    program: String,
    identity: ArtifactVerifierBackendIdentity,
    timeout: Duration,
    runner: Arc<dyn ArtifactVerifierCommandRunner>,
    offline: Option<OfflineVerificationConfig>,
}

impl CosignVerifierBackend {
    pub fn new() -> Self {
        Self {
            program: "cosign".to_string(),
            identity: ArtifactVerifierBackendIdentity::new("cosign", "cli"),
            timeout: DEFAULT_ARTIFACT_VERIFIER_TIMEOUT,
            runner: Arc::new(ProcessArtifactVerifierCommandRunner),
            offline: None,
        }
    }

    pub fn with_program(mut self, program: impl Into<String>) -> Self {
        self.program = program.into();
        self
    }

    pub fn with_identity(mut self, identity: ArtifactVerifierBackendIdentity) -> Self {
        self.identity = identity;
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Result<Self> {
        if timeout.is_zero() {
            return Err(Error::InvalidInput(
                "cosign verifier timeout must be greater than 0".to_string(),
            ));
        }
        self.timeout = timeout;
        Ok(self)
    }

    pub fn with_runner(mut self, runner: Arc<dyn ArtifactVerifierCommandRunner>) -> Self {
        self.runner = runner;
        self
    }

    pub fn with_offline_trusted_root(
        mut self,
        trusted_root_path: impl Into<std::path::PathBuf>,
    ) -> Self {
        self.offline = Some(OfflineVerificationConfig::new(trusted_root_path));
        self
    }
}

impl Default for CosignVerifierBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for CosignVerifierBackend {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CosignVerifierBackend")
            .field("program", &self.program)
            .field("identity", &self.identity)
            .field("timeout", &self.timeout)
            .field("offline", &self.offline)
            .finish_non_exhaustive()
    }
}

impl ArtifactVerifierBackend for CosignVerifierBackend {
    fn verify_artifact(
        &self,
        request: &ArtifactVerificationRequest,
    ) -> ArtifactVerifierResult<ArtifactVerificationEvidence> {
        let ArtifactVerificationSubject::OciImage { reference } = request.subject() else {
            return Err(ArtifactVerifierError::unsupported_artifact(format!(
                "cosign verifier supports `{}` artifacts only",
                super::ArtifactVerificationSubjectKind::OciImage.label()
            )));
        };
        let parsed_reference = parse_oci_image_reference(reference).map_err(|error| {
            ArtifactVerifierError::backend_error(format!(
                "cosign verifier requires a valid OCI image reference: {error}"
            ))
        })?;
        if !has_sha256_digest(&parsed_reference) {
            return Err(ArtifactVerifierError::backend_error(format!(
                "cosign verifier requires an immutable sha256 digest image reference, but `{reference}` is tag-only or missing a valid digest"
            )));
        }
        let expected_digest = parsed_reference.digest().ok_or_else(|| {
            ArtifactVerifierError::backend_error(format!(
                "cosign verifier could not read digest from `{reference}` after OCI parsing"
            ))
        })?;
        let signature = request.policy().signature().ok_or_else(|| {
            ArtifactVerifierError::backend_error(
                "cosign verifier requires a signature policy with certificate issuer and subject",
            )
        })?;
        let issuer = signature.issuer().ok_or_else(|| {
            ArtifactVerifierError::backend_error(
                "cosign verifier requires certificate issuer policy",
            )
        })?;
        let subject = signature.subject().ok_or_else(|| {
            ArtifactVerifierError::backend_error(
                "cosign verifier requires certificate identity policy",
            )
        })?;
        if issuer.trim().is_empty() || subject.trim().is_empty() {
            return Err(ArtifactVerifierError::backend_error(
                "cosign verifier requires non-empty certificate issuer and identity policy",
            ));
        }
        if let Some(offline) = &self.offline {
            offline.validate("cosign verifier")?;
        }
        let canonical_reference = parsed_reference.whole();
        let mut args = vec![
            "verify".to_string(),
            "--output".to_string(),
            "json".to_string(),
            "--check-claims=true".to_string(),
        ];
        if let Some(offline) = &self.offline {
            args.extend([
                "--offline=true".to_string(),
                "--trusted-root".to_string(),
                offline.trusted_root_path().display().to_string(),
            ]);
        }
        args.extend([
            "--certificate-identity".to_string(),
            subject.to_string(),
            "--certificate-oidc-issuer".to_string(),
            issuer.to_string(),
            canonical_reference.clone(),
        ]);
        let invocation = ArtifactVerifierCommandInvocation {
            program: self.program.clone(),
            args,
            timeout: self.timeout,
            stdin: None,
        };
        let output = self
            .runner
            .run(&invocation)
            .map_err(ArtifactVerifierError::redacted)?;
        if !output.is_success() {
            return Err(ArtifactVerifierError::command_failure(format!(
                "cosign verifier exited with status {} for `{canonical_reference}`: stdout: {}; stderr: {}",
                output.status_label(),
                redact_artifact_verifier_output(&output.stdout),
                redact_artifact_verifier_output(&output.stderr)
            )));
        }
        parse_cosign_payload_digest_claims(&output.stdout, expected_digest, &canonical_reference)?;
        Ok(
            ArtifactVerificationEvidence::new(self.identity.clone())
                .with_signature(issuer, subject),
        )
    }
}

#[derive(Debug, Deserialize)]
struct CosignVerifiedPayload {
    #[serde(default, alias = "Critical")]
    critical: Option<CosignCriticalClaims>,
}

#[derive(Debug, Deserialize)]
struct CosignCriticalClaims {
    #[serde(default, alias = "Image")]
    image: Option<CosignImageClaims>,
}

#[derive(Debug, Deserialize)]
struct CosignImageClaims {
    #[serde(
        default,
        rename = "docker-manifest-digest",
        alias = "Docker-manifest-digest"
    )]
    docker_manifest_digest: Option<String>,
}

fn parse_cosign_payload_digest_claims(
    stdout: &str,
    expected_digest: &str,
    image_reference: &str,
) -> ArtifactVerifierResult<()> {
    let values = parse_cosign_payload_values(stdout, image_reference)?;
    if values.is_empty() {
        return Err(ArtifactVerifierError::malformed_output(format!(
            "cosign verifier emitted no verified payloads for `{image_reference}`"
        )));
    }
    for value in values {
        let payload: CosignVerifiedPayload = serde_json::from_value(value).map_err(|error| {
            ArtifactVerifierError::malformed_output(format!(
                "cosign verifier emitted malformed verified payload for `{image_reference}`: {error}; stdout: {}",
                redact_artifact_verifier_output(stdout)
            ))
        })?;
        let claim = payload
            .critical
            .and_then(|critical| critical.image)
            .and_then(|image| image.docker_manifest_digest)
            .ok_or_else(|| {
                ArtifactVerifierError::malformed_output(format!(
                    "cosign verifier output for `{image_reference}` did not include a docker manifest digest claim"
                ))
            })?;
        if claim != expected_digest {
            return Err(ArtifactVerifierError::malformed_output(format!(
                "cosign verifier output for `{image_reference}` claimed digest `{claim}`, expected `{expected_digest}`"
            )));
        }
    }
    Ok(())
}

fn parse_cosign_payload_values(
    stdout: &str,
    image_reference: &str,
) -> ArtifactVerifierResult<Vec<serde_json::Value>> {
    let value: serde_json::Value = serde_json::from_str(stdout).map_err(|error| {
        ArtifactVerifierError::malformed_output(format!(
            "cosign verifier emitted malformed JSON for `{image_reference}`: {error}; stdout: {}",
            redact_artifact_verifier_output(stdout)
        ))
    })?;
    match value {
        serde_json::Value::Array(values) => Ok(values),
        serde_json::Value::Object(_) => Ok(vec![value]),
        _ => Err(ArtifactVerifierError::malformed_output(format!(
            "cosign verifier emitted non-object JSON for `{image_reference}`"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::super::ArtifactVerifierCommandOutput;
    use super::*;
    use crate::tenant::{
        ArtifactVerificationPolicy, ArtifactVerificationSubjectKind, ArtifactVerifierErrorKind,
    };

    const DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const OTHER_DIGEST: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
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

    fn signed_payload(digest: &str) -> String {
        format!(r#"[{{"critical":{{"image":{{"docker-manifest-digest":"sha256:{digest}"}}}}}}]"#)
    }

    fn request(image_reference: &str) -> ArtifactVerificationRequest {
        ArtifactVerificationRequest::oci_image(
            image_reference,
            ArtifactVerificationPolicy::new().require_signature(
                "https://token.actions.githubusercontent.com",
                "https://github.com/nimbus/nimbus/.github/workflows/release.yml@refs/heads/main",
            ),
        )
    }

    fn backend(runner: Arc<StaticCommandRunner>) -> CosignVerifierBackend {
        CosignVerifierBackend::new()
            .with_program("cosign-fixture")
            .with_identity(ArtifactVerifierBackendIdentity::new(
                "cosign-fixture",
                "test",
            ))
            .with_runner(runner)
    }

    fn trusted_root_fixture() -> (tempfile::TempDir, std::path::PathBuf) {
        let temp = tempfile::tempdir().expect("tempdir should create");
        let path = temp.path().join("trusted_root.json");
        std::fs::write(
            &path,
            r#"{"mediaType":"application/vnd.dev.sigstore.trustedroot+json"}"#,
        )
        .expect("trusted root fixture should write");
        (temp, path)
    }

    #[test]
    fn cosign_backend_accepts_signed_digest_pinned_image_and_normalizes_signature() {
        let runner = Arc::new(StaticCommandRunner::success(signed_payload(DIGEST)));
        let backend = backend(Arc::clone(&runner));

        let evidence = backend
            .verify_artifact(&request(IMAGE))
            .expect("matching cosign output should produce signature evidence");

        assert_eq!(evidence.signatures().len(), 1);
        assert_eq!(
            evidence.signatures()[0].issuer(),
            "https://token.actions.githubusercontent.com"
        );
        assert_eq!(
            evidence.signatures()[0].subject(),
            "https://github.com/nimbus/nimbus/.github/workflows/release.yml@refs/heads/main"
        );
        let invocations = runner.invocations();
        assert_eq!(invocations.len(), 1);
        assert_eq!(invocations[0].program(), "cosign-fixture");
        assert_eq!(
            invocations[0].args(),
            [
                "verify",
                "--output",
                "json",
                "--check-claims=true",
                "--certificate-identity",
                "https://github.com/nimbus/nimbus/.github/workflows/release.yml@refs/heads/main",
                "--certificate-oidc-issuer",
                "https://token.actions.githubusercontent.com",
                IMAGE
            ]
        );
        assert_eq!(invocations[0].stdin(), None);
    }

    #[test]
    fn cosign_backend_rejects_unsigned_image_exit_and_redacts_output() {
        let runner = Arc::new(StaticCommandRunner::failure(
            "authorization: Bearer do-not-log-token",
            "no matching signatures; registry_auth=do-not-log-registry-auth",
        ));
        let backend = backend(runner);

        let error = backend
            .verify_artifact(&request(IMAGE))
            .expect_err("unsigned image should fail closed through cosign");

        assert_eq!(error.kind, ArtifactVerifierErrorKind::CommandFailure);
        let rendered = error.to_string();
        assert!(rendered.contains("[redacted verifier output]"));
        assert!(!rendered.contains("do-not-log-token"));
        assert!(!rendered.contains("do-not-log-registry-auth"));
    }

    #[test]
    fn cosign_backend_rejects_wrong_issuer_or_subject_through_command_failure() {
        let runner = Arc::new(StaticCommandRunner::failure(
            "",
            "certificate identity did not match expected subject",
        ));
        let backend = backend(runner);

        let error = backend
            .verify_artifact(&request(IMAGE))
            .expect_err("wrong certificate identity should be denied by cosign");

        assert_eq!(error.kind, ArtifactVerifierErrorKind::CommandFailure);
        assert!(
            error
                .to_string()
                .contains("certificate identity did not match"),
            "wrong identity failure should preserve non-sensitive diagnostics: {error}"
        );
    }

    #[test]
    fn cosign_backend_rejects_mutable_tag_only_images_before_command() {
        let runner = Arc::new(StaticCommandRunner::success(signed_payload(DIGEST)));
        let backend = backend(Arc::clone(&runner));

        let error = backend
            .verify_artifact(&request("registry.example.com/nimbus/api:latest"))
            .expect_err("tag-only image should fail before cosign command execution");

        assert_eq!(error.kind, ArtifactVerifierErrorKind::BackendError);
        assert!(
            error.to_string().contains("immutable sha256 digest"),
            "mutable image error should name digest requirement: {error}"
        );
        assert!(
            runner.invocations().is_empty(),
            "mutable image references should not invoke cosign"
        );
    }

    #[test]
    fn cosign_backend_rejects_wrong_digest_claim() {
        let runner = Arc::new(StaticCommandRunner::success(signed_payload(OTHER_DIGEST)));
        let backend = backend(runner);

        let error = backend
            .verify_artifact(&request(IMAGE))
            .expect_err("digest claim mismatch should fail closed");

        assert_eq!(error.kind, ArtifactVerifierErrorKind::MalformedOutput);
        assert!(
            error.to_string().contains("claimed digest")
                && error
                    .to_string()
                    .contains(&format!("sha256:{OTHER_DIGEST}")),
            "wrong digest claim should be visible without raw secrets: {error}"
        );
    }

    #[test]
    fn cosign_backend_rejects_malformed_verifier_output() {
        let runner = Arc::new(StaticCommandRunner::success("not-json"));
        let backend = backend(runner);

        let error = backend
            .verify_artifact(&request(IMAGE))
            .expect_err("malformed cosign output should fail closed");

        assert_eq!(error.kind, ArtifactVerifierErrorKind::MalformedOutput);
        assert!(
            error.to_string().contains("malformed JSON"),
            "malformed output error should name the JSON boundary: {error}"
        );
    }

    #[test]
    fn cosign_backend_missing_tool_fails_closed() {
        let backend = CosignVerifierBackend::new().with_program("nimbus-cosign-definitely-missing");

        let error = backend
            .verify_artifact(&request(IMAGE))
            .expect_err("missing cosign binary should fail closed");

        assert_eq!(error.kind, ArtifactVerifierErrorKind::Unavailable);
        assert!(
            error
                .to_string()
                .contains("failed to start artifact verifier"),
            "missing-tool error should be actionable: {error}"
        );
    }

    #[test]
    fn cosign_backend_rejects_non_image_artifacts() {
        let runner = Arc::new(StaticCommandRunner::success(signed_payload(DIGEST)));
        let backend = backend(runner);
        let request = ArtifactVerificationRequest::new(
            ArtifactVerificationSubject::File {
                path: "/tmp/bundle.mjs".into(),
                sha256: Some(format!("sha256:{DIGEST}")),
            },
            ArtifactVerificationPolicy::new().require_signature(
                "https://token.actions.githubusercontent.com",
                "repo:nimbus/nimbus",
            ),
        );

        let error = backend
            .verify_artifact(&request)
            .expect_err("cosign image verifier should reject non-image artifacts");

        assert_eq!(error.kind, ArtifactVerifierErrorKind::UnsupportedArtifact);
        assert!(
            error
                .to_string()
                .contains(ArtifactVerificationSubjectKind::OciImage.label()),
            "unsupported error should name the supported artifact class: {error}"
        );
    }

    #[test]
    fn cosign_backend_requires_signature_policy() {
        let runner = Arc::new(StaticCommandRunner::success(signed_payload(DIGEST)));
        let backend = backend(runner);
        let request =
            ArtifactVerificationRequest::oci_image(IMAGE, ArtifactVerificationPolicy::new());

        let error = backend
            .verify_artifact(&request)
            .expect_err("cosign verifier needs identity policy to call the CLI safely");

        assert_eq!(error.kind, ArtifactVerifierErrorKind::BackendError);
        assert!(
            error.to_string().contains("signature policy"),
            "missing signature policy should be explicit: {error}"
        );
    }

    #[test]
    fn cosign_backend_offline_private_root_passes_trusted_root_without_network() {
        let (_temp, trusted_root) = trusted_root_fixture();
        let runner = Arc::new(StaticCommandRunner::success(signed_payload(DIGEST)));
        let backend = backend(Arc::clone(&runner)).with_offline_trusted_root(&trusted_root);

        backend
            .verify_artifact(&request(IMAGE))
            .expect("offline cosign verification with local trusted root should verify");

        let invocations = runner.invocations();
        let args = invocations[0].args();
        let trusted_root_arg = trusted_root.display().to_string();
        assert!(
            args.windows(3).any(|window| {
                window[0] == "--offline=true"
                    && window[1] == "--trusted-root"
                    && window[2] == trusted_root_arg
            }),
            "offline verification should pass local trusted root args: {args:?}"
        );
    }

    #[test]
    fn cosign_backend_offline_private_root_missing_file_fails_closed_before_command() {
        let temp = tempfile::tempdir().expect("tempdir should create");
        let missing_root = temp.path().join("missing-trusted-root.json");
        let runner = Arc::new(StaticCommandRunner::success(signed_payload(DIGEST)));
        let backend = backend(Arc::clone(&runner)).with_offline_trusted_root(missing_root);

        let error = backend
            .verify_artifact(&request(IMAGE))
            .expect_err("missing local trusted root should fail closed");

        assert_eq!(error.kind, ArtifactVerifierErrorKind::BackendError);
        assert!(error.to_string().contains("trusted root"));
        assert!(
            runner.invocations().is_empty(),
            "missing trusted roots should fail before invoking cosign"
        );
    }
}
