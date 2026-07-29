use std::path::{Path, PathBuf};
use std::sync::Arc;

#[cfg(all(test, target_os = "linux"))]
use nimbus::ServiceManager;
use nimbus::{EnginePersistenceConfig, Error, SandboxBackend};
#[cfg(test)]
use nimbus::{SandboxHandle, TenantId};
use nimbus_operator::LocalNodeNetworkRoot;
#[cfg(test)]
use nimbus_sandbox::backends::krun::KrunSandboxStateView;

use crate::cli_ux;
use crate::machine::MachineApiClient;
use crate::network_composition::{PreparedLocalNetworkComposition, StagedLocalNetworkComposition};

mod commands;
pub(crate) mod discovery;
mod execution;
mod file;
mod lifecycle;
mod logs;
mod process;
mod project;
mod quadlet_export;
mod render;

pub(crate) use self::commands::ComposeCommand;
use self::commands::{
    ComposeConfigCommand, ComposeDownCommand, ComposeExportCommand, ComposeExportQuadletCommand,
    ComposeExportSubcommand, ComposeInspectCommand, ComposeLogsCommand, ComposePsCommand,
    ComposeQuadletExportMode, ComposeSubcommand, ComposeTopCommand, ComposeUpCommand,
};
pub(crate) use self::execution::load_forwarded_service_manager_for_selection_with_isolation_mode;
#[cfg(all(test, target_os = "linux"))]
use self::execution::load_host_backed_service_manager_for_platform_selection_with_admission;
pub(crate) use self::execution::prepare_local_service_manager_for_selection_with_isolation_mode;
use self::execution::{
    LocalKrunExecutionSurface, ServiceExecutionSurface, ServiceHostPlatform,
    load_service_definition_catalog_for_execution_platform, lookup_current_remote_service_details,
    machine_api_operation_error, missing_persisted_service_error, render_state_lookup_error,
    requested_service_names, require_krun_backend_for_service_operation,
    resolve_remote_service_down_targets, resolve_service_down_targets,
    resolve_service_execution_surface, validate_forwarded_machine_api_backend,
    validate_forwarded_machine_api_operations,
};
use self::lifecycle::{service_down_outcomes_for_selection, service_up_outcomes_for_selection};
use self::quadlet_export::run_compose_export_quadlet;
use self::render::{
    ServiceSandboxSummaryView, render_service_inspect_view,
    render_service_lifecycle_action_summary, render_service_list_view,
    render_service_sandbox_process_snapshot_view,
};
use crate::compose::discovery::{ResolvedComposeSelection, resolve_compose_selection};
pub(crate) use project::ComposeProjectContext;

pub(crate) async fn run_compose_command(
    command: ComposeCommand,
    persistence_config: &EnginePersistenceConfig,
) -> Result<(), Error> {
    let control_data_dir = control_data_dir_from_persistence_config(persistence_config);
    let network_state_dir = command.network_state_dir;
    match command.command {
        ComposeSubcommand::Config(config) => run_compose_config(config),
        ComposeSubcommand::Up(up) => {
            run_compose_up(up, control_data_dir, network_state_dir.as_deref()).await
        }
        ComposeSubcommand::Down(down) => {
            run_compose_down(down, control_data_dir, network_state_dir.as_deref()).await
        }
        ComposeSubcommand::Ps(list) => {
            run_compose_ps(list, control_data_dir, network_state_dir.as_deref())
        }
        ComposeSubcommand::Inspect(inspect) => {
            run_compose_inspect(inspect, control_data_dir, network_state_dir.as_deref())
        }
        ComposeSubcommand::Logs(logs) => {
            run_compose_logs(logs, control_data_dir, network_state_dir.as_deref())
        }
        ComposeSubcommand::Top(top) => {
            run_compose_top(top, control_data_dir, network_state_dir.as_deref())
        }
        ComposeSubcommand::Export(export) => run_compose_export(export),
    }
}

#[cfg(test)]
pub(crate) fn load_compose_project_context(
    file: &std::path::Path,
    control_data_dir: &std::path::Path,
) -> Result<ComposeProjectContext, Error> {
    load_compose_project_context_for_selection(
        &ResolvedComposeSelection::explicit(file.to_path_buf()),
        control_data_dir,
    )
}

pub(crate) fn load_compose_project_context_for_selection(
    selection: &ResolvedComposeSelection,
    control_data_dir: &std::path::Path,
) -> Result<ComposeProjectContext, Error> {
    ComposeProjectContext::load_selection(selection, control_data_dir)
}

pub(crate) fn load_compose_project_context_for_selection_with_isolation_mode(
    selection: &ResolvedComposeSelection,
    control_data_dir: &std::path::Path,
    tenant_isolation_mode: nimbus_tenant::TenantIsolationMode,
) -> Result<ComposeProjectContext, Error> {
    ComposeProjectContext::load_selection_with_admission(
        selection,
        control_data_dir,
        compose_admission_mode_for_tenant_isolation(tenant_isolation_mode),
    )
}

#[cfg(all(test, target_os = "linux"))]
pub(crate) fn load_host_backed_service_manager_for_selection_with_isolation_mode(
    selection: &ResolvedComposeSelection,
    control_data_dir: &std::path::Path,
    tenant_isolation_mode: nimbus_tenant::TenantIsolationMode,
) -> Result<ServiceManager, Error> {
    load_host_backed_service_manager_for_platform_selection_with_admission(
        selection,
        control_data_dir,
        ServiceHostPlatform::current(),
        None,
        compose_admission_mode_for_tenant_isolation(tenant_isolation_mode),
    )
}

fn compose_admission_mode_for_tenant_isolation(
    tenant_isolation_mode: nimbus_tenant::TenantIsolationMode,
) -> file::ComposeAdmissionMode {
    match tenant_isolation_mode {
        nimbus_tenant::TenantIsolationMode::LocalDevelopment => {
            file::ComposeAdmissionMode::LocalDevelopment
        }
        nimbus_tenant::TenantIsolationMode::Production => file::ComposeAdmissionMode::Production,
    }
}

fn run_compose_config(command: ComposeConfigCommand) -> Result<(), Error> {
    let selection = resolve_required_compose_selection(command.file.as_slice())?;
    let rendered = file::render_compose_project_selection(&selection, command.services)?;

    for warning in rendered.warnings {
        cli_ux::write_stderr_prefixed_line("Warning:", &warning).map_err(|error| {
            Error::InvalidInput(format!("failed to write warning output: {error}"))
        })?;
    }

    emit_service_stdout(&rendered.stdout)?;
    Ok(())
}

async fn run_compose_up(
    command: ComposeUpCommand,
    control_data_dir: &Path,
    network_state_dir: Option<&Path>,
) -> Result<(), Error> {
    let selection = resolve_required_compose_selection(command.file.as_slice())?;
    let prepared =
        prepare_standalone_compose_network(&selection, control_data_dir, network_state_dir)?;
    let rendered = render_service_up_for_selection(
        &command,
        &selection,
        control_data_dir,
        ServiceHostPlatform::current(),
        None,
        local_krun_execution(&prepared),
    )
    .await?;
    emit_service_stdout(&rendered)?;
    Ok(())
}

async fn run_compose_down(
    command: ComposeDownCommand,
    control_data_dir: &Path,
    network_state_dir: Option<&Path>,
) -> Result<(), Error> {
    let selection = resolve_required_compose_selection(command.file.as_slice())?;
    let prepared =
        prepare_standalone_compose_network(&selection, control_data_dir, network_state_dir)?;
    let rendered = render_service_down_for_selection(
        &command,
        &selection,
        control_data_dir,
        ServiceHostPlatform::current(),
        None,
        local_krun_execution(&prepared),
    )
    .await?;
    emit_service_stdout(&rendered)?;
    Ok(())
}

fn run_compose_ps(
    command: ComposePsCommand,
    control_data_dir: &Path,
    network_state_dir: Option<&Path>,
) -> Result<(), Error> {
    let selection = resolve_required_compose_selection(command.file.as_slice())?;
    let prepared =
        prepare_standalone_compose_network(&selection, control_data_dir, network_state_dir)?;
    let rendered = render_service_list_for_selection(
        &command,
        &selection,
        control_data_dir,
        ServiceHostPlatform::current(),
        None,
        local_krun_execution(&prepared),
    )?;
    emit_service_stdout(&rendered)?;
    Ok(())
}

fn run_compose_inspect(
    command: ComposeInspectCommand,
    control_data_dir: &Path,
    network_state_dir: Option<&Path>,
) -> Result<(), Error> {
    let selection = resolve_required_compose_selection(command.file.as_slice())?;
    let prepared =
        prepare_standalone_compose_network(&selection, control_data_dir, network_state_dir)?;
    let rendered = render_service_inspect_for_selection(
        &command,
        &selection,
        control_data_dir,
        ServiceHostPlatform::current(),
        None,
        local_krun_execution(&prepared),
    )?;
    emit_service_stdout(&rendered)?;
    Ok(())
}

fn run_compose_logs(
    command: ComposeLogsCommand,
    control_data_dir: &Path,
    network_state_dir: Option<&Path>,
) -> Result<(), Error> {
    let selection = resolve_required_compose_selection(command.file.as_slice())?;
    let prepared =
        prepare_standalone_compose_network(&selection, control_data_dir, network_state_dir)?;
    self::logs::run_compose_logs_for_selection(
        &command,
        &selection,
        control_data_dir,
        ServiceHostPlatform::current(),
        None,
        local_krun_execution(&prepared),
    )
}

fn run_compose_export(command: ComposeExportCommand) -> Result<(), Error> {
    match command.command {
        ComposeExportSubcommand::Quadlet(command) => run_compose_export_quadlet(command),
    }
}

fn run_compose_top(
    command: ComposeTopCommand,
    control_data_dir: &Path,
    network_state_dir: Option<&Path>,
) -> Result<(), Error> {
    let selection = resolve_required_compose_selection(command.file.as_slice())?;
    let prepared =
        prepare_standalone_compose_network(&selection, control_data_dir, network_state_dir)?;
    let snapshot = self::process::resolve_service_sandbox_process_snapshot_for_selection(
        &command,
        &selection,
        control_data_dir,
        ServiceHostPlatform::current(),
        None,
        local_krun_execution(&prepared),
    )?;
    let rendered = render_service_sandbox_process_snapshot_view(
        &snapshot,
        command.format,
        command.no_heading,
    )?;
    emit_service_stdout(&rendered)?;
    Ok(())
}

fn prepare_standalone_compose_network(
    selection: &ResolvedComposeSelection,
    control_data_dir: &Path,
    explicit_network_state_dir: Option<&Path>,
) -> Result<PreparedLocalNetworkComposition, Error> {
    let root = LocalNodeNetworkRoot::resolve_for_current_platform(explicit_network_state_dir)
        .map_err(|error| {
            Error::InvalidInput(format!("invalid local node network root: {error}"))
        })?;
    let staged = StagedLocalNetworkComposition::claim(&root)
        .map_err(|error| Error::InvalidInput(error.to_string()))?;
    PreparedLocalNetworkComposition::prepare_attachment_only(staged, selection, control_data_dir)
        .map_err(|error| Error::InvalidInput(error.to_string()))
}

fn local_krun_execution(
    prepared: &PreparedLocalNetworkComposition,
) -> Option<LocalKrunExecutionSurface> {
    let backend = prepared.local_krun_backend()?;
    let state_view = prepared.local_krun_state_view()?;
    let backend: Arc<dyn SandboxBackend> = backend;
    Some(LocalKrunExecutionSurface {
        state_view,
        backend,
    })
}

fn emit_service_stdout(rendered: &str) -> Result<(), Error> {
    cli_ux::write_stdout(rendered)
        .map_err(|error| Error::Internal(format!("failed to write compose output: {error}")))
}

pub(crate) fn resolve_required_compose_selection(
    explicit_files: &[PathBuf],
) -> Result<ResolvedComposeSelection, Error> {
    let cwd = std::env::current_dir().map_err(|error| {
        Error::Internal(format!("failed to determine current directory: {error}"))
    })?;
    match resolve_compose_selection(explicit_files, &cwd) {
        Ok(Some(selection)) => Ok(selection),
        Ok(None) => Err(Error::InvalidInput(format!(
            "no Compose file found from {} or its parent directories; create compose.yaml, compose.yml, docker-compose.yaml, or docker-compose.yml, or pass --file",
            cwd.display()
        ))),
        Err(error) => Err(Error::InvalidInput(error.to_string())),
    }
}

#[cfg(test)]
fn render_service_list_for_platform(
    command: &ComposePsCommand,
    control_data_dir: &Path,
    host_platform: ServiceHostPlatform,
    machine_api_client: Option<MachineApiClient>,
) -> Result<String, Error> {
    let selection = resolve_required_compose_selection(command.file.as_slice())?;
    render_service_list_for_selection(
        command,
        &selection,
        control_data_dir,
        host_platform,
        machine_api_client,
        None,
    )
}

fn render_service_list_for_selection(
    command: &ComposePsCommand,
    selection: &ResolvedComposeSelection,
    control_data_dir: &Path,
    host_platform: ServiceHostPlatform,
    machine_api_client: Option<MachineApiClient>,
    local_krun: Option<LocalKrunExecutionSurface>,
) -> Result<String, Error> {
    let context = load_compose_project_context_for_selection(selection, control_data_dir)?;
    match resolve_service_execution_surface(
        &context,
        None,
        "compose ps",
        host_platform,
        machine_api_client,
        local_krun,
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
                "compose ps",
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

#[cfg(test)]
async fn render_service_up_for_platform(
    command: &ComposeUpCommand,
    control_data_dir: &Path,
    host_platform: ServiceHostPlatform,
    machine_api_client: Option<MachineApiClient>,
) -> Result<String, Error> {
    let selection = resolve_required_compose_selection(command.file.as_slice())?;
    render_service_up_for_selection(
        command,
        &selection,
        control_data_dir,
        host_platform,
        machine_api_client,
        None,
    )
    .await
}

async fn render_service_up_for_selection(
    command: &ComposeUpCommand,
    selection: &ResolvedComposeSelection,
    control_data_dir: &Path,
    host_platform: ServiceHostPlatform,
    machine_api_client: Option<MachineApiClient>,
    local_krun: Option<LocalKrunExecutionSurface>,
) -> Result<String, Error> {
    let context = load_compose_project_context_for_selection(selection, control_data_dir)?;
    let tenant = command
        .tenant
        .clone()
        .unwrap_or_else(|| context.control_plane.local_tenant_id.clone());
    let outcomes = service_up_outcomes_for_selection(
        command,
        selection,
        control_data_dir,
        host_platform,
        machine_api_client,
        local_krun,
    )
    .await?;
    Ok(render_service_lifecycle_action_summary(
        "Compose up completed",
        &context.control_plane.project_name,
        &tenant,
        &outcomes,
    ))
}

#[cfg(test)]
async fn render_service_down_for_platform(
    command: &ComposeDownCommand,
    control_data_dir: &Path,
    host_platform: ServiceHostPlatform,
    machine_api_client: Option<MachineApiClient>,
) -> Result<String, Error> {
    let selection = resolve_required_compose_selection(command.file.as_slice())?;
    render_service_down_for_selection(
        command,
        &selection,
        control_data_dir,
        host_platform,
        machine_api_client,
        None,
    )
    .await
}

async fn render_service_down_for_selection(
    command: &ComposeDownCommand,
    selection: &ResolvedComposeSelection,
    control_data_dir: &Path,
    host_platform: ServiceHostPlatform,
    machine_api_client: Option<MachineApiClient>,
    local_krun: Option<LocalKrunExecutionSurface>,
) -> Result<String, Error> {
    let context = load_compose_project_context_for_selection(selection, control_data_dir)?;
    let tenant = command
        .tenant
        .clone()
        .unwrap_or_else(|| context.control_plane.local_tenant_id.clone());
    let outcomes = service_down_outcomes_for_selection(
        command,
        selection,
        control_data_dir,
        host_platform,
        machine_api_client,
        local_krun,
    )
    .await?;
    Ok(render_service_lifecycle_action_summary(
        "Compose down completed",
        &context.control_plane.project_name,
        &tenant,
        &outcomes,
    ))
}

#[cfg(test)]
fn render_service_inspect_for_platform(
    command: &ComposeInspectCommand,
    control_data_dir: &Path,
    host_platform: ServiceHostPlatform,
    machine_api_client: Option<MachineApiClient>,
) -> Result<String, Error> {
    let selection = resolve_required_compose_selection(command.file.as_slice())?;
    render_service_inspect_for_selection(
        command,
        &selection,
        control_data_dir,
        host_platform,
        machine_api_client,
        None,
    )
}

fn render_service_inspect_for_selection(
    command: &ComposeInspectCommand,
    selection: &ResolvedComposeSelection,
    control_data_dir: &Path,
    host_platform: ServiceHostPlatform,
    machine_api_client: Option<MachineApiClient>,
    local_krun: Option<LocalKrunExecutionSurface>,
) -> Result<String, Error> {
    let context = load_compose_project_context_for_selection(selection, control_data_dir)?;
    let tenant = command
        .tenant
        .clone()
        .unwrap_or_else(|| context.control_plane.local_tenant_id.clone());
    match resolve_service_execution_surface(
        &context,
        Some(&command.service),
        "compose inspect",
        host_platform,
        machine_api_client,
        local_krun,
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
                "compose inspect",
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

#[cfg(test)]
fn render_compose_top_for_platform(
    command: &ComposeTopCommand,
    control_data_dir: &Path,
    host_platform: ServiceHostPlatform,
    machine_api_client: Option<MachineApiClient>,
) -> Result<String, Error> {
    let selection = resolve_required_compose_selection(command.file.as_slice())?;
    let snapshot = self::process::resolve_service_sandbox_process_snapshot_for_selection(
        command,
        &selection,
        control_data_dir,
        host_platform,
        machine_api_client,
        None,
    )?;
    render_service_sandbox_process_snapshot_view(&snapshot, command.format, command.no_heading)
}

fn control_data_dir_from_persistence_config(config: &EnginePersistenceConfig) -> &Path {
    match &config.control_plane {
        nimbus::ControlPlaneConfig::EmbeddedRedb { data_dir } => data_dir.as_path(),
    }
}

#[cfg(test)]
mod tests;
