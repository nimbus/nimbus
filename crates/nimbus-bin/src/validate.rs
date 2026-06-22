use std::path::PathBuf;

use clap::{Args, Subcommand};

use crate::function_scaling::{
    FunctionScalingAdmissionEnvelope, FunctionScalingContext, admit_function_scaling_intent,
    known_function_selectors, load_config, load_optional_policy, resolve_function_scaling_intent,
};
use crate::policy::{PolicyFileCommand, PolicyOutputFormat, run_policy_command};

#[derive(Debug, Args)]
pub(crate) struct ValidateCommand {
    #[command(subcommand)]
    resource: Option<ValidateResource>,
}

#[derive(Debug, Subcommand)]
enum ValidateResource {
    /// Validate function scaling app intent and quota fit.
    Functions(ValidateFunctionsCommand),
    /// Validate a Nimbus operator policy file.
    Policy(ValidatePolicyCommand),
}

#[derive(Debug, Args)]
struct ValidateFunctionsCommand {
    /// Path to nimbus.yaml / nimbus.json.
    #[arg(long)]
    config: Option<PathBuf>,
    /// Path to nimbus.policy.yaml for operator quota admission.
    #[arg(long, value_name = "FILE")]
    policy: Option<PathBuf>,
    /// Tenant label to use when building the default operator envelope.
    #[arg(long, conflicts_with = "policy")]
    tenant: Option<String>,
}

#[derive(Debug, Args)]
struct ValidatePolicyCommand {
    /// Path to a nimbus.policy.yaml file.
    #[arg(long, value_name = "FILE", default_value = "nimbus.policy.yaml")]
    file: PathBuf,
}

pub(crate) async fn run_validate_command(command: ValidateCommand) -> nimbus::Result<()> {
    match command.resource {
        Some(ValidateResource::Functions(command)) => run_validate_functions(command),
        Some(ValidateResource::Policy(command)) => {
            run_policy_command(nimbus_policy_validate_command(command)).await
        }
        None => {
            run_validate_functions(ValidateFunctionsCommand {
                config: None,
                policy: None,
                tenant: None,
            })?;
            Ok(())
        }
    }
}

fn run_validate_functions(command: ValidateFunctionsCommand) -> nimbus::Result<()> {
    let config = load_config(command.config)?;
    let policy = load_optional_policy(command.policy)?;
    let mut selectors = known_function_selectors(&config.functions.scaling);
    if selectors.is_empty() {
        selectors.push("__default__".to_string());
    }
    for selector in &selectors {
        let intent = resolve_function_scaling_intent(
            &config.functions.scaling,
            FunctionScalingContext::Start,
            selector,
        )?;
        admit_function_scaling_intent(
            &intent,
            policy.as_ref(),
            command.tenant.as_deref(),
            FunctionScalingAdmissionEnvelope::default(),
        )?;
    }
    println!(
        "Function scaling validation: allowed ({} function plan{})",
        selectors.len(),
        if selectors.len() == 1 { "" } else { "s" }
    );
    Ok(())
}

fn nimbus_policy_validate_command(command: ValidatePolicyCommand) -> crate::policy::PolicyCommand {
    crate::policy::PolicyCommand::Validate(PolicyFileCommand {
        file: command.file,
        format: PolicyOutputFormat::Text,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn validate_functions_uses_explicit_operator_policy_for_quota_admission() {
        let temp = tempfile::tempdir().expect("tempdir should build");
        let config_path = temp.path().join("nimbus.yaml");
        let policy_path = temp.path().join("nimbus.policy.yaml");
        fs::write(
            &config_path,
            r#"
functions:
  scaling:
    overrides:
      "messages:send":
        preset: latency
        min_warm: 2
        reason: "hot write path"
"#,
        )
        .expect("config fixture should write");
        fs::write(
            &policy_path,
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
    max_min_warm_total: 1
workloads:
  - kind: runtime_function
    name: messages:send
"#,
        )
        .expect("policy fixture should write");

        let error = run_validate_functions(ValidateFunctionsCommand {
            config: Some(config_path),
            policy: Some(policy_path),
            tenant: None,
        })
        .expect_err("operator policy should reject over-limit min_warm");

        assert!(error.to_string().contains("requested min_warm=2"));
        assert!(
            error
                .to_string()
                .contains("runtime_safety.max_min_warm_total")
        );
    }
}
