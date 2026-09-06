use clap::{Parser, Subcommand};

const CONVEX_SILO_ENV: &str = "NIMBUS_CONVEX_SILO";

mod app_manifest;
mod auth;
mod authoring_root;
mod backup;
mod cli_ux;
mod codegen;
mod compose;
mod container_runner;
mod credentials;
mod deploy;
mod dev;
mod dirs;
mod embedded_control_plane;
mod encryption;
mod explain;
mod function_scaling;
mod init;
mod kv;
mod list;
mod local_server_client;
mod machine;
mod network_composition;
mod node_runtime;
mod node_service;
mod object_storage;
mod path_boundary;
mod policy;
mod provider_binaries;
mod provision;
mod run;
mod sandbox;
mod sandbox_supervisor;
mod start;
mod target_context;
mod targets;
#[cfg(test)]
mod test_support;
mod token;
mod typeinfo;
mod ui;
mod validate;
mod wire_credentials;
mod workload_boot;

use crate::auth::{AuthCommand, run_auth_command};
use crate::backup::{BackupCommand, run_backup_command};
use crate::codegen::{CodegenCommand, run_codegen_command};
use crate::compose::{ComposeCommand, run_compose_command};
use crate::container_runner::{ContainerRunnerCommand, run_container_runner_command};
use crate::deploy::{DeployCommand, run_deploy_command};
use crate::dev::{DevCommand, run_dev_command};
use crate::encryption::{EncryptionCommand, run_encryption_command};
use crate::explain::{ExplainCommand, run_explain_command};
use crate::init::{InitCommand, run_init_command};
use crate::kv::{KvCommand, run_kv_command};
use crate::list::{ListCommand, run_list_command};
use crate::machine::{
    MachineCommand, machine_command_requires_canonical_engine_authority, run_machine_command,
};
use crate::node_service::{NodeCommand, run_node_command};
use crate::object_storage::{ObjectStorageCommand, run_object_storage_command};
use crate::policy::{PolicyCommand, run_policy_command};
use crate::provision::{PackagesCommand, run_packages_command};
use crate::run::{RunCommand, run_run_command};
use crate::sandbox::{SandboxCommand, run_sandbox_command};
use crate::sandbox_supervisor::{SandboxSupervisorCommand, run_sandbox_supervisor_command};
use crate::start::{StartCommand, persistence_config_from_start_command, run_start_command};
use crate::targets::{TargetCommand, run_target_command};
use crate::token::{TokenCommand, run_token_command};
use crate::ui::{UiCommand, run_ui_command};
use crate::validate::{ValidateCommand, run_validate_command};

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
    /// Manage the named-target registry (`~/.config/nimbus/targets`).
    #[command(subcommand)]
    Target(TargetCommand),
    /// Explain effective Nimbus configuration and runtime admission.
    Explain(ExplainCommand),
    /// Validate Nimbus project config or policy.
    Validate(ValidateCommand),
    /// List Nimbus resources.
    List(ListCommand),
    /// Manage sandbox resources on an explicit Nimbus target.
    Sandbox(SandboxCommand),
    /// Generate app artifacts from nimbus/ or convex/ source code.
    Codegen(CodegenCommand),
    /// Scaffold a new Nimbus project.
    Init(InitCommand),
    /// Run the Nimbus KV RESP listener.
    Kv(KvCommand),
    /// Manage Nimbus object-storage placement, backup, restore, GC, and erasure maintenance.
    #[command(name = "object-storage", subcommand)]
    ObjectStorage(ObjectStorageCommand),
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
    /// Internal typed container runner entrypoint.
    #[command(name = "container-runner", hide = true)]
    ContainerRunner(ContainerRunnerCommand),
    /// Compose-backed local service lifecycle commands.
    #[command(name = "compose")]
    Compose(ComposeCommand),
    /// Validate and explain Nimbus operator policy files.
    #[command(subcommand)]
    Policy(PolicyCommand),
    /// Encryption admin commands.
    #[command(subcommand)]
    Encryption(EncryptionCommand),
    /// Install embedded Nimbus JS packages into an app (`.nimbus/packages/`).
    #[command(subcommand)]
    Packages(PackagesCommand),
    /// Internal sandbox-local supervisor entrypoint.
    #[command(name = "sandbox-supervisor", hide = true)]
    SandboxSupervisor(SandboxSupervisorCommand),
}

pub async fn run_from_env() -> Result<(), Box<dyn std::error::Error>> {
    if container_runner::run_container_runner_argv0_if_requested().await? {
        return Ok(());
    }
    let cli = Cli::parse();
    run_cli(cli).await
}

async fn run_cli(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    match cli.command {
        Command::Start(command) => run_start_command(*command).await?,
        Command::Dev(command) => run_dev_command(*command).await?,
        Command::Deploy(command) => run_deploy_command(command).await?,
        Command::Run(command) => run_run_command(command).await?,
        Command::Target(command) => run_target_command(command)?,
        Command::Explain(command) => run_explain_command(command).await?,
        Command::Validate(command) => run_validate_command(command).await?,
        Command::List(command) => run_list_command(command).await?,
        Command::Sandbox(command) => run_sandbox_command(command).await?,
        Command::Codegen(command) => run_codegen_command(command).await?,
        Command::Init(command) => run_init_command(command).await?,
        Command::Kv(command) => run_kv_command(command).await?,
        Command::ObjectStorage(command) => run_object_storage_command(command).await?,
        Command::Token(command) => run_token_command(command).await?,
        Command::Auth(command) => run_auth_command(command).await?,
        Command::Backup(command) => {
            let persistence_config =
                persistence_config_from_start_command(&StartCommand::default())?;
            run_backup_command(command, &persistence_config).await?;
        }
        Command::Ui(command) => run_ui_command(command).await?,
        Command::Machine(command) => {
            let persistence_config =
                if machine_command_requires_canonical_engine_authority(&command) {
                    Some(persistence_config_from_start_command(
                        &StartCommand::default(),
                    )?)
                } else {
                    None
                };
            run_machine_command(command, persistence_config.as_ref()).await?;
        }
        Command::Node(command) => run_node_command(command).await?,
        Command::ContainerRunner(command) => run_container_runner_command(command).await?,
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
    fn function_scaling_root_verb_commands_parse() {
        let cli = Cli::parse_from(["nimbus", "explain", "functions", "messages:send"]);
        assert!(
            matches!(cli.command, Command::Explain(_)),
            "nimbus explain functions <name> should parse"
        );

        let cli = Cli::parse_from([
            "nimbus",
            "explain",
            "functions",
            "--all",
            "--tenant",
            "tenant-a",
        ]);
        assert!(
            matches!(cli.command, Command::Explain(_)),
            "nimbus explain functions --all --tenant <tenant> should parse"
        );

        let cli = Cli::parse_from(["nimbus", "explain", "config", "functions.scaling"]);
        assert!(
            matches!(cli.command, Command::Explain(_)),
            "nimbus explain config functions.scaling should parse"
        );

        let cli = Cli::parse_from(["nimbus", "validate"]);
        assert!(
            matches!(cli.command, Command::Validate(_)),
            "nimbus validate should parse"
        );

        let cli = Cli::parse_from([
            "nimbus",
            "validate",
            "functions",
            "--policy",
            "nimbus.policy.yaml",
        ]);
        assert!(
            matches!(cli.command, Command::Validate(_)),
            "nimbus validate functions --policy <file> should parse"
        );

        let cli = Cli::parse_from(["nimbus", "validate", "policy"]);
        assert!(
            matches!(cli.command, Command::Validate(_)),
            "nimbus validate policy should parse"
        );

        let cli = Cli::parse_from(["nimbus", "list", "functions"]);
        assert!(
            matches!(cli.command, Command::List(_)),
            "nimbus list functions should parse"
        );

        let cli = Cli::parse_from([
            "nimbus",
            "run",
            "functions",
            "messages:send",
            "{\"body\":\"hello\"}",
            "--policy",
            "nimbus.policy.yaml",
        ]);
        assert!(
            matches!(cli.command, Command::Run(_)),
            "nimbus run functions <name> [jsonArgs] --policy <file> should parse"
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
    fn container_runner_command_parses_as_hidden_internal_entrypoint() {
        let cli = Cli::parse_from([
            "nimbus",
            "container-runner",
            "--bundle",
            "/var/lib/nimbus/control/service-sandboxes/container/bundles/tenants/demo/sandboxes/db-01/bundle",
        ]);

        assert!(
            matches!(cli.command, Command::ContainerRunner(_)),
            "container-runner should parse as the packaged internal entrypoint"
        );
    }

    #[test]
    fn kv_root_command_parses() {
        let cli = Cli::parse_from([
            "nimbus",
            "kv",
            "--bind",
            "127.0.0.1:6380",
            "--tenant",
            "tenant-a",
            "--password",
            "secret",
        ]);

        assert!(
            matches!(cli.command, Command::Kv(_)),
            "nimbus kv should parse as a first-class root command"
        );
    }
}
