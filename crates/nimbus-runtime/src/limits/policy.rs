use std::sync::Arc;

use tokio::sync::Semaphore;

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
        }
    }

    /// Re-derive this policy with an explicit host-resource governor while
    /// preserving the source lane's metrics handle.
    ///
    /// Observability-only: `RuntimeMetrics` is read solely through
    /// `metrics_snapshot()` and never feeds a scheduling, admission, or
    /// fairness decision. Build-time policy overlays must keep the observer
    /// (which can hold the pre-build policy) and worker (which increments the
    /// post-build executor policy) pointed at the same counters.
    pub fn clone_with_host_resource_governor(
        &self,
        host_resource_budget: RuntimeHostResourceBudget,
        host_pressure_source: Arc<dyn RuntimeHostPressureSource>,
    ) -> Self {
        Self {
            runtime_instance_semaphore: Arc::new(Semaphore::new(
                self.limits.max_concurrent_runtime_instances,
            )),
            metrics: self.metrics.clone(),
            limits: self.limits.clone(),
            host_resource_budget,
            host_pressure_source,
            host_resource_governor_enabled: true,
            adaptive_controller_settings: self.adaptive_controller_settings,
            effective_scaling_plans: self.effective_scaling_plans.clone(),
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
            runtime_instance_semaphore: Arc::new(Semaphore::new(
                self.limits.max_concurrent_runtime_instances,
            )),
            // Preserve the source policy's metrics handle: this is a clone-with
            // derivation (the build()-time scaling-plan transform), so the
            // re-derived policy must observe the same counters the original
            // handle exposes. Observability-only; no metric value or scheduling
            // behavior changes.
            metrics: self.metrics.clone(),
            limits: self.limits.clone(),
            host_resource_budget: self.host_resource_budget,
            host_pressure_source: self.host_pressure_source.clone(),
            host_resource_governor_enabled: self.host_resource_governor_enabled,
            adaptive_controller_settings: self.adaptive_controller_settings,
            effective_scaling_plans: plans,
        }
    }

    pub fn limits(&self) -> &RuntimeLimits {
        &self.limits
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

    pub(crate) fn runtime_profile(&self) -> Option<RuntimeProfile> {
        RuntimeProfile::for_limits(&self.limits)
    }

    pub fn host_resource_budget(&self) -> RuntimeHostResourceBudget {
        self.host_resource_budget
    }

    pub fn host_resource_decision(&self) -> RuntimeHostResourceDecision {
        let decision = self.host_resource_budget.decide(
            self.limits.max_concurrent_runtime_instances,
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
