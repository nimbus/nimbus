use clap::{Args, ValueEnum};
use nimbus::{
    SANDBOX_EGRESS_ENFORCEMENT_ENV, SandboxEgressEnforcementMode, SandboxEgressEnforcementPlan,
    SandboxEgressReloadPolicy,
};
use serde::Serialize;

#[derive(Debug, Args)]
#[command(hide = true)]
pub(crate) struct SandboxSupervisorCommand {
    /// Inline enforcement contract JSON. Intended for tests and diagnostics.
    #[arg(long, value_name = "JSON", hide = true)]
    contract_json: Option<String>,

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
    mode: SandboxEgressEnforcementMode,
    reload_policy: SandboxEgressReloadPolicy,
    rule_count: usize,
    packet_enforcement_active: bool,
}

pub(crate) async fn run_sandbox_supervisor_command(
    command: SandboxSupervisorCommand,
) -> nimbus::Result<()> {
    let plan = load_enforcement_plan(command.contract_json.as_deref(), |key| {
        std::env::var(key).ok()
    })?;
    let report = SandboxSupervisorContractReport::from_plan(&plan)?;
    match command.format {
        SandboxSupervisorOutputFormat::Text => print!("{}", report.render_text()),
        SandboxSupervisorOutputFormat::Json => print_json(&report)?,
    }
    Ok(())
}

fn load_enforcement_plan(
    contract_json: Option<&str>,
    env: impl FnOnce(&str) -> Option<String>,
) -> nimbus::Result<SandboxEgressEnforcementPlan> {
    let (source, raw) = match contract_json {
        Some(raw) => ("--contract-json", raw.to_owned()),
        None => (
            SANDBOX_EGRESS_ENFORCEMENT_ENV,
            env(SANDBOX_EGRESS_ENFORCEMENT_ENV).ok_or_else(|| {
                nimbus::Error::InvalidInput(format!(
                    "sandbox supervisor requires {SANDBOX_EGRESS_ENFORCEMENT_ENV}"
                ))
            })?,
        ),
    };
    let plan: SandboxEgressEnforcementPlan = serde_json::from_str(&raw).map_err(|error| {
        nimbus::Error::InvalidInput(format!(
            "failed to parse sandbox supervisor enforcement contract from {source}: {error}"
        ))
    })?;
    plan.validate().map_err(|message| {
        nimbus::Error::InvalidInput(format!(
            "invalid sandbox supervisor enforcement contract from {source}: {message}"
        ))
    })?;
    Ok(plan)
}

impl SandboxSupervisorContractReport {
    fn from_plan(plan: &SandboxEgressEnforcementPlan) -> nimbus::Result<Self> {
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
            packet_enforcement_active: false,
        })
    }

    fn render_text(&self) -> String {
        format!(
            "Sandbox supervisor contract: {status}\nschema_version: {schema_version}\nmode: {mode}\nreload_policy: {reload_policy}\nrules: {rule_count}\npacket_enforcement_active: {packet_enforcement_active}\n",
            status = self.status,
            schema_version = self.schema_version,
            mode = egress_mode_label(self.mode),
            reload_policy = reload_policy_label(self.reload_policy),
            rule_count = self.rule_count,
            packet_enforcement_active = self.packet_enforcement_active,
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

fn egress_mode_label(mode: SandboxEgressEnforcementMode) -> &'static str {
    match mode {
        SandboxEgressEnforcementMode::LaunchMetadata => "launch_metadata",
        SandboxEgressEnforcementMode::SupervisorProxy => "supervisor_proxy",
    }
}

fn reload_policy_label(reload_policy: SandboxEgressReloadPolicy) -> &'static str {
    match reload_policy {
        SandboxEgressReloadPolicy::RecreateRequired => "recreate_required",
        SandboxEgressReloadPolicy::LiveReload => "live_reload",
    }
}

#[cfg(test)]
mod tests {
    use nimbus::{
        SANDBOX_EGRESS_ENFORCEMENT_ENV, SANDBOX_EGRESS_ENFORCEMENT_SCHEMA_VERSION,
        SandboxEgressEnforcementMode, SandboxEgressEnforcementPlan, SandboxEgressPolicy,
        SandboxEgressReloadPolicy,
    };

    use super::{SandboxSupervisorContractReport, load_enforcement_plan};

    #[test]
    fn load_enforcement_plan_reads_packaged_env_contract() {
        let contract = serde_json::json!({
            "schema_version": SANDBOX_EGRESS_ENFORCEMENT_SCHEMA_VERSION,
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

        let plan = load_enforcement_plan(None, |key| {
            (key == SANDBOX_EGRESS_ENFORCEMENT_ENV).then_some(contract)
        })
        .expect("env contract should parse");

        assert_eq!(plan.mode, SandboxEgressEnforcementMode::SupervisorProxy);
        assert_eq!(
            plan.reload_policy,
            SandboxEgressReloadPolicy::RecreateRequired
        );
        assert_eq!(plan.policy().rules()[0].name, "stripe");
    }

    #[test]
    fn load_enforcement_plan_rejects_missing_env_contract() {
        let error =
            load_enforcement_plan(None, |_| None).expect_err("missing contract should fail closed");

        assert!(
            error.to_string().contains(SANDBOX_EGRESS_ENFORCEMENT_ENV),
            "missing contract error should name the required env var: {error}"
        );
    }

    #[test]
    fn load_enforcement_plan_rejects_invalid_contract_policy() {
        let contract = serde_json::json!({
            "schema_version": SANDBOX_EGRESS_ENFORCEMENT_SCHEMA_VERSION,
            "mode": "launch_metadata",
            "reload_policy": "live_reload",
            "policy": {}
        })
        .to_string();

        let error = load_enforcement_plan(Some(&contract), |_| None)
            .expect_err("false live reload claim should fail closed");

        assert!(
            error.to_string().contains("cannot claim live reload"),
            "invalid contract should surface validation reason: {error}"
        );
    }

    #[test]
    fn supervisor_contract_report_is_validation_only_until_packet_path_lands() {
        let plan = SandboxEgressEnforcementPlan::supervisor_proxy(
            &SandboxEgressPolicy::deny_all()
                .compile()
                .expect("deny-all should compile"),
            SandboxEgressReloadPolicy::RecreateRequired,
        );

        let report =
            SandboxSupervisorContractReport::from_plan(&plan).expect("report should render");

        assert_eq!(report.status, "valid");
        assert_eq!(report.mode, SandboxEgressEnforcementMode::SupervisorProxy);
        assert!(!report.packet_enforcement_active);
        assert!(
            report
                .render_text()
                .contains("packet_enforcement_active: false")
        );
    }
}
