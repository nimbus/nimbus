use std::error::Error;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use clap::{Args, Parser};

use crate::cli_ux;

const CONTAINER_RUNNER_ARGV0: &str = "nimbus-container-runner";

#[derive(Debug, Args)]
#[command(help_template = cli_ux::COMMAND_HELP_TEMPLATE)]
pub(crate) struct ContainerRunnerCommand {
    /// Prepared OCI bundle directory to execute.
    #[arg(long)]
    pub(crate) bundle: PathBuf,
}

pub(crate) async fn run_container_runner_command(
    command: ContainerRunnerCommand,
) -> Result<(), Box<dyn Error>> {
    nimbus_sandbox::backends::container::run_prepared_container_service_workload(command.bundle)?;
    Ok(())
}

pub(crate) async fn run_container_runner_argv0_if_requested() -> Result<bool, Box<dyn Error>> {
    let argv0 = std::env::args_os().next();
    if !is_container_runner_argv0(argv0.as_deref()) {
        return Ok(false);
    }
    let command = ContainerRunnerArgv0Command::parse().runner;
    run_container_runner_command(command).await?;
    Ok(true)
}

fn is_container_runner_argv0(argv0: Option<&OsStr>) -> bool {
    argv0
        .and_then(|value| Path::new(value).file_name())
        .and_then(OsStr::to_str)
        == Some(CONTAINER_RUNNER_ARGV0)
}

#[derive(Debug, Parser)]
#[command(name = "nimbus-container-runner", hide = true)]
struct ContainerRunnerArgv0Command {
    #[command(flatten)]
    runner: ContainerRunnerCommand,
}

#[cfg(test)]
mod tests {
    use super::{ContainerRunnerArgv0Command, is_container_runner_argv0};
    use clap::Parser;
    use std::ffi::OsStr;

    #[test]
    fn container_runner_detects_packaged_argv0() {
        assert!(is_container_runner_argv0(Some(OsStr::new(
            "/usr/libexec/nimbus/nimbus-container-runner"
        ))));
        assert!(!is_container_runner_argv0(Some(OsStr::new(
            "/usr/local/bin/nimbus"
        ))));
    }

    #[test]
    fn container_runner_argv0_parses_bundle_argument() {
        let command = ContainerRunnerArgv0Command::parse_from([
            "nimbus-container-runner",
            "--bundle",
            "/var/lib/nimbus/control/service-sandboxes/container/bundles/tenants/demo/sandboxes/db-01/bundle",
        ]);

        assert!(
            command.runner.bundle.ends_with("db-01/bundle"),
            "argv0 runner should parse the prepared bundle path"
        );
    }
}
