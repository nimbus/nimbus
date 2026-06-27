use std::collections::BTreeSet;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use serde::{Deserializer, de};

use super::grants::RuntimeLanguage;
use super::resources::RuntimeLimits;

const NODE_LTS_LANES_JSON: &str =
    include_str!("../../../../tests/runtime/node/compat/node-lts-compat/node-lts-lanes.json");

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeBackendKind {
    #[default]
    #[serde(rename = "v8")]
    V8,
    BunJsc,
    Wasmtime,
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
    WasmtimeComponentModel,
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
    WasmtimePrecompiledModuleCache,
    WasmtimeRetainedStorePool,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeCompatibilityTarget {
    WebStandardIsolate,
    BunJsc,
    WasmComponent,
    Node20,
    Node22,
    Node24,
    Node26,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeNodeSupportPhase {
    EolLegacy,
    MaintenanceLts,
    ActiveLts,
    CurrentNonLts,
}

impl RuntimeNodeSupportPhase {
    pub fn is_supported_lts(self) -> bool {
        matches!(self, Self::MaintenanceLts | Self::ActiveLts)
    }
}

#[derive(Debug, Deserialize)]
struct RuntimeNodeLtsRegistry {
    schema_version: u32,
    registry_kind: String,
    product_default_lane: String,
    lanes: Vec<RuntimeNodeLtsLane>,
}

#[derive(Debug, Deserialize)]
pub struct RuntimeNodeLtsLane {
    pub major: u16,
    pub lane_name: String,
    pub runtime_compatibility_target: Option<RuntimeCompatibilityTarget>,
    pub support_phase: RuntimeNodeSupportPhase,
    pub codename: Option<String>,
    pub release_name: String,
    pub upstream_version: String,
    pub upstream_tag: String,
    pub node_module_version: String,
    pub fixture_corpus_path: Option<String>,
    pub fixture_corpus_upstream_tag: Option<String>,
    pub lts_start: String,
    pub maintenance_start: String,
    pub eol_date: String,
    pub product_default: bool,
    pub evidence_policy: String,
}

static NODE_LTS_REGISTRY: OnceLock<RuntimeNodeLtsRegistry> = OnceLock::new();

fn node_lts_registry() -> &'static RuntimeNodeLtsRegistry {
    NODE_LTS_REGISTRY.get_or_init(|| {
        let registry: RuntimeNodeLtsRegistry =
            serde_json::from_str(NODE_LTS_LANES_JSON).expect("Node LTS registry should parse");
        registry
            .validate()
            .expect("Node LTS registry should validate");
        registry
    })
}

impl RuntimeNodeLtsRegistry {
    fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err(format!(
                "Node LTS registry schema_version must be 1, got {}",
                self.schema_version
            ));
        }
        if self.registry_kind != "nimbus_node_lts_lane_registry" {
            return Err(format!(
                "Node LTS registry has unexpected kind {}",
                self.registry_kind
            ));
        }
        let mut seen_lanes = BTreeSet::new();
        let mut default_lanes = Vec::new();
        for lane in &self.lanes {
            if !seen_lanes.insert(lane.lane_name.as_str()) {
                return Err(format!("duplicate Node LTS lane {}", lane.lane_name));
            }
            if lane.lane_name != format!("node{}", lane.major) {
                return Err(format!(
                    "Node LTS lane {} does not match major {}",
                    lane.lane_name, lane.major
                ));
            }
            if lane.product_default {
                default_lanes.push(lane.lane_name.as_str());
            }
            if lane.release_name != "node" {
                return Err(format!(
                    "Node LTS lane {} has unsupported release name {}",
                    lane.lane_name, lane.release_name
                ));
            }
            if lane.node_module_version.trim().is_empty() {
                return Err(format!(
                    "Node LTS lane {} must declare a node_module_version",
                    lane.lane_name
                ));
            }
            if !lane
                .node_module_version
                .chars()
                .all(|value| value.is_ascii_digit())
            {
                return Err(format!(
                    "Node LTS lane {} node_module_version must be numeric",
                    lane.lane_name
                ));
            }
            if let Some(target) = lane.runtime_compatibility_target
                && target.node_lts_lane_name() != Some(lane.lane_name.as_str())
            {
                return Err(format!(
                    "Node LTS lane {} has mismatched target {:?}",
                    lane.lane_name, target
                ));
            }
        }
        if default_lanes.as_slice() != [self.product_default_lane.as_str()] {
            return Err(format!(
                "Node LTS registry product_default_lane {} does not match exactly one default lane {:?}",
                self.product_default_lane, default_lanes
            ));
        }
        Ok(())
    }
}

impl RuntimeCompatibilityTarget {
    pub fn from_config_str(value: &str) -> Option<Self> {
        match value.trim() {
            "web_standard_isolate"
            | "web-standard-isolate"
            | "web_standard"
            | "WebStandardIsolate"
            | "web" => Some(Self::WebStandardIsolate),
            "bun_jsc" | "bun-jsc" | "bun" | "BunJsc" => Some(Self::BunJsc),
            "wasm_component" | "wasm-component" | "wasmtime" | "WasmComponent" | "wasm" => {
                Some(Self::WasmComponent)
            }
            "node20" | "node_20" | "node-20" | "Node20" | "20" => Some(Self::Node20),
            "node22" | "node_22" | "node-22" | "Node22" | "22" => Some(Self::Node22),
            "node24" | "node_24" | "node-24" | "Node24" | "24" => Some(Self::Node24),
            "node26" | "node_26" | "node-26" | "Node26" | "26" => Some(Self::Node26),
            _ => None,
        }
    }

    pub fn product_default_node_lts_target() -> Self {
        node_lts_registry()
            .lanes
            .iter()
            .find(|lane| lane.product_default)
            .and_then(|lane| lane.runtime_compatibility_target)
            .expect("Node LTS registry product default should have a runtime target")
    }

    pub fn configured_node_lts_targets() -> Vec<Self> {
        node_lts_registry()
            .lanes
            .iter()
            .filter_map(|lane| lane.runtime_compatibility_target)
            .collect()
    }

    pub fn supported_node_lts_targets() -> Vec<Self> {
        node_lts_registry()
            .lanes
            .iter()
            .filter(|lane| lane.support_phase.is_supported_lts())
            .filter_map(|lane| lane.runtime_compatibility_target)
            .collect()
    }

    pub fn is_node(self) -> bool {
        matches!(
            self,
            Self::Node20 | Self::Node22 | Self::Node24 | Self::Node26
        )
    }

    pub fn is_supported_node_lts(self) -> bool {
        self.node_support_phase()
            .is_some_and(RuntimeNodeSupportPhase::is_supported_lts)
    }

    pub fn node_lts_lane_name(self) -> Option<&'static str> {
        match self {
            Self::Node20 => Some("node20"),
            Self::Node22 => Some("node22"),
            Self::Node24 => Some("node24"),
            Self::Node26 => Some("node26"),
            Self::WebStandardIsolate | Self::BunJsc | Self::WasmComponent => None,
        }
    }

    pub fn node_lts_metadata(self) -> Option<&'static RuntimeNodeLtsLane> {
        let lane_name = self.node_lts_lane_name()?;
        node_lts_registry()
            .lanes
            .iter()
            .find(|lane| lane.lane_name == lane_name)
    }

    pub fn node_support_phase(self) -> Option<RuntimeNodeSupportPhase> {
        self.node_lts_metadata().map(|lane| lane.support_phase)
    }

    pub fn node_major_version(self) -> Option<u16> {
        self.node_lts_metadata().map(|lane| lane.major)
    }

    pub fn node_runtime_version(self) -> Option<&'static str> {
        self.node_lts_metadata()
            .map(|lane| lane.upstream_tag.as_str())
    }

    pub fn node_runtime_version_number(self) -> Option<&'static str> {
        self.node_lts_metadata()
            .map(|lane| lane.upstream_version.as_str())
    }

    pub fn node_release_name(self) -> Option<&'static str> {
        self.node_lts_metadata()
            .map(|lane| lane.release_name.as_str())
    }

    pub fn node_release_lts_codename(self) -> Option<&'static str> {
        self.node_lts_metadata()
            .and_then(|lane| lane.codename.as_deref())
    }

    pub fn node_module_version(self) -> Option<&'static str> {
        self.node_lts_metadata()
            .map(|lane| lane.node_module_version.as_str())
    }
}

impl<'de> Deserialize<'de> for RuntimeCompatibilityTarget {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::from_config_str(&value).ok_or_else(|| {
            de::Error::unknown_variant(
                &value,
                &[
                    "web_standard_isolate",
                    "bun_jsc",
                    "wasm_component",
                    "wasmtime",
                    "node20",
                    "node22",
                    "node24",
                    "node26",
                    "20",
                    "22",
                    "24",
                    "26",
                ],
            )
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeExecutionModel {
    RunToCompletion,
    CooperativeLocker,
    CooperativeFuel,
    /// Backend-owned event-loop and API-lock lifecycle. This is not the V8
    /// cooperative locker model; it is for engines such as Bun/JSC where the
    /// backend pool owns guest-entry acknowledgement, event-loop progress,
    /// cancellation, and teardown.
    BackendOwnedEventLoop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeRoutingAffinity {
    None,
    Tenant,
    Function,
    Script,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
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
    /// V8/Deno: retain the worker-local JsRuntime/isolate, but create a fresh
    /// realm and module map for every invocation.
    ///
    /// This is the PIR2 context-recycling pool shape. Node targets require the
    /// explicit `SameOwnerExactAuthority` proof axis and remain non-default
    /// after the NFR6 benchmark rejected adoption for this plan.
    WarmContextRecycle,
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
    /// Wasmtime: retain compiled components process-wide and create a fresh
    /// Store for each invocation.
    PrecompiledModuleCache,
    /// Wasmtime: worker-local retained Store pool. W6 owns the concrete
    /// lifecycle; W3 rejects this pool until that phase lands.
    RetainedStorePool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeNodeFullRealmReusePolicy {
    #[default]
    Unproven,
    SameOwnerExactAuthority,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeMemoryEnforcement {
    #[default]
    V8IsolateHeapLimit,
    OuterQuotaRequired,
    WasmtimeResourceLimiter,
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
                RuntimePoolKind::StartupSnapshotCache
                    | RuntimePoolKind::WarmPool
                    | RuntimePoolKind::WarmContextRecycle
            ) {
                panic!(
                    "V8 runtime backend requires a V8/Deno pool kind, got {:?}",
                    limits.runtime_pool_kind
                );
            }
            if matches!(
                limits.runtime_pool_kind,
                RuntimePoolKind::WarmContextRecycle
            ) && limits.compatibility_target.is_node()
                && !matches!(
                    limits.node_full_realm_reuse_policy,
                    RuntimeNodeFullRealmReusePolicy::SameOwnerExactAuthority
                )
            {
                panic!(
                    "V8 Node warm context recycling requires same-owner exact-authority realm reuse proof, got {:?}",
                    limits.node_full_realm_reuse_policy
                );
            }
            if matches!(
                limits.compatibility_target,
                RuntimeCompatibilityTarget::BunJsc | RuntimeCompatibilityTarget::WasmComponent
            ) {
                panic!(
                    "V8 runtime backend cannot use non-V8 compatibility target {:?}",
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
        RuntimeBackendKind::Wasmtime => {
            if !matches!(
                limits.backend_trust_tier,
                RuntimeBackendTrustTier::InProcessUntrusted
            ) {
                panic!(
                    "Wasmtime runtime backend requires in-process-untrusted trust tier, got {:?}",
                    limits.backend_trust_tier
                );
            }
            if !matches!(
                limits.backend_lockdown_profile,
                RuntimeBackendLockdownProfile::WasmtimeComponentModel
            ) {
                panic!(
                    "Wasmtime runtime backend requires Component Model lockdown profile, got {:?}",
                    limits.backend_lockdown_profile
                );
            }
            if !matches!(
                limits.backend_lifecycle_policy,
                RuntimeBackendLifecyclePolicy::WasmtimePrecompiledModuleCache
            ) {
                panic!(
                    "Wasmtime runtime backend requires precompiled-module-cache lifecycle policy before retained Store pooling lands, got {:?}",
                    limits.backend_lifecycle_policy
                );
            }
            if !matches!(
                limits.bundle_content_kind,
                RuntimeBundleContentKind::WasmComponent
            ) {
                panic!(
                    "Wasmtime runtime backend requires WASM component bundle content, got {:?}",
                    limits.bundle_content_kind
                );
            }
            if !matches!(
                limits.javascript_evaluation_format,
                RuntimeJavaScriptEvaluationFormat::EsModule
            ) {
                panic!(
                    "Wasmtime runtime backend does not use JavaScript program-wrapper evaluation, got {:?}",
                    limits.javascript_evaluation_format
                );
            }
            if !matches!(
                limits.compatibility_target,
                RuntimeCompatibilityTarget::WasmComponent
            ) {
                panic!(
                    "Wasmtime runtime backend requires WASM component compatibility target, got {:?}",
                    limits.compatibility_target
                );
            }
            if !matches!(
                limits.execution_model,
                RuntimeExecutionModel::RunToCompletion
            ) {
                panic!(
                    "Wasmtime runtime backend supports only run-to-completion until cooperative fuel lands, got {:?}",
                    limits.execution_model
                );
            }
            if !matches!(limits.language, RuntimeLanguage::WasmComponent) {
                panic!(
                    "Wasmtime runtime backend requires WASM component runtime language, got {:?}",
                    limits.language
                );
            }
            if !matches!(
                limits.runtime_pool_kind,
                RuntimePoolKind::PrecompiledModuleCache
            ) {
                panic!(
                    "Wasmtime runtime backend requires the precompiled module cache pool before retained Store pooling lands, got {:?}",
                    limits.runtime_pool_kind
                );
            }
            if !matches!(
                limits.memory_enforcement,
                RuntimeMemoryEnforcement::WasmtimeResourceLimiter
            ) {
                panic!(
                    "Wasmtime runtime backend requires Wasmtime ResourceLimiter memory enforcement, got {:?}",
                    limits.memory_enforcement
                );
            }
        }
    }
}
