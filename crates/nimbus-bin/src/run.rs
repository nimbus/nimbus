use std::env;

use clap::Args;
use nimbus::Error;

use crate::target_context::{TargetContext, TargetSelector};

#[derive(Debug, Args)]
#[command(
    help_template = crate::cli_ux::COMMAND_HELP_TEMPLATE,
    after_help = "Examples:\n  nimbus run --local -- npm test\n  nimbus run --target dev -- node scripts/job.mjs\n"
)]
pub(crate) struct RunCommand {
    #[command(flatten)]
    pub(crate) target: TargetSelector,

    /// Command and arguments to run against the selected Nimbus target.
    #[arg(value_name = "COMMAND", trailing_var_arg = true, allow_hyphen_values = true, num_args = 1..)]
    pub(crate) argv: Vec<String>,
}

pub(crate) async fn run_run_command(command: RunCommand) -> Result<(), Error> {
    let target = resolve_run_target(&command)?;
    Err(Error::InvalidInput(format!(
        "nimbus run resolved {target:?}, but workload execution is reserved for the service-sandbox-node workload-control path"
    )))
}

pub(crate) fn resolve_run_target(command: &RunCommand) -> Result<TargetContext, Error> {
    command.target.resolve("run", |name| env::var(name).ok())
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use crate::{Cli, Command};

    #[test]
    fn run_command_resolves_target() {
        let cli = Cli::parse_from(["nimbus", "run", "--target", "dev", "--", "npm", "test"]);
        let Command::Run(command) = cli.command else {
            panic!("run command should parse");
        };

        let context = command
            .target
            .resolve("run", |_| None)
            .expect("target should resolve");

        assert_eq!(
            context.kind,
            crate::target_context::TargetContextKind::NamedTarget("dev".to_owned())
        );
        assert_eq!(
            command.argv,
            vec!["npm".to_owned(), "test".to_owned()],
            "trailing command must be preserved for the future workload executor"
        );
    }
}
