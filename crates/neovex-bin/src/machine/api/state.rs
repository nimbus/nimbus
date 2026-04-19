use super::*;

pub(super) fn container_state_view(state: &MachineApiState) -> ContainerSandboxStateView {
    ContainerSandboxStateView::new(machine_container_state_root(&state.control_data_dir))
}

pub(super) fn service_sandbox_status_needs_refresh(status: SandboxStatus) -> bool {
    matches!(
        status,
        SandboxStatus::Starting
            | SandboxStatus::Ready
            | SandboxStatus::NotReady
            | SandboxStatus::Stopping
    )
}

pub(super) async fn refresh_persisted_service_sandbox_state(
    state: &MachineApiState,
    sandbox_ids: Vec<neovex::SandboxId>,
) -> Result<(), MachineApiHttpError> {
    let backend = require_service_backend(state)?;
    for sandbox_id in sandbox_ids {
        let _ = backend
            .inspect(&sandbox_id)
            .await
            .map_err(sandbox_error_to_http_error)?;
    }
    Ok(())
}

pub(super) fn machine_container_state_root(control_data_dir: &Path) -> PathBuf {
    control_data_dir
        .join("service-sandboxes")
        .join("container")
        .join("state")
}

pub(super) fn machine_api_summary_from_container_summary(
    summary: neovex_sandbox::backends::container::ContainerSandboxSummary,
) -> MachineApiServiceSandboxSummary {
    MachineApiServiceSandboxSummary {
        sandbox_id: summary.sandbox_id,
        tenant_id: summary.tenant_id,
        service_name: summary.service_name,
        status: summary.status,
        published_endpoints: summary.published_endpoints,
        restart_count: summary.restart_count,
        last_exit_code: summary.last_exit_code,
        shutdown_requested: summary.shutdown_requested,
    }
}

pub(super) fn machine_api_details_from_container_details(
    details: neovex_sandbox::backends::container::ContainerSandboxDetails,
) -> MachineApiServiceSandboxDetails {
    MachineApiServiceSandboxDetails {
        summary: machine_api_summary_from_container_summary(details.summary),
        resources: details.resources,
        lifecycle: details.lifecycle,
        port_bindings: details.port_bindings,
        log_paths: MachineApiServiceSandboxLogPaths {
            ctr_log: details.log_paths.ctr_log,
            oci_log: details.log_paths.oci_log,
        },
        state_dir: details.state_dir,
        manifest_path: details.manifest_path,
    }
}

pub(super) fn container_state_error_to_http_error(
    error: neovex_sandbox::SandboxError,
) -> MachineApiHttpError {
    MachineApiHttpError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        message: format!("failed to read persisted service sandbox state: {error}"),
    }
}

pub(super) fn internal_error_to_http_error(message: String) -> MachineApiHttpError {
    MachineApiHttpError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        message,
    }
}
