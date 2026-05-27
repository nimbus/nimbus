use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use nimbus_core::TenantId;
use nimbus_runtime::{
    HostBridge, HostCallCancellation, InvocationRequest, NimbusRuntime, NimbusRuntimeError,
    RuntimeBundle, RuntimeExecutor, RuntimeInvocationContext, RuntimePolicy,
};

use crate::tenant::{
    ArtifactAdmission, ArtifactVerificationPolicy, ArtifactVerifierBackend,
    admit_runtime_bundle_artifact,
};

mod blocking;
mod worker;

#[derive(Clone, Copy)]
pub(crate) enum RuntimeConcurrencyMode {
    EnforcePolicyLimit,
    BudgetedNestedInvocationBypass,
}

#[derive(Clone, Copy)]
pub(crate) enum RuntimeInvocationScope {
    TopLevel,
    Nested,
}

pub(crate) struct RuntimeBundleInvocationOptions<'a> {
    pub(crate) tenant_id: &'a TenantId,
    pub(crate) server_request_id: Option<&'a str>,
    pub(crate) cancellation: Option<HostCallCancellation>,
    pub(crate) concurrency_mode: RuntimeConcurrencyMode,
    pub(crate) scope: RuntimeInvocationScope,
    provenance_gate: Option<&'a RuntimeBundleProvenanceConfig>,
}

#[derive(Clone)]
pub(crate) struct RuntimeBundleProvenanceConfig {
    policy: ArtifactVerificationPolicy,
    verifier: Arc<dyn ArtifactVerifierBackend>,
    context: String,
}

impl RuntimeBundleProvenanceConfig {
    pub(crate) fn new(
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

    fn policy(&self) -> &ArtifactVerificationPolicy {
        &self.policy
    }

    fn verifier(&self) -> &dyn ArtifactVerifierBackend {
        self.verifier.as_ref()
    }

    fn context(&self) -> &str {
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

impl<'a> RuntimeBundleInvocationOptions<'a> {
    pub(crate) fn enforcing_policy_limit(
        tenant_id: &'a TenantId,
        server_request_id: Option<&'a str>,
        cancellation: Option<HostCallCancellation>,
    ) -> Self {
        Self {
            tenant_id,
            server_request_id,
            cancellation,
            concurrency_mode: RuntimeConcurrencyMode::EnforcePolicyLimit,
            scope: RuntimeInvocationScope::TopLevel,
            provenance_gate: None,
        }
        .with_runtime_bundle_provenance_gate(None)
    }

    pub(crate) fn budgeted_nested_invocation_bypass(
        tenant_id: &'a TenantId,
        server_request_id: Option<&'a str>,
        cancellation: Option<HostCallCancellation>,
    ) -> Self {
        Self {
            tenant_id,
            server_request_id,
            cancellation,
            concurrency_mode: RuntimeConcurrencyMode::BudgetedNestedInvocationBypass,
            scope: RuntimeInvocationScope::Nested,
            provenance_gate: None,
        }
        .with_runtime_bundle_provenance_gate(None)
    }

    pub(crate) fn with_runtime_bundle_provenance_gate(
        mut self,
        gate: Option<&'a RuntimeBundleProvenanceConfig>,
    ) -> Self {
        self.provenance_gate = gate;
        self
    }

    pub(crate) fn admit_runtime_bundle_artifact(
        &self,
        bundle: &RuntimeBundle,
    ) -> std::result::Result<Option<ArtifactAdmission>, NimbusRuntimeError> {
        let Some(gate) = self.provenance_gate else {
            return Ok(None);
        };
        admit_runtime_bundle_artifact(bundle, gate.policy(), gate.verifier(), gate.context())
            .map(Some)
            .map_err(|error| {
                NimbusRuntimeError::Contract(format!(
                    "runtime bundle provenance admission failed before invocation: {error}"
                ))
            })
    }
}

pub(crate) fn next_runtime_server_request_id(prefix: &str) -> String {
    static NEXT_RUNTIME_SERVER_REQUEST_ID: AtomicU64 = AtomicU64::new(1);
    format!(
        "{prefix}-{}",
        NEXT_RUNTIME_SERVER_REQUEST_ID.fetch_add(1, Ordering::Relaxed)
    )
}

pub(crate) fn runtime_invocation_context(
    request: &InvocationRequest,
    tenant_id: &TenantId,
    server_request_id: Option<&str>,
    concurrency_mode: RuntimeConcurrencyMode,
    scope: RuntimeInvocationScope,
) -> RuntimeInvocationContext {
    let tenant_label = tenant_id.to_string();
    let context = match (scope, server_request_id) {
        (RuntimeInvocationScope::TopLevel, Some(server_request_id)) => {
            RuntimeInvocationContext::top_level_for_tenant_and_request(
                request,
                tenant_label,
                server_request_id,
            )
        }
        (RuntimeInvocationScope::TopLevel, None) => {
            RuntimeInvocationContext::top_level_for_tenant(request, tenant_label)
        }
        (RuntimeInvocationScope::Nested, Some(server_request_id)) => {
            RuntimeInvocationContext::nested_for_tenant_and_request(
                request,
                tenant_label,
                server_request_id,
            )
        }
        (RuntimeInvocationScope::Nested, None) => {
            RuntimeInvocationContext::nested_for_tenant(request, tenant_label)
        }
    };
    match concurrency_mode {
        RuntimeConcurrencyMode::EnforcePolicyLimit => context,
        RuntimeConcurrencyMode::BudgetedNestedInvocationBypass => {
            context.with_bypassed_concurrency_limit()
        }
    }
}

fn runtime_for_host(
    host_bridge: Arc<dyn HostBridge>,
    runtime_policy: Arc<RuntimePolicy>,
) -> NimbusRuntime {
    NimbusRuntime::with_policy(host_bridge, runtime_policy)
}

pub(crate) use blocking::invoke_runtime_bundle_blocking_with_host;
#[cfg(test)]
pub(crate) use blocking::invoke_runtime_bundle_blocking_with_host_state;
pub(crate) use worker::invoke_runtime_bundle_on_worker_with_host;
pub(crate) use worker::invoke_runtime_bundle_on_worker_with_host_state;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tenant::{
        ArtifactVerificationEvidence, ArtifactVerifierBackendIdentity,
        SLSA_PROVENANCE_V1_PREDICATE_TYPE,
    };

    const BUILDER_ID: &str = "https://github.com/nimbus/builder";

    #[derive(Debug)]
    struct StaticArtifactVerifier;

    impl ArtifactVerifierBackend for StaticArtifactVerifier {
        fn verify_artifact(
            &self,
            _request: &crate::tenant::ArtifactVerificationRequest,
        ) -> crate::tenant::ArtifactVerifierResult<ArtifactVerificationEvidence> {
            Ok(
                ArtifactVerificationEvidence::new(ArtifactVerifierBackendIdentity::new(
                    "fixture", "test",
                ))
                .with_attestation_from_source(
                    BUILDER_ID,
                    "github.com/nimbus/nimbus",
                    SLSA_PROVENANCE_V1_PREDICATE_TYPE,
                ),
            )
        }
    }

    fn write_bundle() -> (tempfile::TempDir, std::path::PathBuf, String) {
        let temp = tempfile::tempdir().expect("tempdir should create");
        let path = temp.path().join("bundle.mjs");
        std::fs::write(&path, "export default 1;\n").expect("bundle should write");
        let sha256 = RuntimeBundle::compute_sha256_for_path(&path).expect("bundle should hash");
        (temp, path, sha256)
    }

    #[test]
    fn runtime_invocation_options_admit_bundle_provenance_before_executor_entry() {
        let (_temp, path, sha256) = write_bundle();
        let bundle = RuntimeBundle::with_expected_sha256(&path, &sha256)
            .expect("bundle identity should accept sha256");
        let tenant_id = TenantId::new("tenant-a").expect("tenant id should parse");
        let policy = ArtifactVerificationPolicy::new().require_provenance_from_source(
            BUILDER_ID,
            "github.com/nimbus/nimbus",
            [SLSA_PROVENANCE_V1_PREDICATE_TYPE],
        );
        let gate = RuntimeBundleProvenanceConfig::new(
            policy,
            Arc::new(StaticArtifactVerifier),
            "runtime invocation",
        );
        let options = RuntimeBundleInvocationOptions::enforcing_policy_limit(
            &tenant_id,
            Some("runtime-request-1"),
            None,
        )
        .with_runtime_bundle_provenance_gate(Some(&gate));

        let admission = options
            .admit_runtime_bundle_artifact(&bundle)
            .expect("matching bundle provenance should admit")
            .expect("provenance gate should produce admission evidence");

        assert_eq!(admission.verification().attestations().len(), 1);
    }
}
