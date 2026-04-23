use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use neovex::Error;
use neovex_sandbox::backends::krun::KrunSandboxStateView;

use crate::cli_ux;
use crate::machine::MachineApiClient;

use super::{
    ComposeLogsCommand, ServiceHostPlatform, load_compose_project_context,
    lookup_current_remote_service_details, machine_api_operation_error,
    missing_persisted_service_error, render_state_lookup_error,
    require_krun_backend_for_service_operation, resolve_service_execution_surface,
    validate_forwarded_machine_api_operations,
};

pub(super) fn run_compose_logs_for_platform(
    command: &ComposeLogsCommand,
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
        "compose logs",
        host_platform,
        machine_api_client,
    )? {
        super::ServiceExecutionSurface::Krun { .. } => {
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
        super::ServiceExecutionSurface::ForwardedContainer { client, .. } => {
            validate_forwarded_machine_api_operations(
                &context,
                &client,
                "compose logs",
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
                        machine_api_operation_error("read persisted compose logs", &client, error)
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

pub(super) fn flush_service_log_chunk(service_name: &str, chunk: &str) -> Result<(), Error> {
    if chunk.is_empty() {
        return Ok(());
    }

    cli_ux::write_stdout(chunk).map_err(|error| {
        Error::Internal(format!(
            "failed to write persisted logs for service {}: {error}",
            service_name
        ))
    })
}

pub(super) fn resolve_service_ctr_log_path(
    command: &ComposeLogsCommand,
    control_data_dir: &Path,
) -> Result<PathBuf, Error> {
    let context = load_compose_project_context(&command.file, control_data_dir)?;
    require_krun_backend_for_service_operation(&context, Some(&command.service), "compose logs")?;
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

pub(super) fn read_log_chunk(path: &Path, offset: u64) -> Result<(String, u64), Error> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok((String::new(), offset));
        }
        Err(error) => {
            return Err(Error::Internal(format!(
                "failed to open persisted service log {}: {error}",
                path.display()
            )));
        }
    };

    file.seek(SeekFrom::Start(offset)).map_err(|error| {
        Error::Internal(format!(
            "failed to seek persisted service log {} to offset {}: {error}",
            path.display(),
            offset
        ))
    })?;
    let mut chunk = String::new();
    file.read_to_string(&mut chunk).map_err(|error| {
        Error::Internal(format!(
            "failed to read persisted service log {}: {error}",
            path.display()
        ))
    })?;
    let next_offset = file.stream_position().map_err(|error| {
        Error::Internal(format!(
            "failed to determine persisted service log offset for {}: {error}",
            path.display()
        ))
    })?;
    Ok((chunk, next_offset))
}
