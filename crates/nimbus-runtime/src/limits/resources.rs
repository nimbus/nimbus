use std::num::NonZeroUsize;
use std::time::Duration;

use serde::Serialize;

use super::axes::validate_backend_policy_axes;
use super::grants::validate_mode_grant_ceiling;
use super::{
    RuntimeBackendKind, RuntimeBackendLifecyclePolicy, RuntimeBackendLockdownProfile,
    RuntimeBackendTrustTier, RuntimeBundleContentKind, RuntimeCompatibilityTarget,
    RuntimeExecutionModel, RuntimeGrants, RuntimeJavaScriptEvaluationFormat, RuntimeLanguage,
    RuntimeMemoryEnforcement, RuntimeMode, RuntimeModuleStateSemantics, RuntimePoolKind,
    RuntimePreset, RuntimeResetCapabilities, RuntimeRoutingAffinity,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RuntimeLimits {
    pub backend_kind: RuntimeBackendKind,
    pub backend_trust_tier: RuntimeBackendTrustTier,
    pub backend_lockdown_profile: RuntimeBackendLockdownProfile,
    pub backend_lifecycle_policy: RuntimeBackendLifecyclePolicy,
    pub bundle_content_kind: RuntimeBundleContentKind,
    pub javascript_evaluation_format: RuntimeJavaScriptEvaluationFormat,
    pub compatibility_target: RuntimeCompatibilityTarget,
    pub execution_model: RuntimeExecutionModel,
    pub mode: RuntimeMode,
    pub language: RuntimeLanguage,
    pub preset: RuntimePreset,
    pub grants: RuntimeGrants,
    pub runtime_pool_kind: RuntimePoolKind,
    pub memory_enforcement: RuntimeMemoryEnforcement,
    pub routing_affinity: RuntimeRoutingAffinity,
    pub routing_affinity_max_entries: usize,
    pub max_warm_pool_entries_per_worker: usize,
    pub max_warm_reuses: usize,
    pub max_heap_mb: usize,
    pub initial_heap_mb: usize,
    pub execution_timeout: Duration,
    pub max_concurrent_runtime_instances: usize,
    pub worker_threads: usize,
    pub max_active_top_level_invocations_per_tenant: usize,
    pub max_in_flight_top_level_invocations_per_tenant: usize,
    pub max_queued_top_level_invocations_per_tenant: usize,
    pub max_nested_runtime_invocations: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct RuntimeTenantBudget {
    pub max_active_runtime_slots: usize,
    pub max_in_flight_top_level_invocations: usize,
    pub max_queued_top_level_invocations: usize,
    pub max_worker_thread_slots: usize,
    pub max_heap_mb_per_runtime: usize,
    pub memory_enforcement: RuntimeMemoryEnforcement,
    pub max_active_heap_mb: usize,
    pub execution_timeout: Duration,
    pub max_nested_runtime_invocations_per_top_level: usize,
}

impl RuntimeLimits {
    pub fn restricted_code() -> Self {
        Self {
            mode: RuntimeMode::Restricted,
            language: RuntimeLanguage::JavaScript,
            preset: RuntimePreset::Code,
            grants: RuntimeGrants::restricted(),
            ..Self::default()
        }
    }

    pub fn privileged_operator() -> Self {
        Self {
            mode: RuntimeMode::Privileged,
            language: RuntimeLanguage::JavaScript,
            preset: RuntimePreset::Operator,
            grants: RuntimeGrants::restricted(),
            ..Self::default()
        }
    }

    pub fn application_web_standard() -> Self {
        Self {
            compatibility_target: RuntimeCompatibilityTarget::WebStandardIsolate,
            mode: RuntimeMode::Standard,
            language: RuntimeLanguage::JavaScript,
            preset: RuntimePreset::Application,
            grants: RuntimeGrants::application_web_standard(),
            ..Self::default()
        }
    }

    pub fn application_node22() -> Self {
        Self::application_node(RuntimeCompatibilityTarget::Node22)
    }

    pub fn application_node20() -> Self {
        Self::application_node(RuntimeCompatibilityTarget::Node20)
    }

    pub fn application_node24() -> Self {
        Self::application_node(RuntimeCompatibilityTarget::Node24)
    }

    pub fn application_node(target: RuntimeCompatibilityTarget) -> Self {
        Self::application_node_production_in_process(target)
    }

    pub fn application_node_production_in_process(target: RuntimeCompatibilityTarget) -> Self {
        assert!(target.is_node(), "application_node requires a Node target");
        Self {
            compatibility_target: target,
            mode: RuntimeMode::Standard,
            language: RuntimeLanguage::JavaScript,
            preset: RuntimePreset::Application,
            grants: RuntimeGrants::application_node_production_in_process(),
            ..Self::default()
        }
    }

    pub fn application_node20_local_development() -> Self {
        Self::application_node_local_development(RuntimeCompatibilityTarget::Node20)
    }

    pub fn application_node22_local_development() -> Self {
        Self::application_node_local_development(RuntimeCompatibilityTarget::Node22)
    }

    pub fn application_node24_local_development() -> Self {
        Self::application_node_local_development(RuntimeCompatibilityTarget::Node24)
    }

    pub fn application_node_local_development(target: RuntimeCompatibilityTarget) -> Self {
        assert!(
            target.is_node(),
            "application_node_local_development requires a Node target"
        );
        Self {
            compatibility_target: target,
            mode: RuntimeMode::Standard,
            language: RuntimeLanguage::JavaScript,
            preset: RuntimePreset::Application,
            grants: RuntimeGrants::application_node_local_development(),
            ..Self::default()
        }
    }

    pub fn application_node20_service_microvm() -> Self {
        Self::application_node_service_microvm(RuntimeCompatibilityTarget::Node20)
    }

    pub fn application_node22_service_microvm() -> Self {
        Self::application_node_service_microvm(RuntimeCompatibilityTarget::Node22)
    }

    pub fn application_node24_service_microvm() -> Self {
        Self::application_node_service_microvm(RuntimeCompatibilityTarget::Node24)
    }

    pub fn application_node_service_microvm(target: RuntimeCompatibilityTarget) -> Self {
        assert!(
            target.is_node(),
            "application_node_service_microvm requires a Node target"
        );
        Self {
            compatibility_target: target,
            mode: RuntimeMode::Standard,
            language: RuntimeLanguage::JavaScript,
            preset: RuntimePreset::Application,
            grants: RuntimeGrants::application_node_service_microvm(),
            ..Self::default()
        }
    }

    pub fn application_bun_jsc() -> Self {
        Self {
            backend_kind: RuntimeBackendKind::BunJsc,
            backend_trust_tier: RuntimeBackendTrustTier::InProcessUntrusted,
            backend_lockdown_profile: RuntimeBackendLockdownProfile::BunJscInProcessUntrusted,
            backend_lifecycle_policy:
                RuntimeBackendLifecyclePolicy::BunJscFreshDiscardPoolOuterQuotaRequired,
            bundle_content_kind: RuntimeBundleContentKind::JavaScript,
            javascript_evaluation_format: RuntimeJavaScriptEvaluationFormat::ProgramWrapper,
            compatibility_target: RuntimeCompatibilityTarget::BunJsc,
            execution_model: RuntimeExecutionModel::BackendOwnedEventLoop,
            mode: RuntimeMode::Standard,
            language: RuntimeLanguage::JavaScript,
            preset: RuntimePreset::Application,
            grants: RuntimeGrants::restricted(),
            runtime_pool_kind: RuntimePoolKind::BunJscFreshDiscard,
            memory_enforcement: RuntimeMemoryEnforcement::OuterQuotaRequired,
            ..Self::default()
        }
    }

    pub fn tooling_node22() -> Self {
        Self::tooling_node(RuntimeCompatibilityTarget::Node22)
    }

    pub fn tooling_node20() -> Self {
        Self::tooling_node(RuntimeCompatibilityTarget::Node20)
    }

    pub fn tooling_node24() -> Self {
        Self::tooling_node(RuntimeCompatibilityTarget::Node24)
    }

    pub fn tooling_node(target: RuntimeCompatibilityTarget) -> Self {
        assert!(target.is_node(), "tooling_node requires a Node target");
        Self {
            compatibility_target: target,
            mode: RuntimeMode::Standard,
            language: RuntimeLanguage::JavaScript,
            preset: RuntimePreset::Tooling,
            grants: RuntimeGrants::tooling(),
            ..Self::default()
        }
    }

    pub fn module_state_semantics(&self) -> RuntimeModuleStateSemantics {
        match self.runtime_pool_kind {
            RuntimePoolKind::WarmPool | RuntimePoolKind::BunJscTrustedRetained => {
                RuntimeModuleStateSemantics::WarmPerBundle
            }
            RuntimePoolKind::StartupSnapshotCache | RuntimePoolKind::BunJscFreshDiscard => {
                RuntimeModuleStateSemantics::FreshPerInvocation
            }
        }
    }

    pub fn reset_capabilities(&self) -> RuntimeResetCapabilities {
        match self.runtime_pool_kind {
            RuntimePoolKind::WarmPool | RuntimePoolKind::BunJscTrustedRetained => {
                RuntimeResetCapabilities {
                    op_state_per_invocation: true,
                    bootstrap_state_per_invocation: true,
                    user_module_state_per_invocation: false,
                }
            }
            RuntimePoolKind::StartupSnapshotCache | RuntimePoolKind::BunJscFreshDiscard => {
                RuntimeResetCapabilities {
                    op_state_per_invocation: true,
                    bootstrap_state_per_invocation: true,
                    user_module_state_per_invocation: true,
                }
            }
        }
    }

    pub fn tenant_budget(&self) -> RuntimeTenantBudget {
        self.normalized().tenant_budget_from_normalized()
    }

    pub(super) fn tenant_budget_from_normalized(&self) -> RuntimeTenantBudget {
        RuntimeTenantBudget {
            max_active_runtime_slots: self.max_active_top_level_invocations_per_tenant,
            max_in_flight_top_level_invocations: self
                .max_in_flight_top_level_invocations_per_tenant,
            max_queued_top_level_invocations: self.max_queued_top_level_invocations_per_tenant,
            max_worker_thread_slots: self
                .worker_threads
                .min(self.max_active_top_level_invocations_per_tenant),
            max_heap_mb_per_runtime: self.max_heap_mb,
            memory_enforcement: self.memory_enforcement,
            max_active_heap_mb: self
                .max_heap_mb
                .saturating_mul(self.max_active_top_level_invocations_per_tenant),
            execution_timeout: self.execution_timeout,
            max_nested_runtime_invocations_per_top_level: self.max_nested_runtime_invocations,
        }
    }

    pub fn normalized(&self) -> Self {
        validate_backend_policy_axes(self);

        if matches!(self.preset, RuntimePreset::Tooling) && !self.compatibility_target.is_node() {
            panic!(
                "Tooling runtime preset requires a Node compatibility target, \
                 got {:?}",
                self.compatibility_target
            );
        }

        if !self.grants.run.is_empty() && !self.compatibility_target.is_node() {
            panic!(
                "runtime run grants require a Node compatibility target, got {:?}",
                self.compatibility_target
            );
        }

        if self
            .grants
            .run
            .iter()
            .any(|grant| grant == "$discovered_tooling")
            && !matches!(self.preset, RuntimePreset::Tooling)
        {
            panic!(
                "$discovered_tooling run grant requires Tooling runtime preset, got {:?}",
                self.preset
            );
        }

        let grants = if matches!(self.preset, RuntimePreset::Application)
            && self.compatibility_target.is_node()
            && self.grants == RuntimeGrants::application_web_standard()
        {
            RuntimeGrants::application_node_production_in_process()
        } else {
            self.grants.clone()
        };
        validate_mode_grant_ceiling(self.mode, &grants);

        // WarmPool requires CooperativeLocker — fail fast.
        if matches!(self.runtime_pool_kind, RuntimePoolKind::WarmPool)
            && !matches!(
                self.execution_model,
                RuntimeExecutionModel::CooperativeLocker
            )
        {
            panic!(
                "WarmPool requires CooperativeLocker execution model, \
                 got {:?}",
                self.execution_model
            );
        }

        let max_concurrent_runtime_instances = self.max_concurrent_runtime_instances.max(1);
        let worker_threads = self
            .worker_threads
            .max(max_concurrent_runtime_instances)
            .max(1);
        let max_heap_mb = self.max_heap_mb.max(1);
        let initial_heap_mb = self.initial_heap_mb.max(1).min(max_heap_mb);
        let max_active_top_level_invocations_per_tenant = self
            .max_active_top_level_invocations_per_tenant
            .max(1)
            .min(max_concurrent_runtime_instances);
        let max_in_flight_top_level_invocations_per_tenant = self
            .max_in_flight_top_level_invocations_per_tenant
            .max(max_active_top_level_invocations_per_tenant)
            .min(worker_threads);
        Self {
            backend_kind: self.backend_kind,
            backend_trust_tier: self.backend_trust_tier,
            backend_lockdown_profile: self.backend_lockdown_profile,
            backend_lifecycle_policy: self.backend_lifecycle_policy,
            bundle_content_kind: self.bundle_content_kind,
            javascript_evaluation_format: self.javascript_evaluation_format,
            compatibility_target: self.compatibility_target,
            execution_model: self.execution_model,
            mode: self.mode,
            language: self.language,
            preset: self.preset,
            grants,
            runtime_pool_kind: self.runtime_pool_kind,
            memory_enforcement: self.memory_enforcement,
            routing_affinity: self.routing_affinity,
            routing_affinity_max_entries: self.routing_affinity_max_entries.max(1),
            max_warm_pool_entries_per_worker: self.max_warm_pool_entries_per_worker.max(1),
            max_warm_reuses: self.max_warm_reuses.max(1),
            max_heap_mb,
            initial_heap_mb,
            execution_timeout: self.execution_timeout,
            max_concurrent_runtime_instances,
            worker_threads,
            max_active_top_level_invocations_per_tenant,
            max_in_flight_top_level_invocations_per_tenant,
            max_queued_top_level_invocations_per_tenant: self
                .max_queued_top_level_invocations_per_tenant,
            max_nested_runtime_invocations: self.max_nested_runtime_invocations,
        }
    }

    pub fn apply_resource_overrides_from(&mut self, source: &Self) {
        self.routing_affinity = source.routing_affinity;
        self.routing_affinity_max_entries = source.routing_affinity_max_entries;
        self.max_warm_pool_entries_per_worker = source.max_warm_pool_entries_per_worker;
        self.max_warm_reuses = source.max_warm_reuses;
        self.max_heap_mb = source.max_heap_mb;
        self.initial_heap_mb = source.initial_heap_mb;
        self.execution_timeout = source.execution_timeout;
        self.max_concurrent_runtime_instances = source.max_concurrent_runtime_instances;
        self.worker_threads = source.worker_threads;
        self.max_active_top_level_invocations_per_tenant =
            source.max_active_top_level_invocations_per_tenant;
        self.max_in_flight_top_level_invocations_per_tenant =
            source.max_in_flight_top_level_invocations_per_tenant;
        self.max_queued_top_level_invocations_per_tenant =
            source.max_queued_top_level_invocations_per_tenant;
        self.max_nested_runtime_invocations = source.max_nested_runtime_invocations;
    }
}

impl Default for RuntimeLimits {
    fn default() -> Self {
        let max_concurrent_runtime_instances = std::thread::available_parallelism()
            .unwrap_or(NonZeroUsize::MIN)
            .get();
        let worker_threads = max_concurrent_runtime_instances.saturating_mul(2).max(1);
        let max_active_top_level_invocations_per_tenant =
            max_concurrent_runtime_instances.saturating_sub(1).max(1);
        let max_in_flight_top_level_invocations_per_tenant =
            max_active_top_level_invocations_per_tenant
                .saturating_mul(2)
                .min(worker_threads)
                .max(max_active_top_level_invocations_per_tenant);
        let routing_affinity_max_entries = worker_threads.saturating_mul(256).max(1024);
        Self {
            backend_kind: RuntimeBackendKind::V8,
            backend_trust_tier: RuntimeBackendTrustTier::InProcessUntrusted,
            backend_lockdown_profile: RuntimeBackendLockdownProfile::V8DenoCore,
            backend_lifecycle_policy: RuntimeBackendLifecyclePolicy::V8DenoCorePool,
            bundle_content_kind: RuntimeBundleContentKind::JavaScript,
            javascript_evaluation_format: RuntimeJavaScriptEvaluationFormat::EsModule,
            compatibility_target: RuntimeCompatibilityTarget::WebStandardIsolate,
            execution_model: RuntimeExecutionModel::CooperativeLocker,
            mode: RuntimeMode::Standard,
            language: RuntimeLanguage::JavaScript,
            preset: RuntimePreset::Application,
            grants: RuntimeGrants::application_web_standard(),
            runtime_pool_kind: RuntimePoolKind::WarmPool,
            memory_enforcement: RuntimeMemoryEnforcement::V8IsolateHeapLimit,
            routing_affinity: RuntimeRoutingAffinity::Tenant,
            routing_affinity_max_entries,
            max_warm_pool_entries_per_worker: 4,
            max_warm_reuses: 10_000,
            max_heap_mb: 128,
            initial_heap_mb: 8,
            execution_timeout: Duration::from_secs(30),
            max_concurrent_runtime_instances,
            worker_threads,
            max_active_top_level_invocations_per_tenant,
            max_in_flight_top_level_invocations_per_tenant,
            max_queued_top_level_invocations_per_tenant:
                max_in_flight_top_level_invocations_per_tenant,
            max_nested_runtime_invocations: 64,
        }
    }
}
