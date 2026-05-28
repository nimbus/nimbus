use std::sync::Arc;

use nimbus_artifacts::{ArtifactVerificationPolicy, ArtifactVerifierBackend};

#[derive(Clone)]
pub struct RuntimeBundleProvenanceConfig {
    policy: ArtifactVerificationPolicy,
    verifier: Arc<dyn ArtifactVerifierBackend>,
    context: String,
}

impl RuntimeBundleProvenanceConfig {
    pub fn new(
        policy: ArtifactVerificationPolicy,
        verifier: Arc<dyn ArtifactVerifierBackend>,
        context: impl Into<String>,
    ) -> Self {
        Self {
            policy,
            verifier,
            context: context.into(),
        }
    }

    pub fn policy(&self) -> &ArtifactVerificationPolicy {
        &self.policy
    }

    pub fn verifier(&self) -> &dyn ArtifactVerifierBackend {
        self.verifier.as_ref()
    }

    pub fn context(&self) -> &str {
        &self.context
    }
}

impl std::fmt::Debug for RuntimeBundleProvenanceConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RuntimeBundleProvenanceConfig")
            .field("policy", &self.policy)
            .field("context", &self.context)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nimbus_artifacts::{
        ArtifactVerificationEvidence, ArtifactVerificationRequest, ArtifactVerifierBackendIdentity,
        ArtifactVerifierResult, SLSA_PROVENANCE_V1_PREDICATE_TYPE,
    };

    #[derive(Debug)]
    struct StaticVerifier;

    impl ArtifactVerifierBackend for StaticVerifier {
        fn verify_artifact(
            &self,
            _request: &ArtifactVerificationRequest,
        ) -> ArtifactVerifierResult<ArtifactVerificationEvidence> {
            Ok(ArtifactVerificationEvidence::new(
                ArtifactVerifierBackendIdentity::new("fixture", "test"),
            ))
        }
    }

    #[test]
    fn runtime_bundle_provenance_config_exposes_policy_and_context_without_debugging_verifier() {
        let policy = ArtifactVerificationPolicy::new().require_provenance_from_source(
            "https://github.com/nimbus/builder",
            "github.com/nimbus/nimbus",
            [SLSA_PROVENANCE_V1_PREDICATE_TYPE],
        );
        let config =
            RuntimeBundleProvenanceConfig::new(policy, Arc::new(StaticVerifier), "runtime lane");

        assert_eq!(config.context(), "runtime lane");
        assert!(config.policy().requires_verification());
        let debug = format!("{config:?}");
        assert!(debug.contains("RuntimeBundleProvenanceConfig"));
        assert!(debug.contains("runtime lane"));
        assert!(!debug.contains("StaticVerifier"));
    }
}
