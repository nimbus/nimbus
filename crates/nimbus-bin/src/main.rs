use clap::{Parser, Subcommand};

mod cli_ux;
mod codegen;
mod compose;
mod deploy;
mod dev;
mod dirs;
mod encryption;
mod init;
mod local_server_client;
mod machine;
mod node;
mod start;
#[cfg(test)]
mod test_support;
mod token;
mod ui;

use crate::codegen::{CodegenCommand, run_codegen_command};
use crate::compose::{ComposeCommand, run_compose_command};
use crate::deploy::{DeployCommand, run_deploy_command};
use crate::dev::{DevCommand, run_dev_command};
use crate::encryption::{EncryptionCommand, run_encryption_command};
use crate::init::{InitCommand, run_init_command};
use crate::machine::{MachineCommand, run_machine_command};
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
    /// Generate app artifacts from nimbus/ or convex/ source code.
    Codegen(CodegenCommand),
    /// Scaffold a new Nimbus project.
    Init(InitCommand),
    /// Local admin token management commands.
    #[command(subcommand)]
    Token(TokenCommand),
    /// Open the Nimbus operator console in a browser.
    Ui(UiCommand),
    /// Manage local developer machines.
    Machine(MachineCommand),
    /// Compose-backed local service lifecycle commands.
    #[command(name = "compose")]
    Compose(ComposeCommand),
    /// Encryption admin commands.
    #[command(subcommand)]
    Encryption(EncryptionCommand),
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();
    match cli.command {
        Command::Start(command) => run_start_command(*command).await?,
        Command::Dev(command) => run_dev_command(*command).await?,
        Command::Deploy(command) => run_deploy_command(command).await?,
        Command::Codegen(command) => run_codegen_command(command).await?,
        Command::Init(command) => run_init_command(command).await?,
        Command::Token(command) => run_token_command(command).await?,
        Command::Ui(command) => run_ui_command(command).await?,
        Command::Machine(command) => {
            run_machine_command(command).await?;
        }
        Command::Compose(command) => {
            let persistence_config =
                persistence_config_from_start_command(&StartCommand::default())?;
            run_compose_command(command, &persistence_config).await?;
        }
        Command::Encryption(command) => {
            let persistence_config =
                persistence_config_from_start_command(&StartCommand::default())?;
            run_encryption_command(command, &persistence_config).await?;
        }
    }
    Ok(())
}
