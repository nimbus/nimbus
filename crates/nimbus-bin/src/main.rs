use clap::{Parser, Subcommand};

mod auth;
mod backup;
mod cli_ux;
mod codegen;
mod compose;
mod credentials;
mod deploy;
mod dev;
mod dirs;
mod encryption;
mod init;
mod local_server_client;
mod machine;
mod node;
mod node_service;
mod node_workload_executor;
mod path_boundary;
mod policy;
mod provision;
mod run;
mod sandbox;
mod sandbox_supervisor;
mod start;
mod target_context;
#[cfg(test)]
mod test_support;
mod token;
mod typeinfo;
mod ui;
mod wire_credentials;

use crate::auth::{AuthCommand, run_auth_command};
use crate::backup::{BackupCommand, run_backup_command};
use crate::codegen::{CodegenCommand, run_codegen_command};
use crate::compose::{ComposeCommand, run_compose_command};
use crate::deploy::{DeployCommand, run_deploy_command};
use crate::dev::{DevCommand, run_dev_command};
use crate::encryption::{EncryptionCommand, run_encryption_command};
use crate::init::{InitCommand, run_init_command};
use crate::machine::{MachineCommand, run_machine_command};
use crate::node_service::{NodeCommand, run_node_command};
use crate::policy::{PolicyCommand, run_policy_command};
use crate::provision::{PackagesCommand, run_packages_command};
use crate::run::{RunCommand, run_run_command};
use crate::sandbox::{SandboxCommand, run_sandbox_command};
use crate::sandbox_supervisor::{SandboxSupervisorCommand, run_sandbox_supervisor_command};
use crate::start::{StartCommand, persistence_config_from_start_command, run_start_command};
use crate::token::{TokenCommand, run_token_command};
use crate::ui::{UiCommand, run_ui_command};

#[derive(Debug, Parser)]
#[command(
    name = "nimbus",
    version,
    about = "Convex-compatible reactive backend with local development and Compose-backed services",
    help_template = cli_ux::ROOT_HELP_TEMPLATE,
    after_help = cli_ux::ROOT_HELP_EXAMPLES,
    subcommand_help_heading = "Available Commands"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Start a Nimbus server in the foreground.
    Start(Box<StartCommand>),
    /// Start a local development server with watched codegen and dev defaults.
    Dev(Box<DevCommand>),
    /// Push app artifacts to an explicit self-hosted Nimbus instance.
    Deploy(DeployCommand),
    /// Run a workload against an explicit Nimbus target.
    Run(RunCommand),
    /// Manage sandbox resources on an explicit Nimbus target.
    Sandbox(SandboxCommand),
    /// Generate app artifacts from nimbus/ or convex/ source code.
    Codegen(CodegenCommand),
    /// Scaffold a new Nimbus project.
    Init(InitCommand),
    /// Local admin token management commands.
    #[command(subcommand)]
    Token(TokenCommand),
    /// Sign-in URLs for the local console and credentials for remote deploys.
    #[command(subcommand)]
    Auth(AuthCommand),
    /// Offline backup and restore of the local data directory.
    #[command(subcommand)]
    Backup(BackupCommand),
    /// Open the Nimbus operator console in a browser.
    Ui(UiCommand),
    /// Manage local developer machines.
    Machine(MachineCommand),
    /// Manage Nimbus node service-manager installation artifacts.
    Node(NodeCommand),
    /// Internal node-local workload executor entrypoint.
    #[command(name = "node-workload-executor", hide = true)]
    NodeWorkloadExecutor(node_workload_executor::NodeWorkloadExecutorCommand),
    /// Compose-backed local service lifecycle commands.
    #[command(name = "compose")]
    Compose(ComposeCommand),
    /// Validate and explain Nimbus operator policy files.
    #[command(subcommand)]
    Policy(PolicyCommand),
    /// Encryption admin commands.
    #[command(subcommand)]
    Encryption(EncryptionCommand),
    /// Provision embedded Nimbus JS packages into an app (`.nimbus/packages/`).
    #[command(subcommand)]
    Packages(PackagesCommand),
    /// Internal sandbox-local supervisor entrypoint.
    #[command(name = "sandbox-supervisor", hide = true)]
    SandboxSupervisor(SandboxSupervisorCommand),
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();
    match cli.command {
        Command::Start(command) => run_start_command(*command).await?,
        Command::Dev(command) => run_dev_command(*command).await?,
        Command::Deploy(command) => run_deploy_command(command).await?,
        Command::Run(command) => run_run_command(command).await?,
        Command::Sandbox(command) => run_sandbox_command(command).await?,
        Command::Codegen(command) => run_codegen_command(command).await?,
        Command::Init(command) => run_init_command(command).await?,
        Command::Token(command) => run_token_command(command).await?,
        Command::Auth(command) => run_auth_command(command).await?,
        Command::Backup(command) => run_backup_command(command).await?,
        Command::Ui(command) => run_ui_command(command).await?,
        Command::Machine(command) => {
            run_machine_command(command).await?;
        }
        Command::Node(command) => run_node_command(command).await?,
        Command::NodeWorkloadExecutor(command) => {
            node_workload_executor::run_node_workload_executor_command(command).await?
        }
        Command::Compose(command) => {
            let persistence_config =
                persistence_config_from_start_command(&StartCommand::default())?;
            run_compose_command(command, &persistence_config).await?;
        }
        Command::Policy(command) => run_policy_command(command).await?,
        Command::Encryption(command) => {
            let persistence_config =
                persistence_config_from_start_command(&StartCommand::default())?;
            run_encryption_command(command, &persistence_config).await?;
        }
        Command::Packages(command) => run_packages_command(command).await?,
        Command::SandboxSupervisor(command) => run_sandbox_supervisor_command(command).await?,
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_subcommands_parse() {
        let cli = Cli::parse_from([
            "nimbus",
            "policy",
            "validate",
            "--file",
            "nimbus.policy.yaml",
            "-f",
            "json",
        ]);

        assert!(
            matches!(cli.command, Command::Policy(PolicyCommand::Validate(_))),
            "policy validate should parse as a first-class root command"
        );

        let cli = Cli::parse_from(["nimbus", "policy", "prove", "--file", "nimbus.policy.yaml"]);

        assert!(
            matches!(cli.command, Command::Policy(PolicyCommand::Prove(_))),
            "policy prove should parse as a first-class root command"
        );

        let cli = Cli::parse_from([
            "nimbus",
            "policy",
            "diff",
            "--from",
            "before.yaml",
            "--to",
            "after.yaml",
        ]);

        assert!(
            matches!(cli.command, Command::Policy(PolicyCommand::Diff(_))),
            "policy diff should parse as a first-class root command"
        );
    }

    #[test]
    fn sandbox_supervisor_command_parses_as_hidden_internal_entrypoint() {
        let cli = Cli::parse_from(["nimbus", "sandbox-supervisor", "-f", "json"]);

        assert!(
            matches!(cli.command, Command::SandboxSupervisor(_)),
            "sandbox-supervisor should parse as the packaged internal entrypoint"
        );
    }

    #[test]
    fn node_workload_executor_command_parses_as_hidden_internal_entrypoint() {
        let cli = Cli::parse_from([
            "nimbus",
            "node-workload-executor",
            "--tenant",
            "demo",
            "--workload",
            "worker",
            "--exec",
            "/usr/bin/true",
            "--status-path",
            "status.jsonl",
            "--once",
        ]);

        assert!(
            matches!(cli.command, Command::NodeWorkloadExecutor(_)),
            "node-workload-executor should parse as the packaged internal entrypoint"
        );
    }
}
