use nimbus_core::{Error, Result};
use nimbus_runtime::{
    EffectiveRuntimeScalingPlan, RequestedRuntimeScalingTarget, RuntimeScalingAdjustmentReason,
    RuntimeScalingLimit, RuntimeScalingPreset, RuntimeScalingTarget,
};
use serde::{Deserialize, Serialize};

use super::{OperatorPolicyDocument, OperatorPolicyWorkload};
use crate::WorkloadKind;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TenantRuntimeScalingRequest {
    pub function: String,
    #[serde(default)]
    pub preset: RuntimeScalingPreset,
    #[serde(default)]
    pub requested: RequestedRuntimeScalingTarget,
}

impl TenantRuntimeScalingRequest {
    pub fn new(
        function: impl Into<String>,
        preset: RuntimeScalingPreset,
        requested: RequestedRuntimeScalingTarget,
    ) -> Self {
        Self {
            function: function.into(),
            preset,
            requested,
        }
    }
}

impl OperatorPolicyDocument {
    pub fn admit_runtime_scaling(
        &self,
        request: TenantRuntimeScalingRequest,
    ) -> Result<EffectiveRuntimeScalingPlan> {
        self.validate()?;
        validate_request_shape(&request)?;
        let envelope = RuntimeScalingEnvelope::from_policy(self, &request.function);
        envelope.admit(request)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RuntimeScalingEnvelope {
    max_total_warm: usize,
    max_min_warm_total: usize,
    max_warm_per_function: usize,
    allow_live_scaling: bool,
}

impl RuntimeScalingEnvelope {
    fn from_policy(policy: &OperatorPolicyDocument, function: &str) -> Self {
        let defaults = policy.defaults.runtime_scaling_limits;
        let workload = policy.workloads.iter().find(|workload| {
            workload.kind == WorkloadKind::RuntimeFunction && workload.name == function
        });
        Self::from_workload(defaults, workload)
    }

    fn from_workload(
        defaults: super::OperatorRuntimeScalingLimits,
        workload: Option<&OperatorPolicyWorkload>,
    ) -> Self {
        let quota = workload.map(|workload| workload.quotas.runtime_scaling);
        let max_warm_per_function = quota
            .and_then(|quota| quota.max_warm)
            .unwrap_or(defaults.max_warm_per_function)
            .min(defaults.max_warm_per_function)
            .min(defaults.max_total_warm);
        let max_min_warm_total = defaults.max_min_warm_total.min(defaults.max_total_warm);
        let max_min_warm = quota
            .and_then(|quota| quota.max_min_warm)
            .unwrap_or(max_min_warm_total)
            .min(max_min_warm_total)
            .min(max_warm_per_function);
        let allow_live_scaling = defaults.allow_live_scaling
            && quota
                .and_then(|quota| quota.allow_live_scaling)
                .unwrap_or(defaults.allow_live_scaling);
        Self {
            max_total_warm: defaults.max_total_warm,
            max_min_warm_total: max_min_warm,
            max_warm_per_function,
            allow_live_scaling,
        }
    }

    fn admit(self, request: TenantRuntimeScalingRequest) -> Result<EffectiveRuntimeScalingPlan> {
        let requested = request.requested;
        if requested.min_warm > self.max_min_warm_total {
            return Err(Error::InvalidInput(format!(
                "{} rejected: requested min_warm={} exceeds operator max_min_warm_total remaining={}; lower min_warm to <= {} or ask an operator to raise runtime_scaling_limits.max_min_warm_total",
                request.function,
                requested.min_warm,
                self.max_min_warm_total,
                self.max_min_warm_total
            )));
        }
        let admitted_max = match requested.max_warm {
            RuntimeScalingLimit::Auto => self.max_warm_per_function,
            RuntimeScalingLimit::Fixed(value) => {
                if value > self.max_warm_per_function {
                    return Err(Error::InvalidInput(format!(
                        "{} rejected: requested max_warm={} exceeds operator max_warm_per_function={}; lower max_warm to <= {} or ask an operator to raise quotas.runtime_scaling.max_warm",
                        request.function,
                        value,
                        self.max_warm_per_function,
                        self.max_warm_per_function
                    )));
                }
                value
            }
        };
        if requested.activation_warm > admitted_max {
            return Err(Error::InvalidInput(format!(
                "{} rejected: requested activation_warm={} exceeds admitted max_warm={}",
                request.function, requested.activation_warm, admitted_max
            )));
        }
        if requested.min_warm > admitted_max {
            return Err(Error::InvalidInput(format!(
                "{} rejected: requested min_warm={} exceeds admitted max_warm={}",
                request.function, requested.min_warm, admitted_max
            )));
        }

        let admitted = RuntimeScalingTarget {
            min_warm: requested.min_warm,
            activation_warm: requested.activation_warm,
            max_warm: admitted_max,
            scale_down_delay_secs: requested.scale_down_delay_secs,
            live_scaling: requested.live_scaling && self.allow_live_scaling,
        };
        let pressure_adjustment = if requested.max_warm == RuntimeScalingLimit::Auto
            || requested.live_scaling != admitted.live_scaling
        {
            RuntimeScalingAdjustmentReason::OperatorEnvelope
        } else {
            RuntimeScalingAdjustmentReason::None
        };
        Ok(EffectiveRuntimeScalingPlan {
            function: request.function,
            preset: request.preset,
            requested,
            admitted,
            effective: admitted,
            pressure_adjustment,
            rejection: None,
        })
    }
}

fn validate_request_shape(request: &TenantRuntimeScalingRequest) -> Result<()> {
    if request.function.trim().is_empty() {
        return Err(Error::InvalidInput(
            "runtime scaling request function cannot be empty".to_string(),
        ));
    }
    if let RuntimeScalingPreset::Fixed = request.preset {
        let RuntimeScalingLimit::Fixed(max_warm) = request.requested.max_warm else {
            return Err(Error::InvalidInput(
                "fixed function scaling preset requires explicit max_warm".to_string(),
            ));
        };
        if request.requested.min_warm != max_warm {
            return Err(Error::InvalidInput(
                "fixed function scaling preset requires min_warm == max_warm".to_string(),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use nimbus_runtime::{RuntimeScalingLimit, RuntimeScalingPreset};

    use super::*;
    use crate::operator_policy::OperatorPolicyDocument;

    fn parse_policy(body: &str) -> OperatorPolicyDocument {
        serde_yaml::from_str(body).expect("policy should parse")
    }

    fn request(
        function: &str,
        min_warm: usize,
        max_warm: RuntimeScalingLimit,
    ) -> TenantRuntimeScalingRequest {
        TenantRuntimeScalingRequest::new(
            function,
            RuntimeScalingPreset::Warm,
            RequestedRuntimeScalingTarget {
                min_warm,
                activation_warm: 1,
                max_warm,
                scale_down_delay_secs: 600,
                live_scaling: true,
            },
        )
    }

    #[test]
    fn admits_auto_inside_operator_envelope() {
        let policy = parse_policy(
            r#"
schema_version: 1
tenant: tenant-a
defaults:
  runtime_scaling_limits:
    max_total_warm: 32
    max_min_warm_total: 8
    max_warm_per_function: 8
    allow_live_scaling: true
workloads:
  - kind: runtime_function
    name: messages:send
    quotas:
      runtime_scaling:
        max_warm: 16
        max_min_warm: 2
"#,
        );

        let plan = policy
            .admit_runtime_scaling(request("messages:send", 2, RuntimeScalingLimit::Auto))
            .expect("request should admit");

        assert_eq!(plan.admitted.min_warm, 2);
        assert_eq!(plan.admitted.max_warm, 8);
        assert!(plan.admitted.live_scaling);
        assert_eq!(
            plan.pressure_adjustment,
            RuntimeScalingAdjustmentReason::OperatorEnvelope
        );
    }

    #[test]
    fn rejects_explicit_min_above_operator_remaining_total() {
        let policy = parse_policy(
            r#"
schema_version: 1
tenant: tenant-a
defaults:
  runtime_scaling_limits:
    max_total_warm: 4
    max_min_warm_total: 2
    max_warm_per_function: 4
workloads:
  - kind: runtime_function
    name: messages:send
"#,
        );

        let error = policy
            .admit_runtime_scaling(request("messages:send", 8, RuntimeScalingLimit::Auto))
            .expect_err("request should reject");

        assert!(error.to_string().contains("requested min_warm=8"));
        assert!(
            error
                .to_string()
                .contains("runtime_scaling_limits.max_min_warm_total")
        );
    }

    #[test]
    fn rejects_explicit_max_above_operator_per_function_limit() {
        let policy = parse_policy(
            r#"
schema_version: 1
tenant: tenant-a
defaults:
  runtime_scaling_limits:
    max_total_warm: 4
    max_min_warm_total: 2
    max_warm_per_function: 4
workloads:
  - kind: runtime_function
    name: messages:send
"#,
        );

        let error = policy
            .admit_runtime_scaling(request("messages:send", 1, RuntimeScalingLimit::Fixed(8)))
            .expect_err("explicit max above operator envelope should reject");

        assert!(error.to_string().contains("requested max_warm=8"));
        assert!(
            error
                .to_string()
                .contains("operator max_warm_per_function=4")
        );
    }

    #[test]
    fn live_scaling_is_operator_gated() {
        let policy = parse_policy(
            r#"
schema_version: 1
tenant: tenant-a
defaults:
  runtime_scaling_limits:
    max_total_warm: 4
    max_min_warm_total: 2
    max_warm_per_function: 4
    allow_live_scaling: false
workloads:
  - kind: runtime_function
    name: messages:send
"#,
        );

        let plan = policy
            .admit_runtime_scaling(request("messages:send", 1, RuntimeScalingLimit::Auto))
            .expect("auto request should admit inside operator envelope");

        assert!(!plan.admitted.live_scaling);
        assert!(!plan.effective.live_scaling);
        assert_eq!(
            plan.pressure_adjustment,
            RuntimeScalingAdjustmentReason::OperatorEnvelope
        );
    }
}
