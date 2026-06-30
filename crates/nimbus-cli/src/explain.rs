use std::path::PathBuf;

use clap::{Args, Subcommand};
use nimbus::Error;

use crate::function_scaling::{
    FunctionScalingAdmissionEnvelope, FunctionScalingContext, FunctionScalingFileConfig,
    admit_function_scaling_intent, known_function_selectors, load_config, load_optional_policy,
    policy_for_function, render_resolved_effective_plan, resolve_function_scaling_intent,
    scaling_limit_label, scaling_source_label,
};

#[derive(Debug, Args)]
pub(crate) struct ExplainCommand {
    #[command(subcommand)]
    resource: ExplainResource,
}

#[derive(Debug, Subcommand)]
enum ExplainResource {
    /// Explain effective function scaling.
    Functions(ExplainFunctionsCommand),
    /// Explain resolved project configuration.
    Config(ExplainConfigCommand),
}

#[derive(Debug, Args)]
struct ExplainFunctionsCommand {
    /// Function selector such as messages:send.
    selector: Option<String>,
    /// Explain every known function override in nimbus.yaml.
    #[arg(long, conflicts_with = "selector")]
    all: bool,
    /// Tenant label to show in diagnostics.
    #[arg(long, conflicts_with = "policy")]
    tenant: Option<String>,
    /// Path to nimbus.yaml / nimbus.json.
    #[arg(long)]
    config: Option<PathBuf>,
    /// Path to nimbus.policy.yaml.
    #[arg(long)]
    policy: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct ExplainConfigCommand {
    /// Config path to explain, currently functions.scaling.
    path: String,
    /// Path to nimbus.yaml / nimbus.json.
    #[arg(long)]
    config: Option<PathBuf>,
}

pub(crate) async fn run_explain_command(command: ExplainCommand) -> nimbus::Result<()> {
    match command.resource {
        ExplainResource::Functions(command) => run_explain_functions(command),
        ExplainResource::Config(command) => run_explain_config(command),
    }
}

fn run_explain_functions(command: ExplainFunctionsCommand) -> nimbus::Result<()> {
    let config = load_config(command.config)?;
    let selectors = if command.all {
        let names = known_function_selectors(&config.functions.scaling);
        if names.is_empty() {
            vec!["__default__".to_string()]
        } else {
            names
        }
    } else {
        vec![command.selector.ok_or_else(|| {
            Error::InvalidInput(
                "nimbus explain functions requires a function selector or --all".to_string(),
            )
        })?]
    };
    let policy = load_optional_policy(command.policy)?;
    for selector in selectors {
        let intent = resolve_function_scaling_intent(
            &config.functions.scaling,
            FunctionScalingContext::Start,
            &selector,
        )?;
        let policy = policy_for_function(policy.as_ref(), command.tenant.as_deref(), &selector);
        let plan = admit_function_scaling_intent(
            &intent,
            Some(&policy),
            command.tenant.as_deref(),
            FunctionScalingAdmissionEnvelope::default(),
        )?;
        print!(
            "{}",
            render_resolved_effective_plan(&intent, &plan, &policy)
        );
    }
    Ok(())
}

fn run_explain_config(command: ExplainConfigCommand) -> nimbus::Result<()> {
    if command.path != "functions.scaling" {
        return Err(Error::InvalidInput(format!(
            "unsupported config explain path `{}`; expected functions.scaling",
            command.path
        )));
    }
    let config = load_config(command.config)?;
    let default = resolve_function_scaling_intent(
        &config.functions.scaling,
        FunctionScalingContext::Start,
        "__default__",
    )?;
    println!(
        "Config functions.scaling:\n  baked default: preset=warm min_warm=0 max_warm=auto scale_down_delay=600s autoscaling: inferred=true\n  tenant default present: {}\n  classes: {}\n  function overrides: {}\n  effective default source: {}\n  effective default: preset={:?} min_warm={} max_warm={} scale_down_delay={}s autoscaling: inferred={}",
        config.functions.scaling.default.is_some(),
        config.functions.scaling.classes.len(),
        config.functions.scaling.overrides.len(),
        scaling_source_label(&default),
        default.request.preset,
        default.request.requested.min_warm,
        scaling_limit_label(default.request.requested.max_warm),
        default.request.requested.scale_down_delay_secs,
        default.request.autoscaling_inferred()
    );
    Ok(())
}

#[allow(dead_code)]
fn _assert_scaling_config_send_sync(_: &FunctionScalingFileConfig) {}
