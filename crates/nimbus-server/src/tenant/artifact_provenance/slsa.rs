use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use nimbus_core::{Error, Result};
use serde::Deserialize;

use super::{
    ArtifactVerificationEvidence, ArtifactVerificationRequest, ArtifactVerificationSubject,
    ArtifactVerifierBackend, ArtifactVerifierBackendIdentity, ArtifactVerifierCommandInvocation,
    ArtifactVerifierCommandRunner, ArtifactVerifierError, ArtifactVerifierResult,
    DEFAULT_ARTIFACT_VERIFIER_TIMEOUT, ProcessArtifactVerifierCommandRunner,
    redact_artifact_verifier_output,
};
use crate::tenant::image_admission::{has_sha256_digest, parse_oci_image_reference};

pub const SLSA_PROVENANCE_V1_PREDICATE_TYPE: &str = "https://slsa.dev/provenance/v1";

pub struct SlsaVerifierBackend {
    program: String,
    identity: ArtifactVerifierBackendIdentity,
    timeout: Duration,
    runner: Arc<dyn ArtifactVerifierCommandRunner>,
    provenance_path: Option<PathBuf>,
}

impl SlsaVerifierBackend {
    pub fn new() -> Self {
        Self {
            program: "slsa-verifier".to_string(),
            identity: ArtifactVerifierBackendIdentity::new("slsa-verifier", "cli"),
            timeout: DEFAULT_ARTIFACT_VERIFIER_TIMEOUT,
            runner: Arc::new(ProcessArtifactVerifierCommandRunner),
            provenance_path: None,
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
                "SLSA verifier timeout must be greater than 0".to_string(),
            ));
        }
        self.timeout = timeout;
        Ok(self)
    }

    pub fn with_runner(mut self, runner: Arc<dyn ArtifactVerifierCommandRunner>) -> Self {
        self.runner = runner;
        self
    }

    pub fn with_provenance_path(mut self, provenance_path: impl Into<PathBuf>) -> Self {
        self.provenance_path = Some(provenance_path.into());
        self
    }
}

impl Default for SlsaVerifierBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for SlsaVerifierBackend {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SlsaVerifierBackend")
            .field("program", &self.program)
            .field("identity", &self.identity)
            .field("timeout", &self.timeout)
            .field("provenance_path", &self.provenance_path)
            .finish_non_exhaustive()
    }
}

impl ArtifactVerifierBackend for SlsaVerifierBackend {
    fn verify_artifact(
        &self,
        request: &ArtifactVerificationRequest,
    ) -> ArtifactVerifierResult<ArtifactVerificationEvidence> {
        let command_subject =
            SlsaCommandSubject::from_request(request, self.provenance_path.as_deref())?;
        let provenance = request.policy().provenance().ok_or_else(|| {
            ArtifactVerifierError::backend_error(
                "SLSA verifier requires provenance policy with builder ID, source URI, and predicate type",
            )
        })?;
        let builder_id = provenance.builder_id().ok_or_else(|| {
            ArtifactVerifierError::backend_error(
                "SLSA verifier requires provenance builder ID policy",
            )
        })?;
        let source_uri = provenance.source_uri().ok_or_else(|| {
            ArtifactVerifierError::backend_error(
                "SLSA verifier requires provenance source_uri policy",
            )
        })?;
        if builder_id.trim().is_empty() || source_uri.trim().is_empty() {
            return Err(ArtifactVerifierError::backend_error(
                "SLSA verifier requires non-empty builder ID and source URI policy",
            ));
        }
        let required_predicates = required_predicate_types(provenance.predicate_types());
        let invocation = ArtifactVerifierCommandInvocation {
            program: self.program.clone(),
            args: command_subject.args(source_uri, builder_id),
            timeout: self.timeout,
            stdin: None,
        };
        let output = self
            .runner
            .run(&invocation)
            .map_err(ArtifactVerifierError::redacted)?;
        if !output.is_success() {
            return Err(ArtifactVerifierError::command_failure(format!(
                "SLSA verifier exited with status {} for `{}`: stdout: {}; stderr: {}",
                output.status_label(),
                command_subject.label(),
                redact_artifact_verifier_output(&output.stdout),
                redact_artifact_verifier_output(&output.stderr)
            )));
        }
        let attestation = parse_slsa_statement(
            &output.stdout,
            command_subject.expected_sha256(),
            builder_id,
            &required_predicates,
            command_subject.label(),
        )?;
        Ok(
            ArtifactVerificationEvidence::new(self.identity.clone()).with_attestation_from_source(
                builder_id,
                source_uri,
                attestation.predicate_type,
            ),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SlsaCommandSubject {
    Image {
        reference: String,
        expected_sha256: String,
    },
    Artifact {
        path: PathBuf,
        expected_sha256: String,
        provenance_path: PathBuf,
    },
}

impl SlsaCommandSubject {
    fn from_request(
        request: &ArtifactVerificationRequest,
        provenance_path: Option<&Path>,
    ) -> ArtifactVerifierResult<Self> {
        match request.subject() {
            ArtifactVerificationSubject::OciImage { reference } => {
                let parsed_reference = parse_oci_image_reference(reference).map_err(|error| {
                    ArtifactVerifierError::backend_error(format!(
                        "SLSA verifier requires a valid OCI image reference: {error}"
                    ))
                })?;
                if !has_sha256_digest(&parsed_reference) {
                    return Err(ArtifactVerifierError::backend_error(format!(
                        "SLSA verifier requires an immutable sha256 digest image reference, but `{reference}` is tag-only or missing a valid digest"
                    )));
                }
                let expected_sha256 =
                    normalize_sha256(parsed_reference.digest().ok_or_else(|| {
                        ArtifactVerifierError::backend_error(format!(
                            "SLSA verifier could not read digest from `{reference}` after OCI parsing"
                        ))
                    })?)?;
                Ok(Self::Image {
                    reference: parsed_reference.whole(),
                    expected_sha256,
                })
            }
            ArtifactVerificationSubject::File { path, sha256 } => {
                let sha256 = sha256.as_deref().ok_or_else(|| {
                    ArtifactVerifierError::backend_error(
                        "SLSA verifier requires file artifacts to carry an immutable sha256 subject",
                    )
                })?;
                Self::artifact(path.clone(), sha256, provenance_path)
            }
            ArtifactVerificationSubject::RuntimeBundle { path, sha256 }
            | ArtifactVerificationSubject::GuestExecutable { path, sha256 } => {
                Self::artifact(path.clone(), sha256, provenance_path)
            }
            ArtifactVerificationSubject::MachineImage { .. } => {
                Err(ArtifactVerifierError::unsupported_artifact(
                    "SLSA verifier does not support machine_image artifacts in this backend",
                ))
            }
        }
    }

    fn artifact(
        path: PathBuf,
        sha256: &str,
        provenance_path: Option<&Path>,
    ) -> ArtifactVerifierResult<Self> {
        let provenance_path = provenance_path.ok_or_else(|| {
            ArtifactVerifierError::backend_error(
                "SLSA verifier requires a provenance path for file artifacts",
            )
        })?;
        Ok(Self::Artifact {
            path,
            expected_sha256: normalize_sha256(sha256)?,
            provenance_path: provenance_path.to_path_buf(),
        })
    }

    fn args(&self, source_uri: &str, builder_id: &str) -> Vec<String> {
        match self {
            Self::Image { reference, .. } => vec![
                "verify-image".to_string(),
                reference.clone(),
                "--source-uri".to_string(),
                source_uri.to_string(),
                "--builder-id".to_string(),
                builder_id.to_string(),
                "--print-provenance".to_string(),
            ],
            Self::Artifact {
                path,
                provenance_path,
                ..
            } => vec![
                "verify-artifact".to_string(),
                path.display().to_string(),
                "--provenance-path".to_string(),
                provenance_path.display().to_string(),
                "--source-uri".to_string(),
                source_uri.to_string(),
                "--builder-id".to_string(),
                builder_id.to_string(),
                "--print-provenance".to_string(),
            ],
        }
    }

    fn expected_sha256(&self) -> &str {
        match self {
            Self::Image {
                expected_sha256, ..
            }
            | Self::Artifact {
                expected_sha256, ..
            } => expected_sha256,
        }
    }

    fn label(&self) -> String {
        match self {
            Self::Image { reference, .. } => reference.clone(),
            Self::Artifact { path, .. } => path.display().to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SlsaAttestation {
    predicate_type: String,
}

#[derive(Debug, Deserialize)]
struct SlsaStatement {
    #[serde(rename = "predicateType", alias = "predicate_type")]
    predicate_type: String,
    #[serde(default)]
    subject: Vec<SlsaSubject>,
    predicate: SlsaPredicate,
}

#[derive(Debug, Deserialize)]
struct SlsaSubject {
    #[serde(default)]
    digest: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct SlsaPredicate {
    builder: SlsaBuilder,
}

#[derive(Debug, Deserialize)]
struct SlsaBuilder {
    id: String,
}

fn parse_slsa_statement(
    stdout: &str,
    expected_sha256: &str,
    expected_builder_id: &str,
    required_predicates: &[String],
    subject_label: String,
) -> ArtifactVerifierResult<SlsaAttestation> {
    let statement: SlsaStatement = serde_json::from_str(stdout).map_err(|error| {
        ArtifactVerifierError::malformed_output(format!(
            "SLSA verifier emitted malformed provenance for `{subject_label}`: {error}; stdout: {}",
            redact_artifact_verifier_output(stdout)
        ))
    })?;
    if statement.predicate.builder.id != expected_builder_id {
        return Err(ArtifactVerifierError::malformed_output(format!(
            "SLSA verifier output for `{subject_label}` used builder `{}`, expected `{expected_builder_id}`",
            statement.predicate.builder.id
        )));
    }
    if !required_predicates
        .iter()
        .any(|predicate| predicate == &statement.predicate_type)
    {
        return Err(ArtifactVerifierError::malformed_output(format!(
            "SLSA verifier output for `{subject_label}` used predicate `{}`, expected one of {:?}",
            statement.predicate_type, required_predicates
        )));
    }
    if !statement.subject.iter().any(|subject| {
        subject
            .digest
            .get("sha256")
            .is_some_and(|sha256| sha256 == expected_sha256)
    }) {
        return Err(ArtifactVerifierError::malformed_output(format!(
            "SLSA verifier output for `{subject_label}` did not include expected sha256 subject `{expected_sha256}`"
        )));
    }
    Ok(SlsaAttestation {
        predicate_type: statement.predicate_type,
    })
}

fn required_predicate_types(predicate_types: &[String]) -> Vec<String> {
    if predicate_types.is_empty() {
        vec![SLSA_PROVENANCE_V1_PREDICATE_TYPE.to_string()]
    } else {
        predicate_types.to_vec()
    }
}

fn normalize_sha256(value: &str) -> ArtifactVerifierResult<String> {
    let digest = value.strip_prefix("sha256:").unwrap_or(value);
    if digest.len() == 64 && digest.as_bytes().iter().all(u8::is_ascii_hexdigit) {
        Ok(digest.to_ascii_lowercase())
    } else {
        Err(ArtifactVerifierError::backend_error(format!(
            "SLSA verifier requires sha256 digest subjects, got `{value}`"
        )))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::tenant::ArtifactVerifierCommandOutput;
    use crate::tenant::{ArtifactVerificationPolicy, ArtifactVerifierErrorKind};

    const DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const OTHER_DIGEST: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const IMAGE: &str = "registry.example.com/nimbus/api@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const SOURCE_URI: &str = "github.com/nimbus/nimbus";
    const BUILDER_ID: &str = "https://github.com/nimbus/builder";

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

    fn statement(builder_id: &str, predicate_type: &str, sha256: &str) -> String {
        format!(
            r#"{{
                "_type": "https://in-toto.io/Statement/v1",
                "predicateType": "{predicate_type}",
                "subject": [
                    {{
                        "name": "artifact",
                        "digest": {{
                            "sha256": "{sha256}"
                        }}
                    }}
                ],
                "predicate": {{
                    "builder": {{
                        "id": "{builder_id}"
                    }}
                }}
            }}"#
        )
    }

    fn policy() -> ArtifactVerificationPolicy {
        ArtifactVerificationPolicy::new().require_provenance_from_source(
            BUILDER_ID,
            SOURCE_URI,
            [SLSA_PROVENANCE_V1_PREDICATE_TYPE],
        )
    }

    fn backend(runner: Arc<StaticCommandRunner>) -> SlsaVerifierBackend {
        SlsaVerifierBackend::new()
            .with_program("slsa-verifier-fixture")
            .with_identity(ArtifactVerifierBackendIdentity::new(
                "slsa-verifier-fixture",
                "test",
            ))
            .with_runner(runner)
    }

    #[test]
    fn slsa_backend_accepts_digest_pinned_image_provenance() {
        let runner = Arc::new(StaticCommandRunner::success(statement(
            BUILDER_ID,
            SLSA_PROVENANCE_V1_PREDICATE_TYPE,
            DIGEST,
        )));
        let backend = backend(Arc::clone(&runner));
        let request = ArtifactVerificationRequest::oci_image(IMAGE, policy());

        let evidence = backend
            .verify_artifact(&request)
            .expect("matching SLSA image provenance should verify");

        assert_eq!(evidence.attestations().len(), 1);
        assert_eq!(evidence.attestations()[0].builder_id(), BUILDER_ID);
        assert_eq!(
            evidence.attestations()[0].predicate_type(),
            SLSA_PROVENANCE_V1_PREDICATE_TYPE
        );
        let invocations = runner.invocations();
        assert_eq!(invocations.len(), 1);
        assert_eq!(
            invocations[0].args(),
            [
                "verify-image",
                IMAGE,
                "--source-uri",
                SOURCE_URI,
                "--builder-id",
                BUILDER_ID,
                "--print-provenance"
            ]
        );
    }

    #[test]
    fn slsa_backend_accepts_file_artifact_with_provenance_path() {
        let runner = Arc::new(StaticCommandRunner::success(statement(
            BUILDER_ID,
            SLSA_PROVENANCE_V1_PREDICATE_TYPE,
            DIGEST,
        )));
        let backend = backend(Arc::clone(&runner)).with_provenance_path("/tmp/bundle.intoto.jsonl");
        let request = ArtifactVerificationRequest::new(
            ArtifactVerificationSubject::File {
                path: "/tmp/bundle.mjs".into(),
                sha256: Some(format!("sha256:{DIGEST}")),
            },
            policy(),
        );

        backend
            .verify_artifact(&request)
            .expect("matching SLSA file provenance should verify");

        assert_eq!(
            runner.invocations()[0].args(),
            [
                "verify-artifact",
                "/tmp/bundle.mjs",
                "--provenance-path",
                "/tmp/bundle.intoto.jsonl",
                "--source-uri",
                SOURCE_URI,
                "--builder-id",
                BUILDER_ID,
                "--print-provenance"
            ]
        );
    }

    #[test]
    fn slsa_backend_rejects_mutable_image_before_command() {
        let runner = Arc::new(StaticCommandRunner::success(statement(
            BUILDER_ID,
            SLSA_PROVENANCE_V1_PREDICATE_TYPE,
            DIGEST,
        )));
        let backend = backend(Arc::clone(&runner));
        let request = ArtifactVerificationRequest::oci_image(
            "registry.example.com/nimbus/api:latest",
            policy(),
        );

        let error = backend
            .verify_artifact(&request)
            .expect_err("tag-only image should fail before slsa-verifier");

        assert_eq!(error.kind, ArtifactVerifierErrorKind::BackendError);
        assert!(error.to_string().contains("immutable sha256 digest"));
        assert!(runner.invocations().is_empty());
    }

    #[test]
    fn slsa_backend_rejects_wrong_builder_id() {
        let runner = Arc::new(StaticCommandRunner::success(statement(
            "https://github.com/other/builder",
            SLSA_PROVENANCE_V1_PREDICATE_TYPE,
            DIGEST,
        )));
        let backend = backend(runner);
        let request = ArtifactVerificationRequest::oci_image(IMAGE, policy());

        let error = backend
            .verify_artifact(&request)
            .expect_err("wrong builder ID should fail closed");

        assert_eq!(error.kind, ArtifactVerifierErrorKind::MalformedOutput);
        assert!(error.to_string().contains("expected"));
    }

    #[test]
    fn slsa_backend_rejects_wrong_predicate_type() {
        let runner = Arc::new(StaticCommandRunner::success(statement(
            BUILDER_ID,
            "https://example.com/not-slsa",
            DIGEST,
        )));
        let backend = backend(runner);
        let request = ArtifactVerificationRequest::oci_image(IMAGE, policy());

        let error = backend
            .verify_artifact(&request)
            .expect_err("wrong predicate type should fail closed");

        assert_eq!(error.kind, ArtifactVerifierErrorKind::MalformedOutput);
        assert!(error.to_string().contains("expected one of"));
    }

    #[test]
    fn slsa_backend_rejects_wrong_subject_digest() {
        let runner = Arc::new(StaticCommandRunner::success(statement(
            BUILDER_ID,
            SLSA_PROVENANCE_V1_PREDICATE_TYPE,
            OTHER_DIGEST,
        )));
        let backend = backend(runner);
        let request = ArtifactVerificationRequest::oci_image(IMAGE, policy());

        let error = backend
            .verify_artifact(&request)
            .expect_err("wrong subject digest should fail closed");

        assert_eq!(error.kind, ArtifactVerifierErrorKind::MalformedOutput);
        assert!(
            error.to_string().contains(DIGEST),
            "wrong subject digest should name the expected immutable subject: {error}"
        );
    }

    #[test]
    fn slsa_backend_rejects_malformed_output() {
        let runner = Arc::new(StaticCommandRunner::success("not-json"));
        let backend = backend(runner);
        let request = ArtifactVerificationRequest::oci_image(IMAGE, policy());

        let error = backend
            .verify_artifact(&request)
            .expect_err("malformed SLSA verifier output should fail closed");

        assert_eq!(error.kind, ArtifactVerifierErrorKind::MalformedOutput);
        assert!(error.to_string().contains("malformed provenance"));
    }

    #[test]
    fn slsa_backend_timeout_fails_closed_and_redacts_runner_error() {
        let runner = Arc::new(StaticCommandRunner::error(ArtifactVerifierError::timeout(
            "deadline exceeded with token=do-not-log-slsa-token",
        )));
        let backend = backend(runner);
        let request = ArtifactVerificationRequest::oci_image(IMAGE, policy());

        let error = backend
            .verify_artifact(&request)
            .expect_err("timeout should fail closed");

        assert_eq!(error.kind, ArtifactVerifierErrorKind::Timeout);
        let rendered = error.to_string();
        assert!(rendered.contains("[redacted verifier output]"));
        assert!(!rendered.contains("do-not-log-slsa-token"));
    }

    #[test]
    fn slsa_backend_missing_tool_fails_closed() {
        let backend =
            SlsaVerifierBackend::new().with_program("nimbus-slsa-verifier-definitely-missing");
        let request = ArtifactVerificationRequest::oci_image(IMAGE, policy());

        let error = backend
            .verify_artifact(&request)
            .expect_err("missing slsa-verifier binary should fail closed");

        assert_eq!(error.kind, ArtifactVerifierErrorKind::Unavailable);
        assert!(
            error
                .to_string()
                .contains("failed to start artifact verifier")
        );
    }

    #[test]
    fn slsa_backend_rejects_file_artifact_without_immutable_subject_or_provenance_path() {
        let runner = Arc::new(StaticCommandRunner::success(statement(
            BUILDER_ID,
            SLSA_PROVENANCE_V1_PREDICATE_TYPE,
            DIGEST,
        )));
        let backend = backend(runner);
        let missing_digest = ArtifactVerificationRequest::new(
            ArtifactVerificationSubject::File {
                path: "/tmp/bundle.mjs".into(),
                sha256: None,
            },
            policy(),
        );
        let missing_path = ArtifactVerificationRequest::new(
            ArtifactVerificationSubject::File {
                path: "/tmp/bundle.mjs".into(),
                sha256: Some(format!("sha256:{DIGEST}")),
            },
            policy(),
        );

        let missing_digest_error = backend
            .verify_artifact(&missing_digest)
            .expect_err("file artifact without sha256 should fail closed");
        let missing_path_error = backend
            .verify_artifact(&missing_path)
            .expect_err("file artifact without provenance path should fail closed");

        assert_eq!(
            missing_digest_error.kind,
            ArtifactVerifierErrorKind::BackendError
        );
        assert_eq!(
            missing_path_error.kind,
            ArtifactVerifierErrorKind::BackendError
        );
    }
}
