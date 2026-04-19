use super::capabilities::machine_api_capability_response;
use super::logs::read_log_chunk;
use super::process::{read_pid_file_if_exists, snapshot_process_rows};
use super::state::{
    container_state_error_to_http_error, container_state_view, internal_error_to_http_error,
    machine_api_details_from_container_details, machine_api_summary_from_container_summary,
    refresh_persisted_service_sandbox_state, service_sandbox_status_needs_refresh,
};
use super::*;

pub(super) fn machine_api_router(state: MachineApiState) -> Router {
    Router::new()
        .route("/healthz", get(machine_api_healthz))
        .route(
            "/v1/machine-api/capabilities",
            get(machine_api_capabilities),
        )
        .route(
            "/v1/machine-api/service-sandboxes/image-start",
            post(machine_api_start_image_service_sandbox),
        )
        .route(
            "/v1/machine-api/service-sandboxes/build-start",
            post(machine_api_start_build_service_sandbox),
        )
        .route(
            "/v1/machine-api/service-sandboxes",
            get(machine_api_list_service_sandboxes),
        )
        .route(
            "/v1/machine-api/service-sandboxes/current",
            get(machine_api_lookup_current_service_sandbox),
        )
        .route(
            "/v1/machine-api/service-sandboxes/{sandbox_id}",
            get(machine_api_inspect_service_sandbox),
        )
        .route(
            "/v1/machine-api/service-sandboxes/{sandbox_id}/logs",
            get(machine_api_read_service_sandbox_logs),
        )
        .route(
            "/v1/machine-api/service-sandboxes/{sandbox_id}/ps",
            get(machine_api_service_sandbox_process_snapshot),
        )
        .route(
            "/v1/machine-api/service-sandboxes/{sandbox_id}/stop",
            post(machine_api_stop_service_sandbox),
        )
        .with_state(state)
}

async fn machine_api_healthz(
    State(state): State<MachineApiState>,
) -> axum::Json<MachineApiHealthResponse> {
    axum::Json(MachineApiHealthResponse {
        status: "ok".to_owned(),
        role: MACHINE_API_ROLE.to_owned(),
        protocol_version: PROTOCOL_VERSION.to_owned(),
        listen_mode: state.listen_mode.as_str().to_owned(),
        control_data_dir: state.control_data_dir.display().to_string(),
    })
}

async fn machine_api_capabilities(
    State(state): State<MachineApiState>,
) -> axum::Json<MachineApiCapabilityResponse> {
    axum::Json(machine_api_capability_response(&state))
}

async fn machine_api_start_image_service_sandbox(
    State(state): State<MachineApiState>,
    Json(request): Json<MachineApiServiceSandboxImageStartRequest>,
) -> Result<Json<MachineApiServiceSandboxStartResponse>, MachineApiHttpError> {
    let backend = require_service_backend(&state)?;
    let handle = backend
        .start_from_image(request.launch)
        .await
        .map_err(sandbox_error_to_http_error)?;
    Ok(Json(MachineApiServiceSandboxStartResponse { handle }))
}

async fn machine_api_start_build_service_sandbox(
    State(state): State<MachineApiState>,
    Json(request): Json<MachineApiServiceSandboxBuildStartRequest>,
) -> Result<Json<MachineApiServiceSandboxStartResponse>, MachineApiHttpError> {
    let backend = require_service_backend(&state)?;
    let handle = backend
        .start_from_build(request.launch)
        .await
        .map_err(sandbox_error_to_http_error)?;
    Ok(Json(MachineApiServiceSandboxStartResponse { handle }))
}

async fn machine_api_inspect_service_sandbox(
    State(state): State<MachineApiState>,
    AxumPath(sandbox_id): AxumPath<String>,
) -> Result<Json<MachineApiServiceSandboxInspectResponse>, MachineApiHttpError> {
    let backend = require_service_backend(&state)?;
    let sandbox_id = neovex::SandboxId::new(sandbox_id);
    let handle = backend
        .inspect(&sandbox_id)
        .await
        .map_err(sandbox_error_to_http_error)?;
    Ok(Json(MachineApiServiceSandboxInspectResponse {
        sandbox_id,
        handle,
    }))
}

#[derive(Debug, Deserialize)]
struct MachineApiServiceSandboxListQuery {
    #[serde(default)]
    tenant_id: Option<TenantId>,
}

async fn machine_api_list_service_sandboxes(
    State(state): State<MachineApiState>,
    Query(query): Query<MachineApiServiceSandboxListQuery>,
) -> Result<Json<MachineApiServiceSandboxListResponse>, MachineApiHttpError> {
    require_service_backend(&state)?;
    let view = container_state_view(&state);
    let summaries = match query.tenant_id.as_ref() {
        Some(tenant_id) => view
            .list_for_tenant(tenant_id)
            .map_err(container_state_error_to_http_error)?,
        None => view.list().map_err(container_state_error_to_http_error)?,
    };
    let sandbox_ids = summaries
        .iter()
        .filter(|summary| service_sandbox_status_needs_refresh(summary.status))
        .map(|summary| summary.sandbox_id.clone())
        .collect::<Vec<_>>();
    refresh_persisted_service_sandbox_state(&state, sandbox_ids).await?;

    let sandboxes = match query.tenant_id.as_ref() {
        Some(tenant_id) => view
            .list_for_tenant(tenant_id)
            .map_err(container_state_error_to_http_error)?,
        None => view.list().map_err(container_state_error_to_http_error)?,
    }
    .into_iter()
    .map(machine_api_summary_from_container_summary)
    .collect();

    Ok(Json(MachineApiServiceSandboxListResponse { sandboxes }))
}

#[derive(Debug, Deserialize)]
struct MachineApiCurrentServiceSandboxQuery {
    tenant_id: TenantId,
    service_name: String,
}

async fn machine_api_lookup_current_service_sandbox(
    State(state): State<MachineApiState>,
    Query(query): Query<MachineApiCurrentServiceSandboxQuery>,
) -> Result<Json<MachineApiServiceSandboxLookupResponse>, MachineApiHttpError> {
    require_service_backend(&state)?;
    let view = container_state_view(&state);
    let sandbox_ids = view
        .list_for_tenant(&query.tenant_id)
        .map_err(container_state_error_to_http_error)?
        .into_iter()
        .filter(|summary| {
            summary.service_name == query.service_name
                && service_sandbox_status_needs_refresh(summary.status)
        })
        .map(|summary| summary.sandbox_id)
        .collect::<Vec<_>>();
    refresh_persisted_service_sandbox_state(&state, sandbox_ids).await?;
    let details = view
        .inspect_service(&query.tenant_id, &query.service_name)
        .map_err(container_state_error_to_http_error)?
        .map(machine_api_details_from_container_details);

    Ok(Json(MachineApiServiceSandboxLookupResponse {
        tenant_id: query.tenant_id,
        service_name: query.service_name,
        details,
    }))
}

#[derive(Debug, Default, Deserialize)]
struct MachineApiServiceSandboxLogQuery {
    #[serde(default)]
    offset: u64,
}

async fn machine_api_read_service_sandbox_logs(
    State(state): State<MachineApiState>,
    AxumPath(sandbox_id): AxumPath<String>,
    Query(query): Query<MachineApiServiceSandboxLogQuery>,
) -> Result<Json<MachineApiServiceSandboxLogChunkResponse>, MachineApiHttpError> {
    require_service_backend(&state)?;
    let sandbox_id = neovex::SandboxId::new(sandbox_id);
    let view = container_state_view(&state);
    let log_paths = view
        .log_paths(&sandbox_id)
        .map_err(container_state_error_to_http_error)?
        .ok_or_else(|| MachineApiHttpError {
            status: StatusCode::NOT_FOUND,
            message: format!("sandbox instance was not found: {sandbox_id}"),
        })?;
    let (chunk, next_offset) =
        read_log_chunk(&log_paths.ctr_log, query.offset).map_err(internal_error_to_http_error)?;

    Ok(Json(MachineApiServiceSandboxLogChunkResponse {
        sandbox_id,
        offset: query.offset,
        next_offset,
        chunk,
    }))
}

async fn machine_api_service_sandbox_process_snapshot(
    State(state): State<MachineApiState>,
    AxumPath(sandbox_id): AxumPath<String>,
) -> Result<Json<MachineApiServiceProcessSnapshotResponse>, MachineApiHttpError> {
    require_service_backend(&state)?;
    let sandbox_id = neovex::SandboxId::new(sandbox_id);
    let view = container_state_view(&state);
    let details = view
        .inspect(&sandbox_id)
        .map_err(container_state_error_to_http_error)?
        .ok_or_else(|| MachineApiHttpError {
            status: StatusCode::NOT_FOUND,
            message: format!("sandbox instance was not found: {sandbox_id}"),
        })?;
    let runtime_pidfile = details.state_dir.join("pidfile");
    let conmon_pidfile = details.state_dir.join("conmon.pid");
    let runtime_pid =
        read_pid_file_if_exists(&runtime_pidfile).map_err(internal_error_to_http_error)?;
    let conmon_pid =
        read_pid_file_if_exists(&conmon_pidfile).map_err(internal_error_to_http_error)?;
    let process_rows = snapshot_process_rows(runtime_pid, conmon_pid)
        .map_err(internal_error_to_http_error)?
        .into_iter()
        .map(|row| MachineApiServiceProcessRow {
            pid: row.pid,
            ppid: row.ppid,
            command: row.command,
        })
        .collect();

    Ok(Json(MachineApiServiceProcessSnapshotResponse {
        snapshot: MachineApiServiceProcessSnapshot {
            sandbox_id: details.summary.sandbox_id,
            tenant_id: details.summary.tenant_id,
            service_name: details.summary.service_name,
            status: details.summary.status,
            runtime_pidfile,
            conmon_pidfile,
            runtime_pid,
            conmon_pid,
            process_rows,
        },
    }))
}

async fn machine_api_stop_service_sandbox(
    State(state): State<MachineApiState>,
    AxumPath(sandbox_id): AxumPath<String>,
) -> Result<Json<MachineApiServiceSandboxStopResponse>, MachineApiHttpError> {
    let backend = require_service_backend(&state)?;
    let sandbox_id = neovex::SandboxId::new(sandbox_id);
    backend
        .stop(&sandbox_id)
        .await
        .map_err(sandbox_error_to_http_error)?;
    Ok(Json(MachineApiServiceSandboxStopResponse {
        sandbox_id,
        stopped: true,
    }))
}
