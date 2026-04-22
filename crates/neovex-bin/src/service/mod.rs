use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;
use std::sync::Arc;

use neovex::{
    Error, SandboxBackend, SandboxServiceCatalog, SandboxServiceManager, ServicePersistenceConfig,
};
#[cfg(test)]
use neovex::{SandboxHandle, TenantId};
#[cfg(test)]
use neovex_sandbox::backends::krun::KrunSandboxStateView;

use crate::cli_ux;
use crate::machine::MachineApiClient;

mod commands;
mod compose;
mod execution;
mod lifecycle;
mod logs;
mod process;
mod project;
mod render;

pub(crate) use self::commands::ServiceCommand;
use self::commands::{
    ServiceConfigCommand, ServiceDownCommand, ServiceInspectCommand, ServiceListCommand,
    ServiceLogsCommand, ServicePsCommand, ServiceSubcommand, ServiceUpCommand,
};
#[allow(unused_imports)]
use self::commands::{ServiceInspectOutputFormat, ServiceListOutputFormat, ServicePsOutputFormat};
use self::execution::{
    ServiceExecutionSurface, ServiceHostPlatform,
    load_host_backed_sandbox_service_manager_for_platform,
    load_sandbox_service_catalog_for_execution_platform, lookup_current_remote_service_details,
    machine_api_operation_error, missing_persisted_service_error, render_state_lookup_error,
    requested_service_names, require_krun_backend_for_service_operation,
    resolve_remote_service_down_targets, resolve_service_down_targets,
    resolve_service_execution_surface, validate_forwarded_machine_api_backend,
    validate_forwarded_machine_api_operations,
};
#[allow(unused_imports)]
use self::lifecycle::{ServiceLifecycleAction, ServiceLifecycleTarget};
use self::lifecycle::{
    ServiceLifecycleOutcome, service_down_outcomes_for_platform, service_up_outcomes_for_platform,
};
use self::logs::run_service_logs_for_platform;
#[allow(unused_imports)]
use self::process::ServiceProcessRow;
use self::process::{ServiceProcessSnapshot, resolve_service_process_snapshot_for_platform};
use self::render::{
    ServiceSandboxSummaryView, render_service_inspect_view,
    render_service_lifecycle_action_summary, render_service_list_view,
    render_service_process_snapshot_view,
};
pub(crate) use project::ComposeProjectContext;

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

fn control_data_dir_from_service_config(config: &ServicePersistenceConfig) -> &Path {
    match &config.control_plane {
        neovex::ControlPlaneConfig::EmbeddedRedb { data_dir } => data_dir.as_path(),
    }
}

#[cfg(test)]
mod tests;
