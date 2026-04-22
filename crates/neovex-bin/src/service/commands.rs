use std::path::PathBuf;

use clap::{Args, Subcommand, ValueEnum};
use neovex::TenantId;

use crate::cli_ux;

use super::compose;

#[derive(Debug, Args)]
#[command(
    help_template = cli_ux::COMMAND_GROUP_HELP_TEMPLATE,
    after_help = cli_ux::SERVICE_HELP_EXAMPLES,
    subcommand_help_heading = "Available Commands"
)]
pub(crate) struct ServiceCommand {
    #[command(subcommand)]
    pub(super) command: ServiceSubcommand,
}

#[derive(Debug, Subcommand)]
pub(super) enum ServiceSubcommand {
    /// Validate and print the resolved service plan from a Compose file.
    Config(ServiceConfigCommand),
    /// Start one or more declared services for the current Compose project.
    Up(ServiceUpCommand),
    /// Stop one or more persisted services for the current Compose project.
    Down(ServiceDownCommand),
    /// Show persisted sandbox state for the current Compose project.
    List(ServiceListCommand),
    /// Show persisted sandbox details for one service in the current Compose project.
    Inspect(ServiceInspectCommand),
    /// Print persisted service logs for one service in the current Compose project.
    Logs(ServiceLogsCommand),
    /// Show the persisted PID snapshot for one service in the current Compose project.
    Ps(ServicePsCommand),
}

#[derive(Debug, Args)]
#[command(
    help_template = cli_ux::COMMAND_HELP_TEMPLATE,
    after_help = cli_ux::SERVICE_CONFIG_HELP_EXAMPLES
)]
pub(super) struct ServiceConfigCommand {
    /// Compose file to read. Defaults to ./compose.yaml.
    #[arg(long, default_value = compose::DEFAULT_COMPOSE_FILE)]
    pub(super) file: PathBuf,

    /// Print only service names, one per line.
    #[arg(long)]
    pub(super) services: bool,
}

#[derive(Debug, Args)]
#[command(
    help_template = cli_ux::COMMAND_HELP_TEMPLATE,
    after_help = cli_ux::SERVICE_UP_HELP_EXAMPLES
)]
pub(super) struct ServiceUpCommand {
    /// Optional service name. When omitted, starts all declared services.
    pub(super) service: Option<String>,

    /// Compose file to read. Defaults to ./compose.yaml.
    #[arg(long, default_value = compose::DEFAULT_COMPOSE_FILE)]
    pub(super) file: PathBuf,

    /// Optional tenant override. Defaults to the deterministic local project tenant.
    #[arg(long)]
    pub(super) tenant: Option<TenantId>,
}

#[derive(Debug, Args)]
#[command(
    help_template = cli_ux::COMMAND_HELP_TEMPLATE,
    after_help = cli_ux::SERVICE_DOWN_HELP_EXAMPLES
)]
pub(super) struct ServiceDownCommand {
    /// Optional service name. When omitted, stops all persisted services in the tenant.
    pub(super) service: Option<String>,

    /// Compose file to read. Defaults to ./compose.yaml.
    #[arg(long, default_value = compose::DEFAULT_COMPOSE_FILE)]
    pub(super) file: PathBuf,

    /// Optional tenant override. Defaults to the deterministic local project tenant.
    #[arg(long)]
    pub(super) tenant: Option<TenantId>,
}

#[derive(Debug, Args)]
#[command(
    help_template = cli_ux::COMMAND_HELP_TEMPLATE,
    after_help = cli_ux::SERVICE_LIST_HELP_EXAMPLES
)]
pub(super) struct ServiceListCommand {
    /// Compose file to read. Defaults to ./compose.yaml.
    #[arg(long, default_value = compose::DEFAULT_COMPOSE_FILE)]
    pub(super) file: PathBuf,

    /// Output format.
    #[arg(short = 'f', long, value_enum, default_value_t = ServiceListOutputFormat::Table)]
    pub(super) format: ServiceListOutputFormat,

    /// Omit table headings from table output.
    #[arg(short = 'n', long = "noheading")]
    pub(super) no_heading: bool,

    /// Show all tenants under the project-scoped backend root, not just the
    /// deterministic local project tenant.
    #[arg(long)]
    pub(super) all_tenants: bool,
}

#[derive(Debug, Args)]
#[command(
    help_template = cli_ux::COMMAND_HELP_TEMPLATE,
    after_help = cli_ux::SERVICE_INSPECT_HELP_EXAMPLES
)]
pub(super) struct ServiceInspectCommand {
    /// Service name to inspect.
    pub(super) service: String,

    /// Compose file to read. Defaults to ./compose.yaml.
    #[arg(long, default_value = compose::DEFAULT_COMPOSE_FILE)]
    pub(super) file: PathBuf,

    /// Optional tenant override. Defaults to the deterministic local project tenant.
    #[arg(long)]
    pub(super) tenant: Option<TenantId>,

    /// Output format.
    #[arg(short = 'f', long, value_enum, default_value_t = ServiceInspectOutputFormat::Json)]
    pub(super) format: ServiceInspectOutputFormat,
}

#[derive(Debug, Args)]
#[command(
    help_template = cli_ux::COMMAND_HELP_TEMPLATE,
    after_help = cli_ux::SERVICE_LOGS_HELP_EXAMPLES
)]
pub(super) struct ServiceLogsCommand {
    /// Service name to read logs for.
    pub(super) service: String,

    /// Compose file to read. Defaults to ./compose.yaml.
    #[arg(long, default_value = compose::DEFAULT_COMPOSE_FILE)]
    pub(super) file: PathBuf,

    /// Optional tenant override. Defaults to the deterministic local project tenant.
    #[arg(long)]
    pub(super) tenant: Option<TenantId>,

    /// Keep polling the persisted log file for appended output.
    #[arg(long)]
    pub(super) follow: bool,
}

#[derive(Debug, Args)]
#[command(
    help_template = cli_ux::COMMAND_HELP_TEMPLATE,
    after_help = cli_ux::SERVICE_PS_HELP_EXAMPLES
)]
pub(super) struct ServicePsCommand {
    /// Service name to inspect process state for.
    pub(super) service: String,

    /// Compose file to read. Defaults to ./compose.yaml.
    #[arg(long, default_value = compose::DEFAULT_COMPOSE_FILE)]
    pub(super) file: PathBuf,

    /// Optional tenant override. Defaults to the deterministic local project tenant.
    #[arg(long)]
    pub(super) tenant: Option<TenantId>,

    /// Output format.
    #[arg(short = 'f', long, value_enum, default_value_t = ServicePsOutputFormat::Table)]
    pub(super) format: ServicePsOutputFormat,

    /// Omit table headings from table output.
    #[arg(short = 'n', long = "noheading")]
    pub(super) no_heading: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub(super) enum ServiceListOutputFormat {
    Json,
    Yaml,
    #[default]
    Table,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub(super) enum ServiceInspectOutputFormat {
    #[default]
    Json,
    Yaml,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub(super) enum ServicePsOutputFormat {
    Json,
    Yaml,
    #[default]
    Table,
}
