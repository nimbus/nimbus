use std::fs;
use std::path::{Path, PathBuf};

use clap::{Args, Subcommand, ValueEnum};
use nimbus_server::{OperatorPolicyDiff, OperatorPolicyDocument};

#[derive(Debug, Subcommand)]
pub(crate) enum PolicyCommand {
    /// Validate a Nimbus operator policy file.
    Validate(PolicyFileCommand),
    /// Explain the tenant-isolation decisions produced by a policy file.
    Explain(PolicyFileCommand),
    /// Prove policy advisories and accepted-risk status.
    Prove(PolicyFileCommand),
    /// Show authority changes between two policy files.
    Diff(PolicyDiffCommand),
}

#[derive(Debug, Args)]
pub(crate) struct PolicyFileCommand {
    /// Path to a nimbus.policy.yaml file.
    #[arg(long, value_name = "FILE")]
    file: PathBuf,
    /// Output format.
    #[arg(short = 'f', long, default_value = "text")]
    format: PolicyOutputFormat,
}

#[derive(Debug, Args)]
pub(crate) struct PolicyDiffCommand {
    /// Previous policy file.
    #[arg(long, value_name = "FILE")]
    from: PathBuf,
    /// Next policy file.
    #[arg(long, value_name = "FILE")]
    to: PathBuf,
    /// Output format.
    #[arg(short = 'f', long, default_value = "text")]
    format: PolicyOutputFormat,
}

#[derive(Debug, Clone, Copy, Default, ValueEnum)]
enum PolicyOutputFormat {
    #[default]
    Text,
    Json,
}

pub(crate) async fn run_policy_command(command: PolicyCommand) -> nimbus::Result<()> {
    match command {
        PolicyCommand::Validate(command) => run_validate_command(command),
        PolicyCommand::Explain(command) => run_explain_command(command),
        PolicyCommand::Prove(command) => run_prove_command(command),
        PolicyCommand::Diff(command) => run_diff_command(command),
    }
}

fn run_validate_command(command: PolicyFileCommand) -> nimbus::Result<()> {
    let document = load_policy_document(&command.file)?;
    let evaluation = document.evaluate()?;
    match command.format {
        PolicyOutputFormat::Text => print!("{}", evaluation.render_validate_text()),
        PolicyOutputFormat::Json => print_json(&evaluation)?,
    }
    Ok(())
}

fn run_explain_command(command: PolicyFileCommand) -> nimbus::Result<()> {
    let document = load_policy_document(&command.file)?;
    let evaluation = document.evaluate()?;
    match command.format {
        PolicyOutputFormat::Text => print!("{}", evaluation.render_explain_text()),
        PolicyOutputFormat::Json => print_json(&evaluation)?,
    }
    Ok(())
}

fn run_prove_command(command: PolicyFileCommand) -> nimbus::Result<()> {
    let document = load_policy_document(&command.file)?;
    let report = document.prove()?;
    match command.format {
        PolicyOutputFormat::Text => print!("{}", report.render_text()),
        PolicyOutputFormat::Json => print_json(&report)?,
    }
    Ok(())
}

fn run_diff_command(command: PolicyDiffCommand) -> nimbus::Result<()> {
    let from = load_policy_document(&command.from)?;
    let to = load_policy_document(&command.to)?;
    let diff = OperatorPolicyDiff::between(&from, &to)?;
    match command.format {
        PolicyOutputFormat::Text => print!("{}", diff.render_text()),
        PolicyOutputFormat::Json => print_json(&diff)?,
    }
    Ok(())
}

fn load_policy_document(path: &Path) -> nimbus::Result<OperatorPolicyDocument> {
    let body = fs::read_to_string(path).map_err(|error| {
        nimbus::Error::InvalidInput(format!(
            "failed to read policy file {}: {error}",
            path.display()
        ))
    })?;
    serde_yaml::from_str(&body).map_err(|error| {
        nimbus::Error::InvalidInput(format!(
            "failed to parse policy file {}: {error}",
            path.display()
        ))
    })
}

fn print_json(value: &impl serde::Serialize) -> nimbus::Result<()> {
    let json = serde_json::to_string_pretty(value).map_err(|error| {
        nimbus::Error::Serialization(format!("failed to serialize policy output: {error}"))
    })?;
    println!("{json}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_POLICY: &str =
        include_str!("../../nimbus-tenant/tests/fixtures/policy/valid-enterprise.yaml");

    #[test]
    fn load_policy_document_reports_parse_errors_with_path() {
        let temp = tempfile::tempdir().expect("tempdir should build");
        let path = temp.path().join("nimbus.policy.yaml");
        fs::write(&path, "schema_version: 1\nunknown: true\n").expect("fixture should write");

        let error = load_policy_document(&path).expect_err("unknown field should fail");

        assert!(
            error.to_string().contains("failed to parse policy file"),
            "error should name parse failure: {error}"
        );
        assert!(
            error.to_string().contains("nimbus.policy.yaml"),
            "error should name the policy path: {error}"
        );
    }

    #[test]
    fn validate_and_explain_render_stable_text() {
        let document: OperatorPolicyDocument =
            serde_yaml::from_str(VALID_POLICY).expect("fixture should parse");
        let evaluation = document.evaluate().expect("policy should evaluate");

        let validation = evaluation.render_validate_text();
        assert!(validation.contains("Policy validation: allowed"));
        assert!(validation.contains("runtime_function/messages:send"));

        let explanation = evaluation.render_explain_text();
        assert!(explanation.contains("runtime_admission: admit_in_process"));
        assert!(explanation.contains("network_endpoints: db/postgres"));

        let proof = document.prove().expect("policy proof should run");
        let proof_text = proof.render_text();
        assert!(proof_text.contains("Policy prove"));
        assert!(proof_text.contains("Advisories:"));
    }
}
