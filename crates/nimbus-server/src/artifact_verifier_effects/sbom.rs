use std::sync::Arc;
use std::time::Duration;

use nimbus_core::{Error, Result};

use super::{
    ArtifactVerificationEvidence, ArtifactVerificationRequest, ArtifactVerificationSubject,
    ArtifactVerifierBackend, ArtifactVerifierBackendIdentity, ArtifactVerifierCommandInvocation,
    ArtifactVerifierCommandRunner, ArtifactVerifierError, ArtifactVerifierResult,
    DEFAULT_ARTIFACT_VERIFIER_TIMEOUT, ProcessArtifactVerifierCommandRunner,
    redact_artifact_verifier_output,
};
use nimbus_artifacts::{has_sha256_digest, parse_oci_image_reference};

pub struct SbomVerifierBackend {
    program: String,
    identity: ArtifactVerifierBackendIdentity,
    timeout: Duration,
    runner: Arc<dyn ArtifactVerifierCommandRunner>,
}

impl SbomVerifierBackend {
    pub fn new() -> Self {
        Self {
            program: "cosign".to_string(),
            identity: ArtifactVerifierBackendIdentity::new("cosign-sbom", "cli"),
            timeout: DEFAULT_ARTIFACT_VERIFIER_TIMEOUT,
            runner: Arc::new(ProcessArtifactVerifierCommandRunner),
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
                "SBOM verifier timeout must be greater than 0".to_string(),
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

impl Default for SbomVerifierBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for SbomVerifierBackend {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SbomVerifierBackend")
            .field("program", &self.program)
            .field("identity", &self.identity)
            .field("timeout", &self.timeout)
            .finish_non_exhaustive()
    }
}

impl ArtifactVerifierBackend for SbomVerifierBackend {
    fn verify_artifact(
        &self,
        request: &ArtifactVerificationRequest,
    ) -> ArtifactVerifierResult<ArtifactVerificationEvidence> {
        if !request.policy().sbom_required() {
            return Err(ArtifactVerifierError::backend_error(
                "SBOM verifier requires sbom_required policy",
            ));
        }
        let ArtifactVerificationSubject::OciImage { reference } = request.subject() else {
            return Err(ArtifactVerifierError::unsupported_artifact(
                "SBOM verifier currently supports oci_image artifacts only",
            ));
        };
        let parsed_reference = parse_oci_image_reference(reference).map_err(|error| {
            ArtifactVerifierError::backend_error(format!(
                "SBOM verifier requires a valid OCI image reference: {error}"
            ))
        })?;
        if !has_sha256_digest(&parsed_reference) {
            return Err(ArtifactVerifierError::backend_error(format!(
                "SBOM verifier requires an immutable sha256 digest image reference, but `{reference}` is tag-only or missing a valid digest"
            )));
        }
        let canonical_reference = parsed_reference.whole();
        let invocation = ArtifactVerifierCommandInvocation {
            program: self.program.clone(),
            args: vec![
                "download".to_string(),
                "sbom".to_string(),
                "--output-file".to_string(),
                "-".to_string(),
                canonical_reference.clone(),
            ],
            timeout: self.timeout,
            stdin: None,
        };
        let output = self
            .runner
            .run(&invocation)
            .map_err(ArtifactVerifierError::redacted)?;
        if !output.is_success() {
            return Err(ArtifactVerifierError::command_failure(format!(
                "SBOM verifier exited with status {} for `{canonical_reference}`: stdout: {}; stderr: {}",
                output.status_label(),
                redact_artifact_verifier_output(&output.stdout),
                redact_artifact_verifier_output(&output.stderr)
            )));
        }
        if output.stdout.trim().is_empty() {
            return Err(ArtifactVerifierError::malformed_output(format!(
                "SBOM verifier returned empty SBOM output for `{canonical_reference}`"
            )));
        }
        Ok(ArtifactVerificationEvidence::new(self.identity.clone()).with_sbom())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::super::ArtifactVerifierCommandOutput;
    use super::*;
    use nimbus_artifacts::{ArtifactVerificationPolicy, ArtifactVerifierErrorKind};
    use nimbus_tenant::{
        ArtifactImageVerificationProvider, TenantImageAdmissionSource, TenantImagePolicyDecision,
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

    fn backend(runner: Arc<StaticCommandRunner>) -> SbomVerifierBackend {
        SbomVerifierBackend::new()
            .with_program("cosign-sbom-fixture")
            .with_identity(ArtifactVerifierBackendIdentity::new(
                "cosign-sbom-fixture",
                "test",
            ))
            .with_runner(runner)
    }

    #[test]
    fn sbom_backend_accepts_present_image_sbom_and_normalizes_evidence() {
        let runner = Arc::new(StaticCommandRunner::success(
            r#"{"spdxVersion":"SPDX-2.3","packages":[]}"#,
        ));
        let backend = backend(Arc::clone(&runner));
        let request = ArtifactVerificationRequest::oci_image(
            IMAGE,
            ArtifactVerificationPolicy::new().require_sbom(),
        );

        let evidence = backend
            .verify_artifact(&request)
            .expect("present SBOM should produce evidence");

        assert!(evidence.sbom_present());
        assert_eq!(
            runner.invocations()[0].args(),
            ["download", "sbom", "--output-file", "-", IMAGE]
        );
    }

    #[test]
    fn sbom_backend_denies_missing_image_sbom_and_redacts_output() {
        let runner = Arc::new(StaticCommandRunner::failure(
            "",
            "registry_auth=do-not-log-registry-token: no SBOM found",
        ));
        let backend = backend(runner);
        let request = ArtifactVerificationRequest::oci_image(
            IMAGE,
            ArtifactVerificationPolicy::new().require_sbom(),
        );

        let error = backend
            .verify_artifact(&request)
            .expect_err("missing SBOM should fail closed");

        assert_eq!(error.kind, ArtifactVerifierErrorKind::CommandFailure);
        assert!(error.to_string().contains("[redacted verifier output]"));
        assert!(!error.to_string().contains("do-not-log-registry-token"));
    }

    #[test]
    fn sbom_backend_rejects_empty_malformed_output() {
        let runner = Arc::new(StaticCommandRunner::success(" \n\t"));
        let backend = backend(runner);
        let request = ArtifactVerificationRequest::oci_image(
            IMAGE,
            ArtifactVerificationPolicy::new().require_sbom(),
        );

        let error = backend
            .verify_artifact(&request)
            .expect_err("empty SBOM output should fail closed");

        assert_eq!(error.kind, ArtifactVerifierErrorKind::MalformedOutput);
        assert!(error.to_string().contains("empty SBOM"));
    }

    #[test]
    fn sbom_backend_rejects_mutable_images_before_command() {
        let runner = Arc::new(StaticCommandRunner::success("{}"));
        let backend = backend(Arc::clone(&runner));
        let request = ArtifactVerificationRequest::oci_image(
            "registry.example.com/nimbus/api:latest",
            ArtifactVerificationPolicy::new().require_sbom(),
        );

        let error = backend
            .verify_artifact(&request)
            .expect_err("tag-only image should fail closed before command execution");

        assert_eq!(error.kind, ArtifactVerifierErrorKind::BackendError);
        assert!(error.to_string().contains("immutable sha256 digest"));
        assert!(runner.invocations().is_empty());
    }

    #[test]
    fn sbom_backend_missing_tool_fails_closed() {
        let backend = SbomVerifierBackend::new().with_program("nimbus-sbom-definitely-missing");
        let request = ArtifactVerificationRequest::oci_image(
            IMAGE,
            ArtifactVerificationPolicy::new().require_sbom(),
        );

        let error = backend
            .verify_artifact(&request)
            .expect_err("missing SBOM verifier binary should fail closed");

        assert_eq!(error.kind, ArtifactVerifierErrorKind::Unavailable);
        assert!(
            error
                .to_string()
                .contains("failed to start artifact verifier")
        );
    }

    #[test]
    fn sbom_backend_rejects_zero_timeout() {
        let error = SbomVerifierBackend::new()
            .with_timeout(Duration::ZERO)
            .expect_err("zero verifier timeout should be rejected");

        assert!(
            error.to_string().contains("greater than 0"),
            "zero-timeout error should be actionable: {error}"
        );
    }

    #[test]
    fn sbom_backend_satisfies_tenant_image_admission_policy() {
        let runner = Arc::new(StaticCommandRunner::success(
            r#"{"spdxVersion":"SPDX-2.3","packages":[]}"#,
        ));
        let provider = ArtifactImageVerificationProvider::new(backend(runner));
        let policy = TenantImagePolicyDecision::digest_pinned(IMAGE).require_sbom();

        let admission = policy
            .admit_image(TenantImageAdmissionSource::registry(IMAGE), &provider)
            .expect("SBOM verifier evidence should satisfy tenant image policy");

        assert!(admission.verification().sbom_present());
    }

    #[test]
    fn sbom_backend_requires_sbom_policy() {
        let runner = Arc::new(StaticCommandRunner::success("{}"));
        let backend = backend(runner);
        let request = ArtifactVerificationRequest::oci_image(
            format!("registry.example.com/nimbus/api@sha256:{DIGEST}"),
            ArtifactVerificationPolicy::new(),
        );

        let error = backend
            .verify_artifact(&request)
            .expect_err("SBOM verifier should require SBOM policy context");

        assert_eq!(error.kind, ArtifactVerifierErrorKind::BackendError);
        assert!(error.to_string().contains("sbom_required"));
    }
}
