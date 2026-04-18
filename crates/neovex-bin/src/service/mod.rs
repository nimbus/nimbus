use std::collections::BTreeSet;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use clap::{Args, Subcommand};
use neovex::{
    Error, SandboxBackend, SandboxBackendKind, SandboxHandle, SandboxServiceCatalog,
    SandboxServiceLaunch, SandboxServiceManager, SandboxStatus, ServicePersistenceConfig, TenantId,
};
use neovex_sandbox::backends::krun::{KrunSandboxBackend, KrunSandboxStateView};
use serde::Serialize;

use crate::machine::{
    ForwardedMachineApiSandboxBackend, MachineApiClient, MachineApiServiceSandboxDetails,
    ensure_default_machine_api_client_started, require_default_machine_api_client,
};

mod compose;
mod project;

pub(crate) use project::ComposeProjectContext;

#[derive(Debug, Args)]
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
struct ServiceConfigCommand {
    /// Compose file to read. Defaults to ./compose.yaml.
    #[arg(long, default_value = compose::DEFAULT_COMPOSE_FILE)]
    file: PathBuf,

    /// Print only service names, one per line.
    #[arg(long)]
    services: bool,
}

#[derive(Debug, Args)]
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
struct ServiceListCommand {
    /// Compose file to read. Defaults to ./compose.yaml.
    #[arg(long, default_value = compose::DEFAULT_COMPOSE_FILE)]
    file: PathBuf,

    /// Show all tenants under the project-scoped backend root, not just the
    /// deterministic local project tenant.
    #[arg(long)]
    all_tenants: bool,
}

#[derive(Debug, Args)]
struct ServiceInspectCommand {
    /// Service name to inspect.
    service: String,

    /// Compose file to read. Defaults to ./compose.yaml.
    #[arg(long, default_value = compose::DEFAULT_COMPOSE_FILE)]
    file: PathBuf,

    /// Optional tenant override. Defaults to the deterministic local project tenant.
    #[arg(long)]
    tenant: Option<TenantId>,
}

#[derive(Debug, Args)]
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
struct ServicePsCommand {
    /// Service name to inspect process state for.
    service: String,

    /// Compose file to read. Defaults to ./compose.yaml.
    #[arg(long, default_value = compose::DEFAULT_COMPOSE_FILE)]
    file: PathBuf,

    /// Optional tenant override. Defaults to the deterministic local project tenant.
    #[arg(long)]
    tenant: Option<TenantId>,
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

fn load_host_backed_sandbox_service_manager_for_platform(
    file: &std::path::Path,
    control_data_dir: &std::path::Path,
    host_platform: ServiceHostPlatform,
    machine_api_client: Option<MachineApiClient>,
) -> Result<SandboxServiceManager, Error> {
    let context = load_compose_project_context(file, control_data_dir)?;
    let machine_api_client = match machine_api_client {
        Some(client) => Some(client),
        None if should_auto_start_default_machine_for_host_loader(&context, host_platform)? => {
            Some(ensure_default_machine_api_client_started()?)
        }
        None => None,
    };
    let backend = load_host_backed_project_backend(&context, host_platform, machine_api_client)?;
    Ok(SandboxServiceManager::new(
        load_sandbox_service_catalog_for_execution_platform(file, host_platform)?,
        backend,
    ))
}

fn should_auto_start_default_machine_for_host_loader(
    context: &ComposeProjectContext,
    host_platform: ServiceHostPlatform,
) -> Result<bool, Error> {
    if host_platform != ServiceHostPlatform::Macos {
        return Ok(false);
    }

    Ok(matches!(
        required_effective_project_backend(
            context,
            None,
            "load a compose-backed sandbox manager",
            host_platform,
        )?,
        SandboxBackendKind::Container
    ))
}

fn run_service_config(command: ServiceConfigCommand) -> Result<(), Error> {
    let rendered = compose::render_compose_project(&command.file, command.services)?;

    for warning in rendered.warnings {
        writeln!(io::stderr(), "Warning: {warning}").map_err(|error| {
            Error::InvalidInput(format!("failed to write warning output: {error}"))
        })?;
    }

    print!("{}", rendered.stdout);
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
    print!("{rendered}");
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
    print!("{rendered}");
    Ok(())
}

fn run_service_list(command: ServiceListCommand, control_data_dir: &Path) -> Result<(), Error> {
    let rendered = render_service_list_for_platform(
        &command,
        control_data_dir,
        ServiceHostPlatform::current(),
        None,
    )?;
    print!("{rendered}");
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
    print!("{rendered}");
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
    print!("{rendered}");
    Ok(())
}

#[cfg(test)]
#[allow(dead_code)]
async fn render_service_up(
    command: &ServiceUpCommand,
    control_data_dir: &Path,
) -> Result<String, Error> {
    let outcomes = service_up_outcomes_for_platform(
        command,
        control_data_dir,
        ServiceHostPlatform::current(),
        None,
    )
    .await?;
    serde_yaml::to_string(&outcomes).map_err(|error| {
        Error::Serialization(format!("failed to render service up results: {error}"))
    })
}

#[cfg(test)]
#[allow(dead_code)]
async fn render_service_down(
    command: &ServiceDownCommand,
    control_data_dir: &Path,
) -> Result<String, Error> {
    let outcomes = service_down_outcomes_for_platform(
        command,
        control_data_dir,
        ServiceHostPlatform::current(),
        None,
    )
    .await?;
    serde_yaml::to_string(&outcomes).map_err(|error| {
        Error::Serialization(format!("failed to render service down results: {error}"))
    })
}

fn render_state_lookup_error(operation: &str, error: neovex::SandboxError) -> Error {
    Error::Internal(format!("failed to {operation}: {error}"))
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
            serde_yaml::to_string(&summaries).map_err(|error| {
                Error::Serialization(format!("failed to render service list: {error}"))
            })
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
            serde_yaml::to_string(&summaries).map_err(|error| {
                Error::Serialization(format!("failed to render service list: {error}"))
            })
        }
    }
}

async fn render_service_up_for_platform(
    command: &ServiceUpCommand,
    control_data_dir: &Path,
    host_platform: ServiceHostPlatform,
    machine_api_client: Option<MachineApiClient>,
) -> Result<String, Error> {
    let outcomes = service_up_outcomes_for_platform(
        command,
        control_data_dir,
        host_platform,
        machine_api_client,
    )
    .await?;
    serde_yaml::to_string(&outcomes).map_err(|error| {
        Error::Serialization(format!("failed to render service up results: {error}"))
    })
}

async fn render_service_down_for_platform(
    command: &ServiceDownCommand,
    control_data_dir: &Path,
    host_platform: ServiceHostPlatform,
    machine_api_client: Option<MachineApiClient>,
) -> Result<String, Error> {
    let outcomes = service_down_outcomes_for_platform(
        command,
        control_data_dir,
        host_platform,
        machine_api_client,
    )
    .await?;
    serde_yaml::to_string(&outcomes).map_err(|error| {
        Error::Serialization(format!("failed to render service down results: {error}"))
    })
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
            serde_yaml::to_string(&details).map_err(|error| {
                Error::Serialization(format!(
                    "failed to render sandbox details for service {}: {error}",
                    command.service
                ))
            })
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
            serde_yaml::to_string(&details).map_err(|error| {
                Error::Serialization(format!(
                    "failed to render sandbox details for service {}: {error}",
                    command.service
                ))
            })
        }
    }
}

fn run_service_logs_for_platform(
    command: &ServiceLogsCommand,
    control_data_dir: &Path,
    host_platform: ServiceHostPlatform,
    machine_api_client: Option<MachineApiClient>,
) -> Result<(), Error> {
    let context = load_compose_project_context(&command.file, control_data_dir)?;
    let tenant = command
        .tenant
        .clone()
        .unwrap_or_else(|| context.control_plane.local_tenant_id.clone());
    match resolve_service_execution_surface(
        &context,
        Some(&command.service),
        "service logs",
        host_platform,
        machine_api_client,
    )? {
        ServiceExecutionSurface::Krun { .. } => {
            let log_path = resolve_service_ctr_log_path(command, control_data_dir)?;
            let mut offset = 0;
            loop {
                let (chunk, next_offset) = read_log_chunk(&log_path, offset)?;
                flush_service_log_chunk(&command.service, &chunk)?;
                offset = next_offset;
                if !command.follow {
                    return Ok(());
                }
                thread::sleep(Duration::from_millis(250));
            }
        }
        ServiceExecutionSurface::ForwardedContainer { client, .. } => {
            validate_forwarded_machine_api_operations(
                &context,
                &client,
                "service logs",
                &[
                    "service-sandboxes.inspect-current",
                    "service-sandboxes.logs",
                ],
            )?;
            let details = lookup_current_remote_service_details(
                &context,
                &client,
                &tenant,
                &command.service,
                "resolve persisted service logs",
            )?
            .ok_or_else(|| {
                missing_persisted_service_error(
                    &context.control_plane.project_name,
                    &tenant,
                    &command.service,
                )
            })?;
            let mut offset = 0;
            loop {
                let response = client
                    .read_service_sandbox_log_chunk(&details.summary.sandbox_id, offset)
                    .map_err(|error| {
                        machine_api_operation_error("read persisted service logs", &client, error)
                    })?;
                flush_service_log_chunk(&command.service, &response.chunk)?;
                offset = response.next_offset;
                if !command.follow {
                    return Ok(());
                }
                thread::sleep(Duration::from_millis(250));
            }
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
    serde_yaml::to_string(&snapshot)
        .map_err(|error| Error::Serialization(format!("failed to render service ps: {error}")))
}

async fn service_up_outcomes_for_platform(
    command: &ServiceUpCommand,
    control_data_dir: &Path,
    host_platform: ServiceHostPlatform,
    machine_api_client: Option<MachineApiClient>,
) -> Result<Vec<ServiceLifecycleOutcome>, Error> {
    let context = load_compose_project_context(&command.file, control_data_dir)?;
    let tenant = command
        .tenant
        .clone()
        .unwrap_or_else(|| context.control_plane.local_tenant_id.clone());
    let service_names = requested_service_names(&context, command.service.as_deref())?;
    let service_catalog =
        load_sandbox_service_catalog_for_execution_platform(&command.file, host_platform)?;

    match resolve_service_execution_surface(
        &context,
        command.service.as_deref(),
        "service up",
        host_platform,
        machine_api_client,
    )? {
        ServiceExecutionSurface::Krun {
            state_view,
            backend,
        } => {
            let mut outcomes = Vec::new();
            for service_name in service_names {
                if let Some(handle) = resolve_live_service_handle(
                    &state_view,
                    backend.as_ref(),
                    &tenant,
                    &service_name,
                )
                .await?
                {
                    outcomes.push(ServiceLifecycleOutcome::from_handle(
                        ServiceLifecycleAction::AlreadyRunning,
                        &tenant,
                        &service_name,
                        handle,
                    ));
                    continue;
                }

                let launch = service_catalog
                    .sandbox_service_for_tenant(&tenant, &service_name)
                    .ok_or_else(|| {
                        Error::InvalidInput(format!(
                            "service {} is not declared in compose project {}",
                            service_name, context.control_plane.project_name
                        ))
                    })?;
                let handle =
                    start_service_launch(backend.as_ref(), &tenant, &service_name, launch).await?;
                outcomes.push(ServiceLifecycleOutcome::from_handle(
                    ServiceLifecycleAction::Started,
                    &tenant,
                    &service_name,
                    handle,
                ));
            }
            Ok(outcomes)
        }
        ServiceExecutionSurface::ForwardedContainer { client, backend } => {
            validate_forwarded_machine_api_backend(&context, &client)?;
            let mut outcomes = Vec::new();
            for service_name in service_names {
                if let Some(details) = lookup_current_remote_service_details(
                    &context,
                    &client,
                    &tenant,
                    &service_name,
                    "resolve persisted sandbox state",
                )? && is_active_status(details.summary.status)
                {
                    outcomes.push(ServiceLifecycleOutcome {
                        action: ServiceLifecycleAction::AlreadyRunning,
                        tenant_id: details.summary.tenant_id,
                        service_name: details.summary.service_name,
                        sandbox_id: details.summary.sandbox_id,
                        status: details.summary.status,
                    });
                    continue;
                }

                let launch = service_catalog
                    .sandbox_service_for_tenant(&tenant, &service_name)
                    .ok_or_else(|| {
                        Error::InvalidInput(format!(
                            "service {} is not declared in compose project {}",
                            service_name, context.control_plane.project_name
                        ))
                    })?;
                let handle =
                    start_service_launch(backend.as_ref(), &tenant, &service_name, launch).await?;
                outcomes.push(ServiceLifecycleOutcome::from_handle(
                    ServiceLifecycleAction::Started,
                    &tenant,
                    &service_name,
                    handle,
                ));
            }
            Ok(outcomes)
        }
    }
}

async fn service_down_outcomes_for_platform(
    command: &ServiceDownCommand,
    control_data_dir: &Path,
    host_platform: ServiceHostPlatform,
    machine_api_client: Option<MachineApiClient>,
) -> Result<Vec<ServiceLifecycleOutcome>, Error> {
    let context = load_compose_project_context(&command.file, control_data_dir)?;
    let tenant = command
        .tenant
        .clone()
        .unwrap_or_else(|| context.control_plane.local_tenant_id.clone());

    match resolve_service_execution_surface(
        &context,
        command.service.as_deref(),
        "service down",
        host_platform,
        machine_api_client,
    )? {
        ServiceExecutionSurface::Krun {
            state_view,
            backend,
        } => {
            let targets = resolve_service_down_targets(
                &state_view,
                &tenant,
                command.service.as_deref(),
                &context.control_plane.project_name,
            )?;
            let mut outcomes = Vec::new();
            for target in targets {
                outcomes.push(stop_service_target(backend.as_ref(), &tenant, target).await?);
            }
            Ok(outcomes)
        }
        ServiceExecutionSurface::ForwardedContainer { client, backend } => {
            let required_operations = if command.service.is_some() {
                vec![
                    "service-sandboxes.inspect-current",
                    "service-sandboxes.stop",
                ]
            } else {
                vec![
                    "service-sandboxes.list",
                    "service-sandboxes.inspect-current",
                    "service-sandboxes.stop",
                ]
            };
            validate_forwarded_machine_api_operations(
                &context,
                &client,
                "service down",
                &required_operations,
            )?;
            let targets = resolve_remote_service_down_targets(
                &context,
                &client,
                &tenant,
                command.service.as_deref(),
            )?;
            let mut outcomes = Vec::new();
            for target in targets {
                outcomes.push(stop_service_target(backend.as_ref(), &tenant, target).await?);
            }
            Ok(outcomes)
        }
    }
}

fn flush_service_log_chunk(service_name: &str, chunk: &str) -> Result<(), Error> {
    if chunk.is_empty() {
        return Ok(());
    }
    print!("{chunk}");
    io::stdout().flush().map_err(|error| {
        Error::Internal(format!(
            "failed to flush service logs for {}: {error}",
            service_name
        ))
    })
}

fn lookup_current_remote_service_details(
    _context: &ComposeProjectContext,
    client: &MachineApiClient,
    tenant: &TenantId,
    service_name: &str,
    operation: &str,
) -> Result<Option<MachineApiServiceSandboxDetails>, Error> {
    client
        .inspect_current_service_sandbox(tenant, service_name)
        .map(|response| response.details)
        .map_err(|error| machine_api_operation_error(operation, client, error))
}

fn resolve_remote_service_down_targets(
    context: &ComposeProjectContext,
    client: &MachineApiClient,
    tenant: &TenantId,
    requested_service: Option<&str>,
) -> Result<Vec<ServiceLifecycleTarget>, Error> {
    match requested_service {
        Some(service_name) => {
            let details = lookup_current_remote_service_details(
                context,
                client,
                tenant,
                service_name,
                "resolve persisted sandbox state",
            )?
            .ok_or_else(|| {
                missing_persisted_service_error(
                    &context.control_plane.project_name,
                    tenant,
                    service_name,
                )
            })?;
            Ok(vec![ServiceLifecycleTarget {
                sandbox_id: details.summary.sandbox_id,
                service_name: details.summary.service_name,
                status: details.summary.status,
            }])
        }
        None => {
            let service_names = client
                .list_service_sandboxes(Some(tenant))
                .map_err(|error| {
                    machine_api_operation_error("list persisted sandbox state", client, error)
                })?
                .into_iter()
                .map(|summary| summary.service_name)
                .collect::<BTreeSet<_>>();

            service_names
                .into_iter()
                .map(|service_name| {
                    lookup_current_remote_service_details(
                        context,
                        client,
                        tenant,
                        &service_name,
                        "resolve persisted sandbox state",
                    )?
                    .map(|details| ServiceLifecycleTarget {
                        sandbox_id: details.summary.sandbox_id,
                        service_name: details.summary.service_name,
                        status: details.summary.status,
                    })
                    .ok_or_else(|| {
                        Error::Internal(format!(
                            "persisted sandbox state changed while resolving service {} in tenant {} under project {}",
                            service_name, tenant, context.control_plane.project_name
                        ))
                    })
                })
                .collect()
        }
    }
}

fn missing_persisted_service_error(
    project_name: &str,
    tenant: &TenantId,
    service_name: &str,
) -> Error {
    Error::InvalidInput(format!(
        "no persisted sandbox state found for service {} in tenant {} under project {}",
        service_name, tenant, project_name
    ))
}

fn machine_api_operation_error(operation: &str, client: &MachineApiClient, error: Error) -> Error {
    Error::Internal(format!(
        "failed to {operation} through the default machine API at {}: {error}",
        client.socket_path().display()
    ))
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

fn requested_service_names(
    context: &ComposeProjectContext,
    requested_service: Option<&str>,
) -> Result<Vec<String>, Error> {
    match requested_service {
        Some(service_name) => {
            if !context.plan.services.contains_key(service_name) {
                return Err(Error::InvalidInput(format!(
                    "service {} is not declared in compose project {}",
                    service_name, context.control_plane.project_name
                )));
            }
            Ok(vec![service_name.to_owned()])
        }
        None => Ok(context.plan.services.keys().cloned().collect()),
    }
}

fn require_krun_backend_for_service_operation(
    context: &ComposeProjectContext,
    requested_service: Option<&str>,
    operation: &str,
) -> Result<(), Error> {
    let backend = required_project_backend(context, requested_service, operation)?;
    if backend == SandboxBackendKind::Krun {
        return Ok(());
    }

    match requested_service {
        Some(service_name) => Err(Error::InvalidInput(format!(
            "service {} in compose project {} selects sandbox backend {}, but neovex {} only supports the krun backend today",
            service_name,
            context.control_plane.project_name,
            sandbox_backend_name(backend),
            operation,
        ))),
        None => Err(Error::InvalidInput(format!(
            "compose project {} selects sandbox backend {}, but neovex {} only supports the krun backend today",
            context.control_plane.project_name,
            sandbox_backend_name(backend),
            operation,
        ))),
    }
}

fn required_project_backend(
    context: &ComposeProjectContext,
    requested_service: Option<&str>,
    operation: &str,
) -> Result<SandboxBackendKind, Error> {
    match requested_service {
        Some(service_name) => context
            .plan
            .services
            .get(service_name)
            .map(|service| service.backend)
            .ok_or_else(|| {
                Error::InvalidInput(format!(
                    "service {} is not declared in compose project {}",
                    service_name, context.control_plane.project_name
                ))
            }),
        None => {
            let mut services = context.plan.services.iter();
            let Some((_, first_service)) = services.next() else {
                return Err(Error::InvalidInput(format!(
                    "compose project {} does not declare any services",
                    context.control_plane.project_name
                )));
            };
            let first_backend = first_service.backend;
            if services.any(|(_, service)| service.backend != first_backend) {
                return Err(Error::InvalidInput(format!(
                    "compose project {} mixes sandbox backends across services ({}); neovex {} currently requires one backend family per project-wide operation",
                    context.control_plane.project_name,
                    project_backend_assignments(context),
                    operation,
                )));
            }
            Ok(first_backend)
        }
    }
}

fn load_sandbox_service_catalog_for_execution_platform(
    file: &Path,
    host_platform: ServiceHostPlatform,
) -> Result<Arc<dyn SandboxServiceCatalog>, Error> {
    let mut plan = compose::ComposeProjectPlan::load(file)?;
    apply_platform_backend_defaults(&mut plan, host_platform);
    Ok(Arc::new(plan.into_service_catalog()?))
}

fn apply_platform_backend_defaults(
    plan: &mut compose::ComposeProjectPlan,
    host_platform: ServiceHostPlatform,
) {
    if host_platform != ServiceHostPlatform::Macos {
        return;
    }

    for service in plan.services.values_mut() {
        if service.backend == SandboxBackendKind::Krun && !service_declares_backend(service) {
            service.backend = SandboxBackendKind::Container;
        }
    }
}

fn effective_service_backend(
    service: &compose::ComposeServicePlan,
    host_platform: ServiceHostPlatform,
) -> SandboxBackendKind {
    if host_platform == ServiceHostPlatform::Macos
        && service.backend == SandboxBackendKind::Krun
        && !service_declares_backend(service)
    {
        SandboxBackendKind::Container
    } else {
        service.backend
    }
}

fn service_declares_backend(service: &compose::ComposeServicePlan) -> bool {
    service
        .x_neovex
        .as_ref()
        .and_then(|extensions| extensions.backend)
        .is_some()
}

fn required_effective_project_backend(
    context: &ComposeProjectContext,
    requested_service: Option<&str>,
    operation: &str,
    host_platform: ServiceHostPlatform,
) -> Result<SandboxBackendKind, Error> {
    match requested_service {
        Some(service_name) => context
            .plan
            .services
            .get(service_name)
            .map(|service| effective_service_backend(service, host_platform))
            .ok_or_else(|| {
                Error::InvalidInput(format!(
                    "service {} is not declared in compose project {}",
                    service_name, context.control_plane.project_name
                ))
            }),
        None => {
            let mut services = context.plan.services.iter();
            let Some((_, first_service)) = services.next() else {
                return Err(Error::InvalidInput(format!(
                    "compose project {} does not declare any services",
                    context.control_plane.project_name
                )));
            };
            let first_backend = effective_service_backend(first_service, host_platform);
            if services.any(|(_, service)| {
                effective_service_backend(service, host_platform) != first_backend
            }) {
                return Err(Error::InvalidInput(format!(
                    "compose project {} mixes sandbox backends across services ({}); neovex {} currently requires one backend family per project-wide operation",
                    context.control_plane.project_name,
                    effective_project_backend_assignments(context, host_platform),
                    operation,
                )));
            }
            Ok(first_backend)
        }
    }
}

fn effective_project_backend_assignments(
    context: &ComposeProjectContext,
    host_platform: ServiceHostPlatform,
) -> String {
    context
        .plan
        .services
        .iter()
        .map(|(service_name, service)| {
            format!(
                "{service_name}={}",
                sandbox_backend_name(effective_service_backend(service, host_platform))
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn load_host_backed_project_backend(
    context: &ComposeProjectContext,
    host_platform: ServiceHostPlatform,
    machine_api_client: Option<MachineApiClient>,
) -> Result<Arc<dyn SandboxBackend>, Error> {
    let backend = required_effective_project_backend(
        context,
        None,
        "load a compose-backed sandbox manager",
        host_platform,
    )?;
    match backend {
        SandboxBackendKind::Krun => Ok(Arc::new(KrunSandboxBackend::new(
            context.control_plane.krun_backend_config(),
        ))),
        SandboxBackendKind::Container => {
            load_forwarded_machine_api_backend(context, host_platform, machine_api_client)
        }
    }
}

fn load_forwarded_machine_api_backend(
    context: &ComposeProjectContext,
    host_platform: ServiceHostPlatform,
    machine_api_client: Option<MachineApiClient>,
) -> Result<Arc<dyn SandboxBackend>, Error> {
    match host_platform {
        ServiceHostPlatform::Macos => {
            let client = match machine_api_client {
                Some(client) => client,
                None => require_default_machine_api_client()?,
            };
            validate_forwarded_machine_api_backend(context, &client)?;
            Ok(Arc::new(ForwardedMachineApiSandboxBackend::new(client)))
        }
        ServiceHostPlatform::Linux => Err(Error::InvalidInput(format!(
            "compose project {} selects sandbox backend container, but neovex load a compose-backed sandbox manager only supports that backend through the macOS guest machine API today",
            context.control_plane.project_name
        ))),
        ServiceHostPlatform::Other => Err(Error::InvalidInput(format!(
            "compose project {} selects sandbox backend container, but neovex load a compose-backed sandbox manager does not support the current host platform for forwarded guest execution",
            context.control_plane.project_name
        ))),
    }
}

fn validate_forwarded_machine_api_backend(
    context: &ComposeProjectContext,
    client: &MachineApiClient,
) -> Result<(), Error> {
    let capabilities = client.capabilities().map_err(|error| {
        Error::InvalidInput(format!(
            "compose project {} selects sandbox backend container, but the default machine API at {} is not reachable: {error}",
            context.control_plane.project_name,
            client.socket_path().display()
        ))
    })?;
    if !capabilities
        .supported_service_backends
        .contains(&SandboxBackendKind::Container)
    {
        return Err(Error::InvalidInput(format!(
            "compose project {} selects sandbox backend container, but the default machine API at {} does not advertise container backend support",
            context.control_plane.project_name,
            client.socket_path().display()
        )));
    }
    if !capabilities.service_execution_ready {
        let blockers = if capabilities.service_execution_blockers.is_empty() {
            "guest machine API did not report readiness blockers".to_owned()
        } else {
            capabilities.service_execution_blockers.join("; ")
        };
        return Err(Error::InvalidInput(format!(
            "compose project {} selects sandbox backend container, but the default machine API at {} is not ready for container-backed service execution: {}",
            context.control_plane.project_name,
            client.socket_path().display(),
            blockers,
        )));
    }
    Ok(())
}

fn sandbox_backend_name(backend: SandboxBackendKind) -> &'static str {
    match backend {
        SandboxBackendKind::Container => "container",
        SandboxBackendKind::Krun => "krun",
    }
}

fn project_backend_assignments(context: &ComposeProjectContext) -> String {
    context
        .plan
        .services
        .iter()
        .map(|(service_name, service)| {
            format!("{service_name}={}", sandbox_backend_name(service.backend))
        })
        .collect::<Vec<_>>()
        .join(", ")
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

fn resolve_service_execution_surface(
    context: &ComposeProjectContext,
    requested_service: Option<&str>,
    operation: &str,
    host_platform: ServiceHostPlatform,
    machine_api_client: Option<MachineApiClient>,
) -> Result<ServiceExecutionSurface, Error> {
    let backend =
        required_effective_project_backend(context, requested_service, operation, host_platform)?;
    match backend {
        SandboxBackendKind::Krun => Ok(ServiceExecutionSurface::Krun {
            state_view: KrunSandboxStateView::from_config(
                &context.control_plane.krun_backend_config(),
            ),
            backend: Arc::new(KrunSandboxBackend::new(
                context.control_plane.krun_backend_config(),
            )),
        }),
        SandboxBackendKind::Container => {
            let client = resolve_forwarded_machine_api_client(
                context,
                host_platform,
                machine_api_client,
                operation,
            )?;
            let backend: Arc<dyn SandboxBackend> =
                Arc::new(ForwardedMachineApiSandboxBackend::new(client.clone()));
            Ok(ServiceExecutionSurface::ForwardedContainer { client, backend })
        }
    }
}

fn resolve_forwarded_machine_api_client(
    context: &ComposeProjectContext,
    host_platform: ServiceHostPlatform,
    machine_api_client: Option<MachineApiClient>,
    operation: &str,
) -> Result<MachineApiClient, Error> {
    match host_platform {
        ServiceHostPlatform::Macos => match machine_api_client {
            Some(client) => Ok(client),
            None => require_default_machine_api_client(),
        },
        ServiceHostPlatform::Linux => Err(Error::InvalidInput(format!(
            "compose project {} selects sandbox backend container, but neovex {} only supports that backend through the macOS guest machine API today",
            context.control_plane.project_name, operation,
        ))),
        ServiceHostPlatform::Other => Err(Error::InvalidInput(format!(
            "compose project {} selects sandbox backend container, but neovex {} does not support the current host platform for forwarded guest execution",
            context.control_plane.project_name, operation,
        ))),
    }
}

fn validate_forwarded_machine_api_operations(
    context: &ComposeProjectContext,
    client: &MachineApiClient,
    operation: &str,
    required_operations: &[&str],
) -> Result<(), Error> {
    let capabilities = client.capabilities().map_err(|error| {
        Error::InvalidInput(format!(
            "compose project {} selects sandbox backend container, but the default machine API at {} is not reachable: {error}",
            context.control_plane.project_name,
            client.socket_path().display()
        ))
    })?;
    if !capabilities
        .supported_service_backends
        .contains(&SandboxBackendKind::Container)
    {
        return Err(Error::InvalidInput(format!(
            "compose project {} selects sandbox backend container, but the default machine API at {} does not advertise container backend support",
            context.control_plane.project_name,
            client.socket_path().display()
        )));
    }

    let missing = required_operations
        .iter()
        .copied()
        .filter(|required_operation| {
            !capabilities
                .supported_operations
                .iter()
                .any(|advertised| advertised == required_operation)
        })
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return Ok(());
    }

    let operation_blockers = capabilities.blockers_for_operations(missing.iter().copied());
    let blockers = if !operation_blockers.is_empty() {
        operation_blockers.join("; ")
    } else if capabilities.service_execution_blockers.is_empty() {
        "guest machine API did not report readiness blockers".to_owned()
    } else {
        capabilities.service_execution_blockers.join("; ")
    };
    Err(Error::InvalidInput(format!(
        "compose project {} selects sandbox backend container, but neovex {} requires guest machine API operations [{}] that are not available at {}: {}",
        context.control_plane.project_name,
        operation,
        missing.join(", "),
        client.socket_path().display(),
        blockers,
    )))
}

fn resolve_service_down_targets(
    state_view: &KrunSandboxStateView,
    tenant: &TenantId,
    requested_service: Option<&str>,
    project_name: &str,
) -> Result<Vec<ServiceLifecycleTarget>, Error> {
    match requested_service {
        Some(service_name) => {
            let details = state_view
                .inspect_service(tenant, service_name)
                .map_err(|error| render_state_lookup_error("resolve persisted sandbox state", error))?
                .ok_or_else(|| {
                    Error::InvalidInput(format!(
                        "no persisted sandbox state found for service {} in tenant {} under project {}",
                        service_name, tenant, project_name
                    ))
                })?;
            Ok(vec![ServiceLifecycleTarget::from_details(details)])
        }
        None => {
            let service_names = state_view
                .list_for_tenant(tenant)
                .map_err(|error| render_state_lookup_error("list persisted sandbox state", error))?
                .into_iter()
                .map(|summary| summary.service_name)
                .collect::<BTreeSet<_>>();

            service_names
                .into_iter()
                .map(|service_name| {
                    state_view
                        .inspect_service(tenant, &service_name)
                        .map_err(|error| {
                            render_state_lookup_error("resolve persisted sandbox state", error)
                        })?
                        .map(ServiceLifecycleTarget::from_details)
                        .ok_or_else(|| {
                            Error::Internal(format!(
                                "persisted sandbox state changed while resolving service {} in tenant {} under project {}",
                                service_name, tenant, project_name
                            ))
                        })
                })
                .collect()
        }
    }
}

async fn resolve_live_service_handle(
    state_view: &KrunSandboxStateView,
    backend: &dyn SandboxBackend,
    tenant: &TenantId,
    service_name: &str,
) -> Result<Option<SandboxHandle>, Error> {
    let Some(details) = state_view
        .inspect_service(tenant, service_name)
        .map_err(|error| render_state_lookup_error("resolve persisted sandbox state", error))?
    else {
        return Ok(None);
    };

    let refreshed = backend
        .inspect(&details.summary.sandbox_id)
        .await
        .map_err(|error| backend_operation_error("inspect", tenant, service_name, error))?;

    Ok(refreshed.filter(|handle| is_active_status(handle.status)))
}

async fn start_service_launch(
    backend: &dyn SandboxBackend,
    tenant: &TenantId,
    service_name: &str,
    launch: SandboxServiceLaunch,
) -> Result<SandboxHandle, Error> {
    if launch.spec().name != service_name {
        return Err(Error::InvalidInput(format!(
            "sandbox service catalog returned launch spec name {} for requested service {}",
            launch.spec().name,
            service_name
        )));
    }
    if &launch.spec().tenant_id != tenant {
        return Err(Error::InvalidInput(format!(
            "sandbox service catalog returned tenant {} for requested tenant {}",
            launch.spec().tenant_id,
            tenant
        )));
    }
    if launch.spec().backend != backend.kind() {
        return Err(Error::InvalidInput(format!(
            "sandbox service {} for tenant {} requested backend {:?}, but the configured backend is {:?}",
            service_name,
            tenant,
            launch.spec().backend,
            backend.kind()
        )));
    }

    match launch {
        SandboxServiceLaunch::Image(launch) => backend
            .start_from_image(launch)
            .await
            .map_err(|error| backend_operation_error("start", tenant, service_name, error)),
        SandboxServiceLaunch::Build(launch) => backend
            .start_from_build(launch)
            .await
            .map_err(|error| backend_operation_error("start", tenant, service_name, error)),
    }
}

async fn stop_service_target(
    backend: &dyn SandboxBackend,
    tenant: &TenantId,
    target: ServiceLifecycleTarget,
) -> Result<ServiceLifecycleOutcome, Error> {
    let refreshed = backend
        .inspect(&target.sandbox_id)
        .await
        .map_err(|error| backend_operation_error("inspect", tenant, &target.service_name, error))?;

    if refreshed
        .as_ref()
        .is_none_or(|handle| !is_active_status(handle.status))
    {
        let status = refreshed
            .map(|handle| handle.status)
            .unwrap_or(target.status);
        return Ok(ServiceLifecycleOutcome {
            action: ServiceLifecycleAction::AlreadyStopped,
            tenant_id: tenant.clone(),
            service_name: target.service_name,
            sandbox_id: target.sandbox_id,
            status,
        });
    }

    backend
        .stop(&target.sandbox_id)
        .await
        .map_err(|error| backend_operation_error("stop", tenant, &target.service_name, error))?;
    let status = backend
        .inspect(&target.sandbox_id)
        .await
        .map_err(|error| backend_operation_error("inspect", tenant, &target.service_name, error))?
        .map(|handle| handle.status)
        .unwrap_or(SandboxStatus::Stopped);

    Ok(ServiceLifecycleOutcome {
        action: ServiceLifecycleAction::Stopped,
        tenant_id: tenant.clone(),
        service_name: target.service_name,
        sandbox_id: target.sandbox_id,
        status,
    })
}

fn backend_operation_error(
    operation: &str,
    tenant: &TenantId,
    service_name: &str,
    error: neovex::SandboxError,
) -> Error {
    Error::Internal(format!(
        "failed to {operation} service {} for tenant {}: {error}",
        service_name, tenant
    ))
}

fn is_active_status(status: SandboxStatus) -> bool {
    matches!(
        status,
        SandboxStatus::Starting
            | SandboxStatus::Ready
            | SandboxStatus::NotReady
            | SandboxStatus::Stopping
    )
}

fn resolve_service_ctr_log_path(
    command: &ServiceLogsCommand,
    control_data_dir: &Path,
) -> Result<PathBuf, Error> {
    let context = load_compose_project_context(&command.file, control_data_dir)?;
    require_krun_backend_for_service_operation(&context, Some(&command.service), "service logs")?;
    let state_view =
        KrunSandboxStateView::from_config(&context.control_plane.krun_backend_config());
    let tenant = command
        .tenant
        .clone()
        .unwrap_or_else(|| context.control_plane.local_tenant_id.clone());
    let details = state_view
        .inspect_service(&tenant, &command.service)
        .map_err(|error| render_state_lookup_error("resolve persisted service logs", error))?
        .ok_or_else(|| {
            Error::InvalidInput(format!(
                "no persisted sandbox state found for service {} in tenant {} under project {}",
                command.service, tenant, context.control_plane.project_name
            ))
        })?;
    Ok(details.log_paths.ctr_log)
}

fn resolve_service_process_snapshot(
    command: &ServicePsCommand,
    control_data_dir: &Path,
) -> Result<ServiceProcessSnapshot, Error> {
    let context = load_compose_project_context(&command.file, control_data_dir)?;
    require_krun_backend_for_service_operation(&context, Some(&command.service), "service ps")?;
    let state_view =
        KrunSandboxStateView::from_config(&context.control_plane.krun_backend_config());
    let tenant = command
        .tenant
        .clone()
        .unwrap_or_else(|| context.control_plane.local_tenant_id.clone());
    let details = state_view
        .inspect_service(&tenant, &command.service)
        .map_err(|error| render_state_lookup_error("resolve persisted service processes", error))?
        .ok_or_else(|| {
            Error::InvalidInput(format!(
                "no persisted sandbox state found for service {} in tenant {} under project {}",
                command.service, tenant, context.control_plane.project_name
            ))
        })?;

    let runtime_pidfile = details.state_dir.join("pidfile");
    let conmon_pidfile = details.state_dir.join("conmon.pid");
    let runtime_pid = read_pid_file_if_exists(&runtime_pidfile)?;
    let conmon_pid = read_pid_file_if_exists(&conmon_pidfile)?;
    let process_rows = snapshot_process_rows(runtime_pid, conmon_pid)?;

    Ok(ServiceProcessSnapshot {
        sandbox_id: details.summary.sandbox_id,
        tenant_id: details.summary.tenant_id,
        service_name: details.summary.service_name,
        status: details.summary.status,
        runtime_pidfile,
        conmon_pidfile,
        runtime_pid,
        conmon_pid,
        process_rows,
    })
}

fn resolve_service_process_snapshot_for_platform(
    command: &ServicePsCommand,
    control_data_dir: &Path,
    host_platform: ServiceHostPlatform,
    machine_api_client: Option<MachineApiClient>,
) -> Result<ServiceProcessSnapshot, Error> {
    let context = load_compose_project_context(&command.file, control_data_dir)?;
    let tenant = command
        .tenant
        .clone()
        .unwrap_or_else(|| context.control_plane.local_tenant_id.clone());
    match resolve_service_execution_surface(
        &context,
        Some(&command.service),
        "service ps",
        host_platform,
        machine_api_client,
    )? {
        ServiceExecutionSurface::Krun { .. } => {
            resolve_service_process_snapshot(command, control_data_dir)
        }
        ServiceExecutionSurface::ForwardedContainer { client, .. } => {
            validate_forwarded_machine_api_operations(
                &context,
                &client,
                "service ps",
                &["service-sandboxes.inspect-current", "service-sandboxes.ps"],
            )?;
            let details = lookup_current_remote_service_details(
                &context,
                &client,
                &tenant,
                &command.service,
                "resolve persisted service processes",
            )?
            .ok_or_else(|| {
                missing_persisted_service_error(
                    &context.control_plane.project_name,
                    &tenant,
                    &command.service,
                )
            })?;
            let snapshot = client
                .service_process_snapshot(&details.summary.sandbox_id)
                .map_err(|error| {
                    machine_api_operation_error(
                        "resolve persisted service processes",
                        &client,
                        error,
                    )
                })?;

            Ok(ServiceProcessSnapshot {
                sandbox_id: snapshot.sandbox_id,
                tenant_id: snapshot.tenant_id,
                service_name: snapshot.service_name,
                status: snapshot.status,
                runtime_pidfile: snapshot.runtime_pidfile,
                conmon_pidfile: snapshot.conmon_pidfile,
                runtime_pid: snapshot.runtime_pid,
                conmon_pid: snapshot.conmon_pid,
                process_rows: snapshot
                    .process_rows
                    .into_iter()
                    .map(|row| ServiceProcessRow {
                        pid: row.pid,
                        ppid: row.ppid,
                        command: row.command,
                    })
                    .collect(),
            })
        }
    }
}

fn read_log_chunk(path: &Path, offset: u64) -> Result<(String, u64), Error> {
    let Ok(mut file) = File::open(path) else {
        return Ok((String::new(), offset));
    };

    let metadata = file.metadata().map_err(|error| {
        Error::Internal(format!(
            "failed to inspect persisted log file {}: {error}",
            path.display()
        ))
    })?;
    let file_len = metadata.len();
    let start = offset.min(file_len);
    file.seek(SeekFrom::Start(start)).map_err(|error| {
        Error::Internal(format!(
            "failed to seek persisted log file {}: {error}",
            path.display()
        ))
    })?;

    let mut buffer = String::new();
    file.read_to_string(&mut buffer).map_err(|error| {
        Error::Internal(format!(
            "failed to read persisted log file {}: {error}",
            path.display()
        ))
    })?;

    Ok((buffer, file_len))
}

fn read_pid_file_if_exists(path: &Path) -> Result<Option<u32>, Error> {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Ok(None);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    trimmed.parse::<u32>().map(Some).map_err(|error| {
        Error::InvalidInput(format!(
            "failed to parse pidfile {} containing {:?}: {error}",
            path.display(),
            trimmed
        ))
    })
}

fn snapshot_process_rows(
    runtime_pid: Option<u32>,
    conmon_pid: Option<u32>,
) -> Result<Vec<ServiceProcessRow>, Error> {
    let pid_set = [runtime_pid, conmon_pid]
        .into_iter()
        .flatten()
        .collect::<BTreeSet<_>>();
    if pid_set.is_empty() {
        return Ok(Vec::new());
    }

    let output = Command::new("ps")
        .args(["-ax", "-o", "pid=,ppid=,command="])
        .output()
        .map_err(|error| {
            Error::Internal(format!("failed to run ps for service snapshot: {error}"))
        })?;
    if !output.status.success() {
        return Err(Error::Internal(format!(
            "ps exited with status {} while collecting service snapshot",
            output.status
        )));
    }
    let stdout = String::from_utf8(output.stdout)
        .map_err(|error| Error::Serialization(format!("ps output was not valid utf-8: {error}")))?;
    Ok(parse_process_rows(&stdout, &pid_set))
}

fn parse_process_rows(stdout: &str, pid_set: &BTreeSet<u32>) -> Vec<ServiceProcessRow> {
    let mut rows = stdout
        .lines()
        .filter_map(parse_process_row)
        .filter(|row| pid_set.contains(&row.pid))
        .collect::<Vec<_>>();
    rows.sort_by_key(|row| (row.ppid, row.pid));
    rows
}

fn parse_process_row(line: &str) -> Option<ServiceProcessRow> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut parts = trimmed.split_whitespace();
    let pid = parts.next()?.parse::<u32>().ok()?;
    let ppid = parts.next()?.parse::<u32>().ok()?;
    let command = parts.collect::<Vec<_>>().join(" ");
    if command.is_empty() {
        return None;
    }

    Some(ServiceProcessRow { pid, ppid, command })
}

fn control_data_dir_from_service_config(config: &ServicePersistenceConfig) -> &Path {
    match &config.control_plane {
        neovex::ControlPlaneConfig::EmbeddedRedb { data_dir } => data_dir.as_path(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::path::Path;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use super::*;
    use clap::Parser;
    use neovex::{
        SandboxBackendKind, SandboxBuildLaunchSpec, SandboxFilesystemSpec, SandboxId,
        SandboxImageLaunchSpec, SandboxProcessSpec, SandboxSpec, SandboxStatus,
    };
    use neovex_sandbox::SandboxFuture;
    use neovex_sandbox::backends::container::{
        ContainerLaunchMode, ContainerSandboxBackend, ContainerSandboxBackendConfig,
    };
    use serde_json::json;
    use tempfile::TempDir;

    use crate::machine::{
        MachineApiClient, MachineApiListenMode, MachineApiState, bind_direct_listener,
        default_guest_helper_binary_dirs, serve_machine_api,
    };

    #[derive(Debug, clap::Parser)]
    struct RootCli {
        #[command(subcommand)]
        command: Option<RootCommand>,
    }

    #[derive(Debug, clap::Subcommand)]
    enum RootCommand {
        Service(ServiceCommand),
    }

    #[test]
    fn parses_service_config_subcommand() {
        let cli = RootCli::parse_from(["neovex", "service", "config", "--file", "stack.yml"]);
        let Some(RootCommand::Service(service)) = cli.command else {
            panic!("service subcommand should parse");
        };

        match service.command {
            ServiceSubcommand::Config(config) => {
                assert_eq!(config.file, PathBuf::from("stack.yml"));
                assert!(!config.services);
            }
            _ => panic!("expected config subcommand"),
        }
    }

    #[test]
    fn parses_service_config_services_listing_flag() {
        let cli = RootCli::parse_from(["neovex", "service", "config", "--services"]);
        let Some(RootCommand::Service(service)) = cli.command else {
            panic!("service subcommand should parse");
        };

        match service.command {
            ServiceSubcommand::Config(config) => {
                assert_eq!(config.file, PathBuf::from(compose::DEFAULT_COMPOSE_FILE));
                assert!(config.services);
            }
            _ => panic!("expected config subcommand"),
        }
    }

    #[test]
    fn parses_service_list_all_tenants_flag() {
        let cli = RootCli::parse_from(["neovex", "service", "list", "--all-tenants"]);
        let Some(RootCommand::Service(service)) = cli.command else {
            panic!("service subcommand should parse");
        };

        match service.command {
            ServiceSubcommand::List(list) => {
                assert_eq!(list.file, PathBuf::from(compose::DEFAULT_COMPOSE_FILE));
                assert!(list.all_tenants);
            }
            _ => panic!("expected list subcommand"),
        }
    }

    #[test]
    fn parses_service_up_with_optional_service_and_tenant_override() {
        let cli = RootCli::parse_from(["neovex", "service", "up", "db", "--tenant", "svc-demo"]);
        let Some(RootCommand::Service(service)) = cli.command else {
            panic!("service subcommand should parse");
        };

        match service.command {
            ServiceSubcommand::Up(up) => {
                assert_eq!(up.service.as_deref(), Some("db"));
                assert_eq!(
                    up.tenant.expect("tenant override should parse").as_str(),
                    "svc-demo"
                );
                assert_eq!(up.file, PathBuf::from(compose::DEFAULT_COMPOSE_FILE));
            }
            _ => panic!("expected up subcommand"),
        }
    }

    #[test]
    fn parses_service_down_without_service_uses_default_compose_file() {
        let cli = RootCli::parse_from(["neovex", "service", "down"]);
        let Some(RootCommand::Service(service)) = cli.command else {
            panic!("service subcommand should parse");
        };

        match service.command {
            ServiceSubcommand::Down(down) => {
                assert_eq!(down.service, None);
                assert_eq!(down.file, PathBuf::from(compose::DEFAULT_COMPOSE_FILE));
                assert_eq!(down.tenant, None);
            }
            _ => panic!("expected down subcommand"),
        }
    }

    #[test]
    fn parses_service_inspect_with_optional_tenant_override() {
        let cli =
            RootCli::parse_from(["neovex", "service", "inspect", "db", "--tenant", "svc-demo"]);
        let Some(RootCommand::Service(service)) = cli.command else {
            panic!("service subcommand should parse");
        };

        match service.command {
            ServiceSubcommand::Inspect(inspect) => {
                assert_eq!(inspect.service, "db");
                assert_eq!(
                    inspect
                        .tenant
                        .expect("tenant override should parse")
                        .as_str(),
                    "svc-demo"
                );
            }
            _ => panic!("expected inspect subcommand"),
        }
    }

    #[test]
    fn parses_service_logs_with_follow_flag() {
        let cli = RootCli::parse_from(["neovex", "service", "logs", "db", "--follow"]);
        let Some(RootCommand::Service(service)) = cli.command else {
            panic!("service subcommand should parse");
        };

        match service.command {
            ServiceSubcommand::Logs(logs) => {
                assert_eq!(logs.service, "db");
                assert_eq!(logs.file, PathBuf::from(compose::DEFAULT_COMPOSE_FILE));
                assert!(logs.follow);
            }
            _ => panic!("expected logs subcommand"),
        }
    }

    #[test]
    fn parses_service_ps_with_optional_tenant_override() {
        let cli = RootCli::parse_from(["neovex", "service", "ps", "db", "--tenant", "svc-demo"]);
        let Some(RootCommand::Service(service)) = cli.command else {
            panic!("service subcommand should parse");
        };

        match service.command {
            ServiceSubcommand::Ps(ps) => {
                assert_eq!(ps.service, "db");
                assert_eq!(
                    ps.tenant.expect("tenant override should parse").as_str(),
                    "svc-demo"
                );
            }
            _ => panic!("expected ps subcommand"),
        }
    }

    #[test]
    fn render_service_list_defaults_to_local_project_tenant_and_can_expand_to_all_tenants() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let compose_path = write_compose_fixture(temp_dir.path());
        let control_data_dir = temp_dir.path().join("control");
        let context = load_compose_project_context(&compose_path, &control_data_dir)
            .expect("compose project context should load");
        let krun_config = context.control_plane.krun_backend_config();

        write_manifest(
            &krun_config.state_root,
            "db-01aaa",
            context.control_plane.local_tenant_id.as_str(),
            "db",
            SandboxStatus::Ready,
        );
        write_manifest(
            &krun_config.state_root,
            "db-01bbb",
            "tenant-other",
            "db",
            SandboxStatus::Ready,
        );

        let rendered_local = render_service_list_for_platform(
            &ServiceListCommand {
                file: compose_path.clone(),
                all_tenants: false,
            },
            &control_data_dir,
            ServiceHostPlatform::Linux,
            None,
        )
        .expect("local list should render");
        assert!(rendered_local.contains(context.control_plane.local_tenant_id.as_str()));
        assert!(!rendered_local.contains("tenant-other"));

        let rendered_all = render_service_list_for_platform(
            &ServiceListCommand {
                file: compose_path,
                all_tenants: true,
            },
            &control_data_dir,
            ServiceHostPlatform::Linux,
            None,
        )
        .expect("all-tenant list should render");
        assert!(rendered_all.contains(context.control_plane.local_tenant_id.as_str()));
        assert!(rendered_all.contains("tenant-other"));
    }

    #[test]
    fn render_service_inspect_defaults_to_local_project_tenant_and_honors_tenant_override() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let compose_path = write_compose_fixture(temp_dir.path());
        let control_data_dir = temp_dir.path().join("control");
        let context = load_compose_project_context(&compose_path, &control_data_dir)
            .expect("compose project context should load");
        let krun_config = context.control_plane.krun_backend_config();

        write_manifest(
            &krun_config.state_root,
            "db-01aaa",
            context.control_plane.local_tenant_id.as_str(),
            "db",
            SandboxStatus::Ready,
        );
        write_manifest(
            &krun_config.state_root,
            "db-01bbb",
            "tenant-other",
            "db",
            SandboxStatus::Stopped,
        );

        let rendered_default = render_service_inspect_for_platform(
            &ServiceInspectCommand {
                service: "db".to_owned(),
                file: compose_path.clone(),
                tenant: None,
            },
            &control_data_dir,
            ServiceHostPlatform::Linux,
            None,
        )
        .expect("default inspect should render");
        assert!(rendered_default.contains(context.control_plane.local_tenant_id.as_str()));
        assert!(rendered_default.contains("db-01aaa"));
        assert!(rendered_default.contains("ctr.log"));

        let rendered_override = render_service_inspect_for_platform(
            &ServiceInspectCommand {
                service: "db".to_owned(),
                file: compose_path,
                tenant: Some(TenantId::new("tenant-other").expect("tenant should parse")),
            },
            &control_data_dir,
            ServiceHostPlatform::Linux,
            None,
        )
        .expect("tenant override inspect should render");
        assert!(rendered_override.contains("tenant-other"));
        assert!(rendered_override.contains("db-01bbb"));
    }

    #[test]
    fn resolve_service_ctr_log_path_defaults_to_local_project_tenant_and_honors_override() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let compose_path = write_compose_fixture(temp_dir.path());
        let control_data_dir = temp_dir.path().join("control");
        let context = load_compose_project_context(&compose_path, &control_data_dir)
            .expect("compose project context should load");
        let krun_config = context.control_plane.krun_backend_config();

        write_manifest(
            &krun_config.state_root,
            "db-01aaa",
            context.control_plane.local_tenant_id.as_str(),
            "db",
            SandboxStatus::Ready,
        );
        write_manifest(
            &krun_config.state_root,
            "db-01bbb",
            "tenant-other",
            "db",
            SandboxStatus::Ready,
        );

        let default_path = resolve_service_ctr_log_path(
            &ServiceLogsCommand {
                service: "db".to_owned(),
                file: compose_path.clone(),
                tenant: None,
                follow: false,
            },
            &control_data_dir,
        )
        .expect("default log path should resolve");
        assert!(default_path.ends_with("containers/db-01aaa/ctr.log"));

        let override_path = resolve_service_ctr_log_path(
            &ServiceLogsCommand {
                service: "db".to_owned(),
                file: compose_path,
                tenant: Some(TenantId::new("tenant-other").expect("tenant should parse")),
                follow: false,
            },
            &control_data_dir,
        )
        .expect("override log path should resolve");
        assert!(override_path.ends_with("containers/db-01bbb/ctr.log"));
    }

    #[test]
    fn read_log_chunk_returns_empty_for_missing_files_and_only_appended_bytes() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let log_path = temp_dir.path().join("ctr.log");

        let (missing_chunk, missing_offset) =
            read_log_chunk(&log_path, 0).expect("missing files should read as empty");
        assert!(missing_chunk.is_empty());
        assert_eq!(missing_offset, 0);

        fs::write(&log_path, "line one\nline two\n").expect("log fixture should write");
        let (full_chunk, full_offset) =
            read_log_chunk(&log_path, 0).expect("initial read should succeed");
        assert_eq!(full_chunk, "line one\nline two\n");
        assert_eq!(full_offset, 18);

        fs::write(&log_path, "line one\nline two\nline three\n")
            .expect("appended log fixture should write");
        let (appended_chunk, appended_offset) =
            read_log_chunk(&log_path, full_offset).expect("appended read should succeed");
        assert_eq!(appended_chunk, "line three\n");
        assert_eq!(appended_offset, 29);
    }

    #[test]
    fn read_pid_file_if_exists_returns_none_for_missing_and_parses_trimmed_values() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let pidfile = temp_dir.path().join("pidfile");

        assert_eq!(
            read_pid_file_if_exists(&pidfile).expect("missing pidfile should read as none"),
            None
        );

        fs::write(&pidfile, "1234\n").expect("pidfile should write");
        assert_eq!(
            read_pid_file_if_exists(&pidfile).expect("pidfile should parse"),
            Some(1234)
        );
    }

    #[test]
    fn parse_process_rows_filters_requested_pids_and_preserves_command_text() {
        let stdout = "\
  101   1 /usr/bin/conmon --runtime /usr/libexec/neovex/crun\n\
  202 101 /usr/libexec/neovex/crun --root /run/user/1000/crun\n\
  303   1 /usr/sbin/unrelated\n";
        let pid_set = BTreeSet::from([101_u32, 202_u32]);

        let rows = parse_process_rows(stdout, &pid_set);

        assert_eq!(
            rows,
            vec![
                ServiceProcessRow {
                    pid: 101,
                    ppid: 1,
                    command: "/usr/bin/conmon --runtime /usr/libexec/neovex/crun".to_owned()
                },
                ServiceProcessRow {
                    pid: 202,
                    ppid: 101,
                    command: "/usr/libexec/neovex/crun --root /run/user/1000/crun".to_owned()
                }
            ]
        );
    }

    #[test]
    fn render_service_ps_reads_pidfiles_from_persisted_state() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let compose_path = write_compose_fixture(temp_dir.path());
        let control_data_dir = temp_dir.path().join("control");
        let context = load_compose_project_context(&compose_path, &control_data_dir)
            .expect("compose project context should load");
        let krun_config = context.control_plane.krun_backend_config();

        write_manifest(
            &krun_config.state_root,
            "db-01aaa",
            context.control_plane.local_tenant_id.as_str(),
            "db",
            SandboxStatus::Ready,
        );
        let container_dir = krun_config.state_root.join("containers").join("db-01aaa");
        fs::write(container_dir.join("pidfile"), "2002\n").expect("pidfile should write");
        fs::write(container_dir.join("conmon.pid"), "1001\n").expect("conmon pidfile should write");

        let rendered = render_service_ps_for_platform(
            &ServicePsCommand {
                service: "db".to_owned(),
                file: compose_path,
                tenant: None,
            },
            &control_data_dir,
            ServiceHostPlatform::Linux,
            None,
        )
        .expect("service ps should render");
        assert!(rendered.contains("db-01aaa"));
        assert!(rendered.contains("runtime_pid: 2002"));
        assert!(rendered.contains("conmon_pid: 1001"));
        assert!(rendered.contains("runtime_pidfile:"));
        assert!(rendered.contains("conmon_pidfile:"));
    }

    #[test]
    fn resolve_service_down_targets_deduplicates_manifest_history_per_service_identity() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let compose_path = write_compose_fixture(temp_dir.path());
        let control_data_dir = temp_dir.path().join("control");
        let context = load_compose_project_context(&compose_path, &control_data_dir)
            .expect("compose project context should load");
        let krun_config = context.control_plane.krun_backend_config();
        let tenant = context.control_plane.local_tenant_id.clone();

        write_manifest(
            &krun_config.state_root,
            "db-01aaa",
            tenant.as_str(),
            "db",
            SandboxStatus::Stopped,
        );
        write_manifest(
            &krun_config.state_root,
            "db-01bbb",
            tenant.as_str(),
            "db",
            SandboxStatus::Ready,
        );
        write_manifest(
            &krun_config.state_root,
            "cache-01aaa",
            tenant.as_str(),
            "cache",
            SandboxStatus::Stopped,
        );

        let state_view = KrunSandboxStateView::from_config(&krun_config);
        let targets = resolve_service_down_targets(
            &state_view,
            &tenant,
            None,
            &context.control_plane.project_name,
        )
        .expect("targets should resolve");

        assert_eq!(targets.len(), 2);
        assert_eq!(
            targets
                .iter()
                .map(|target| {
                    (
                        target.service_name.as_str(),
                        target.sandbox_id.as_str(),
                        target.status,
                    )
                })
                .collect::<Vec<_>>(),
            vec![
                ("cache", "cache-01aaa", SandboxStatus::Stopped),
                ("db", "db-01bbb", SandboxStatus::Ready),
            ]
        );
    }

    #[tokio::test]
    async fn start_service_launch_starts_image_launches_and_validates_identity() {
        let tenant = TenantId::new("svc-demo").expect("tenant should parse");
        let backend = StubBackend::default();
        let service_name = "db";
        let launch = SandboxServiceLaunch::image(SandboxImageLaunchSpec::new(
            sample_spec(&tenant, service_name),
            "busybox:latest",
        ));

        let handle = start_service_launch(&backend, &tenant, service_name, launch)
            .await
            .expect("launch should start");

        assert_eq!(handle.name, "db");
        assert_eq!(
            backend
                .started_services
                .lock()
                .expect("started services lock should hold")
                .as_slice(),
            &["db".to_owned()]
        );
    }

    #[tokio::test]
    async fn stop_service_target_stops_active_handles_and_reports_already_stopped_terminal_ones() {
        let tenant = TenantId::new("svc-demo").expect("tenant should parse");
        let active_id = SandboxId::new("db-01aaa");
        let stopped_id = SandboxId::new("db-01bbb");
        let backend = StubBackend::with_handles([
            stub_handle(&active_id, "db", SandboxStatus::Ready),
            stub_handle(&stopped_id, "db", SandboxStatus::Stopped),
        ]);

        let stopped = stop_service_target(
            &backend,
            &tenant,
            ServiceLifecycleTarget {
                sandbox_id: active_id.clone(),
                service_name: "db".to_owned(),
                status: SandboxStatus::Ready,
            },
        )
        .await
        .expect("active handle should stop");
        assert_eq!(stopped.action, ServiceLifecycleAction::Stopped);
        assert_eq!(stopped.status, SandboxStatus::Stopped);

        let already_stopped = stop_service_target(
            &backend,
            &tenant,
            ServiceLifecycleTarget {
                sandbox_id: stopped_id.clone(),
                service_name: "db".to_owned(),
                status: SandboxStatus::Stopped,
            },
        )
        .await
        .expect("stopped handle should no-op");
        assert_eq!(
            already_stopped.action,
            ServiceLifecycleAction::AlreadyStopped
        );

        let stopped_ids = backend
            .stopped_ids
            .lock()
            .expect("stopped ids lock should hold");
        assert_eq!(stopped_ids.as_slice(), &[active_id.as_str().to_owned()]);
    }

    #[test]
    fn require_krun_backend_rejects_container_only_projects() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let compose_path = write_compose_fixture_with_body(
            temp_dir.path(),
            r#"
name: Demo App
services:
  db:
    image: busybox:latest
    x_neovex:
      backend: container
"#,
        );
        let control_data_dir = temp_dir.path().join("control");
        let context = load_compose_project_context(&compose_path, &control_data_dir)
            .expect("compose project context should load");

        let error = require_krun_backend_for_service_operation(
            &context,
            None,
            "load a compose-backed sandbox manager",
        )
        .expect_err("container-only project should fail fast");

        assert_eq!(
            error.to_string(),
            "invalid input: compose project demo-app selects sandbox backend container, but neovex load a compose-backed sandbox manager only supports the krun backend today"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn host_loader_accepts_container_projects_with_ready_forwarded_machine_api_on_macos() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let compose_path = write_compose_fixture_with_body(
            temp_dir.path(),
            r#"
name: Demo App
services:
  db:
    image: busybox:latest
    x_neovex:
      backend: container
"#,
        );
        let control_data_dir = temp_dir.path().join("control");
        let context = load_compose_project_context(&compose_path, &control_data_dir)
            .expect("compose project context should load");
        let socket_path = temp_dir.path().join("default-api.sock");
        let listener = bind_direct_listener(&socket_path).expect("listener should bind");
        let state = MachineApiState {
            control_data_dir: temp_dir.path().join("machine-control"),
            listen_mode: MachineApiListenMode::DirectSocket,
            binary_lookup_path: Some(temp_dir.path().as_os_str().to_owned()),
            helper_binary_dirs: default_guest_helper_binary_dirs(),
            service_backend: Some(Arc::new(StubMachineApiSandboxBackend)),
            machine_port_forwarder: None,
        };
        write_fake_runtime_binaries(temp_dir.path());
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let server = tokio::spawn(serve_machine_api(listener, state, async move {
            let _ = shutdown_rx.await;
        }));
        let client = MachineApiClient::new(socket_path.clone());

        wait_for_machine_api_health(&client);
        let _manager = load_host_backed_sandbox_service_manager_for_platform(
            &compose_path,
            &control_data_dir,
            ServiceHostPlatform::Macos,
            Some(client.clone()),
        )
        .expect("host loader should accept ready container backend");
        let backend =
            load_host_backed_project_backend(&context, ServiceHostPlatform::Macos, Some(client))
                .expect("project backend should load");
        assert_eq!(backend.kind(), SandboxBackendKind::Container);

        let _ = shutdown_tx.send(());
        server
            .await
            .expect("machine API server task should join")
            .expect("machine API server should shut down cleanly");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn host_loader_accepts_default_projects_with_ready_forwarded_machine_api_on_macos() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let compose_path = write_compose_fixture(temp_dir.path());
        let control_data_dir = temp_dir.path().join("control");
        let context = load_compose_project_context(&compose_path, &control_data_dir)
            .expect("compose project context should load");
        let socket_path = temp_dir.path().join("default-api.sock");
        let listener = bind_direct_listener(&socket_path).expect("listener should bind");
        let state = MachineApiState {
            control_data_dir: temp_dir.path().join("machine-control"),
            listen_mode: MachineApiListenMode::DirectSocket,
            binary_lookup_path: Some(temp_dir.path().as_os_str().to_owned()),
            helper_binary_dirs: default_guest_helper_binary_dirs(),
            service_backend: Some(Arc::new(StubMachineApiSandboxBackend)),
            machine_port_forwarder: None,
        };
        write_fake_runtime_binaries(temp_dir.path());
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let server = tokio::spawn(serve_machine_api(listener, state, async move {
            let _ = shutdown_rx.await;
        }));
        let client = MachineApiClient::new(socket_path);

        wait_for_machine_api_health(&client);
        let backend =
            load_host_backed_project_backend(&context, ServiceHostPlatform::Macos, Some(client))
                .expect("host loader should accept default macOS service backend");
        assert_eq!(backend.kind(), SandboxBackendKind::Container);

        let _ = shutdown_tx.send(());
        server
            .await
            .expect("machine API server task should join")
            .expect("machine API server should shut down cleanly");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn host_loader_reports_machine_api_readiness_blockers_for_container_projects() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let compose_path = write_compose_fixture_with_body(
            temp_dir.path(),
            r#"
name: Demo App
services:
  db:
    image: busybox:latest
    x_neovex:
      backend: container
"#,
        );
        let control_data_dir = temp_dir.path().join("control");
        let context = load_compose_project_context(&compose_path, &control_data_dir)
            .expect("compose project context should load");
        let socket_path = temp_dir.path().join("default-api.sock");
        let listener = bind_direct_listener(&socket_path).expect("listener should bind");
        let state = MachineApiState {
            control_data_dir: temp_dir.path().join("machine-control"),
            listen_mode: MachineApiListenMode::DirectSocket,
            binary_lookup_path: Some(temp_dir.path().as_os_str().to_owned()),
            helper_binary_dirs: default_guest_helper_binary_dirs(),
            service_backend: None,
            machine_port_forwarder: None,
        };
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let server = tokio::spawn(serve_machine_api(listener, state, async move {
            let _ = shutdown_rx.await;
        }));
        let client = MachineApiClient::new(socket_path);

        wait_for_machine_api_health(&client);
        let error = match load_host_backed_project_backend(
            &context,
            ServiceHostPlatform::Macos,
            Some(client),
        ) {
            Ok(_) => panic!("container backend should reject unready machine API"),
            Err(error) => error,
        };
        assert!(
            error
                .to_string()
                .contains("not ready for container-backed service execution"),
            "{error}"
        );
        assert!(
            error
                .to_string()
                .contains("guest machine API does not yet expose service lifecycle operations"),
            "{error}"
        );

        let _ = shutdown_tx.send(());
        server
            .await
            .expect("machine API server task should join")
            .expect("machine API server should shut down cleanly");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn macos_service_up_uses_forwarded_machine_api_for_container_projects() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let compose_path = write_compose_fixture_with_body(
            temp_dir.path(),
            r#"
name: Demo App
services:
  db:
    image: busybox:latest
    x_neovex:
      backend: container
"#,
        );
        let control_data_dir = temp_dir.path().join("control");
        let socket_path = temp_dir.path().join("default-api.sock");
        let listener = bind_direct_listener(&socket_path).expect("listener should bind");
        let state = MachineApiState {
            control_data_dir: temp_dir.path().join("machine-control"),
            listen_mode: MachineApiListenMode::DirectSocket,
            binary_lookup_path: Some(temp_dir.path().as_os_str().to_owned()),
            helper_binary_dirs: default_guest_helper_binary_dirs(),
            service_backend: Some(Arc::new(StubMachineApiSandboxBackend)),
            machine_port_forwarder: None,
        };
        write_fake_runtime_binaries(temp_dir.path());
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let server = tokio::spawn(serve_machine_api(listener, state, async move {
            let _ = shutdown_rx.await;
        }));
        let client = MachineApiClient::new(socket_path);
        wait_for_machine_api_health(&client);

        let rendered_up = render_service_up_for_platform(
            &ServiceUpCommand {
                service: Some("db".to_owned()),
                file: compose_path,
                tenant: None,
            },
            &control_data_dir,
            ServiceHostPlatform::Macos,
            Some(client),
        )
        .await
        .expect("service up should render");
        assert!(rendered_up.contains("action: started"), "{rendered_up}");
        assert!(rendered_up.contains("service_name: db"), "{rendered_up}");

        let _ = shutdown_tx.send(());
        server
            .await
            .expect("machine API server task should join")
            .expect("machine API server should shut down cleanly");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn macos_service_up_uses_forwarded_machine_api_for_default_projects() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let compose_path = write_compose_fixture(temp_dir.path());
        let control_data_dir = temp_dir.path().join("control");
        let socket_path = temp_dir.path().join("default-api.sock");
        let listener = bind_direct_listener(&socket_path).expect("listener should bind");
        let state = MachineApiState {
            control_data_dir: temp_dir.path().join("machine-control"),
            listen_mode: MachineApiListenMode::DirectSocket,
            binary_lookup_path: Some(temp_dir.path().as_os_str().to_owned()),
            helper_binary_dirs: default_guest_helper_binary_dirs(),
            service_backend: Some(Arc::new(StubMachineApiSandboxBackend)),
            machine_port_forwarder: None,
        };
        write_fake_runtime_binaries(temp_dir.path());
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let server = tokio::spawn(serve_machine_api(listener, state, async move {
            let _ = shutdown_rx.await;
        }));
        let client = MachineApiClient::new(socket_path);
        wait_for_machine_api_health(&client);

        let rendered_up = render_service_up_for_platform(
            &ServiceUpCommand {
                service: Some("db".to_owned()),
                file: compose_path,
                tenant: None,
            },
            &control_data_dir,
            ServiceHostPlatform::Macos,
            Some(client),
        )
        .await
        .expect("service up should render for default macOS backend");
        assert!(rendered_up.contains("action: started"), "{rendered_up}");
        assert!(rendered_up.contains("service_name: db"), "{rendered_up}");

        let _ = shutdown_tx.send(());
        server
            .await
            .expect("machine API server task should join")
            .expect("machine API server should shut down cleanly");
    }

    #[test]
    fn macos_effective_backend_preserves_explicit_krun_selection() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let compose_path = write_compose_fixture_with_body(
            temp_dir.path(),
            r#"
name: Demo App
services:
  db:
    image: busybox:latest
    x_neovex:
      backend: krun
"#,
        );
        let control_data_dir = temp_dir.path().join("control");
        let context = load_compose_project_context(&compose_path, &control_data_dir)
            .expect("compose project context should load");

        let surface = resolve_service_execution_surface(
            &context,
            Some("db"),
            "service up",
            ServiceHostPlatform::Macos,
            None,
        )
        .expect("explicit krun selection should remain local");

        assert!(
            matches!(surface, ServiceExecutionSurface::Krun { .. }),
            "explicit macOS krun selection should not be rewritten to container"
        );
    }

    #[test]
    fn macos_host_loader_auto_starts_default_machine_only_for_container_projects() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let container_compose_path = write_compose_fixture_with_body(
            temp_dir.path(),
            r#"
name: Demo App
services:
  db:
    image: busybox:latest
    x_neovex:
      backend: container
"#,
        );
        let krun_compose_path = temp_dir.path().join("compose-krun.yaml");
        fs::write(
            &krun_compose_path,
            r#"
name: Demo App
services:
  db:
    image: busybox:latest
    x_neovex:
      backend: krun
"#,
        )
        .expect("krun compose fixture should write");
        let control_data_dir = temp_dir.path().join("control");
        let container_context =
            load_compose_project_context(&container_compose_path, &control_data_dir)
                .expect("container compose project context should load");
        let krun_context = load_compose_project_context(&krun_compose_path, &control_data_dir)
            .expect("krun compose project context should load");

        assert!(
            should_auto_start_default_machine_for_host_loader(
                &container_context,
                ServiceHostPlatform::Macos,
            )
            .expect("container compose project should evaluate"),
            "container-backed macOS serve should auto-start the default machine"
        );
        assert!(
            !should_auto_start_default_machine_for_host_loader(
                &krun_context,
                ServiceHostPlatform::Macos,
            )
            .expect("krun compose project should evaluate"),
            "krun-backed macOS serve should stay on the local backend"
        );
        assert!(
            !should_auto_start_default_machine_for_host_loader(
                &container_context,
                ServiceHostPlatform::Linux,
            )
            .expect("linux compose project should evaluate"),
            "non-macOS serve should not auto-start the default machine"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn macos_service_commands_use_forwarded_machine_api_for_container_projects() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let compose_path = write_compose_fixture_with_body(
            temp_dir.path(),
            r#"
name: Demo App
services:
  db:
    image: busybox:latest
    x_neovex:
      backend: container
"#,
        );
        let control_data_dir = temp_dir.path().join("control");
        let context = load_compose_project_context(&compose_path, &control_data_dir)
            .expect("compose project context should load");
        let machine_control_data_dir = temp_dir.path().join("machine-control");
        let socket_path = temp_dir.path().join("default-api.sock");
        let listener = bind_direct_listener(&socket_path).expect("listener should bind");
        let mut backend_config = ContainerSandboxBackendConfig::under_root(
            machine_control_data_dir
                .join("service-sandboxes")
                .join("container"),
        );
        backend_config.launch_mode = ContainerLaunchMode::PlanOnly;
        let state = MachineApiState {
            control_data_dir: machine_control_data_dir.clone(),
            listen_mode: MachineApiListenMode::DirectSocket,
            binary_lookup_path: Some(temp_dir.path().as_os_str().to_owned()),
            helper_binary_dirs: default_guest_helper_binary_dirs(),
            service_backend: Some(Arc::new(ContainerSandboxBackend::new(backend_config))),
            machine_port_forwarder: None,
        };
        write_fake_runtime_binaries(temp_dir.path());
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let server = tokio::spawn(serve_machine_api(listener, state, async move {
            let _ = shutdown_rx.await;
        }));
        let client = MachineApiClient::new(socket_path);
        wait_for_machine_api_health(&client);
        let container_dir = write_container_machine_manifest(
            &machine_control_data_dir
                .join("service-sandboxes")
                .join("container")
                .join("state"),
            "db-01aaa",
            context.control_plane.local_tenant_id.as_str(),
            "db",
            SandboxStatus::Ready,
        );
        fs::write(container_dir.join("ctr.log"), "guest log line\n")
            .expect("guest ctr.log should write");
        fs::write(container_dir.join("pidfile"), "2002\n").expect("pidfile should write");
        fs::write(container_dir.join("conmon.pid"), "1001\n").expect("conmon pidfile should write");

        let rendered_list = render_service_list_for_platform(
            &ServiceListCommand {
                file: compose_path.clone(),
                all_tenants: false,
            },
            &control_data_dir,
            ServiceHostPlatform::Macos,
            Some(client.clone()),
        )
        .expect("service list should render");
        assert!(
            rendered_list.contains(context.control_plane.local_tenant_id.as_str()),
            "{rendered_list}"
        );
        assert!(
            rendered_list.contains("service_name: db"),
            "{rendered_list}"
        );

        let rendered_inspect = render_service_inspect_for_platform(
            &ServiceInspectCommand {
                service: "db".to_owned(),
                file: compose_path.clone(),
                tenant: None,
            },
            &control_data_dir,
            ServiceHostPlatform::Macos,
            Some(client.clone()),
        )
        .expect("service inspect should render");
        assert!(
            rendered_inspect.contains("service_name: db"),
            "{rendered_inspect}"
        );
        assert!(rendered_inspect.contains("ctr.log"), "{rendered_inspect}");

        let current = client
            .inspect_current_service_sandbox(&context.control_plane.local_tenant_id, "db")
            .expect("current sandbox lookup should succeed")
            .details
            .expect("current sandbox should exist");
        fs::write(&current.log_paths.ctr_log, "guest log line\n")
            .expect("guest ctr.log should write");
        fs::write(current.state_dir.join("pidfile"), "2002\n").expect("pidfile should write");
        fs::write(current.state_dir.join("conmon.pid"), "1001\n")
            .expect("conmon pidfile should write");

        let rendered_ps = render_service_ps_for_platform(
            &ServicePsCommand {
                service: "db".to_owned(),
                file: compose_path.clone(),
                tenant: None,
            },
            &control_data_dir,
            ServiceHostPlatform::Macos,
            Some(client.clone()),
        )
        .expect("service ps should render");
        assert!(rendered_ps.contains("runtime_pid: 2002"), "{rendered_ps}");
        assert!(rendered_ps.contains("conmon_pid: 1001"), "{rendered_ps}");

        let rendered_down = render_service_down_for_platform(
            &ServiceDownCommand {
                service: Some("db".to_owned()),
                file: compose_path,
                tenant: None,
            },
            &control_data_dir,
            ServiceHostPlatform::Macos,
            Some(client),
        )
        .await
        .expect("service down should render");
        assert!(rendered_down.contains("action: stopped"), "{rendered_down}");
        assert!(rendered_down.contains("status: stopped"), "{rendered_down}");

        let _ = shutdown_tx.send(());
        server
            .await
            .expect("machine API server task should join")
            .expect("machine API server should shut down cleanly");
    }

    #[test]
    fn project_wide_service_operations_reject_mixed_backend_projects() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let compose_path = write_compose_fixture_with_body(
            temp_dir.path(),
            r#"
name: Demo App
services:
  api:
    image: busybox:latest
  db:
    image: busybox:latest
    x_neovex:
      backend: container
"#,
        );
        let control_data_dir = temp_dir.path().join("control");
        let context = load_compose_project_context(&compose_path, &control_data_dir)
            .expect("compose project context should load");

        let error = require_krun_backend_for_service_operation(&context, None, "service up")
            .expect_err("mixed backend project should fail fast");

        assert_eq!(
            error.to_string(),
            "invalid input: compose project demo-app mixes sandbox backends across services (api=krun, db=container); neovex service up currently requires one backend family per project-wide operation"
        );
    }

    #[test]
    fn service_scoped_operations_allow_mixed_projects_when_requested_service_is_krun() {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let compose_path = write_compose_fixture_with_body(
            temp_dir.path(),
            r#"
name: Demo App
services:
  api:
    image: busybox:latest
  db:
    image: busybox:latest
    x_neovex:
      backend: container
"#,
        );
        let control_data_dir = temp_dir.path().join("control");
        let context = load_compose_project_context(&compose_path, &control_data_dir)
            .expect("compose project context should load");

        require_krun_backend_for_service_operation(&context, Some("api"), "service up")
            .expect("krun service in mixed project should remain allowed");

        let error = require_krun_backend_for_service_operation(&context, Some("db"), "service up")
            .expect_err("container service should fail fast");
        assert_eq!(
            error.to_string(),
            "invalid input: service db in compose project demo-app selects sandbox backend container, but neovex service up only supports the krun backend today"
        );
    }

    fn write_compose_fixture(root: &Path) -> PathBuf {
        let compose_path = root.join("compose.yaml");
        fs::write(
            &compose_path,
            r#"
name: Demo App
services:
  db:
    image: busybox:latest
"#,
        )
        .expect("compose fixture should write");
        compose_path
    }

    fn write_compose_fixture_with_body(root: &Path, body: &str) -> PathBuf {
        let compose_path = root.join("compose.yaml");
        fs::write(&compose_path, body).expect("compose fixture should write");
        compose_path
    }

    fn wait_for_machine_api_health(client: &MachineApiClient) {
        let start = std::time::Instant::now();
        loop {
            match client.health() {
                Ok(_) => return,
                Err(_) if start.elapsed() < Duration::from_secs(5) => {
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(error) => panic!("machine API never became reachable: {error}"),
            }
        }
    }

    fn write_fake_runtime_binaries(dir: &Path) {
        for binary in [
            "buildah",
            "conmon",
            "crun",
            "netavark",
            "aardvark-dns",
            "fuse-overlayfs",
        ] {
            let path = dir.join(binary);
            crate::test_support::write_executable_stub(&path, "#!/bin/sh\nexit 0\n");
        }
    }

    fn write_container_machine_manifest(
        state_root: &Path,
        sandbox_id: &str,
        tenant_id: &str,
        service_name: &str,
        status: SandboxStatus,
    ) -> PathBuf {
        let container_dir = state_root.join("containers").join(sandbox_id);
        let exit_dir = state_root.join("exits");
        let persist_dir = state_root.join("persist").join(sandbox_id);
        let bundle_dir = state_root.join("bundles").join(sandbox_id);
        let network_root = state_root.join("networks");
        let run_root = network_root.join("run");
        let netns_root = network_root.join("netns");
        let container_network_dir = network_root.join("containers").join(sandbox_id);
        fs::create_dir_all(&container_dir).expect("container directory should build");
        fs::create_dir_all(&exit_dir).expect("exit directory should build");
        fs::create_dir_all(&persist_dir).expect("persist directory should build");
        fs::create_dir_all(&bundle_dir).expect("bundle directory should build");
        fs::create_dir_all(&container_network_dir)
            .expect("container network directory should build");

        let handle = neovex::SandboxHandle::new(
            neovex::SandboxId::new(sandbox_id),
            service_name,
            neovex::SandboxBackendKind::Container,
            status,
            vec![neovex::PublishedEndpoint::new(
                "http",
                neovex::PublishedEndpointProtocol::Tcp,
                SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 18080),
            )],
        );
        let manifest = json!({
            "handle": handle,
            "spec": {
                "tenant_id": tenant_id,
                "name": service_name,
                "backend": "container",
                "filesystem": {
                    "rootfs": "/tmp/rootfs",
                    "readonly": true
                },
                "process": {
                    "args": ["/bin/server"],
                    "env": ["PATH=/usr/bin"],
                    "cwd": "/",
                    "terminal": false
                },
                "resources": neovex::SandboxResourceLimits::default(),
                "lifecycle": {
                    "restart_policy": "never"
                },
                "port_bindings": [neovex::SandboxPortBinding::tcp("http", 18080, 8080)]
            },
            "image_metadata": {},
            "launch_artifact": null,
            "bundle_layout": {
                "bundle_dir": bundle_dir,
                "config_path": bundle_dir.join("config.json")
            },
            "conmon_layout": {
                "state_root": state_root,
                "container_state_dir": container_dir,
                "exit_dir": exit_dir,
                "persist_dir": persist_dir,
                "ctr_log": container_dir.join("ctr.log"),
                "oci_log": container_dir.join("oci.log"),
                "pidfile": container_dir.join("pidfile"),
                "conmon_pidfile": container_dir.join("conmon.pid"),
                "exit_status_file": exit_dir.join(sandbox_id),
                "manifest_path": container_dir.join("manifest.json")
            },
            "network_layout": {
                "network_root": network_root,
                "run_root": run_root,
                "netns_root": netns_root,
                "container_network_dir": container_network_dir,
                "netns_path": netns_root.join(sandbox_id),
                "status_path": container_network_dir.join("status.json"),
                "ipam_state_path": run_root.join("ipam-state.json"),
                "ipam_lock_path": run_root.join("ipam.lock")
            },
            "conmon_launch": {
                "create_command": {
                    "program": "/bin/true",
                    "args": []
                },
                "state_command": {
                    "program": "/bin/true",
                    "args": []
                },
                "start_command": {
                    "program": "/bin/true",
                    "args": []
                },
                "delete_command": {
                    "program": "/bin/true",
                    "args": []
                }
            },
            "last_exit_code": null,
            "launch_mode": "plan_only",
            "shutdown_requested": matches!(status, SandboxStatus::Stopped),
            "status": status
        });
        fs::write(
            container_dir.join("manifest.json"),
            serde_json::to_vec_pretty(&manifest).expect("manifest should serialize"),
        )
        .expect("manifest should write");
        container_dir
    }

    fn write_manifest(
        state_root: &Path,
        sandbox_id: &str,
        tenant_id: &str,
        service_name: &str,
        status: SandboxStatus,
    ) {
        let container_dir = state_root.join("containers").join(sandbox_id);
        fs::create_dir_all(&container_dir).expect("container directory should build");

        let handle = neovex::SandboxHandle::new(
            neovex::SandboxId::new(sandbox_id),
            service_name,
            neovex::SandboxBackendKind::Krun,
            status,
            vec![neovex::PublishedEndpoint::new(
                "http",
                neovex::PublishedEndpointProtocol::Tcp,
                SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 18080),
            )],
        );
        let manifest = json!({
            "handle": handle,
            "spec": {
                "tenant_id": tenant_id,
                "name": service_name,
                "backend": "krun",
                "filesystem": {
                    "rootfs": "/tmp/rootfs",
                    "readonly": true
                },
                "process": {
                    "args": ["/bin/server"],
                    "env": ["PATH=/usr/bin"],
                    "cwd": "/",
                    "terminal": false
                },
                "resources": neovex::SandboxResourceLimits::default(),
                "lifecycle": {
                    "restart_policy": "never"
                },
                "port_bindings": [neovex::SandboxPortBinding::tcp("http", 18080, 8080)]
            },
            "conmon_layout": {
                "container_state_dir": container_dir,
                "ctr_log": container_dir.join("ctr.log"),
                "oci_log": container_dir.join("oci.log")
            },
            "last_exit_code": null,
            "restart_count": 0,
            "shutdown_requested": matches!(status, SandboxStatus::Stopped),
            "status": status
        });
        fs::write(
            container_dir.join("manifest.json"),
            serde_json::to_vec_pretty(&manifest).expect("manifest should serialize"),
        )
        .expect("manifest should write");
    }

    fn sample_spec(tenant: &TenantId, service_name: &str) -> SandboxSpec {
        SandboxSpec::new(
            tenant.clone(),
            service_name,
            SandboxBackendKind::Krun,
            SandboxFilesystemSpec::new("/tmp/rootfs"),
            SandboxProcessSpec::new(["/bin/server"]),
        )
    }

    fn stub_handle(id: &SandboxId, service_name: &str, status: SandboxStatus) -> SandboxHandle {
        SandboxHandle::new(
            id.clone(),
            service_name,
            SandboxBackendKind::Krun,
            status,
            Vec::new(),
        )
    }

    #[derive(Default)]
    struct StubBackend {
        handles: Mutex<BTreeMap<String, SandboxHandle>>,
        started_services: Mutex<Vec<String>>,
        stopped_ids: Mutex<Vec<String>>,
    }

    impl StubBackend {
        fn with_handles(handles: impl IntoIterator<Item = SandboxHandle>) -> Self {
            let backend = Self::default();
            for handle in handles {
                backend
                    .handles
                    .lock()
                    .expect("handles lock should hold")
                    .insert(handle.id.as_str().to_owned(), handle);
            }
            backend
        }
    }

    #[derive(Default)]
    struct StubMachineApiSandboxBackend;

    impl SandboxBackend for StubMachineApiSandboxBackend {
        fn kind(&self) -> SandboxBackendKind {
            SandboxBackendKind::Container
        }

        fn start(&self, spec: SandboxSpec) -> SandboxFuture<SandboxHandle> {
            let handle = SandboxHandle::new(
                SandboxId::new(format!("{}-01stub", spec.name)),
                &spec.name,
                SandboxBackendKind::Container,
                SandboxStatus::Ready,
                Vec::new(),
            );
            Box::pin(async move { Ok(handle) })
        }

        fn start_from_image(&self, launch: SandboxImageLaunchSpec) -> SandboxFuture<SandboxHandle> {
            self.start(launch.spec)
        }

        fn start_from_build(&self, launch: SandboxBuildLaunchSpec) -> SandboxFuture<SandboxHandle> {
            self.start(launch.spec)
        }

        fn inspect(&self, _id: &SandboxId) -> SandboxFuture<Option<SandboxHandle>> {
            Box::pin(async move { Ok(None) })
        }

        fn stop(&self, _id: &SandboxId) -> SandboxFuture<()> {
            Box::pin(async move { Ok(()) })
        }
    }

    impl SandboxBackend for StubBackend {
        fn kind(&self) -> SandboxBackendKind {
            SandboxBackendKind::Krun
        }

        fn start(&self, spec: SandboxSpec) -> SandboxFuture<SandboxHandle> {
            let handle = stub_handle(
                &SandboxId::new(format!("{}-01stub", spec.name)),
                &spec.name,
                SandboxStatus::Starting,
            );
            self.handles
                .lock()
                .expect("handles lock should hold")
                .insert(handle.id.as_str().to_owned(), handle.clone());
            Box::pin(async move { Ok(handle) })
        }

        fn start_from_image(&self, launch: SandboxImageLaunchSpec) -> SandboxFuture<SandboxHandle> {
            self.started_services
                .lock()
                .expect("started services lock should hold")
                .push(launch.spec.name.clone());
            self.start(launch.spec)
        }

        fn start_from_build(&self, launch: SandboxBuildLaunchSpec) -> SandboxFuture<SandboxHandle> {
            self.started_services
                .lock()
                .expect("started services lock should hold")
                .push(launch.spec.name.clone());
            self.start(launch.spec)
        }

        fn inspect(&self, id: &SandboxId) -> SandboxFuture<Option<SandboxHandle>> {
            let handle = self
                .handles
                .lock()
                .expect("handles lock should hold")
                .get(id.as_str())
                .cloned();
            Box::pin(async move { Ok(handle) })
        }

        fn stop(&self, id: &SandboxId) -> SandboxFuture<()> {
            self.stopped_ids
                .lock()
                .expect("stopped ids lock should hold")
                .push(id.as_str().to_owned());
            if let Some(handle) = self
                .handles
                .lock()
                .expect("handles lock should hold")
                .get_mut(id.as_str())
            {
                handle.status = SandboxStatus::Stopped;
            }
            Box::pin(async move { Ok(()) })
        }
    }
}
