use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use clap::{Args, Subcommand, ValueEnum};
use neovex::{
    Error, PublishedEndpoint, SandboxBackend, SandboxHandle, SandboxServiceCatalog,
    SandboxServiceManager, SandboxStatus, ServicePersistenceConfig, TenantId,
};
use neovex_sandbox::backends::krun::KrunSandboxStateView;
use serde::Serialize;

use crate::cli_ux;
use crate::machine::MachineApiClient;

mod compose;
mod execution;
mod lifecycle;
mod logs;
mod process;
mod project;
mod render;

use self::execution::{
    load_host_backed_sandbox_service_manager_for_platform,
    load_sandbox_service_catalog_for_execution_platform, lookup_current_remote_service_details,
    machine_api_operation_error, missing_persisted_service_error, render_state_lookup_error,
    requested_service_names, require_krun_backend_for_service_operation,
    resolve_remote_service_down_targets, resolve_service_down_targets,
    resolve_service_execution_surface, validate_forwarded_machine_api_backend,
    validate_forwarded_machine_api_operations,
};
use self::lifecycle::{service_down_outcomes_for_platform, service_up_outcomes_for_platform};
use self::logs::run_service_logs_for_platform;
use self::process::resolve_service_process_snapshot_for_platform;
use self::render::{
    render_service_inspect_view, render_service_lifecycle_action_summary, render_service_list_view,
    render_service_process_snapshot_view,
};
pub(crate) use project::ComposeProjectContext;

#[derive(Debug, Args)]
#[command(
    help_template = cli_ux::COMMAND_GROUP_HELP_TEMPLATE,
    after_help = cli_ux::SERVICE_HELP_EXAMPLES,
    subcommand_help_heading = "Available Commands"
)]
pub(crate) struct ServiceCommand {
    #[command(subcommand)]
    command: ServiceSubcommand,
}

#[derive(Debug, Subcommand)]
enum ServiceSubcommand {
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
struct ServiceConfigCommand {
    /// Compose file to read. Defaults to ./compose.yaml.
    #[arg(long, default_value = compose::DEFAULT_COMPOSE_FILE)]
    file: PathBuf,

    /// Print only service names, one per line.
    #[arg(long)]
    services: bool,
}

#[derive(Debug, Args)]
#[command(
    help_template = cli_ux::COMMAND_HELP_TEMPLATE,
    after_help = cli_ux::SERVICE_UP_HELP_EXAMPLES
)]
struct ServiceUpCommand {
    /// Optional service name. When omitted, starts all declared services.
    service: Option<String>,

    /// Compose file to read. Defaults to ./compose.yaml.
    #[arg(long, default_value = compose::DEFAULT_COMPOSE_FILE)]
    file: PathBuf,

    /// Optional tenant override. Defaults to the deterministic local project tenant.
    #[arg(long)]
    tenant: Option<TenantId>,
}

#[derive(Debug, Args)]
#[command(
    help_template = cli_ux::COMMAND_HELP_TEMPLATE,
    after_help = cli_ux::SERVICE_DOWN_HELP_EXAMPLES
)]
struct ServiceDownCommand {
    /// Optional service name. When omitted, stops all persisted services in the tenant.
    service: Option<String>,

    /// Compose file to read. Defaults to ./compose.yaml.
    #[arg(long, default_value = compose::DEFAULT_COMPOSE_FILE)]
    file: PathBuf,

    /// Optional tenant override. Defaults to the deterministic local project tenant.
    #[arg(long)]
    tenant: Option<TenantId>,
}

#[derive(Debug, Args)]
#[command(
    help_template = cli_ux::COMMAND_HELP_TEMPLATE,
    after_help = cli_ux::SERVICE_LIST_HELP_EXAMPLES
)]
struct ServiceListCommand {
    /// Compose file to read. Defaults to ./compose.yaml.
    #[arg(long, default_value = compose::DEFAULT_COMPOSE_FILE)]
    file: PathBuf,

    /// Output format.
    #[arg(short = 'f', long, value_enum, default_value_t = ServiceListOutputFormat::Table)]
    format: ServiceListOutputFormat,

    /// Omit table headings from table output.
    #[arg(short = 'n', long = "noheading")]
    no_heading: bool,

    /// Show all tenants under the project-scoped backend root, not just the
    /// deterministic local project tenant.
    #[arg(long)]
    all_tenants: bool,
}

#[derive(Debug, Args)]
#[command(
    help_template = cli_ux::COMMAND_HELP_TEMPLATE,
    after_help = cli_ux::SERVICE_INSPECT_HELP_EXAMPLES
)]
struct ServiceInspectCommand {
    /// Service name to inspect.
    service: String,

    /// Compose file to read. Defaults to ./compose.yaml.
    #[arg(long, default_value = compose::DEFAULT_COMPOSE_FILE)]
    file: PathBuf,

    /// Optional tenant override. Defaults to the deterministic local project tenant.
    #[arg(long)]
    tenant: Option<TenantId>,

    /// Output format.
    #[arg(short = 'f', long, value_enum, default_value_t = ServiceInspectOutputFormat::Json)]
    format: ServiceInspectOutputFormat,
}

#[derive(Debug, Args)]
#[command(
    help_template = cli_ux::COMMAND_HELP_TEMPLATE,
    after_help = cli_ux::SERVICE_LOGS_HELP_EXAMPLES
)]
struct ServiceLogsCommand {
    /// Service name to read logs for.
    service: String,

    /// Compose file to read. Defaults to ./compose.yaml.
    #[arg(long, default_value = compose::DEFAULT_COMPOSE_FILE)]
    file: PathBuf,

    /// Optional tenant override. Defaults to the deterministic local project tenant.
    #[arg(long)]
    tenant: Option<TenantId>,

    /// Keep polling the persisted log file for appended output.
    #[arg(long)]
    follow: bool,
}

#[derive(Debug, Args)]
#[command(
    help_template = cli_ux::COMMAND_HELP_TEMPLATE,
    after_help = cli_ux::SERVICE_PS_HELP_EXAMPLES
)]
struct ServicePsCommand {
    /// Service name to inspect process state for.
    service: String,

    /// Compose file to read. Defaults to ./compose.yaml.
    #[arg(long, default_value = compose::DEFAULT_COMPOSE_FILE)]
    file: PathBuf,

    /// Optional tenant override. Defaults to the deterministic local project tenant.
    #[arg(long)]
    tenant: Option<TenantId>,

    /// Output format.
    #[arg(short = 'f', long, value_enum, default_value_t = ServicePsOutputFormat::Table)]
    format: ServicePsOutputFormat,

    /// Omit table headings from table output.
    #[arg(short = 'n', long = "noheading")]
    no_heading: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
enum ServiceListOutputFormat {
    Json,
    Yaml,
    #[default]
    Table,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
enum ServiceInspectOutputFormat {
    #[default]
    Json,
    Yaml,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
enum ServicePsOutputFormat {
    Json,
    Yaml,
    #[default]
    Table,
}

pub(crate) async fn run_service_command(
    command: ServiceCommand,
    service_config: &ServicePersistenceConfig,
) -> Result<(), Error> {
    let control_data_dir = control_data_dir_from_service_config(service_config);
    match command.command {
        ServiceSubcommand::Config(config) => run_service_config(config),
        ServiceSubcommand::Up(up) => run_service_up(up, control_data_dir).await,
        ServiceSubcommand::Down(down) => run_service_down(down, control_data_dir).await,
        ServiceSubcommand::List(list) => run_service_list(list, control_data_dir),
        ServiceSubcommand::Inspect(inspect) => run_service_inspect(inspect, control_data_dir),
        ServiceSubcommand::Logs(logs) => run_service_logs(logs, control_data_dir),
        ServiceSubcommand::Ps(ps) => run_service_ps(ps, control_data_dir),
    }
}

#[allow(dead_code)]
pub(crate) fn load_sandbox_service_catalog(
    file: &std::path::Path,
) -> Result<Arc<dyn SandboxServiceCatalog>, Error> {
    Ok(Arc::new(
        compose::ComposeProjectPlan::load(file)?.into_service_catalog()?,
    ))
}

#[allow(dead_code)]
pub(crate) fn load_sandbox_service_manager(
    file: &std::path::Path,
    sandbox_backend: Arc<dyn SandboxBackend>,
) -> Result<SandboxServiceManager, Error> {
    Ok(SandboxServiceManager::new(
        load_sandbox_service_catalog(file)?,
        sandbox_backend,
    ))
}

pub(crate) fn load_compose_project_context(
    file: &std::path::Path,
    control_data_dir: &std::path::Path,
) -> Result<ComposeProjectContext, Error> {
    ComposeProjectContext::load(file, control_data_dir)
}

pub(crate) fn load_host_backed_sandbox_service_manager(
    file: &std::path::Path,
    control_data_dir: &std::path::Path,
) -> Result<SandboxServiceManager, Error> {
    load_host_backed_sandbox_service_manager_for_platform(
        file,
        control_data_dir,
        ServiceHostPlatform::current(),
        None,
    )
}

fn run_service_config(command: ServiceConfigCommand) -> Result<(), Error> {
    let rendered = compose::render_compose_project(&command.file, command.services)?;

    for warning in rendered.warnings {
        cli_ux::write_stderr_prefixed_line("Warning:", &warning).map_err(|error| {
            Error::InvalidInput(format!("failed to write warning output: {error}"))
        })?;
    }

    emit_service_stdout(&rendered.stdout)?;
    Ok(())
}

async fn run_service_up(command: ServiceUpCommand, control_data_dir: &Path) -> Result<(), Error> {
    let rendered = render_service_up_for_platform(
        &command,
        control_data_dir,
        ServiceHostPlatform::current(),
        None,
    )
    .await?;
    emit_service_stdout(&rendered)?;
    Ok(())
}

async fn run_service_down(
    command: ServiceDownCommand,
    control_data_dir: &Path,
) -> Result<(), Error> {
    let rendered = render_service_down_for_platform(
        &command,
        control_data_dir,
        ServiceHostPlatform::current(),
        None,
    )
    .await?;
    emit_service_stdout(&rendered)?;
    Ok(())
}

fn run_service_list(command: ServiceListCommand, control_data_dir: &Path) -> Result<(), Error> {
    let rendered = render_service_list_for_platform(
        &command,
        control_data_dir,
        ServiceHostPlatform::current(),
        None,
    )?;
    emit_service_stdout(&rendered)?;
    Ok(())
}

fn run_service_inspect(
    command: ServiceInspectCommand,
    control_data_dir: &Path,
) -> Result<(), Error> {
    let rendered = render_service_inspect_for_platform(
        &command,
        control_data_dir,
        ServiceHostPlatform::current(),
        None,
    )?;
    emit_service_stdout(&rendered)?;
    Ok(())
}

fn run_service_logs(command: ServiceLogsCommand, control_data_dir: &Path) -> Result<(), Error> {
    run_service_logs_for_platform(
        &command,
        control_data_dir,
        ServiceHostPlatform::current(),
        None,
    )
}

fn run_service_ps(command: ServicePsCommand, control_data_dir: &Path) -> Result<(), Error> {
    let rendered = render_service_ps_for_platform(
        &command,
        control_data_dir,
        ServiceHostPlatform::current(),
        None,
    )?;
    emit_service_stdout(&rendered)?;
    Ok(())
}

fn emit_service_stdout(rendered: &str) -> Result<(), Error> {
    cli_ux::write_stdout(rendered)
        .map_err(|error| Error::Internal(format!("failed to write service output: {error}")))
}

#[cfg(test)]
#[allow(dead_code)]
async fn render_service_up(
    command: &ServiceUpCommand,
    control_data_dir: &Path,
) -> Result<String, Error> {
    render_service_up_for_platform(
        command,
        control_data_dir,
        ServiceHostPlatform::current(),
        None,
    )
    .await
}

#[cfg(test)]
#[allow(dead_code)]
async fn render_service_down(
    command: &ServiceDownCommand,
    control_data_dir: &Path,
) -> Result<String, Error> {
    render_service_down_for_platform(
        command,
        control_data_dir,
        ServiceHostPlatform::current(),
        None,
    )
    .await
}

fn render_service_list_for_platform(
    command: &ServiceListCommand,
    control_data_dir: &Path,
    host_platform: ServiceHostPlatform,
    machine_api_client: Option<MachineApiClient>,
) -> Result<String, Error> {
    let context = load_compose_project_context(&command.file, control_data_dir)?;
    match resolve_service_execution_surface(
        &context,
        None,
        "service list",
        host_platform,
        machine_api_client,
    )? {
        ServiceExecutionSurface::Krun { state_view, .. } => {
            let summaries = if command.all_tenants {
                state_view.list()
            } else {
                state_view.list_for_tenant(&context.control_plane.local_tenant_id)
            }
            .map_err(|error| render_state_lookup_error("list persisted sandbox state", error))?;
            let views = summaries
                .into_iter()
                .map(|summary| ServiceSandboxSummaryView {
                    sandbox_id: summary.sandbox_id,
                    tenant_id: summary.tenant_id,
                    service_name: summary.service_name,
                    status: summary.status,
                    published_endpoints: summary.published_endpoints,
                    restart_count: summary.restart_count,
                    last_exit_code: summary.last_exit_code,
                    shutdown_requested: summary.shutdown_requested,
                })
                .collect::<Vec<_>>();
            render_service_list_view(&views, command.format, command.no_heading)
        }
        ServiceExecutionSurface::ForwardedContainer { client, .. } => {
            validate_forwarded_machine_api_operations(
                &context,
                &client,
                "service list",
                &["service-sandboxes.list"],
            )?;
            let summaries = client
                .list_service_sandboxes(
                    (!command.all_tenants).then_some(&context.control_plane.local_tenant_id),
                )
                .map_err(|error| {
                    machine_api_operation_error("list persisted sandbox state", &client, error)
                })?;
            let views = summaries
                .into_iter()
                .map(|summary| ServiceSandboxSummaryView {
                    sandbox_id: summary.sandbox_id,
                    tenant_id: summary.tenant_id,
                    service_name: summary.service_name,
                    status: summary.status,
                    published_endpoints: summary.published_endpoints,
                    restart_count: summary.restart_count,
                    last_exit_code: summary.last_exit_code,
                    shutdown_requested: summary.shutdown_requested,
                })
                .collect::<Vec<_>>();
            render_service_list_view(&views, command.format, command.no_heading)
        }
    }
}

async fn render_service_up_for_platform(
    command: &ServiceUpCommand,
    control_data_dir: &Path,
    host_platform: ServiceHostPlatform,
    machine_api_client: Option<MachineApiClient>,
) -> Result<String, Error> {
    let context = load_compose_project_context(&command.file, control_data_dir)?;
    let tenant = command
        .tenant
        .clone()
        .unwrap_or_else(|| context.control_plane.local_tenant_id.clone());
    let outcomes = service_up_outcomes_for_platform(
        command,
        control_data_dir,
        host_platform,
        machine_api_client,
    )
    .await?;
    Ok(render_service_lifecycle_action_summary(
        "Service up completed",
        &context.control_plane.project_name,
        &tenant,
        &outcomes,
    ))
}

async fn render_service_down_for_platform(
    command: &ServiceDownCommand,
    control_data_dir: &Path,
    host_platform: ServiceHostPlatform,
    machine_api_client: Option<MachineApiClient>,
) -> Result<String, Error> {
    let context = load_compose_project_context(&command.file, control_data_dir)?;
    let tenant = command
        .tenant
        .clone()
        .unwrap_or_else(|| context.control_plane.local_tenant_id.clone());
    let outcomes = service_down_outcomes_for_platform(
        command,
        control_data_dir,
        host_platform,
        machine_api_client,
    )
    .await?;
    Ok(render_service_lifecycle_action_summary(
        "Service down completed",
        &context.control_plane.project_name,
        &tenant,
        &outcomes,
    ))
}

fn render_service_inspect_for_platform(
    command: &ServiceInspectCommand,
    control_data_dir: &Path,
    host_platform: ServiceHostPlatform,
    machine_api_client: Option<MachineApiClient>,
) -> Result<String, Error> {
    let context = load_compose_project_context(&command.file, control_data_dir)?;
    let tenant = command
        .tenant
        .clone()
        .unwrap_or_else(|| context.control_plane.local_tenant_id.clone());
    match resolve_service_execution_surface(
        &context,
        Some(&command.service),
        "service inspect",
        host_platform,
        machine_api_client,
    )? {
        ServiceExecutionSurface::Krun { state_view, .. } => {
            let details = state_view
                .inspect_service(&tenant, &command.service)
                .map_err(|error| {
                    render_state_lookup_error("inspect persisted sandbox state", error)
                })?
                .ok_or_else(|| {
                    missing_persisted_service_error(
                        &context.control_plane.project_name,
                        &tenant,
                        &command.service,
                    )
                })?;
            render_service_inspect_view(&details, command.format, &command.service)
        }
        ServiceExecutionSurface::ForwardedContainer { client, .. } => {
            validate_forwarded_machine_api_operations(
                &context,
                &client,
                "service inspect",
                &["service-sandboxes.inspect-current"],
            )?;
            let details = lookup_current_remote_service_details(
                &context,
                &client,
                &tenant,
                &command.service,
                "inspect persisted sandbox state",
            )?
            .ok_or_else(|| {
                missing_persisted_service_error(
                    &context.control_plane.project_name,
                    &tenant,
                    &command.service,
                )
            })?;
            render_service_inspect_view(&details, command.format, &command.service)
        }
    }
}

fn render_service_ps_for_platform(
    command: &ServicePsCommand,
    control_data_dir: &Path,
    host_platform: ServiceHostPlatform,
    machine_api_client: Option<MachineApiClient>,
) -> Result<String, Error> {
    let snapshot = resolve_service_process_snapshot_for_platform(
        command,
        control_data_dir,
        host_platform,
        machine_api_client,
    )?;
    render_service_process_snapshot_view(&snapshot, command.format, command.no_heading)
}

#[cfg(test)]
#[allow(dead_code)]
async fn service_up_outcomes(
    command: &ServiceUpCommand,
    control_data_dir: &Path,
) -> Result<Vec<ServiceLifecycleOutcome>, Error> {
    service_up_outcomes_for_platform(
        command,
        control_data_dir,
        ServiceHostPlatform::current(),
        None,
    )
    .await
}

#[cfg(test)]
#[allow(dead_code)]
async fn service_down_outcomes(
    command: &ServiceDownCommand,
    control_data_dir: &Path,
) -> Result<Vec<ServiceLifecycleOutcome>, Error> {
    service_down_outcomes_for_platform(
        command,
        control_data_dir,
        ServiceHostPlatform::current(),
        None,
    )
    .await
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ServiceHostPlatform {
    Macos,
    Linux,
    Other,
}

impl ServiceHostPlatform {
    fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::Macos
        } else if cfg!(target_os = "linux") {
            Self::Linux
        } else {
            Self::Other
        }
    }
}

enum ServiceExecutionSurface {
    Krun {
        state_view: KrunSandboxStateView,
        backend: Arc<dyn SandboxBackend>,
    },
    ForwardedContainer {
        client: MachineApiClient,
        backend: Arc<dyn SandboxBackend>,
    },
}

fn control_data_dir_from_service_config(config: &ServicePersistenceConfig) -> &Path {
    match &config.control_plane {
        neovex::ControlPlaneConfig::EmbeddedRedb { data_dir } => data_dir.as_path(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum ServiceLifecycleAction {
    Started,
    AlreadyRunning,
    Stopped,
    AlreadyStopped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct ServiceLifecycleOutcome {
    action: ServiceLifecycleAction,
    tenant_id: TenantId,
    service_name: String,
    sandbox_id: neovex::SandboxId,
    status: SandboxStatus,
}

impl ServiceLifecycleOutcome {
    fn from_handle(
        action: ServiceLifecycleAction,
        tenant_id: &TenantId,
        service_name: &str,
        handle: SandboxHandle,
    ) -> Self {
        Self {
            action,
            tenant_id: tenant_id.clone(),
            service_name: service_name.to_owned(),
            sandbox_id: handle.id,
            status: handle.status,
        }
    }
}

impl ServiceLifecycleAction {
    fn as_str(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::AlreadyRunning => "already_running",
            Self::Stopped => "stopped",
            Self::AlreadyStopped => "already_stopped",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ServiceLifecycleTarget {
    sandbox_id: neovex::SandboxId,
    service_name: String,
    status: SandboxStatus,
}

impl ServiceLifecycleTarget {
    fn from_details(details: neovex_sandbox::backends::krun::KrunSandboxDetails) -> Self {
        Self {
            sandbox_id: details.summary.sandbox_id,
            service_name: details.summary.service_name,
            status: details.summary.status,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct ServiceProcessSnapshot {
    sandbox_id: neovex::SandboxId,
    tenant_id: TenantId,
    service_name: String,
    status: neovex::SandboxStatus,
    runtime_pidfile: PathBuf,
    conmon_pidfile: PathBuf,
    runtime_pid: Option<u32>,
    conmon_pid: Option<u32>,
    process_rows: Vec<ServiceProcessRow>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct ServiceProcessRow {
    pid: u32,
    ppid: u32,
    command: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct ServiceSandboxSummaryView {
    sandbox_id: neovex::SandboxId,
    tenant_id: TenantId,
    service_name: String,
    status: SandboxStatus,
    published_endpoints: Vec<PublishedEndpoint>,
    restart_count: u32,
    last_exit_code: Option<i32>,
    shutdown_requested: bool,
}

#[cfg(test)]
mod tests;
