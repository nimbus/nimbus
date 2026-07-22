use clap::{Args, ValueEnum};
use nimbus::{
    EGRESS_ENFORCEMENT_ENV, EgressEnforcementMode, EgressEnforcementPlan, EgressReloadPolicy,
};
use serde::Serialize;

#[derive(Debug, Args)]
#[command(hide = true)]
pub(crate) struct SandboxSupervisorCommand {
    /// Output format.
    #[arg(short = 'f', long, default_value = "text")]
    format: SandboxSupervisorOutputFormat,
}

#[derive(Debug, Clone, Copy, Default, ValueEnum)]
enum SandboxSupervisorOutputFormat {
    #[default]
    Text,
    Json,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct SandboxSupervisorContractReport {
    status: &'static str,
    schema_version: u32,
    mode: EgressEnforcementMode,
    reload_policy: EgressReloadPolicy,
    rule_count: usize,
}

pub(crate) async fn run_sandbox_supervisor_command(
    command: SandboxSupervisorCommand,
) -> nimbus::Result<()> {
    let report = evaluate_sandbox_supervisor_command(&command, |key| std::env::var(key).ok())?;
    match command.format {
        SandboxSupervisorOutputFormat::Text => print!("{}", report.render_text()),
        SandboxSupervisorOutputFormat::Json => print_json(&report)?,
    }
    Ok(())
}

fn evaluate_sandbox_supervisor_command(
    _command: &SandboxSupervisorCommand,
    env: impl FnOnce(&str) -> Option<String>,
) -> nimbus::Result<SandboxSupervisorContractReport> {
    let plan = load_enforcement_plan(env)?;
    SandboxSupervisorContractReport::from_plan(&plan)
}

fn load_enforcement_plan(
    env: impl FnOnce(&str) -> Option<String>,
) -> nimbus::Result<EgressEnforcementPlan> {
    let raw = env(EGRESS_ENFORCEMENT_ENV).ok_or_else(|| {
        nimbus::Error::InvalidInput(format!(
            "sandbox supervisor requires {EGRESS_ENFORCEMENT_ENV}"
        ))
    })?;
    serde_json::from_str(&raw).map_err(|error| {
        nimbus::Error::InvalidInput(format!(
            "failed to parse sandbox supervisor enforcement contract from {EGRESS_ENFORCEMENT_ENV}: {error}"
        ))
    })
}

impl SandboxSupervisorContractReport {
    fn from_plan(plan: &EgressEnforcementPlan) -> nimbus::Result<Self> {
        plan.validate().map_err(|message| {
            nimbus::Error::InvalidInput(format!(
                "invalid sandbox supervisor enforcement contract: {message}"
            ))
        })?;
        Ok(Self {
            status: "valid",
            schema_version: plan.schema_version,
            mode: plan.mode,
            reload_policy: plan.reload_policy,
            rule_count: plan.policy().rules().len(),
        })
    }

    fn render_text(&self) -> String {
        format!(
            "Sandbox supervisor contract: {status}\nschema_version: {schema_version}\nmode: {mode}\nreload_policy: {reload_policy}\nrules: {rule_count}\n",
            status = self.status,
            schema_version = self.schema_version,
            mode = egress_mode_label(self.mode),
            reload_policy = reload_policy_label(self.reload_policy),
            rule_count = self.rule_count,
        )
    }
}

fn print_json(value: &impl Serialize) -> nimbus::Result<()> {
    let json = serde_json::to_string_pretty(value).map_err(|error| {
        nimbus::Error::Serialization(format!(
            "failed to serialize sandbox supervisor output: {error}"
        ))
    })?;
    println!("{json}");
    Ok(())
}

fn egress_mode_label(mode: EgressEnforcementMode) -> &'static str {
    match mode {
        EgressEnforcementMode::SupervisorProxy => "supervisor_proxy",
    }
}

fn reload_policy_label(reload_policy: EgressReloadPolicy) -> &'static str {
    match reload_policy {
        EgressReloadPolicy::RecreateRequired => "recreate_required",
        EgressReloadPolicy::LiveReload => "live_reload",
    }
}

#[cfg(test)]
mod tests {
    use nimbus::{
        EGRESS_ENFORCEMENT_ENV, EGRESS_ENFORCEMENT_SCHEMA_VERSION, EgressEnforcementMode,
        EgressEnforcementPlan, EgressPolicy, EgressReloadPolicy,
    };

    use super::{
        SandboxSupervisorCommand, SandboxSupervisorContractReport, SandboxSupervisorOutputFormat,
        evaluate_sandbox_supervisor_command, load_enforcement_plan,
    };

    #[test]
    fn load_enforcement_plan_reads_packaged_env_contract() {
        let contract = serde_json::json!({
            "schema_version": EGRESS_ENFORCEMENT_SCHEMA_VERSION,
            "mode": "supervisor_proxy",
            "reload_policy": "recreate_required",
            "policy": {
                "allow": [{
                    "name": "stripe",
                    "protocol": "https",
                    "host": "api.stripe.com",
                    "port": 443,
                    "methods": ["POST"],
                    "path_prefixes": ["/v1/"]
                }]
            }
        })
        .to_string();

        let plan = load_enforcement_plan(|key| (key == EGRESS_ENFORCEMENT_ENV).then_some(contract))
            .expect("env contract should parse");

        assert_eq!(plan.mode, EgressEnforcementMode::SupervisorProxy);
        assert_eq!(plan.reload_policy, EgressReloadPolicy::RecreateRequired);
        assert_eq!(plan.policy().rules()[0].name, "stripe");
    }

    #[test]
    fn load_enforcement_plan_rejects_missing_env_contract() {
        let error =
            load_enforcement_plan(|_| None).expect_err("missing contract should fail closed");

        assert!(
            error.to_string().contains(EGRESS_ENFORCEMENT_ENV),
            "missing contract error should name the required env var: {error}"
        );
    }

    #[test]
    fn supervisor_command_rejects_removed_launch_metadata_mode() {
        let contract = serde_json::json!({
            "schema_version": EGRESS_ENFORCEMENT_SCHEMA_VERSION,
            "mode": "launch_metadata",
            "reload_policy": "recreate_required",
            "policy": {}
        })
        .to_string();
        let command = SandboxSupervisorCommand {
            format: SandboxSupervisorOutputFormat::Json,
        };

        let error = evaluate_sandbox_supervisor_command(&command, |key| {
            (key == EGRESS_ENFORCEMENT_ENV).then_some(contract)
        })
        .expect_err("removed launch-metadata mode should fail closed");

        assert!(
            error
                .to_string()
                .contains("unknown variant `launch_metadata`"),
            "removed contract mode should surface the validation reason: {error}"
        );
    }

    #[test]
    fn supervisor_command_evaluates_env_backed_contract() {
        let contract = serde_json::json!({
            "schema_version": EGRESS_ENFORCEMENT_SCHEMA_VERSION,
            "mode": "supervisor_proxy",
            "reload_policy": "recreate_required",
            "policy": {
                "allow": [{
                    "name": "github",
                    "protocol": "https",
                    "host": "api.github.com",
                    "port": 443
                }]
            }
        })
        .to_string();
        let command = SandboxSupervisorCommand {
            format: SandboxSupervisorOutputFormat::Json,
        };

        let report = evaluate_sandbox_supervisor_command(&command, |key| {
            (key == EGRESS_ENFORCEMENT_ENV).then_some(contract)
        })
        .expect("command should consume env-backed supervisor contract");

        assert_eq!(report.status, "valid");
        assert_eq!(report.rule_count, 1);
        assert_eq!(report.mode, EgressEnforcementMode::SupervisorProxy);
    }

    #[test]
    fn supervisor_contract_report_is_validation_only_until_packet_path_lands() {
        let plan = EgressEnforcementPlan::supervisor_proxy(
            &EgressPolicy::deny_all()
                .compile()
                .expect("deny-all should compile"),
            EgressReloadPolicy::RecreateRequired,
        );

        let report =
            SandboxSupervisorContractReport::from_plan(&plan).expect("report should render");

        assert_eq!(report.status, "valid");
        assert_eq!(report.mode, EgressEnforcementMode::SupervisorProxy);
        assert!(report.render_text().contains("mode: supervisor_proxy"));
    }
}
