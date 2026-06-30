use serde::{Deserialize, Serialize};

use crate::env::EGRESS_ENFORCEMENT_SCHEMA_VERSION;
use crate::policy::{CompiledEgressPolicy, EgressPolicy};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EgressEnforcementMode {
    LaunchMetadata,
    SupervisorProxy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EgressReloadPolicy {
    RecreateRequired,
    LiveReload,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EgressLaunchEnforcement {
    LaunchMetadata,
    ProcessSupervisorProxy,
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
    pub fn launch_metadata(policy: &CompiledEgressPolicy) -> Self {
        Self {
            schema_version: EGRESS_ENFORCEMENT_SCHEMA_VERSION,
            mode: EgressEnforcementMode::LaunchMetadata,
            reload_policy: EgressReloadPolicy::RecreateRequired,
            policy: policy.policy().clone(),
        }
    }

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

    pub fn from_launch_policy(policy: &EgressPolicy) -> std::result::Result<Self, String> {
        policy
            .compile()
            .map(|compiled| Self::launch_metadata(&compiled))
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
        match (self.mode, self.reload_policy) {
            (EgressEnforcementMode::LaunchMetadata, EgressReloadPolicy::RecreateRequired)
            | (EgressEnforcementMode::SupervisorProxy, EgressReloadPolicy::LiveReload)
            | (EgressEnforcementMode::SupervisorProxy, EgressReloadPolicy::RecreateRequired) => {}
            (EgressEnforcementMode::LaunchMetadata, EgressReloadPolicy::LiveReload) => {
                return Err(
                    "launch-metadata sandbox egress enforcement cannot claim live reload"
                        .to_owned(),
                );
            }
        }
        self.policy.compile()
    }
}

impl EgressLaunchEnforcement {
    pub fn materialize(
        self,
        policy: &EgressPolicy,
    ) -> std::result::Result<EgressEnforcementPlan, String> {
        let compiled = policy.compile()?;
        Ok(match self {
            Self::LaunchMetadata => EgressEnforcementPlan::launch_metadata(&compiled),
            Self::ProcessSupervisorProxy => EgressEnforcementPlan::supervisor_proxy(
                &compiled,
                EgressReloadPolicy::RecreateRequired,
            ),
        })
    }
}
