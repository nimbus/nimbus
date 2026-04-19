use clap::{Parser, Subcommand};

mod cli_ux;
mod machine;
mod serve;
mod service;
#[cfg(test)]
mod test_support;

use crate::machine::{MachineCommand, run_machine_command};
use crate::serve::{
    ServeCommand, run_serve_command, service_persistence_config_from_serve_command,
};
use crate::service::{ServiceCommand, run_service_command};

#[derive(Debug, Parser)]
#[command(
    name = "neovex",
    version,
    about = "Reactive document database with machine and service orchestration",
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
    Serve(Box<ServeCommand>),
    Machine(MachineCommand),
    Service(ServiceCommand),
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();
    match cli.command {
        Command::Serve(command) => run_serve_command(*command).await?,
        Command::Machine(command) => {
            run_machine_command(command).await?;
        }
        Command::Service(command) => {
            let service_config =
                service_persistence_config_from_serve_command(&ServeCommand::default())?;
            run_service_command(command, &service_config).await?;
        }
    }
    Ok(())
}
