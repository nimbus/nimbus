use std::sync::Arc;

use nimbus_runtime::{
    EffectiveRuntimeScalingPlan, NominalRuntimeHostPressureSource,
    RuntimeAdaptiveControllerSettings, RuntimeHostPressureSource, RuntimeHostResourceBudget,
    RuntimeLimits, RuntimePolicy, RuntimeScalingPlanSet,
};

#[derive(Clone)]
pub struct RuntimeGovernorConfig {
    base_runtime_limits: RuntimeLimits,
    runtime_host_resource_budget: RuntimeHostResourceBudget,
    runtime_host_pressure_source: Arc<dyn RuntimeHostPressureSource>,
    runtime_adaptive_controller_settings: RuntimeAdaptiveControllerSettings,
    effective_runtime_scaling_plans: RuntimeScalingPlanSet,
}

impl Default for RuntimeGovernorConfig {
    fn default() -> Self {
        Self {
            base_runtime_limits: RuntimeLimits::default(),
            runtime_host_resource_budget: default_runtime_host_resource_budget(),
            runtime_host_pressure_source: default_runtime_host_pressure_source(),
            runtime_adaptive_controller_settings: RuntimeAdaptiveControllerSettings::default(),
            effective_runtime_scaling_plans: RuntimeScalingPlanSet::default(),
        }
    }
}

impl RuntimeGovernorConfig {
    pub fn with_base_runtime_limits(mut self, limits: RuntimeLimits) -> Self {
        self.base_runtime_limits = limits;
        self
    }

    pub fn base_runtime_limits(&self) -> &RuntimeLimits {
        &self.base_runtime_limits
    }

    pub fn with_runtime_host_resource_budget(mut self, budget: RuntimeHostResourceBudget) -> Self {
        self.runtime_host_resource_budget = budget;
        self
    }

    pub fn with_runtime_host_pressure_source(
        mut self,
        pressure_source: Arc<dyn RuntimeHostPressureSource>,
    ) -> Self {
        self.runtime_host_pressure_source = pressure_source;
        self
    }

    pub fn with_runtime_adaptive_controller_settings(
        mut self,
        settings: RuntimeAdaptiveControllerSettings,
    ) -> Self {
        self.runtime_adaptive_controller_settings = settings;
        self
    }

    pub fn with_effective_runtime_scaling_plan(self, plan: EffectiveRuntimeScalingPlan) -> Self {
        self.with_effective_runtime_scaling_plans(RuntimeScalingPlanSet::single(plan))
    }

    pub fn with_effective_runtime_scaling_plans(mut self, plans: RuntimeScalingPlanSet) -> Self {
        self.effective_runtime_scaling_plans = plans;
        self
    }

    pub fn runtime_host_resource_budget(&self) -> RuntimeHostResourceBudget {
        self.runtime_host_resource_budget
    }

    pub fn runtime_adaptive_controller_settings(&self) -> RuntimeAdaptiveControllerSettings {
        self.runtime_adaptive_controller_settings
    }

    pub fn effective_runtime_scaling_plan(&self) -> &EffectiveRuntimeScalingPlan {
        self.effective_runtime_scaling_plans.default_plan()
    }

    pub fn effective_runtime_scaling_plans(&self) -> &RuntimeScalingPlanSet {
        &self.effective_runtime_scaling_plans
    }

    pub(crate) fn policy_for_limits(&self, limits: RuntimeLimits) -> Arc<RuntimePolicy> {
        Arc::new(
            RuntimePolicy::with_host_resource_governor(
                limits,
                self.runtime_host_resource_budget,
                self.runtime_host_pressure_source.clone(),
            )
            .with_adaptive_controller_settings(self.runtime_adaptive_controller_settings)
            .with_effective_scaling_plans(self.effective_runtime_scaling_plans.clone()),
        )
    }
}

fn default_runtime_host_resource_budget() -> RuntimeHostResourceBudget {
    let fallback_cpus = std::num::NonZeroUsize::new(1).expect("one logical CPU is nonzero");
    let host_logical_cpus = std::thread::available_parallelism().unwrap_or(fallback_cpus);
    RuntimeHostResourceBudget::conservative_for_logical_cpus(host_logical_cpus)
}

fn default_runtime_host_pressure_source() -> Arc<dyn RuntimeHostPressureSource> {
    #[cfg(target_os = "linux")]
    {
        match nimbus_node::CgroupV2HostPressureSource::for_current_process() {
            Ok(source) => return Arc::new(source),
            Err(error) => {
                tracing::debug!(
                    error = %error,
                    "cgroup v2 host pressure source unavailable; using nominal runtime host pressure source"
                );
            }
        }
    }
    Arc::new(NominalRuntimeHostPressureSource)
}
