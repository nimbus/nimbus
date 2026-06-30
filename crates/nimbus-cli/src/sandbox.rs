use std::env;

use clap::{Args, Subcommand};
use nimbus::Error;

use crate::target_context::{TargetContext, TargetSelector};

#[derive(Debug, Args)]
#[command(
    help_template = crate::cli_ux::COMMAND_GROUP_HELP_TEMPLATE,
    subcommand_help_heading = "Available Commands",
    after_help = "Examples:\n  nimbus sandbox create --local --template agent-browser\n  nimbus sandbox list --target dev\n"
)]
pub(crate) struct SandboxCommand {
    #[command(subcommand)]
    pub(crate) command: SandboxSubcommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum SandboxSubcommand {
    /// Lease a sandbox from an admitted template.
    Create(SandboxCreateCommand),
    /// List sandboxes known to the selected target.
    List(SandboxListCommand),
}

#[derive(Debug, Args)]
pub(crate) struct SandboxCreateCommand {
    #[command(flatten)]
    pub(crate) target: TargetSelector,

    /// Admitted sandbox template to lease.
    #[arg(long)]
    pub(crate) template: String,
}

#[derive(Debug, Args)]
pub(crate) struct SandboxListCommand {
    #[command(flatten)]
    pub(crate) target: TargetSelector,
}

pub(crate) async fn run_sandbox_command(command: SandboxCommand) -> Result<(), Error> {
    let target = resolve_sandbox_target(&command)?;
    Err(Error::InvalidInput(format!(
        "nimbus sandbox resolved {target:?}, but sandbox lifecycle execution is reserved for the service-sandbox-node workload-control path"
    )))
}

pub(crate) fn resolve_sandbox_target(command: &SandboxCommand) -> Result<TargetContext, Error> {
    match &command.command {
        SandboxSubcommand::Create(command) => command
            .target
            .resolve("sandbox create", |name| env::var(name).ok()),
        SandboxSubcommand::List(command) => command
            .target
            .resolve("sandbox list", |name| env::var(name).ok()),
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;
    use crate::{Cli, Command};

    #[test]
    fn sandbox_command_resolves_target() {
        let cli = Cli::parse_from([
            "nimbus",
            "sandbox",
            "create",
            "--url",
            "https://nimbus.example.test",
            "--template",
            "agent-browser",
        ]);
        let Command::Sandbox(command) = cli.command else {
            panic!("sandbox command should parse");
        };

        let context = resolve_sandbox_target(&command).expect("target should resolve");

        assert_eq!(
            context.kind,
            crate::target_context::TargetContextKind::RemoteUrl(
                "https://nimbus.example.test/".to_owned()
            )
        );
        let SandboxSubcommand::Create(create) = command.command else {
            panic!("sandbox create should parse");
        };
        assert_eq!(create.template, "agent-browser");
    }
}
