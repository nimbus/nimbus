use serde::{Deserialize, Serialize};

use crate::env::EGRESS_ENFORCEMENT_SCHEMA_VERSION;
use crate::policy::{CompiledEgressPolicy, EgressPolicy};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EgressEnforcementMode {
    SupervisorProxy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EgressReloadPolicy {
    RecreateRequired,
    LiveReload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EgressEnforcementPlan {
    pub schema_version: u32,
    pub mode: EgressEnforcementMode,
    pub reload_policy: EgressReloadPolicy,
    pub policy: EgressPolicy,
}

impl EgressEnforcementPlan {
    pub fn supervisor_proxy(
        policy: &CompiledEgressPolicy,
        reload_policy: EgressReloadPolicy,
    ) -> Self {
        Self {
            schema_version: EGRESS_ENFORCEMENT_SCHEMA_VERSION,
            mode: EgressEnforcementMode::SupervisorProxy,
            reload_policy,
            policy: policy.policy().clone(),
        }
    }

    pub fn policy(&self) -> &EgressPolicy {
        &self.policy
    }

    pub fn validate(&self) -> std::result::Result<CompiledEgressPolicy, String> {
        if self.schema_version != EGRESS_ENFORCEMENT_SCHEMA_VERSION {
            return Err(format!(
                "sandbox egress enforcement schema_version must be {}, got {}",
                EGRESS_ENFORCEMENT_SCHEMA_VERSION, self.schema_version
            ));
        }
        self.policy.compile_for_supervisor_proxy()
    }
}
