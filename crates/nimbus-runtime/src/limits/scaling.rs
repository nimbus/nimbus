use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeScalingPreset {
    Economy,
    #[default]
    Warm,
    Latency,
    Fixed,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeScalingLimit {
    #[default]
    Auto,
    Fixed(usize),
}

impl<'de> Deserialize<'de> for RuntimeScalingLimit {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct Visitor;

        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = RuntimeScalingLimit;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("`auto` or an unsigned integer")
            }

            fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                let value = usize::try_from(value)
                    .map_err(|_| E::custom("runtime scaling limit does not fit usize"))?;
                Ok(RuntimeScalingLimit::Fixed(value))
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                match value.trim().to_ascii_lowercase().as_str() {
                    "auto" => Ok(RuntimeScalingLimit::Auto),
                    other => other
                        .parse::<usize>()
                        .map(RuntimeScalingLimit::Fixed)
                        .map_err(|error| {
                            E::custom(format!(
                                "expected `auto` or unsigned integer for runtime scaling limit: {error}"
                            ))
                        }),
                }
            }
        }

        deserializer.deserialize_any(Visitor)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestedRuntimeScalingTarget {
    pub min_warm: usize,
    pub max_warm: RuntimeScalingLimit,
    pub scale_down_delay_secs: u64,
}

impl RequestedRuntimeScalingTarget {
    pub const fn warm_standard() -> Self {
        Self {
            min_warm: 0,
            max_warm: RuntimeScalingLimit::Auto,
            scale_down_delay_secs: 600,
        }
    }

    pub fn inferred_autoscaling(self, preset: RuntimeScalingPreset) -> bool {
        if matches!(preset, RuntimeScalingPreset::Fixed) {
            return false;
        }
        match self.max_warm {
            RuntimeScalingLimit::Auto => true,
            RuntimeScalingLimit::Fixed(max_warm) => self.min_warm != max_warm,
        }
    }
}

impl Default for RequestedRuntimeScalingTarget {
    fn default() -> Self {
        Self::warm_standard()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeScalingTarget {
    pub min_warm: usize,
    pub max_warm: usize,
    pub scale_down_delay_secs: u64,
    pub autoscaling: bool,
}

impl RuntimeScalingTarget {
    pub const fn warm_standard(max_warm: usize) -> Self {
        Self {
            min_warm: 0,
            max_warm,
            scale_down_delay_secs: 600,
            autoscaling: true,
        }
    }
}

impl Default for RuntimeScalingTarget {
    fn default() -> Self {
        Self::warm_standard(4)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeScalingAdjustmentReason {
    #[default]
    None,
    OperatorEnvelope,
    HostPressure,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectiveRuntimeScalingPlan {
    pub function: String,
    pub preset: RuntimeScalingPreset,
    pub requested: RequestedRuntimeScalingTarget,
    pub admitted: RuntimeScalingTarget,
    pub effective: RuntimeScalingTarget,
    pub pressure_adjustment: RuntimeScalingAdjustmentReason,
    pub rejection: Option<String>,
}

impl EffectiveRuntimeScalingPlan {
    pub fn baked_standard(function: impl Into<String>, max_warm: usize) -> Self {
        let requested = RequestedRuntimeScalingTarget::warm_standard();
        let admitted = RuntimeScalingTarget::warm_standard(max_warm);
        Self {
            function: function.into(),
            preset: RuntimeScalingPreset::Warm,
            requested,
            admitted,
            effective: admitted,
            pressure_adjustment: RuntimeScalingAdjustmentReason::None,
            rejection: None,
        }
    }

    pub fn with_pressure_adjustment(
        mut self,
        effective: RuntimeScalingTarget,
        reason: RuntimeScalingAdjustmentReason,
    ) -> Self {
        self.effective = effective;
        self.pressure_adjustment = reason;
        self
    }

    pub fn autoscaling_inferred(&self) -> bool {
        self.requested.inferred_autoscaling(self.preset)
    }
}

impl Default for EffectiveRuntimeScalingPlan {
    fn default() -> Self {
        Self::baked_standard("__default__", 4)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeScalingPlanSet {
    default_plan: EffectiveRuntimeScalingPlan,
    function_overrides: BTreeMap<String, EffectiveRuntimeScalingPlan>,
}

impl RuntimeScalingPlanSet {
    pub fn new(default_plan: EffectiveRuntimeScalingPlan) -> Self {
        Self {
            default_plan,
            function_overrides: BTreeMap::new(),
        }
    }

    pub fn single(plan: EffectiveRuntimeScalingPlan) -> Self {
        Self::new(plan)
    }

    pub fn default_plan(&self) -> &EffectiveRuntimeScalingPlan {
        &self.default_plan
    }

    pub fn insert_function_override(
        &mut self,
        plan: EffectiveRuntimeScalingPlan,
    ) -> Option<EffectiveRuntimeScalingPlan> {
        self.function_overrides.insert(plan.function.clone(), plan)
    }

    pub fn plan_for_function(&self, function_name: &str) -> &EffectiveRuntimeScalingPlan {
        self.function_overrides
            .get(function_name)
            .unwrap_or(&self.default_plan)
    }

    pub fn function_overrides(&self) -> &BTreeMap<String, EffectiveRuntimeScalingPlan> {
        &self.function_overrides
    }

    pub fn function_override_count(&self) -> usize {
        self.function_overrides.len()
    }
}

impl Default for RuntimeScalingPlanSet {
    fn default() -> Self {
        Self::single(EffectiveRuntimeScalingPlan::default())
    }
}
