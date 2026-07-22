use std::sync::Arc;

use tokio::sync::Semaphore;

use crate::fs::RuntimeFileSystem;
use crate::metrics::{RuntimeMetrics, RuntimeMetricsSnapshot};

use super::{
    EffectiveRuntimeScalingPlan, NominalRuntimeHostPressureSource,
    RuntimeAdaptiveControllerSettings, RuntimeAdaptivePressureAdapter, RuntimeBundleContentKind,
    RuntimeHostPressureSource, RuntimeHostResourceBudget, RuntimeHostResourceDecision,
    RuntimeLimits, RuntimeProfile, RuntimeScalingPlanSet, RuntimeTenantBudget,
};

#[derive(Debug)]
pub struct RuntimePolicy {
    limits: RuntimeLimits,
    runtime_instance_semaphore: Arc<Semaphore>,
    metrics: Arc<RuntimeMetrics>,
    host_resource_budget: RuntimeHostResourceBudget,
    host_pressure_source: Arc<dyn RuntimeHostPressureSource>,
    host_resource_governor_enabled: bool,
    adaptive_controller_settings: RuntimeAdaptiveControllerSettings,
    effective_scaling_plans: RuntimeScalingPlanSet,
    file_system: RuntimeFileSystem,
}

impl RuntimePolicy {
    pub fn new(limits: RuntimeLimits) -> Self {
        let limits = limits.normalized();
        let host_resource_budget = default_host_resource_budget_for_limits(&limits);
        Self::from_parts(
            limits,
            host_resource_budget,
            Arc::new(NominalRuntimeHostPressureSource),
            false,
        )
    }

    /// Constructs a policy for backend contract tests whose axis combination
    /// is intentionally rejected by product normalization.
    #[cfg(test)]
    pub(crate) fn new_unchecked_for_backend_contract_test(limits: RuntimeLimits) -> Self {
        let host_resource_budget = default_host_resource_budget_for_limits(&limits);
        Self {
            runtime_instance_semaphore: Arc::new(Semaphore::new(
                limits.max_concurrent_runtime_instances,
            )),
            metrics: Arc::new(RuntimeMetrics::default()),
            limits,
            host_resource_budget,
            host_pressure_source: Arc::new(NominalRuntimeHostPressureSource),
            host_resource_governor_enabled: false,
            adaptive_controller_settings: RuntimeAdaptiveControllerSettings::default(),
            effective_scaling_plans: RuntimeScalingPlanSet::default(),
            file_system: RuntimeFileSystem::default(),
        }
    }

    pub fn with_host_resource_governor(
        limits: RuntimeLimits,
        host_resource_budget: RuntimeHostResourceBudget,
        host_pressure_source: Arc<dyn RuntimeHostPressureSource>,
    ) -> Self {
        Self::from_parts(limits, host_resource_budget, host_pressure_source, true)
    }

    fn from_parts(
        limits: RuntimeLimits,
        host_resource_budget: RuntimeHostResourceBudget,
        host_pressure_source: Arc<dyn RuntimeHostPressureSource>,
        host_resource_governor_enabled: bool,
    ) -> Self {
        let limits = limits.normalized();
        Self {
            runtime_instance_semaphore: Arc::new(Semaphore::new(
                limits.max_concurrent_runtime_instances,
            )),
            metrics: Arc::new(RuntimeMetrics::default()),
            limits,
            host_resource_budget,
            host_pressure_source,
            host_resource_governor_enabled,
            adaptive_controller_settings: RuntimeAdaptiveControllerSettings::default(),
            effective_scaling_plans: RuntimeScalingPlanSet::default(),
            file_system: RuntimeFileSystem::default(),
        }
    }

    pub fn with_adaptive_controller_settings(
        mut self,
        settings: RuntimeAdaptiveControllerSettings,
    ) -> Self {
        self.adaptive_controller_settings = settings;
        self
    }

    pub fn with_effective_scaling_plan(mut self, plan: EffectiveRuntimeScalingPlan) -> Self {
        self.effective_scaling_plans = RuntimeScalingPlanSet::single(plan);
        self
    }

    pub fn with_effective_scaling_plans(mut self, plans: RuntimeScalingPlanSet) -> Self {
        self.effective_scaling_plans = plans;
        self
    }

    pub fn clone_with_effective_scaling_plan(&self, plan: EffectiveRuntimeScalingPlan) -> Self {
        self.clone_with_effective_scaling_plans(RuntimeScalingPlanSet::single(plan))
    }

    pub fn clone_with_effective_scaling_plans(&self, plans: RuntimeScalingPlanSet) -> Self {
        Self {
            // Clone derivations preserve runtime-owned handles. Rebuilding the
            // policy here would silently detach metrics, concurrency permits,
            // and injected filesystem authority from the source policy.
            runtime_instance_semaphore: self.runtime_instance_semaphore.clone(),
            metrics: self.metrics.clone(),
            limits: self.limits.clone(),
            host_resource_budget: self.host_resource_budget,
            host_pressure_source: self.host_pressure_source.clone(),
            host_resource_governor_enabled: self.host_resource_governor_enabled,
            adaptive_controller_settings: self.adaptive_controller_settings,
            effective_scaling_plans: plans,
            file_system: self.file_system.clone(),
        }
    }

    pub fn clone_with_host_resource_governor(
        &self,
        host_resource_budget: RuntimeHostResourceBudget,
        host_pressure_source: Arc<dyn RuntimeHostPressureSource>,
        adaptive_controller_settings: RuntimeAdaptiveControllerSettings,
    ) -> Self {
        Self {
            // Runtime policy overlays must preserve all runtime-owned handles.
            // Rebuilding these here would silently detach metrics, concurrency
            // permits, and injected filesystem authority from the source lane.
            runtime_instance_semaphore: self.runtime_instance_semaphore.clone(),
            metrics: self.metrics.clone(),
            limits: self.limits.clone(),
            host_resource_budget,
            host_pressure_source,
            host_resource_governor_enabled: true,
            adaptive_controller_settings,
            effective_scaling_plans: self.effective_scaling_plans.clone(),
            file_system: self.file_system.clone(),
        }
    }

    pub fn clone_with_file_system(&self, file_system: deno_fs::FileSystemRc) -> Self {
        Self {
            runtime_instance_semaphore: self.runtime_instance_semaphore.clone(),
            metrics: self.metrics.clone(),
            limits: self.limits.clone(),
            host_resource_budget: self.host_resource_budget,
            host_pressure_source: self.host_pressure_source.clone(),
            host_resource_governor_enabled: self.host_resource_governor_enabled,
            adaptive_controller_settings: self.adaptive_controller_settings,
            effective_scaling_plans: self.effective_scaling_plans.clone(),
            file_system: RuntimeFileSystem::new(file_system),
        }
    }

    pub fn limits(&self) -> &RuntimeLimits {
        &self.limits
    }

    pub fn file_system(&self) -> deno_fs::FileSystemRc {
        self.file_system.clone_inner()
    }

    pub(crate) fn validate_bundle_content_kind(
        &self,
        content_kind: RuntimeBundleContentKind,
    ) -> crate::Result<()> {
        if self.limits.bundle_content_kind == content_kind {
            return Ok(());
        }
        Err(crate::NimbusRuntimeError::Contract(format!(
            "runtime bundle content kind {:?} does not match policy content kind {:?}",
            content_kind, self.limits.bundle_content_kind
        )))
    }

    pub(crate) fn runtime_instance_semaphore(&self) -> Arc<Semaphore> {
        self.runtime_instance_semaphore.clone()
    }

    pub fn metrics(&self) -> Arc<RuntimeMetrics> {
        self.metrics.clone()
    }

    pub fn metrics_snapshot(&self) -> RuntimeMetricsSnapshot {
        self.metrics.snapshot()
    }

    /// Returns the derived low-cardinality runtime profile used by lane
    /// diagnostics and telemetry. Non-V8 lanes do not have a V8 profile.
    pub fn runtime_profile(&self) -> Option<RuntimeProfile> {
        RuntimeProfile::for_limits(&self.limits)
    }

    pub fn host_resource_budget(&self) -> RuntimeHostResourceBudget {
        self.host_resource_budget
    }

    pub fn host_resource_decision(&self) -> RuntimeHostResourceDecision {
        let decision = self.host_resource_budget.decide(
            self.limits.worker_threads,
            self.host_pressure_source.sample(),
        );
        if self.host_resource_governor_enabled {
            self.metrics.record_host_resource_decision(decision);
        }
        decision
    }

    pub(crate) fn host_resource_governor_enabled(&self) -> bool {
        self.host_resource_governor_enabled
    }

    pub fn adaptive_controller_settings(&self) -> RuntimeAdaptiveControllerSettings {
        self.adaptive_controller_settings
    }

    pub fn effective_scaling_plan(&self) -> &EffectiveRuntimeScalingPlan {
        self.effective_scaling_plans.default_plan()
    }

    pub fn effective_scaling_plans(&self) -> &RuntimeScalingPlanSet {
        &self.effective_scaling_plans
    }

    pub fn effective_scaling_plan_for_function(
        &self,
        function_name: &str,
    ) -> &EffectiveRuntimeScalingPlan {
        self.effective_scaling_plans
            .plan_for_function(function_name)
    }

    pub fn tenant_budget(&self) -> RuntimeTenantBudget {
        self.limits.tenant_budget_from_normalized()
    }
}

impl Default for RuntimePolicy {
    fn default() -> Self {
        Self::new(RuntimeLimits::default())
    }
}

impl RuntimeAdaptivePressureAdapter for RuntimePolicy {
    fn host_resource_decision(&self) -> RuntimeHostResourceDecision {
        RuntimePolicy::host_resource_decision(self)
    }
}

fn default_host_resource_budget_for_limits(limits: &RuntimeLimits) -> RuntimeHostResourceBudget {
    RuntimeHostResourceBudget {
        host_millicpus: usize_to_u32_saturating(limits.max_concurrent_runtime_instances)
            .saturating_mul(1000),
        system_reserved_millicpus: 0,
        nimbus_control_plane_reserved_millicpus: 0,
        runtime_hard_ceiling_millicpus: None,
        runtime_seat_millicpus: std::num::NonZeroU32::new(1000).expect("one CPU seat is nonzero"),
    }
}

fn usize_to_u32_saturating(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}
