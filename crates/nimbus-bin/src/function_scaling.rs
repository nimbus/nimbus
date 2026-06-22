use std::collections::BTreeMap;
use std::path::PathBuf;

use nimbus::{
    EffectiveRuntimeScalingPlan, Error, RequestedRuntimeScalingTarget, RuntimeScalingLimit,
    RuntimeScalingPreset,
};
use nimbus_server::{
    OPERATOR_POLICY_SCHEMA_VERSION, OperatorPolicyDefaults, OperatorPolicyDocument,
    OperatorPolicyWorkload, OperatorQuotaPolicy, OperatorRuntimePolicy, OperatorSandboxPolicy,
    OperatorServicePolicy, WorkloadKind,
};
use serde::Deserialize;

use crate::policy::load_policy_document;
use crate::start::{StartCommand, runtime_config_from_start_command};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FunctionScalingContext {
    Dev,
    Start,
}

impl FunctionScalingContext {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Dev => "dev",
            Self::Start => "start",
        }
    }

    fn baked_policy(self) -> RuntimeScalingPolicyBuilder {
        match self {
            Self::Dev => RuntimeScalingPolicyBuilder {
                preset: RuntimeScalingPreset::Warm,
                min_warm: 1,
                activation_warm: 1,
                max_warm: RuntimeScalingLimit::Auto,
                scale_down_delay_secs: 120,
                live_scaling: false,
            },
            Self::Start => RuntimeScalingPolicyBuilder {
                preset: RuntimeScalingPreset::Warm,
                min_warm: 0,
                activation_warm: 1,
                max_warm: RuntimeScalingLimit::Auto,
                scale_down_delay_secs: 600,
                live_scaling: false,
            },
        }
    }

    pub(crate) fn active_recent_min_warm(self) -> usize {
        match self {
            Self::Dev => 1,
            Self::Start => 0,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct NimbusFunctionsFileConfig {
    pub(crate) scaling: FunctionScalingFileConfig,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct FunctionScalingFileConfig {
    pub(crate) default: Option<FunctionScalingPolicyConfig>,
    pub(crate) classes: BTreeMap<String, FunctionScalingPolicyConfig>,
    pub(crate) overrides: BTreeMap<String, FunctionScalingOverrideConfig>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct FunctionScalingPolicyConfig {
    pub(crate) preset: Option<RuntimeScalingPreset>,
    pub(crate) min_warm: Option<usize>,
    pub(crate) activation_warm: Option<usize>,
    pub(crate) max_warm: Option<RuntimeScalingLimit>,
    pub(crate) scale_down_delay: Option<DurationInput>,
    pub(crate) live_scaling: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct FunctionScalingOverrideConfig {
    pub(crate) class: Option<String>,
    pub(crate) preset: Option<RuntimeScalingPreset>,
    pub(crate) min_warm: Option<usize>,
    pub(crate) activation_warm: Option<usize>,
    pub(crate) max_warm: Option<RuntimeScalingLimit>,
    pub(crate) scale_down_delay: Option<DurationInput>,
    pub(crate) live_scaling: Option<bool>,
    pub(crate) reason: Option<String>,
}

impl FunctionScalingOverrideConfig {
    fn policy(&self) -> FunctionScalingPolicyConfig {
        FunctionScalingPolicyConfig {
            preset: self.preset,
            min_warm: self.min_warm,
            activation_warm: self.activation_warm,
            max_warm: self.max_warm,
            scale_down_delay: self.scale_down_delay.clone(),
            live_scaling: self.live_scaling,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub(crate) enum DurationInput {
    Seconds(u64),
    Text(String),
}

impl DurationInput {
    fn as_secs(&self) -> Result<u64, Error> {
        match self {
            Self::Seconds(seconds) => Ok(*seconds),
            Self::Text(value) => parse_duration_seconds(value),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedFunctionScalingIntent {
    pub(crate) request: nimbus_server::TenantRuntimeScalingRequest,
    pub(crate) active_recent_min_warm: usize,
    pub(crate) class: Option<String>,
    pub(crate) reason: Option<String>,
    pub(crate) used_baked_defaults: bool,
}

impl ResolvedFunctionScalingIntent {
    pub(crate) fn boot_summary(&self, context: FunctionScalingContext) -> String {
        format!(
            "Function scaling: {} defaults, active_recent_min_warm={}, max_warm={}. Run nimbus explain functions <name>.",
            context.label(),
            self.active_recent_min_warm,
            scaling_limit_label(self.request.requested.max_warm)
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RuntimeScalingPolicyBuilder {
    preset: RuntimeScalingPreset,
    min_warm: usize,
    activation_warm: usize,
    max_warm: RuntimeScalingLimit,
    scale_down_delay_secs: u64,
    live_scaling: bool,
}

impl RuntimeScalingPolicyBuilder {
    fn apply_preset(&mut self, preset: RuntimeScalingPreset) {
        *self = match preset {
            RuntimeScalingPreset::Economy => Self {
                preset,
                min_warm: 0,
                activation_warm: 0,
                max_warm: RuntimeScalingLimit::Auto,
                scale_down_delay_secs: 60,
                live_scaling: false,
            },
            RuntimeScalingPreset::Warm => Self {
                preset,
                min_warm: 0,
                activation_warm: 1,
                max_warm: RuntimeScalingLimit::Auto,
                scale_down_delay_secs: 600,
                live_scaling: false,
            },
            RuntimeScalingPreset::Latency => Self {
                preset,
                min_warm: 1,
                activation_warm: 1,
                max_warm: RuntimeScalingLimit::Auto,
                scale_down_delay_secs: 900,
                live_scaling: false,
            },
            RuntimeScalingPreset::Fixed => Self {
                preset,
                min_warm: self.min_warm,
                activation_warm: self.activation_warm,
                max_warm: self.max_warm,
                scale_down_delay_secs: self.scale_down_delay_secs,
                live_scaling: self.live_scaling,
            },
        };
    }

    fn apply_config(&mut self, config: &FunctionScalingPolicyConfig) -> Result<(), Error> {
        if let Some(preset) = config.preset {
            self.apply_preset(preset);
        }
        if let Some(min_warm) = config.min_warm {
            self.min_warm = min_warm;
        }
        if let Some(activation_warm) = config.activation_warm {
            self.activation_warm = activation_warm;
        }
        if let Some(max_warm) = config.max_warm {
            self.max_warm = max_warm;
        }
        if let Some(delay) = &config.scale_down_delay {
            self.scale_down_delay_secs = delay.as_secs()?;
        }
        if let Some(live_scaling) = config.live_scaling {
            self.live_scaling = live_scaling;
        }
        Ok(())
    }

    fn request(self, function: &str) -> nimbus_server::TenantRuntimeScalingRequest {
        nimbus_server::TenantRuntimeScalingRequest::new(
            function,
            self.preset,
            RequestedRuntimeScalingTarget {
                min_warm: self.min_warm,
                activation_warm: self.activation_warm,
                max_warm: self.max_warm,
                scale_down_delay_secs: self.scale_down_delay_secs,
                live_scaling: self.live_scaling,
            },
        )
    }
}

pub(crate) fn resolve_function_scaling_intent(
    config: &FunctionScalingFileConfig,
    context: FunctionScalingContext,
    function: &str,
) -> Result<ResolvedFunctionScalingIntent, Error> {
    if function.trim().is_empty() {
        return Err(Error::InvalidInput(
            "function selector cannot be empty".to_string(),
        ));
    }
    let mut builder = context.baked_policy();
    if let Some(default) = &config.default {
        builder.apply_config(default)?;
    }
    let tenant_default = builder;
    let override_config = config.overrides.get(function);
    let used_baked_defaults = config.default.is_none() && override_config.is_none();
    let mut class = None;
    let mut reason = None;
    if let Some(override_config) = override_config {
        if let Some(class_name) = &override_config.class {
            let class_config = config.classes.get(class_name).ok_or_else(|| {
                Error::InvalidInput(format!(
                    "functions.scaling.overrides[\"{function}\"].class references unknown class `{class_name}`"
                ))
            })?;
            builder.apply_config(class_config)?;
            class = Some(class_name.clone());
        }
        let before_override = builder;
        let override_policy = override_config.policy();
        builder.apply_config(&override_policy)?;
        reason = override_config.reason.clone();
        validate_override_reason(
            function,
            tenant_default,
            before_override,
            builder,
            reason.as_deref(),
        )?;
    }
    validate_fixed_preset(function, builder)?;
    Ok(ResolvedFunctionScalingIntent {
        request: builder.request(function),
        active_recent_min_warm: context.active_recent_min_warm(),
        class,
        reason,
        used_baked_defaults,
    })
}

pub(crate) fn known_function_selectors(config: &FunctionScalingFileConfig) -> Vec<String> {
    let mut names: Vec<_> = config.overrides.keys().cloned().collect();
    names.sort();
    names
}

pub(crate) fn load_config(
    config: Option<PathBuf>,
) -> nimbus::Result<crate::start::RuntimeConfigFile> {
    runtime_config_from_start_command(&StartCommand {
        config,
        ..StartCommand::default()
    })
}

pub(crate) fn load_optional_policy(
    policy: Option<PathBuf>,
) -> nimbus::Result<Option<OperatorPolicyDocument>> {
    policy.map(|path| load_policy_document(&path)).transpose()
}

pub(crate) fn policy_for_function(
    policy: Option<&OperatorPolicyDocument>,
    tenant: Option<&str>,
    function: &str,
) -> OperatorPolicyDocument {
    policy
        .cloned()
        .unwrap_or_else(|| default_policy_for_function(tenant, function))
}

pub(crate) fn default_policy_for_function(
    tenant: Option<&str>,
    function: &str,
) -> OperatorPolicyDocument {
    OperatorPolicyDocument {
        schema_version: OPERATOR_POLICY_SCHEMA_VERSION,
        tenant: tenant.unwrap_or("tenant-a").to_string(),
        metadata: Default::default(),
        accepted_risks: Vec::new(),
        defaults: OperatorPolicyDefaults::default(),
        workloads: vec![OperatorPolicyWorkload {
            kind: WorkloadKind::RuntimeFunction,
            name: function.to_string(),
            runtime: OperatorRuntimePolicy::default(),
            sandbox: OperatorSandboxPolicy::default(),
            services: OperatorServicePolicy::default(),
            network: Default::default(),
            storage: Default::default(),
            volumes: Default::default(),
            image: Default::default(),
            secrets: Default::default(),
            quotas: OperatorQuotaPolicy::default(),
            audit: Default::default(),
        }],
    }
}

pub(crate) fn render_resolved_effective_plan(
    intent: &ResolvedFunctionScalingIntent,
    plan: &EffectiveRuntimeScalingPlan,
    policy: &OperatorPolicyDocument,
) -> String {
    let source = scaling_source_label(intent);
    let class = intent.class.as_deref().unwrap_or("none");
    let reason = intent.reason.as_deref().unwrap_or("none");
    format!(
        "Config source: {source}; class={class}; reason={reason}\n{}",
        render_effective_plan(plan, policy)
    )
}

pub(crate) fn render_effective_plan(
    plan: &EffectiveRuntimeScalingPlan,
    policy: &OperatorPolicyDocument,
) -> String {
    let limits = policy.defaults.runtime_scaling_limits;
    format!(
        "Tenant request: {} preset={:?} min_warm={} max_warm={}\nOperator envelope: max_warm_per_function={} max_min_warm_total remaining={} max_total_warm={} allow_live_scaling={}\nEffective: min_warm={} activation_warm={} max_warm={} pressure_adjustment={:?}\n",
        plan.function,
        plan.preset,
        plan.requested.min_warm,
        scaling_limit_label(plan.requested.max_warm),
        limits.max_warm_per_function,
        limits.max_min_warm_total,
        limits.max_total_warm,
        limits.allow_live_scaling,
        plan.effective.min_warm,
        plan.effective.activation_warm,
        plan.effective.max_warm,
        plan.pressure_adjustment
    )
}

pub(crate) fn scaling_source_label(intent: &ResolvedFunctionScalingIntent) -> &'static str {
    if intent.used_baked_defaults {
        "baked default"
    } else if intent.class.is_some() || intent.reason.is_some() {
        "function override"
    } else {
        "tenant default"
    }
}

fn validate_override_reason(
    function: &str,
    tenant_default: RuntimeScalingPolicyBuilder,
    before_override: RuntimeScalingPolicyBuilder,
    after_override: RuntimeScalingPolicyBuilder,
    reason: Option<&str>,
) -> Result<(), Error> {
    let increases_min = after_override.min_warm > tenant_default.min_warm
        || after_override.min_warm > before_override.min_warm;
    let increases_max = match (
        tenant_default.max_warm,
        before_override.max_warm,
        after_override.max_warm,
    ) {
        (_, _, RuntimeScalingLimit::Auto) => false,
        (RuntimeScalingLimit::Fixed(tenant), _, RuntimeScalingLimit::Fixed(after))
            if after > tenant =>
        {
            true
        }
        (_, RuntimeScalingLimit::Fixed(before), RuntimeScalingLimit::Fixed(after))
            if after > before =>
        {
            true
        }
        _ => false,
    };
    if (increases_min || increases_max) && reason.map(str::trim).unwrap_or_default().is_empty() {
        return Err(Error::InvalidInput(format!(
            "functions.scaling.overrides[\"{function}\"].reason is required when increasing min_warm or max_warm above the tenant default"
        )));
    }
    Ok(())
}

fn validate_fixed_preset(
    function: &str,
    builder: RuntimeScalingPolicyBuilder,
) -> Result<(), Error> {
    if !matches!(builder.preset, RuntimeScalingPreset::Fixed) {
        return Ok(());
    }
    let RuntimeScalingLimit::Fixed(max_warm) = builder.max_warm else {
        return Err(Error::InvalidInput(format!(
            "functions.scaling override for `{function}` uses preset=fixed but does not set explicit max_warm"
        )));
    };
    if builder.min_warm != max_warm {
        return Err(Error::InvalidInput(format!(
            "functions.scaling override for `{function}` uses preset=fixed but min_warm={} and max_warm={max_warm}; fixed requires min_warm == max_warm",
            builder.min_warm
        )));
    }
    Ok(())
}

fn parse_duration_seconds(value: &str) -> Result<u64, Error> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(Error::InvalidInput(
            "scale_down_delay cannot be empty".to_string(),
        ));
    }
    let (number, multiplier) = match trimmed.chars().last() {
        Some('s') | Some('S') => (&trimmed[..trimmed.len() - 1], 1),
        Some('m') | Some('M') => (&trimmed[..trimmed.len() - 1], 60),
        Some('h') | Some('H') => (&trimmed[..trimmed.len() - 1], 60 * 60),
        _ => (trimmed, 1),
    };
    let amount = number.parse::<u64>().map_err(|error| {
        Error::InvalidInput(format!(
            "failed to parse scale_down_delay `{value}` as seconds/minutes/hours: {error}"
        ))
    })?;
    amount
        .checked_mul(multiplier)
        .ok_or_else(|| Error::InvalidInput(format!("scale_down_delay `{value}` is too large")))
}

pub(crate) fn scaling_limit_label(limit: RuntimeScalingLimit) -> String {
    match limit {
        RuntimeScalingLimit::Auto => "auto".to_string(),
        RuntimeScalingLimit::Fixed(value) => value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(body: &str) -> NimbusFunctionsFileConfig {
        serde_yaml::from_str(body).expect("functions config should parse")
    }

    #[test]
    fn no_yaml_dev_uses_baked_showcase_default() {
        let config = FunctionScalingFileConfig::default();
        let resolved =
            resolve_function_scaling_intent(&config, FunctionScalingContext::Dev, "messages:send")
                .expect("default should resolve");

        assert!(resolved.used_baked_defaults);
        assert_eq!(resolved.request.requested.min_warm, 1);
        assert_eq!(resolved.active_recent_min_warm, 1);
        assert_eq!(
            resolved.request.requested.max_warm,
            RuntimeScalingLimit::Auto
        );
    }

    #[test]
    fn no_yaml_start_uses_measured_standard_default() {
        let config = FunctionScalingFileConfig::default();
        let resolved = resolve_function_scaling_intent(
            &config,
            FunctionScalingContext::Start,
            "messages:send",
        )
        .expect("default should resolve");

        assert!(resolved.used_baked_defaults);
        assert_eq!(resolved.request.requested.min_warm, 0);
        assert_eq!(resolved.active_recent_min_warm, 0);
        assert_eq!(resolved.request.requested.activation_warm, 1);
        assert_eq!(
            resolved.request.requested.max_warm,
            RuntimeScalingLimit::Auto
        );
    }

    #[test]
    fn unrelated_classes_do_not_hide_baked_default_source() {
        let config = parse(
            r#"
scaling:
  classes:
    hot-write:
      preset: latency
      min_warm: 2
"#,
        );

        let resolved = resolve_function_scaling_intent(
            &config.scaling,
            FunctionScalingContext::Start,
            "messages:send",
        )
        .expect("unoverridden function should resolve");

        assert!(
            resolved.used_baked_defaults,
            "unused classes should not make an unconfigured function look tenant-overridden"
        );
        assert_eq!(resolved.request.requested.min_warm, 0);
    }

    #[test]
    fn selectors_do_not_fan_out_to_theoretical_authority_keys() {
        let config = parse(
            r#"
scaling:
  overrides:
    "messages:send":
      preset: latency
      reason: "primary write path"
"#,
        );

        assert_eq!(
            known_function_selectors(&config.scaling),
            vec!["messages:send".to_string()],
            "public scaling intent must list concrete function overrides only, not tenant/script/grant authority-key variants"
        );
    }

    #[test]
    fn unknown_function_scaling_shapes_reject_actionably() {
        let error = serde_yaml::from_str::<NimbusFunctionsFileConfig>(
            r#"
scaling:
  default:
    pool_kind: isolate
"#,
        )
        .expect_err("unknown public runtime-internal field should reject");

        assert!(
            error.to_string().contains("pool_kind"),
            "error should name the rejected field: {error}"
        );
    }

    #[test]
    fn preset_class_and_override_merge_in_order() {
        let config = parse(
            r#"
scaling:
  default:
    preset: warm
    max_warm: auto
  classes:
    hot-write:
      preset: latency
      min_warm: 2
      max_warm: 8
      scale_down_delay: 15m
  overrides:
    "messages:send":
      class: hot-write
      max_warm: 16
      reason: "hot write path"
"#,
        );

        let resolved = resolve_function_scaling_intent(
            &config.scaling,
            FunctionScalingContext::Start,
            "messages:send",
        )
        .expect("override should resolve");

        assert_eq!(resolved.class.as_deref(), Some("hot-write"));
        assert_eq!(resolved.reason.as_deref(), Some("hot write path"));
        assert_eq!(resolved.request.preset, RuntimeScalingPreset::Latency);
        assert_eq!(resolved.request.requested.min_warm, 2);
        assert_eq!(
            resolved.request.requested.max_warm,
            RuntimeScalingLimit::Fixed(16)
        );
        assert_eq!(resolved.request.requested.scale_down_delay_secs, 900);
    }

    #[test]
    fn override_increase_requires_reason() {
        let config = parse(
            r#"
scaling:
  default:
    preset: warm
  overrides:
    "messages:send":
      preset: latency
"#,
        );

        let error = resolve_function_scaling_intent(
            &config.scaling,
            FunctionScalingContext::Start,
            "messages:send",
        )
        .expect_err("missing reason should reject");

        assert!(error.to_string().contains("reason is required"));
    }
}
