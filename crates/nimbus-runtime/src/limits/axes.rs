use serde::{Deserialize, Serialize};

use super::grants::RuntimeLanguage;
use super::resources::RuntimeLimits;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeBackendKind {
    #[default]
    #[serde(rename = "v8")]
    V8,
    BunJsc,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeBackendTrustTier {
    ProofOnly,
    InProcessTrustedOnly,
    #[default]
    InProcessUntrusted,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeBackendLockdownProfile {
    #[default]
    V8DenoCore,
    BunJscProofOnly,
    BunJscTrustedGeneratedWrapper,
    BunJscInProcessUntrusted,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeBackendLifecyclePolicy {
    #[default]
    V8DenoCorePool,
    /// Future Bun/JSC pool shape for trusted generated-wrapper proof lanes.
    /// This is not the V8 warm pool and must stay non-selectable until there
    /// is a real Bun backend implementation behind it.
    BunJscTrustedRetainedPool,
    /// Future Bun/JSC pool shape for untrusted tenants: the pool may own
    /// concurrency, quota, cancellation, and teardown, but individual VMs are
    /// fresh or discarded unless Bun/JSC proves a hard in-process boundary.
    BunJscFreshDiscardPoolOuterQuotaRequired,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeBundleContentKind {
    #[serde(rename = "javascript")]
    #[default]
    JavaScript,
    WasmComponent,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeJavaScriptEvaluationFormat {
    #[default]
    EsModule,
    ProgramWrapper,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeCompatibilityTarget {
    WebStandardIsolate,
    BunJsc,
    Node20,
    Node22,
    Node24,
}

impl RuntimeCompatibilityTarget {
    pub fn is_node(self) -> bool {
        matches!(self, Self::Node20 | Self::Node22 | Self::Node24)
    }

    pub fn node_major_version(self) -> Option<u16> {
        match self {
            Self::Node20 => Some(20),
            Self::Node22 => Some(22),
            Self::Node24 => Some(24),
            Self::WebStandardIsolate | Self::BunJsc => None,
        }
    }

    pub fn node_runtime_version(self) -> Option<&'static str> {
        match self {
            Self::Node20 => Some("v20.0.0-nimbus"),
            Self::Node22 => Some("v22.0.0-nimbus"),
            Self::Node24 => Some("v24.0.0-nimbus"),
            Self::WebStandardIsolate | Self::BunJsc => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeExecutionModel {
    RunToCompletion,
    CooperativeLocker,
    /// Backend-owned event-loop and API-lock lifecycle. This is not the V8
    /// cooperative locker model; it is for engines such as Bun/JSC where the
    /// backend pool owns guest-entry acknowledgement, event-loop progress,
    /// cancellation, and teardown.
    BackendOwnedEventLoop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeRoutingAffinity {
    None,
    Tenant,
    Function,
    Script,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimePoolKind {
    /// V8/Deno: reuse the worker-local bootstrap snapshot, then build a fresh
    /// JsRuntime for every invocation.
    ///
    /// This preserves the freshest execution boundary and is currently the
    /// default low-latency mode.
    StartupSnapshotCache,
    /// V8/Deno: retain whole JsRuntime instances with evaluated modules alive
    /// across invocations. No realm reset, no module reload — only surgical
    /// per-request state cleanup via `reset_request_state()`.
    ///
    /// Requires `CooperativeLocker` execution model. Fails fast with
    /// `RunToCompletion`.
    WarmPool,
    /// Bun/JSC: future trusted generated-wrapper pool shape. Retains VMs only
    /// for host-authored generated wrappers, never for untrusted tenants.
    ///
    /// This is typed diagnostic/admission metadata only until BEP4+ land a real
    /// Bun pool behind the backend seam.
    BunJscTrustedRetained,
    /// Bun/JSC: future untrusted pool shape. The pool may coordinate quota and
    /// lifecycle, but each VM is fresh or discarded unless a hard in-process
    /// Bun/JSC isolation boundary is proven.
    ///
    /// This is typed diagnostic/admission metadata only until BEP4+ land a real
    /// Bun pool behind the backend seam.
    BunJscFreshDiscard,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeMemoryEnforcement {
    #[default]
    V8IsolateHeapLimit,
    OuterQuotaRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeModuleStateSemantics {
    FreshPerInvocation,
    /// Modules persist across invocations by contract. Module-level side
    /// effects (e.g. `let counter = 0`) accumulate across requests.
    WarmPerBundle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RuntimeResetCapabilities {
    pub op_state_per_invocation: bool,
    pub bootstrap_state_per_invocation: bool,
    pub user_module_state_per_invocation: bool,
}

pub(super) fn validate_backend_policy_axes(limits: &RuntimeLimits) {
    match limits.backend_kind {
        RuntimeBackendKind::V8 => {
            if !matches!(
                limits.backend_trust_tier,
                RuntimeBackendTrustTier::InProcessUntrusted
            ) {
                panic!(
                    "V8 runtime backend requires in-process-untrusted trust tier, got {:?}",
                    limits.backend_trust_tier
                );
            }
            if !matches!(
                limits.backend_lockdown_profile,
                RuntimeBackendLockdownProfile::V8DenoCore
            ) {
                panic!(
                    "V8 runtime backend requires V8/Deno lockdown profile, got {:?}",
                    limits.backend_lockdown_profile
                );
            }
            if !matches!(
                limits.backend_lifecycle_policy,
                RuntimeBackendLifecyclePolicy::V8DenoCorePool
            ) {
                panic!(
                    "V8 runtime backend requires V8/Deno lifecycle policy, got {:?}",
                    limits.backend_lifecycle_policy
                );
            }
            if !matches!(
                limits.runtime_pool_kind,
                RuntimePoolKind::StartupSnapshotCache | RuntimePoolKind::WarmPool
            ) {
                panic!(
                    "V8 runtime backend requires a V8/Deno pool kind, got {:?}",
                    limits.runtime_pool_kind
                );
            }
            if matches!(
                limits.compatibility_target,
                RuntimeCompatibilityTarget::BunJsc
            ) {
                panic!(
                    "V8 runtime backend cannot use Bun/JSC compatibility target {:?}",
                    limits.compatibility_target
                );
            }
            if !matches!(
                limits.execution_model,
                RuntimeExecutionModel::RunToCompletion | RuntimeExecutionModel::CooperativeLocker
            ) {
                panic!(
                    "V8 runtime backend requires a V8/Deno execution model, got {:?}",
                    limits.execution_model
                );
            }
            if !matches!(limits.language, RuntimeLanguage::JavaScript) {
                panic!(
                    "V8 runtime backend requires JavaScript runtime language, got {:?}",
                    limits.language
                );
            }
            if !matches!(
                limits.bundle_content_kind,
                RuntimeBundleContentKind::JavaScript
            ) {
                panic!(
                    "V8 runtime backend requires JavaScript bundle content, got {:?}",
                    limits.bundle_content_kind
                );
            }
            if !matches!(
                limits.javascript_evaluation_format,
                RuntimeJavaScriptEvaluationFormat::EsModule
            ) {
                panic!(
                    "V8 runtime backend requires ES module evaluation format, got {:?}",
                    limits.javascript_evaluation_format
                );
            }
            if !matches!(
                limits.memory_enforcement,
                RuntimeMemoryEnforcement::V8IsolateHeapLimit
            ) {
                panic!(
                    "V8 runtime backend requires V8 isolate heap-limit enforcement, got {:?}",
                    limits.memory_enforcement
                );
            }
        }
        RuntimeBackendKind::BunJsc => {
            if !matches!(
                limits.bundle_content_kind,
                RuntimeBundleContentKind::JavaScript
            ) {
                panic!(
                    "Bun/JSC runtime backend requires JavaScript bundle content, got {:?}",
                    limits.bundle_content_kind
                );
            }
            if !matches!(
                limits.javascript_evaluation_format,
                RuntimeJavaScriptEvaluationFormat::ProgramWrapper
            ) {
                panic!(
                    "Bun/JSC runtime backend requires program-wrapper evaluation format, got {:?}",
                    limits.javascript_evaluation_format
                );
            }
            if !matches!(
                limits.compatibility_target,
                RuntimeCompatibilityTarget::BunJsc
            ) {
                panic!(
                    "Bun/JSC runtime backend requires Bun/JSC compatibility target, got {:?}",
                    limits.compatibility_target
                );
            }
            if !matches!(
                limits.execution_model,
                RuntimeExecutionModel::BackendOwnedEventLoop
            ) {
                panic!(
                    "Bun/JSC runtime backend requires backend-owned event-loop execution model, got {:?}",
                    limits.execution_model
                );
            }
            if !matches!(
                limits.memory_enforcement,
                RuntimeMemoryEnforcement::OuterQuotaRequired
            ) {
                panic!(
                    "Bun/JSC runtime backend requires outer quota memory enforcement, got {:?}",
                    limits.memory_enforcement
                );
            }
            match (
                limits.backend_trust_tier,
                limits.backend_lockdown_profile,
                limits.backend_lifecycle_policy,
                limits.runtime_pool_kind,
            ) {
                (
                    RuntimeBackendTrustTier::ProofOnly,
                    RuntimeBackendLockdownProfile::BunJscProofOnly,
                    RuntimeBackendLifecyclePolicy::BunJscTrustedRetainedPool,
                    RuntimePoolKind::BunJscTrustedRetained,
                ) => {
                    panic!("Bun/JSC proof-only runtime backend is not selectable")
                }
                (
                    RuntimeBackendTrustTier::InProcessTrustedOnly,
                    RuntimeBackendLockdownProfile::BunJscTrustedGeneratedWrapper,
                    RuntimeBackendLifecyclePolicy::BunJscTrustedRetainedPool,
                    RuntimePoolKind::BunJscTrustedRetained,
                ) => {
                    panic!(
                        "Bun/JSC trusted generated-wrapper profile is not a product runtime route"
                    )
                }
                (
                    RuntimeBackendTrustTier::InProcessUntrusted,
                    RuntimeBackendLockdownProfile::BunJscInProcessUntrusted,
                    RuntimeBackendLifecyclePolicy::BunJscFreshDiscardPoolOuterQuotaRequired,
                    RuntimePoolKind::BunJscFreshDiscard,
                ) => {}
                (trust_tier, lockdown_profile, lifecycle_policy, runtime_pool_kind) => {
                    panic!(
                        "Bun/JSC runtime backend requires matching lockdown, lifecycle, and pool profiles for {:?}, got {:?}, {:?}, and {:?}",
                        trust_tier, lockdown_profile, lifecycle_policy, runtime_pool_kind
                    )
                }
            }
        }
    }
}
