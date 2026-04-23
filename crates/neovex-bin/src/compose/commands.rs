use std::path::PathBuf;

use clap::{Args, Subcommand, ValueEnum};
use neovex::TenantId;

use crate::cli_ux;

use super::file;

#[derive(Debug, Args)]
#[command(
    help_template = cli_ux::COMMAND_GROUP_HELP_TEMPLATE,
    after_help = cli_ux::COMPOSE_HELP_EXAMPLES,
    subcommand_help_heading = "Available Commands"
)]
pub(crate) struct ComposeCommand {
    #[command(subcommand)]
    pub(super) command: ComposeSubcommand,
}

#[derive(Debug, Subcommand)]
pub(super) enum ComposeSubcommand {
    /// Validate and print the resolved service plan from a Compose file.
    Config(ComposeConfigCommand),
    /// Start one or more declared services for the current Compose project.
    Up(ComposeUpCommand),
    /// Stop one or more persisted services for the current Compose project.
    Down(ComposeDownCommand),
    /// Show persisted sandbox state for the current Compose project.
    Ps(ComposePsCommand),
    /// Show persisted sandbox details for one service in the current Compose project.
    Inspect(ComposeInspectCommand),
    /// Print persisted service logs for one service in the current Compose project.
    Logs(ComposeLogsCommand),
    /// Show the persisted PID snapshot for one service in the current Compose project.
    Top(ComposeTopCommand),
}

#[derive(Debug, Args)]
#[command(
    help_template = cli_ux::COMMAND_HELP_TEMPLATE,
    after_help = cli_ux::COMPOSE_CONFIG_HELP_EXAMPLES
)]
pub(super) struct ComposeConfigCommand {
    /// Compose file to read. Defaults to ./compose.yaml.
    #[arg(long, default_value = file::DEFAULT_COMPOSE_FILE)]
    pub(super) file: PathBuf,

    /// Print only service names, one per line.
    #[arg(long)]
    pub(super) services: bool,
}

#[derive(Debug, Args)]
#[command(
    help_template = cli_ux::COMMAND_HELP_TEMPLATE,
    after_help = cli_ux::COMPOSE_UP_HELP_EXAMPLES
)]
pub(super) struct ComposeUpCommand {
    /// Optional service name. When omitted, starts all declared services.
    pub(super) service: Option<String>,

    /// Compose file to read. Defaults to ./compose.yaml.
    #[arg(long, default_value = file::DEFAULT_COMPOSE_FILE)]
    pub(super) file: PathBuf,

    /// Optional tenant override. Defaults to the deterministic local project tenant.
    #[arg(long)]
    pub(super) tenant: Option<TenantId>,
}

#[derive(Debug, Args)]
#[command(
    help_template = cli_ux::COMMAND_HELP_TEMPLATE,
    after_help = cli_ux::COMPOSE_DOWN_HELP_EXAMPLES
)]
pub(super) struct ComposeDownCommand {
    /// Optional service name. When omitted, stops all persisted services in the tenant.
    pub(super) service: Option<String>,

    /// Compose file to read. Defaults to ./compose.yaml.
    #[arg(long, default_value = file::DEFAULT_COMPOSE_FILE)]
    pub(super) file: PathBuf,

    /// Optional tenant override. Defaults to the deterministic local project tenant.
    #[arg(long)]
    pub(super) tenant: Option<TenantId>,
}

#[derive(Debug, Args)]
#[command(
    help_template = cli_ux::COMMAND_HELP_TEMPLATE,
    after_help = cli_ux::COMPOSE_PS_HELP_EXAMPLES
)]
pub(super) struct ComposePsCommand {
    /// Compose file to read. Defaults to ./compose.yaml.
    #[arg(long, default_value = file::DEFAULT_COMPOSE_FILE)]
    pub(super) file: PathBuf,

    /// Output format.
    #[arg(short = 'f', long, value_enum, default_value_t = ComposePsOutputFormat::Table)]
    pub(super) format: ComposePsOutputFormat,

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
    after_help = cli_ux::COMPOSE_INSPECT_HELP_EXAMPLES
)]
pub(super) struct ComposeInspectCommand {
    /// Service name to inspect.
    pub(super) service: String,

    /// Compose file to read. Defaults to ./compose.yaml.
    #[arg(long, default_value = file::DEFAULT_COMPOSE_FILE)]
    pub(super) file: PathBuf,

    /// Optional tenant override. Defaults to the deterministic local project tenant.
    #[arg(long)]
    pub(super) tenant: Option<TenantId>,

    /// Output format.
    #[arg(short = 'f', long, value_enum, default_value_t = ComposeInspectOutputFormat::Json)]
    pub(super) format: ComposeInspectOutputFormat,
}

#[derive(Debug, Args)]
#[command(
    help_template = cli_ux::COMMAND_HELP_TEMPLATE,
    after_help = cli_ux::COMPOSE_LOGS_HELP_EXAMPLES
)]
pub(super) struct ComposeLogsCommand {
    /// Service name to read logs for.
    pub(super) service: String,

    /// Compose file to read. Defaults to ./compose.yaml.
    #[arg(long, default_value = file::DEFAULT_COMPOSE_FILE)]
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
    after_help = cli_ux::COMPOSE_TOP_HELP_EXAMPLES
)]
pub(super) struct ComposeTopCommand {
    /// Service name to inspect process state for.
    pub(super) service: String,

    /// Compose file to read. Defaults to ./compose.yaml.
    #[arg(long, default_value = file::DEFAULT_COMPOSE_FILE)]
    pub(super) file: PathBuf,

    /// Optional tenant override. Defaults to the deterministic local project tenant.
    #[arg(long)]
    pub(super) tenant: Option<TenantId>,

    /// Output format.
    #[arg(short = 'f', long, value_enum, default_value_t = ComposeTopOutputFormat::Table)]
    pub(super) format: ComposeTopOutputFormat,

    /// Omit table headings from table output.
    #[arg(short = 'n', long = "noheading")]
    pub(super) no_heading: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub(super) enum ComposePsOutputFormat {
    Json,
    Yaml,
    #[default]
    Table,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub(super) enum ComposeInspectOutputFormat {
    #[default]
    Json,
    Yaml,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub(super) enum ComposeTopOutputFormat {
    Json,
    Yaml,
    #[default]
    Table,
}
