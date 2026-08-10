use super::bootc::{
    machine_api_bootc_rollback, machine_api_bootc_status, machine_api_bootc_switch,
    machine_api_bootc_upgrade,
};
use super::capabilities::machine_api_capability_response;
use super::logs::read_log_chunk;
use super::process::{read_pid_file_if_exists, snapshot_process_rows};
use super::state::{
    container_state_error_to_http_error, container_state_view, internal_error_to_http_error,
    machine_api_details_from_container_details, machine_api_summary_from_container_summary,
    refresh_persisted_service_sandbox_state, service_sandbox_status_needs_refresh,
};
use super::*;
use nimbus_machine::api::{
    MACHINE_API_BOOTC_ROLLBACK_PATH, MACHINE_API_BOOTC_STATUS_PATH, MACHINE_API_BOOTC_SWITCH_PATH,
    MACHINE_API_BOOTC_UPGRADE_PATH, MACHINE_API_CAPABILITIES_PATH,
    MACHINE_API_CURRENT_SERVICE_SANDBOX_PATH, MACHINE_API_HEALTH_PATH,
    MACHINE_API_SERVICE_SANDBOX_LOGS_PATH, MACHINE_API_SERVICE_SANDBOX_PATH,
    MACHINE_API_SERVICE_SANDBOX_PROCESS_SNAPSHOT_PATH, MACHINE_API_SERVICE_SANDBOX_STOP_PATH,
    MACHINE_API_SERVICE_SANDBOXES_PATH, MACHINE_API_WORKLOAD_PROVISION_PHASE_PATH,
    MACHINE_API_WORKLOAD_RESTART_PHASE_PATH, MACHINE_API_WORKLOAD_TEARDOWN_PHASE_PATH,
    MachineApiWorkloadProvisionPhaseRequest, MachineApiWorkloadProvisionPhaseResponse,
    MachineApiWorkloadRestartPhaseRequest, MachineApiWorkloadRestartPhaseResponse,
    MachineApiWorkloadTeardownPhaseRequest, MachineApiWorkloadTeardownPhaseResponse,
};

/// Bounded well above the closed JSON envelope while preventing unbounded
/// allocation before strict request decoding and authority authentication.
pub(super) const MAX_WORKLOAD_TEARDOWN_REQUEST_BODY_BYTES: usize = 1024 * 1024;

pub(super) fn machine_api_router(state: MachineApiState) -> Router {
    Router::new()
        .route(MACHINE_API_HEALTH_PATH, get(machine_api_healthz))
        .route(MACHINE_API_CAPABILITIES_PATH, get(machine_api_capabilities))
        .route(MACHINE_API_BOOTC_STATUS_PATH, get(machine_api_bootc_status))
        .route(
            MACHINE_API_BOOTC_SWITCH_PATH,
            post(machine_api_bootc_switch),
        )
        .route(
            MACHINE_API_BOOTC_UPGRADE_PATH,
            post(machine_api_bootc_upgrade),
        )
        .route(
            MACHINE_API_BOOTC_ROLLBACK_PATH,
            post(machine_api_bootc_rollback),
        )
        .route(
            MACHINE_API_WORKLOAD_PROVISION_PHASE_PATH,
            post(machine_api_workload_provision_phase),
        )
        .route(
            MACHINE_API_WORKLOAD_RESTART_PHASE_PATH,
            post(machine_api_workload_restart_phase),
        )
        .route(
            MACHINE_API_WORKLOAD_TEARDOWN_PHASE_PATH,
            post(machine_api_workload_teardown_phase).layer(DefaultBodyLimit::max(
                MAX_WORKLOAD_TEARDOWN_REQUEST_BODY_BYTES,
            )),
        )
        .route(
            MACHINE_API_SERVICE_SANDBOXES_PATH,
            get(machine_api_list_service_sandboxes),
        )
        .route(
            MACHINE_API_CURRENT_SERVICE_SANDBOX_PATH,
            get(machine_api_lookup_current_service_sandbox),
        )
        .route(
            MACHINE_API_SERVICE_SANDBOX_PATH,
            get(machine_api_inspect_service_sandbox),
        )
        .route(
            MACHINE_API_SERVICE_SANDBOX_LOGS_PATH,
            get(machine_api_read_service_sandbox_logs),
        )
        .route(
            MACHINE_API_SERVICE_SANDBOX_PROCESS_SNAPSHOT_PATH,
            get(machine_api_service_sandbox_process_snapshot),
        )
        .route(
            MACHINE_API_SERVICE_SANDBOX_STOP_PATH,
            post(machine_api_stop_service_sandbox),
        )
        .with_state(state)
}

async fn machine_api_workload_provision_phase(
    State(state): State<MachineApiState>,
    Json(request): Json<MachineApiWorkloadProvisionPhaseRequest>,
) -> Result<Json<MachineApiWorkloadProvisionPhaseResponse>, MachineApiHttpError> {
    // Authentication deliberately precedes facade lookup and every guest
    // provider call. A stale or foreign parent cannot probe or mutate guest
    // workload state merely by presenting an internally consistent command.
    require_forwarder_authority(&state, request.forwarder_authority())?;
    let workloads = require_service_workloads(&state)?;
    let observation = workloads
        .provision_phase(request.command(), request.forwarder_authority())
        .await?;
    let response = MachineApiWorkloadProvisionPhaseResponse::for_request(&request, observation)
        .map_err(|error| MachineApiHttpError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: format!(
                "machine API workload provision response violated its exact wire contract: {error}"
            ),
        })?;
    Ok(Json(response))
}

async fn machine_api_workload_restart_phase(
    State(state): State<MachineApiState>,
    Json(request): Json<MachineApiWorkloadRestartPhaseRequest>,
) -> Result<Json<MachineApiWorkloadRestartPhaseResponse>, MachineApiHttpError> {
    // Authentication precedes facade lookup, journal access, inspection, and
    // every provider effect. The request digest already binds this authority
    // to the complete compute-confirmed command.
    require_forwarder_authority(&state, request.forwarder_authority())?;
    let workloads = require_service_workloads(&state)?;
    let observation = workloads.restart_phase(request.command()).await?;
    let response = MachineApiWorkloadRestartPhaseResponse::for_request(&request, observation)
        .map_err(|error| MachineApiHttpError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: format!(
                "machine API workload restart response violated its exact wire contract: {error}"
            ),
        })?;
    Ok(Json(response))
}

async fn machine_api_workload_teardown_phase(
    State(state): State<MachineApiState>,
    Json(request): Json<MachineApiWorkloadTeardownPhaseRequest>,
) -> Result<Json<MachineApiWorkloadTeardownPhaseResponse>, MachineApiHttpError> {
    // Strict wire decoding and parent authority authentication precede facade
    // lookup, journal inspection, and every guest provider operation.
    require_forwarder_authority(&state, request.forwarder_authority())?;
    let workloads = require_service_workloads(&state)?;
    let result = workloads
        .teardown_phase(request.command(), request.forwarder_authority())
        .await?;
    let response = MachineApiWorkloadTeardownPhaseResponse::for_request(&request, result).map_err(
        |error| MachineApiHttpError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: format!(
                "machine API workload teardown response violated its exact wire contract: {error}"
            ),
        },
    )?;
    Ok(Json(response))
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

async fn machine_api_inspect_service_sandbox(
    State(state): State<MachineApiState>,
    AxumPath(sandbox_id): AxumPath<String>,
) -> Result<Json<MachineApiServiceSandboxInspectResponse>, MachineApiHttpError> {
    let workloads = require_service_workloads(&state)?;
    let sandbox_id = nimbus::SandboxId::new(sandbox_id);
    let inspection = workloads.inspect(&sandbox_id).await?;
    Ok(Json(MachineApiServiceSandboxInspectResponse {
        sandbox_id,
        inspection,
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
    require_service_workloads(&state)?;
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
    require_service_workloads(&state)?;
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
    require_service_workloads(&state)?;
    let sandbox_id = nimbus::SandboxId::new(sandbox_id);
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
    require_service_workloads(&state)?;
    let sandbox_id = nimbus::SandboxId::new(sandbox_id);
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
    Json(request): Json<MachineApiServiceSandboxStopRequest>,
) -> Result<Json<MachineApiServiceSandboxStopResponse>, MachineApiHttpError> {
    require_forwarder_authority(&state, &request.forwarder_authority)?;
    let workloads = require_service_workloads(&state)?;
    let sandbox_id = nimbus::SandboxId::new(sandbox_id);
    if workloads.inspect(&sandbox_id).await?.is_some() {
        workloads.stop(&sandbox_id).await?;
    }
    let Some(absence) = workloads.absent_machine_port_receipts(&sandbox_id).await? else {
        return Err(MachineApiHttpError {
            status: StatusCode::NOT_FOUND,
            message: format!("sandbox instance was not found: {sandbox_id}"),
        });
    };
    if absence.sandbox_id != sandbox_id {
        return Err(MachineApiHttpError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: format!(
                "machine port absence evidence identified sandbox {} instead of {sandbox_id}",
                absence.sandbox_id
            ),
        });
    }
    Ok(Json(MachineApiServiceSandboxStopResponse {
        tenant_id: absence.tenant_id,
        sandbox_id,
        stopped: true,
        forwarder_authority: request.forwarder_authority,
        confirmed_absent_evidence: absence.receipts,
    }))
}
