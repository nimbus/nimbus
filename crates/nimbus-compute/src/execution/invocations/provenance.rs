use nimbus_artifacts::ArtifactAdmission;
use nimbus_provenance::RuntimeBundleProvenanceConfig;
use nimbus_runtime::{NimbusRuntimeError, RuntimeBundle};

use super::{RuntimeBundleInvocationOptions, RuntimeBundleProvenanceGate};
use crate::artifact_verifier_effects::admit_runtime_bundle_artifact;

impl<'a> RuntimeBundleInvocationOptions<'a> {
    pub(crate) fn with_runtime_bundle_provenance_gate(
        mut self,
        gate: &'a RuntimeBundleProvenanceConfig,
    ) -> Self {
        self.provenance_gate = RuntimeBundleProvenanceGate::Configured(gate);
        self
    }

    pub(crate) fn without_runtime_bundle_provenance_gate(mut self) -> Self {
        self.provenance_gate = RuntimeBundleProvenanceGate::Disabled;
        self
    }

    pub fn with_optional_runtime_bundle_provenance_gate(
        self,
        gate: Option<&'a RuntimeBundleProvenanceConfig>,
    ) -> Self {
        match gate {
            Some(gate) => self.with_runtime_bundle_provenance_gate(gate),
            None => self.without_runtime_bundle_provenance_gate(),
        }
    }

    pub(crate) fn admit_runtime_bundle_artifact(
        &self,
        bundle: &RuntimeBundle,
    ) -> std::result::Result<Option<ArtifactAdmission>, NimbusRuntimeError> {
        match self.provenance_gate {
            RuntimeBundleProvenanceGate::Disabled => Ok(None),
            RuntimeBundleProvenanceGate::Configured(gate) => admit_runtime_bundle_artifact(
                bundle,
                gate.policy(),
                gate.verifier(),
                gate.context(),
            )
            .map(Some)
            .map_err(|error| {
                NimbusRuntimeError::Contract(format!(
                    "runtime bundle provenance admission failed before invocation: {error}"
                ))
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nimbus_artifacts::{
        ArtifactVerificationEvidence, ArtifactVerificationPolicy, ArtifactVerificationRequest,
        ArtifactVerifierBackend, ArtifactVerifierBackendIdentity, ArtifactVerifierResult,
        SLSA_PROVENANCE_V1_PREDICATE_TYPE,
    };
    use nimbus_core::TenantId;

    use super::*;

    const BUILDER_ID: &str = "https://github.com/nimbus/builder";
    const SOURCE_URI: &str = "github.com/nimbus/nimbus";

    #[derive(Debug, Clone)]
    struct StaticArtifactVerifier {
        evidence: ArtifactVerificationEvidence,
    }

    impl StaticArtifactVerifier {
        fn with_attestation(builder_id: &str) -> Self {
            Self {
                evidence: ArtifactVerificationEvidence::new(ArtifactVerifierBackendIdentity::new(
                    "fixture", "test",
                ))
                .with_attestation_from_source(
                    builder_id,
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
            panic!("runtime bundle digest failures must not invoke provenance verifier")
        }
    }

    fn policy() -> ArtifactVerificationPolicy {
        ArtifactVerificationPolicy::new().require_provenance_from_source(
            BUILDER_ID,
            SOURCE_URI,
            [SLSA_PROVENANCE_V1_PREDICATE_TYPE],
        )
    }

    fn write_bundle() -> (tempfile::TempDir, std::path::PathBuf, String) {
        use std::io::Write as _;

        let temp = tempfile::tempdir().expect("tempdir should create");
        let mut bundle = tempfile::NamedTempFile::with_prefix_in("bundle", temp.path())
            .expect("bundle file should create");
        bundle
            .write_all(b"export default 1;\n")
            .expect("bundle should write");
        let (_file, path) = bundle.keep().expect("bundle file should persist");
        let sha256 = RuntimeBundle::compute_sha256_for_path(&path).expect("bundle should hash");
        (temp, path, sha256)
    }

    fn options<'a>(
        tenant_id: &'a TenantId,
        gate: &'a RuntimeBundleProvenanceConfig,
    ) -> RuntimeBundleInvocationOptions<'a> {
        RuntimeBundleInvocationOptions::enforcing_policy_limit(
            tenant_id,
            Some("runtime-request-1"),
            None,
        )
        .with_runtime_bundle_provenance_gate(gate)
    }

    #[test]
    fn runtime_bundle_provenance_gate_disabled_state_skips_admission_explicitly() {
        let (_temp, path, sha256) = write_bundle();
        let bundle = RuntimeBundle::with_expected_sha256(&path, &sha256)
            .expect("bundle identity should accept sha256");
        let tenant_id = TenantId::new("tenant-a").expect("tenant id should parse");
        let options =
            RuntimeBundleInvocationOptions::enforcing_policy_limit(&tenant_id, None, None)
                .without_runtime_bundle_provenance_gate();

        let admission = options
            .admit_runtime_bundle_artifact(&bundle)
            .expect("explicitly disabled provenance gate should skip admission");

        assert!(admission.is_none());
    }

    #[test]
    fn runtime_bundle_provenance_gate_admits_matching_bundle_before_executor_entry() {
        let (_temp, path, sha256) = write_bundle();
        let bundle = RuntimeBundle::with_expected_sha256(&path, &sha256)
            .expect("bundle identity should accept sha256");
        let tenant_id = TenantId::new("tenant-a").expect("tenant id should parse");
        let gate = RuntimeBundleProvenanceConfig::new(
            policy(),
            Arc::new(StaticArtifactVerifier::with_attestation(BUILDER_ID)),
            "runtime invocation",
        );

        let admission = options(&tenant_id, &gate)
            .admit_runtime_bundle_artifact(&bundle)
            .expect("matching bundle provenance should admit")
            .expect("provenance gate should produce admission evidence");

        assert_eq!(admission.verification().attestations().len(), 1);
    }

    #[test]
    fn runtime_bundle_provenance_gate_rejects_missing_digest_before_verifier() {
        let (_temp, path, _sha256) = write_bundle();
        let bundle = RuntimeBundle::new(&path);
        let tenant_id = TenantId::new("tenant-a").expect("tenant id should parse");
        let gate = RuntimeBundleProvenanceConfig::new(
            policy(),
            Arc::new(PanicArtifactVerifier),
            "runtime invocation",
        );

        let error = options(&tenant_id, &gate)
            .admit_runtime_bundle_artifact(&bundle)
            .expect_err("missing bundle digest should fail closed");

        assert!(matches!(error, NimbusRuntimeError::Contract(_)));
        assert!(
            error.to_string().contains("no immutable sha256"),
            "missing digest error should be actionable: {error}"
        );
    }

    #[test]
    fn runtime_bundle_provenance_gate_rejects_checksum_mismatch_before_verifier() {
        let (_temp, path, _sha256) = write_bundle();
        let wrong_sha256 = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let bundle = RuntimeBundle::with_expected_sha256(&path, wrong_sha256)
            .expect("syntactically valid wrong sha should build");
        let tenant_id = TenantId::new("tenant-a").expect("tenant id should parse");
        let gate = RuntimeBundleProvenanceConfig::new(
            policy(),
            Arc::new(PanicArtifactVerifier),
            "runtime invocation",
        );

        let error = options(&tenant_id, &gate)
            .admit_runtime_bundle_artifact(&bundle)
            .expect_err("wrong bundle digest should fail closed");

        assert!(matches!(error, NimbusRuntimeError::Contract(_)));
        assert!(
            error
                .to_string()
                .contains("failed immutable sha256 admission"),
            "checksum mismatch should be explicit: {error}"
        );
    }

    #[test]
    fn runtime_bundle_provenance_gate_rejects_wrong_attestation_evidence() {
        let (_temp, path, sha256) = write_bundle();
        let bundle = RuntimeBundle::with_expected_sha256(&path, &sha256)
            .expect("bundle identity should accept sha256");
        let tenant_id = TenantId::new("tenant-a").expect("tenant id should parse");
        let gate = RuntimeBundleProvenanceConfig::new(
            policy(),
            Arc::new(StaticArtifactVerifier::with_attestation(
                "https://github.com/other/builder",
            )),
            "runtime invocation",
        );

        let error = options(&tenant_id, &gate)
            .admit_runtime_bundle_artifact(&bundle)
            .expect_err("wrong provenance evidence should fail closed");

        assert!(matches!(error, NimbusRuntimeError::Contract(_)));
        assert!(
            error
                .to_string()
                .contains("requires provenance from builder"),
            "wrong-builder attestation evidence should be explicit: {error}"
        );
    }
}
