use nimbus_runtime::{RuntimeExecutionModel, RuntimeLimits, RuntimePoolKind, RuntimeProfile};

use crate::{RuntimeIsolationTier, TenantRuntimePolicyAdmission, TenantRuntimePolicyDecision};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeEfficiencyPlan {
    profile: Option<RuntimeProfile>,
    state: RuntimeEfficiencyPlanState,
    effective_pool_kind: RuntimePoolKind,
    effective_execution_model: RuntimeExecutionModel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeEfficiencyPlanState {
    FlagOffCurrentBehavior,
    EscalatedOrRouted,
    UnsupportedSurface,
}

impl RuntimeEfficiencyPlan {
    pub fn profile(&self) -> Option<RuntimeProfile> {
        self.profile
    }

    pub fn state(&self) -> RuntimeEfficiencyPlanState {
        self.state
    }

    pub fn effective_pool_kind(&self) -> RuntimePoolKind {
        self.effective_pool_kind
    }

    pub fn effective_execution_model(&self) -> RuntimeExecutionModel {
        self.effective_execution_model
    }
}

fn classify_runtime_efficiency_plan(
    normalized_limits: &RuntimeLimits,
    admitted_decision: &TenantRuntimePolicyDecision,
) -> RuntimeEfficiencyPlan {
    let profile = RuntimeProfile::for_limits(normalized_limits);
    let state = match profile {
        None => RuntimeEfficiencyPlanState::UnsupportedSurface,
        Some(_) if !admitted_decision.allows_in_process_efficiency() => {
            RuntimeEfficiencyPlanState::EscalatedOrRouted
        }
        Some(_) => RuntimeEfficiencyPlanState::FlagOffCurrentBehavior,
    };
    RuntimeEfficiencyPlan {
        profile,
        state,
        effective_pool_kind: normalized_limits.runtime_pool_kind,
        effective_execution_model: normalized_limits.execution_model,
    }
}

impl TenantRuntimePolicyDecision {
    pub fn runtime_efficiency_plan(
        &self,
        normalized_limits: &RuntimeLimits,
    ) -> RuntimeEfficiencyPlan {
        classify_runtime_efficiency_plan(normalized_limits, self)
    }

    fn allows_in_process_efficiency(&self) -> bool {
        matches!(
            self.admission(),
            TenantRuntimePolicyAdmission::AdmitInProcess
        ) && matches!(self.tier(), RuntimeIsolationTier::InProcessUntrusted)
    }
}
