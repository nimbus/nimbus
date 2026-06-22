use nimbus_core::{Error, Result};
use nimbus_runtime::{
    EffectiveRuntimeScalingPlan, RequestedRuntimeScalingTarget, RuntimeScalingAdjustmentReason,
    RuntimeScalingLimit, RuntimeScalingPreset, RuntimeScalingTarget,
};
use serde::{Deserialize, Serialize};

use super::{OperatorPolicyDocument, OperatorPolicyWorkload};
use crate::WorkloadKind;

const DERIVED_RUNTIME_SEAT_MILLICPUS: usize = 250;
const DERIVED_RETAINED_RUNTIME_RSS_BYTES: u64 = 64 * 1024 * 1024;

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

    pub fn autoscaling_inferred(&self) -> bool {
        self.requested.inferred_autoscaling(self.preset)
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
    derived_from_resources: usize,
    max_total_warm: usize,
    max_min_warm_total: usize,
    max_warm_per_function: usize,
}

impl RuntimeScalingEnvelope {
    fn from_policy(policy: &OperatorPolicyDocument, function: &str) -> Self {
        let resources = policy.defaults.runtime_resources;
        let safety = policy.defaults.runtime_safety;
        let workload = policy.workloads.iter().find(|workload| {
            workload.kind == WorkloadKind::RuntimeFunction && workload.name == function
        });
        Self::from_workload(resources, safety, workload)
    }

    fn from_workload(
        resources: super::OperatorRuntimeResourceEnvelope,
        safety: super::OperatorRuntimeSafetyCaps,
        workload: Option<&OperatorPolicyWorkload>,
    ) -> Self {
        let derived_from_resources = derived_from_resources(resources);
        let max_total_warm = safety
            .max_total_warm
            .unwrap_or(derived_from_resources)
            .min(derived_from_resources);
        let quota = workload.map(|workload| workload.quotas.runtime_scaling);
        let max_warm_per_function = quota
            .and_then(|quota| quota.max_warm)
            .or(safety.max_warm_per_function)
            .unwrap_or(max_total_warm)
            .min(max_total_warm);
        let max_min_warm_total = safety
            .max_min_warm_total
            .unwrap_or(max_total_warm)
            .min(max_total_warm);
        let max_min_warm = quota
            .and_then(|quota| quota.max_min_warm)
            .unwrap_or(max_min_warm_total)
            .min(max_min_warm_total)
            .min(max_warm_per_function);
        Self {
            derived_from_resources,
            max_total_warm,
            max_min_warm_total: max_min_warm,
            max_warm_per_function,
        }
    }

    fn admit(self, request: TenantRuntimeScalingRequest) -> Result<EffectiveRuntimeScalingPlan> {
        let requested = request.requested;
        if requested.min_warm > self.max_min_warm_total {
            return Err(Error::InvalidInput(format!(
                "{} rejected: requested min_warm={} exceeds operator effective max_min_warm_total remaining={}; lower min_warm to <= {} or ask an operator to raise tenant runtime resources or runtime_safety.max_min_warm_total",
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
                        "{} rejected: requested max_warm={} exceeds operator effective max_warm_per_function={}; lower max_warm to <= {} or ask an operator to raise tenant runtime resources or quotas.runtime_scaling.max_warm",
                        request.function,
                        value,
                        self.max_warm_per_function,
                        self.max_warm_per_function
                    )));
                }
                value
            }
        };
        if requested.min_warm > admitted_max {
            return Err(Error::InvalidInput(format!(
                "{} rejected: requested min_warm={} exceeds admitted max_warm={}",
                request.function, requested.min_warm, admitted_max
            )));
        }

        let requested_autoscaling = request.autoscaling_inferred();
        let admitted_autoscaling = requested_autoscaling && admitted_max > requested.min_warm;
        let admitted = RuntimeScalingTarget {
            min_warm: requested.min_warm,
            max_warm: admitted_max,
            scale_down_delay_secs: requested.scale_down_delay_secs,
            autoscaling: admitted_autoscaling,
        };
        let pressure_adjustment = if requested.max_warm == RuntimeScalingLimit::Auto
            || requested_autoscaling != admitted_autoscaling
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

fn derived_from_resources(resources: super::OperatorRuntimeResourceEnvelope) -> usize {
    let allocatable_millicpus = resources
        .cpu_millicpus
        .saturating_sub(resources.host_cpu_reserve_millicpus);
    let cpu_limit = allocatable_millicpus
        .checked_div(DERIVED_RUNTIME_SEAT_MILLICPUS)
        .unwrap_or(0);
    let allocatable_memory = resources
        .memory_bytes
        .saturating_sub(resources.host_memory_reserve_bytes);
    let memory_limit = allocatable_memory
        .checked_div(DERIVED_RETAINED_RUNTIME_RSS_BYTES)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(0);
    cpu_limit.min(memory_limit).max(1)
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
                max_warm,
                scale_down_delay_secs: 600,
            },
        )
    }

    #[test]
    fn admits_auto_inside_resource_derived_operator_envelope() {
        let policy = parse_policy(
            r#"
schema_version: 1
tenant: tenant-a
defaults:
  runtime_resources:
    cpu_millicpus: 2000
    memory_bytes: 1073741824
    storage_bytes: 10737418240
    host_cpu_reserve_millicpus: 500
    host_memory_reserve_bytes: 268435456
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
        assert_eq!(plan.admitted.max_warm, 6);
        assert!(plan.admitted.autoscaling);
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
  runtime_resources:
    cpu_millicpus: 1000
    memory_bytes: 536870912
    storage_bytes: 10737418240
    host_cpu_reserve_millicpus: 250
    host_memory_reserve_bytes: 134217728
  runtime_safety:
    max_min_warm_total: 2
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
                .contains("operator effective max_min_warm_total")
        );
    }

    #[test]
    fn rejects_explicit_max_above_operator_per_function_limit() {
        let policy = parse_policy(
            r#"
schema_version: 1
tenant: tenant-a
defaults:
  runtime_resources:
    cpu_millicpus: 1000
    memory_bytes: 536870912
    storage_bytes: 10737418240
    host_cpu_reserve_millicpus: 250
    host_memory_reserve_bytes: 134217728
  runtime_safety:
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
                .contains("operator effective max_warm_per_function=3")
        );
    }

    #[test]
    fn fixed_range_disables_admitted_autoscaling() {
        let policy = parse_policy(
            r#"
schema_version: 1
tenant: tenant-a
defaults:
  runtime_resources:
    cpu_millicpus: 1000
    memory_bytes: 536870912
    storage_bytes: 10737418240
    host_cpu_reserve_millicpus: 250
    host_memory_reserve_bytes: 134217728
workloads:
  - kind: runtime_function
    name: messages:send
"#,
        );

        let plan = policy
            .admit_runtime_scaling(request("messages:send", 2, RuntimeScalingLimit::Fixed(2)))
            .expect("fixed range should admit inside operator envelope");

        assert!(!plan.autoscaling_inferred());
        assert!(!plan.admitted.autoscaling);
        assert!(!plan.effective.autoscaling);
        assert_eq!(
            plan.pressure_adjustment,
            RuntimeScalingAdjustmentReason::None
        );
    }
}
