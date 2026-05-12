use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use nimbus::{Error, SandboxStatus, TenantId};
use nimbus_sandbox::backends::krun::KrunSandboxStateView;
use serde::Serialize;

use crate::compose::discovery::ResolvedComposeSelection;
use crate::machine::MachineApiClient;

use super::{
    ComposeProjectContext, ComposeTopCommand, ServiceHostPlatform,
    load_compose_project_context_for_selection, lookup_current_remote_service_details,
    machine_api_operation_error, missing_persisted_service_error, render_state_lookup_error,
    require_krun_backend_for_service_operation, resolve_service_execution_surface,
    validate_forwarded_machine_api_operations,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct ServiceProcessSnapshot {
    pub(super) sandbox_id: nimbus::SandboxId,
    pub(super) tenant_id: TenantId,
    pub(super) service_name: String,
    pub(super) status: SandboxStatus,
    pub(super) runtime_pidfile: PathBuf,
    pub(super) conmon_pidfile: PathBuf,
    pub(super) runtime_pid: Option<u32>,
    pub(super) conmon_pid: Option<u32>,
    pub(super) process_rows: Vec<ServiceProcessRow>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct ServiceProcessRow {
    pub(super) pid: u32,
    pub(super) ppid: u32,
    pub(super) command: String,
}

pub(super) fn resolve_service_process_snapshot_for_selection(
    command: &ComposeTopCommand,
    selection: &ResolvedComposeSelection,
    control_data_dir: &Path,
    host_platform: ServiceHostPlatform,
    machine_api_client: Option<MachineApiClient>,
) -> Result<ServiceProcessSnapshot, Error> {
    let context = load_compose_project_context_for_selection(selection, control_data_dir)?;
    let tenant = command
        .tenant
        .clone()
        .unwrap_or_else(|| context.control_plane.local_tenant_id.clone());
    match resolve_service_execution_surface(
        &context,
        Some(&command.service),
        "compose top",
        host_platform,
        machine_api_client,
    )? {
        super::ServiceExecutionSurface::Krun { .. } => {
            resolve_krun_service_process_snapshot(&context, &tenant, &command.service)
        }
        super::ServiceExecutionSurface::ForwardedContainer { client, .. } => {
            validate_forwarded_machine_api_operations(
                &context,
                &client,
                "compose top",
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

fn resolve_krun_service_process_snapshot(
    context: &ComposeProjectContext,
    tenant: &TenantId,
    service_name: &str,
) -> Result<ServiceProcessSnapshot, Error> {
    require_krun_backend_for_service_operation(context, Some(service_name), "compose top")?;
    let state_view =
        KrunSandboxStateView::from_config(&context.control_plane.krun_backend_config());
    let details = state_view
        .inspect_service(tenant, service_name)
        .map_err(|error| render_state_lookup_error("resolve persisted service processes", error))?
        .ok_or_else(|| {
            Error::InvalidInput(format!(
                "no persisted sandbox state found for service {} in tenant {} under project {}",
                service_name, tenant, context.control_plane.project_name
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

pub(super) fn read_pid_file_if_exists(path: &Path) -> Result<Option<u32>, Error> {
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

pub(super) fn snapshot_process_rows(
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

pub(super) fn parse_process_rows(stdout: &str, pid_set: &BTreeSet<u32>) -> Vec<ServiceProcessRow> {
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
