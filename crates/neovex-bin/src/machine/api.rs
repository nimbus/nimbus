use std::ffi::{OsStr, OsString};
use std::fs;
use std::fs::File;
use std::future::Future;
use std::io::{Read, Seek, SeekFrom};
use std::net::{TcpStream, ToSocketAddrs};
use std::os::fd::FromRawFd;
use std::os::unix::net::UnixListener as StdUnixListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use neovex::{Error, SandboxBackend, SandboxBackendKind, SandboxError, TenantId};
use neovex_sandbox::backends::container::{
    ContainerSandboxBackend, ContainerSandboxBackendConfig, ContainerSandboxStateView,
    OciMachinePortForwarderConfig,
};
use serde::Deserialize;

use super::protocol::{
    MACHINE_API_ROLE, MachineApiCapabilityResponse, MachineApiErrorResponse,
    MachineApiHealthResponse, MachineApiRequiredBinaryStatus, MachineApiServiceExecutionMode,
    MachineApiServiceProcessRow, MachineApiServiceProcessSnapshot,
    MachineApiServiceProcessSnapshotResponse, MachineApiServiceSandboxBuildStartRequest,
    MachineApiServiceSandboxDetails, MachineApiServiceSandboxImageStartRequest,
    MachineApiServiceSandboxInspectResponse, MachineApiServiceSandboxListResponse,
    MachineApiServiceSandboxLogChunkResponse, MachineApiServiceSandboxLogPaths,
    MachineApiServiceSandboxLookupResponse, MachineApiServiceSandboxStartResponse,
    MachineApiServiceSandboxStopResponse, MachineApiServiceSandboxSummary, PROTOCOL_VERSION,
};
use super::{MachineApiCommand, MachineRootLayout};

const DEFAULT_SYSTEMD_SOCKET_FD: i32 = 3;
const STANDARD_CONTAINER_RUNTIME_BINARIES: &[&str] = &[
    "buildah",
    "conmon",
    "crun",
    "netavark",
    "aardvark-dns",
    "fuse-overlayfs",
];
const STANDARD_CONTAINER_IMAGE_START_BINARIES: &[&str] =
    &["conmon", "crun", "netavark", "aardvark-dns"];
const STANDARD_CONTAINER_BUILD_START_BINARIES: &[&str] = &["buildah", "fuse-overlayfs"];
const DEFAULT_GUEST_HELPER_BINARY_DIRS: &[&str] = &[
    "/usr/local/libexec/podman",
    "/usr/local/lib/podman",
    "/usr/libexec/podman",
    "/usr/lib/podman",
];
const MACHINE_API_OPERATION_BLOCKER: &str =
    "guest machine API does not yet expose service lifecycle operations";
const MACHINE_API_IMAGE_START_OPERATION: &str = "service-sandboxes.image-start";
const MACHINE_API_BUILD_START_OPERATION: &str = "service-sandboxes.build-start";
const MACHINE_API_LIST_OPERATION: &str = "service-sandboxes.list";
const MACHINE_API_INSPECT_OPERATION: &str = "service-sandboxes.inspect";
const MACHINE_API_INSPECT_CURRENT_OPERATION: &str = "service-sandboxes.inspect-current";
const MACHINE_API_LOGS_OPERATION: &str = "service-sandboxes.logs";
const MACHINE_API_PS_OPERATION: &str = "service-sandboxes.ps";
const MACHINE_API_STOP_OPERATION: &str = "service-sandboxes.stop";
const MACHINE_PORT_FORWARDER_TIMEOUT: Duration = Duration::from_millis(200);

#[derive(Clone)]
pub(crate) struct MachineApiState {
    pub(crate) control_data_dir: PathBuf,
    pub(crate) listen_mode: MachineApiListenMode,
    pub(crate) binary_lookup_path: Option<OsString>,
    pub(crate) helper_binary_dirs: Vec<PathBuf>,
    pub(crate) service_backend: Option<Arc<dyn SandboxBackend>>,
    pub(crate) machine_port_forwarder: Option<OciMachinePortForwarderConfig>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MachineApiListenMode {
    DirectSocket,
    SystemdSocketActivation,
}

impl MachineApiListenMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::DirectSocket => "direct-socket",
            Self::SystemdSocketActivation => "systemd-socket-activation",
        }
    }
}

pub(super) async fn run_machine_api_command(
    command: MachineApiCommand,
    roots: &MachineRootLayout,
) -> Result<(), Error> {
    let default_control_data_dir = roots
        .state_root
        .join(super::DEFAULT_MACHINE_NAME)
        .join("control");
    let control_data_dir = command
        .control_data_dir
        .as_ref()
        .cloned()
        .unwrap_or(default_control_data_dir);
    fs::create_dir_all(&control_data_dir).map_err(|error| {
        Error::Internal(format!(
            "failed to create machine API control directory {}: {error}",
            control_data_dir.display()
        ))
    })?;

    let (listener, listen_mode) = resolve_machine_api_listener(&command)?;
    let binary_lookup_path = std::env::var_os("PATH");
    let helper_binary_dirs = default_guest_helper_binary_dirs();
    let state = MachineApiState {
        service_backend: Some(Arc::new(ContainerSandboxBackend::new({
            let mut config = ContainerSandboxBackendConfig::under_root(
                control_data_dir.join("service-sandboxes").join("container"),
            );
            apply_resolved_runtime_paths(
                &mut config,
                binary_lookup_path.as_deref(),
                &helper_binary_dirs,
            );
            config.machine_port_forwarder = Some(OciMachinePortForwarderConfig::gvproxy_default());
            config
        }))),
        control_data_dir,
        listen_mode,
        binary_lookup_path,
        helper_binary_dirs,
        machine_port_forwarder: Some(OciMachinePortForwarderConfig::gvproxy_default()),
    };
    serve_machine_api(listener, state, std::future::pending()).await
}

pub(crate) async fn serve_machine_api<F>(
    listener: tokio::net::UnixListener,
    state: MachineApiState,
    shutdown: F,
) -> Result<(), Error>
where
    F: Future<Output = ()> + Send + 'static,
{
    axum::serve(listener, machine_api_router(state))
        .with_graceful_shutdown(shutdown)
        .await
        .map_err(|error| Error::Internal(format!("machine API server failed: {error}")))
}

fn machine_api_router(state: MachineApiState) -> Router {
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

fn machine_api_capability_response(state: &MachineApiState) -> MachineApiCapabilityResponse {
    let required_binaries = resolve_required_binaries(
        state.binary_lookup_path.as_deref(),
        &state.helper_binary_dirs,
    );
    let image_start_binaries_ready =
        required_binaries_ready(&required_binaries, STANDARD_CONTAINER_IMAGE_START_BINARIES);
    let build_start_binaries_ready =
        required_binaries_ready(&required_binaries, STANDARD_CONTAINER_BUILD_START_BINARIES);
    let mut service_execution_blockers = Vec::new();
    let state_operations_available = state.service_backend.is_some();
    if state.service_backend.is_none() {
        service_execution_blockers.push(MACHINE_API_OPERATION_BLOCKER.to_owned());
    }
    if state.service_backend.is_some()
        && let Some(forwarder) = state.machine_port_forwarder.as_ref()
        && let Err(error) = probe_machine_port_forwarder(forwarder)
    {
        service_execution_blockers.push(error);
    }
    service_execution_blockers.extend(
        required_binaries
            .iter()
            .filter(|binary| {
                STANDARD_CONTAINER_IMAGE_START_BINARIES
                    .iter()
                    .any(|name| *name == binary.name)
            })
            .filter(|binary| !binary.present)
            .map(|binary| format!("missing required guest runtime binary: {}", binary.name)),
    );
    let service_execution_ready = service_execution_blockers.is_empty();
    let mut supported_operations = vec!["healthz".to_owned(), "capabilities".to_owned()];
    if state_operations_available {
        supported_operations.extend([
            MACHINE_API_LIST_OPERATION.to_owned(),
            MACHINE_API_INSPECT_OPERATION.to_owned(),
            MACHINE_API_INSPECT_CURRENT_OPERATION.to_owned(),
            MACHINE_API_LOGS_OPERATION.to_owned(),
            MACHINE_API_PS_OPERATION.to_owned(),
        ]);
    }
    if state.service_backend.is_some() && image_start_binaries_ready && service_execution_ready {
        supported_operations.extend([
            MACHINE_API_IMAGE_START_OPERATION.to_owned(),
            MACHINE_API_STOP_OPERATION.to_owned(),
        ]);
        if build_start_binaries_ready {
            supported_operations.push(MACHINE_API_BUILD_START_OPERATION.to_owned());
        }
    }
    let supported_service_backends = state
        .service_backend
        .as_ref()
        .map(|backend| vec![backend.kind()])
        .unwrap_or_else(|| vec![SandboxBackendKind::Container]);

    MachineApiCapabilityResponse {
        protocol_version: PROTOCOL_VERSION.to_owned(),
        service_execution_ready,
        service_execution_mode: MachineApiServiceExecutionMode::StandardContainers,
        supported_service_backends,
        supported_operations,
        required_binaries,
        service_execution_blockers,
    }
}

fn required_binaries_ready(
    required_binaries: &[MachineApiRequiredBinaryStatus],
    required_names: &[&str],
) -> bool {
    required_names.iter().all(|required_name| {
        required_binaries
            .iter()
            .find(|binary| binary.name == *required_name)
            .map(|binary| binary.present)
            .unwrap_or(false)
    })
}

pub(crate) fn default_guest_helper_binary_dirs() -> Vec<PathBuf> {
    DEFAULT_GUEST_HELPER_BINARY_DIRS
        .iter()
        .map(PathBuf::from)
        .collect()
}

fn apply_resolved_runtime_paths(
    config: &mut ContainerSandboxBackendConfig,
    path_env: Option<&OsStr>,
    helper_binary_dirs: &[PathBuf],
) {
    if let Some(path) = resolve_binary("conmon", path_env, helper_binary_dirs) {
        config.conmon_path = path;
    }
    if let Some(path) = resolve_binary("crun", path_env, helper_binary_dirs) {
        config.runtime_path = path;
    }
    if let Some(path) = resolve_binary("buildah", path_env, helper_binary_dirs) {
        config.buildah_path = path;
    }
    if let Some(path) = resolve_binary("netavark", path_env, helper_binary_dirs) {
        config.netavark_path = path;
    }
    if let Some(path) = resolve_binary("aardvark-dns", path_env, helper_binary_dirs) {
        config.aardvark_dns_path = path;
    }
}

fn container_state_view(state: &MachineApiState) -> ContainerSandboxStateView {
    ContainerSandboxStateView::new(machine_container_state_root(&state.control_data_dir))
}

fn machine_container_state_root(control_data_dir: &Path) -> PathBuf {
    control_data_dir
        .join("service-sandboxes")
        .join("container")
        .join("state")
}

fn machine_api_summary_from_container_summary(
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

fn machine_api_details_from_container_details(
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

fn container_state_error_to_http_error(error: neovex_sandbox::SandboxError) -> MachineApiHttpError {
    MachineApiHttpError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        message: format!("failed to read persisted service sandbox state: {error}"),
    }
}

fn internal_error_to_http_error(message: String) -> MachineApiHttpError {
    MachineApiHttpError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        message,
    }
}

fn read_log_chunk(path: &Path, offset: u64) -> Result<(String, u64), String> {
    let Ok(mut file) = File::open(path) else {
        return Ok((String::new(), offset));
    };

    let metadata = file.metadata().map_err(|error| {
        format!(
            "failed to inspect persisted log file {}: {error}",
            path.display()
        )
    })?;
    let file_len = metadata.len();
    let start = offset.min(file_len);
    file.seek(SeekFrom::Start(start)).map_err(|error| {
        format!(
            "failed to seek persisted log file {}: {error}",
            path.display()
        )
    })?;

    let mut buffer = String::new();
    file.read_to_string(&mut buffer).map_err(|error| {
        format!(
            "failed to read persisted log file {}: {error}",
            path.display()
        )
    })?;

    Ok((buffer, file_len))
}

fn read_pid_file_if_exists(path: &Path) -> Result<Option<u32>, String> {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Ok(None);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    trimmed.parse::<u32>().map(Some).map_err(|error| {
        format!(
            "failed to parse pidfile {} containing {:?}: {error}",
            path.display(),
            trimmed
        )
    })
}

#[derive(Debug)]
struct MachineApiProcessRow {
    pid: u32,
    ppid: u32,
    command: String,
}

fn snapshot_process_rows(
    runtime_pid: Option<u32>,
    conmon_pid: Option<u32>,
) -> Result<Vec<MachineApiProcessRow>, String> {
    let pid_set = [runtime_pid, conmon_pid]
        .into_iter()
        .flatten()
        .collect::<std::collections::BTreeSet<_>>();
    if pid_set.is_empty() {
        return Ok(Vec::new());
    }

    let output = std::process::Command::new("ps")
        .args(["-ax", "-o", "pid=,ppid=,command="])
        .output()
        .map_err(|error| format!("failed to run ps for service snapshot: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "ps exited with status {} while collecting service snapshot",
            output.status
        ));
    }
    let stdout = String::from_utf8(output.stdout)
        .map_err(|error| format!("ps output was not valid utf-8: {error}"))?;
    Ok(parse_process_rows(&stdout, &pid_set))
}

fn parse_process_rows(
    stdout: &str,
    pid_set: &std::collections::BTreeSet<u32>,
) -> Vec<MachineApiProcessRow> {
    stdout
        .lines()
        .filter_map(parse_process_row)
        .filter(|row| pid_set.contains(&row.pid))
        .collect()
}

fn parse_process_row(line: &str) -> Option<MachineApiProcessRow> {
    let trimmed = line.trim_start();
    if trimmed.is_empty() {
        return None;
    }

    let mut fields = trimmed.split_whitespace();
    let pid = fields.next()?.parse::<u32>().ok()?;
    let ppid = fields.next()?.parse::<u32>().ok()?;
    let command = fields.collect::<Vec<_>>().join(" ");
    if command.is_empty() {
        return None;
    }

    Some(MachineApiProcessRow { pid, ppid, command })
}

fn probe_machine_port_forwarder(config: &OciMachinePortForwarderConfig) -> Result<(), String> {
    let mut addresses = (config.host.as_str(), config.port)
        .to_socket_addrs()
        .map_err(|error| {
            format!(
                "guest machine port forwarder DNS lookup failed for {}:{}: {error}",
                config.host, config.port
            )
        })?;
    let address = addresses.next().ok_or_else(|| {
        format!(
            "guest machine port forwarder {}:{} did not resolve to an address",
            config.host, config.port
        )
    })?;
    TcpStream::connect_timeout(&address, MACHINE_PORT_FORWARDER_TIMEOUT).map_err(|error| {
        format!(
            "guest machine port forwarder is not reachable at {}:{}: {error}",
            config.host, config.port
        )
    })?;
    Ok(())
}

fn require_service_backend(
    state: &MachineApiState,
) -> Result<&Arc<dyn SandboxBackend>, MachineApiHttpError> {
    state
        .service_backend
        .as_ref()
        .ok_or_else(|| MachineApiHttpError {
            status: StatusCode::SERVICE_UNAVAILABLE,
            message: MACHINE_API_OPERATION_BLOCKER.to_owned(),
        })
}

fn sandbox_error_to_http_error(error: SandboxError) -> MachineApiHttpError {
    match error {
        SandboxError::InvalidSpec { message } => MachineApiHttpError {
            status: StatusCode::BAD_REQUEST,
            message,
        },
        SandboxError::BackendUnavailable { message } => MachineApiHttpError {
            status: StatusCode::SERVICE_UNAVAILABLE,
            message,
        },
        SandboxError::NotFound { sandbox_id } => MachineApiHttpError {
            status: StatusCode::NOT_FOUND,
            message: format!("sandbox instance was not found: {sandbox_id}"),
        },
        SandboxError::OperationFailed { message } => MachineApiHttpError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message,
        },
    }
}

struct MachineApiHttpError {
    status: StatusCode,
    message: String,
}

impl IntoResponse for MachineApiHttpError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(MachineApiErrorResponse {
                error: self.message,
            }),
        )
            .into_response()
    }
}

fn resolve_required_binaries(
    path_env: Option<&OsStr>,
    helper_binary_dirs: &[PathBuf],
) -> Vec<MachineApiRequiredBinaryStatus> {
    STANDARD_CONTAINER_RUNTIME_BINARIES
        .iter()
        .map(|name| {
            let resolved_path = resolve_binary(name, path_env, helper_binary_dirs);
            MachineApiRequiredBinaryStatus {
                name: (*name).to_owned(),
                present: resolved_path.is_some(),
                resolved_path: resolved_path.map(|path| path.display().to_string()),
            }
        })
        .collect()
}

fn resolve_binary(
    name: &str,
    path_env: Option<&OsStr>,
    helper_binary_dirs: &[PathBuf],
) -> Option<PathBuf> {
    let binary_name = Path::new(name);
    if binary_name.components().count() > 1 {
        return is_executable_file(binary_name).then(|| binary_name.to_path_buf());
    }

    for directory in helper_binary_dirs {
        let candidate = directory.join(name);
        if is_executable_file(&candidate) {
            return Some(candidate);
        }
    }

    let path_env = path_env?;
    std::env::split_paths(path_env)
        .map(|directory| directory.join(name))
        .find(|candidate| is_executable_file(candidate))
}

fn is_executable_file(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        std::fs::metadata(path)
            .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }

    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

fn resolve_machine_api_listener(
    command: &MachineApiCommand,
) -> Result<(tokio::net::UnixListener, MachineApiListenMode), Error> {
    if command.socket_activation {
        return inherited_systemd_listener()
            .map(|listener| (listener, MachineApiListenMode::SystemdSocketActivation));
    }

    let socket_path = command.socket_path.as_ref().ok_or_else(|| {
        Error::InvalidInput(
            "machine api requires either --socket-path <path> or --socket-activation".to_owned(),
        )
    })?;
    bind_direct_listener(socket_path).map(|listener| (listener, MachineApiListenMode::DirectSocket))
}

pub(crate) fn bind_direct_listener(path: &Path) -> Result<tokio::net::UnixListener, Error> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            Error::Internal(format!(
                "failed to create machine API socket directory {}: {error}",
                parent.display()
            ))
        })?;
    }
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(Error::Internal(format!(
                "failed to clear stale machine API socket {}: {error}",
                path.display()
            )));
        }
    }

    let listener = StdUnixListener::bind(path).map_err(|error| {
        Error::Internal(format!(
            "failed to bind machine API socket {}: {error}",
            path.display()
        ))
    })?;
    listener.set_nonblocking(true).map_err(|error| {
        Error::Internal(format!(
            "failed to configure machine API socket {}: {error}",
            path.display()
        ))
    })?;
    tokio::net::UnixListener::from_std(listener).map_err(|error| {
        Error::Internal(format!(
            "failed to convert machine API socket {} to tokio listener: {error}",
            path.display()
        ))
    })
}

fn inherited_systemd_listener() -> Result<tokio::net::UnixListener, Error> {
    let current_pid = std::process::id();
    let listen_pid = std::env::var("LISTEN_PID")
        .map_err(|_| {
            Error::InvalidInput(
                "machine API socket activation requires LISTEN_PID from systemd".to_owned(),
            )
        })?
        .parse::<u32>()
        .map_err(|error| {
            Error::InvalidInput(format!(
                "machine API socket activation could not parse LISTEN_PID: {error}"
            ))
        })?;
    let listen_fds = std::env::var("LISTEN_FDS")
        .map_err(|_| {
            Error::InvalidInput(
                "machine API socket activation requires LISTEN_FDS from systemd".to_owned(),
            )
        })?
        .parse::<u32>()
        .map_err(|error| {
            Error::InvalidInput(format!(
                "machine API socket activation could not parse LISTEN_FDS: {error}"
            ))
        })?;

    if listen_pid != current_pid {
        return Err(Error::InvalidInput(format!(
            "machine API socket activation expected LISTEN_PID={} but found {}",
            current_pid, listen_pid
        )));
    }
    if listen_fds != 1 {
        return Err(Error::InvalidInput(format!(
            "machine API socket activation supports exactly one inherited socket, found {}",
            listen_fds
        )));
    }

    remove_env_var("LISTEN_PID");
    remove_env_var("LISTEN_FDS");
    tokio_listener_from_inherited_fd(DEFAULT_SYSTEMD_SOCKET_FD)
}

fn tokio_listener_from_inherited_fd(fd: i32) -> Result<tokio::net::UnixListener, Error> {
    let listener = unsafe { StdUnixListener::from_raw_fd(fd) };
    listener.set_nonblocking(true).map_err(|error| {
        Error::Internal(format!(
            "failed to configure inherited machine API socket fd {}: {error}",
            fd
        ))
    })?;
    tokio::net::UnixListener::from_std(listener).map_err(|error| {
        Error::Internal(format!(
            "failed to convert inherited machine API socket fd {} to tokio listener: {error}",
            fd
        ))
    })
}

#[cfg(test)]
fn set_env_var(key: &str, value: &str) {
    // SAFETY: the machine API test lane mutates process-local LISTEN_* values
    // in a serialized scope and restores them before returning.
    unsafe { std::env::set_var(key, value) }
}

fn remove_env_var(key: &str) {
    // SAFETY: the machine API activation path clears only the inherited
    // LISTEN_* variables for the current process after validating them.
    unsafe { std::env::remove_var(key) }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::os::fd::AsRawFd;
    use std::os::unix::net::UnixStream;
    use std::sync::Arc;
    use std::time::Duration;

    use tempfile::{Builder, TempDir};

    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn machine_api_serves_health_and_capabilities_over_unix_socket() {
        let temp_dir = short_socket_tempdir();
        let socket_path = temp_dir.path().join("neovex.sock");
        let listener = bind_direct_listener(&socket_path).expect("listener should bind");
        let state = MachineApiState {
            control_data_dir: temp_dir.path().join("control"),
            listen_mode: MachineApiListenMode::DirectSocket,
            binary_lookup_path: Some(fake_runtime_path(&temp_dir)),
            helper_binary_dirs: Vec::new(),
            service_backend: None,
            machine_port_forwarder: None,
        };
        for binary in STANDARD_CONTAINER_RUNTIME_BINARIES {
            write_fake_binary(&temp_dir, binary);
        }
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

        let server = tokio::spawn(serve_machine_api(listener, state, async move {
            let _ = shutdown_rx.await;
        }));
        wait_for_socket_path(&socket_path);

        let health = wait_for_http_response_contains(&socket_path, "/healthz", "\"status\":\"ok\"");
        assert!(health.contains("200 OK"), "{health}");
        assert!(health.contains("\"status\":\"ok\""), "{health}");
        assert!(
            health.contains("\"role\":\"guest-machine-api\""),
            "{health}"
        );

        let capabilities = unix_http_get(&socket_path, "/v1/machine-api/capabilities");
        assert!(capabilities.contains("200 OK"), "{capabilities}");
        assert!(
            capabilities.contains("\"service_execution_ready\":false"),
            "{capabilities}"
        );
        assert!(
            capabilities.contains("\"service_execution_mode\":\"standard_containers\""),
            "{capabilities}"
        );
        assert!(
            capabilities.contains("\"supported_service_backends\":[\"container\"]"),
            "{capabilities}"
        );
        assert!(
            capabilities.contains("\"service_execution_blockers\":["),
            "{capabilities}"
        );
        assert!(
            capabilities
                .contains("\"guest machine API does not yet expose service lifecycle operations\""),
            "{capabilities}"
        );

        let _ = shutdown_tx.send(());
        server
            .await
            .expect("machine API server task should join")
            .expect("machine API server should shut down cleanly");
    }

    #[test]
    fn capability_response_reports_required_binaries_and_explicit_blockers() {
        let temp_dir = short_socket_tempdir();
        write_fake_binary(&temp_dir, "buildah");
        write_fake_binary(&temp_dir, "conmon");
        write_fake_binary(&temp_dir, "crun");

        let state = MachineApiState {
            control_data_dir: temp_dir.path().join("control"),
            listen_mode: MachineApiListenMode::DirectSocket,
            binary_lookup_path: Some(fake_runtime_path(&temp_dir)),
            helper_binary_dirs: Vec::new(),
            service_backend: None,
            machine_port_forwarder: None,
        };
        let capabilities = machine_api_capability_response(&state);

        assert_eq!(
            capabilities.service_execution_mode,
            MachineApiServiceExecutionMode::StandardContainers
        );
        assert_eq!(
            capabilities.supported_service_backends,
            vec![SandboxBackendKind::Container]
        );
        assert_eq!(
            capabilities.supported_operations,
            vec!["healthz".to_owned(), "capabilities".to_owned()]
        );
        assert!(!capabilities.service_execution_ready);
        assert!(
            capabilities
                .service_execution_blockers
                .iter()
                .any(|blocker| blocker == MACHINE_API_OPERATION_BLOCKER)
        );
        assert!(
            capabilities
                .required_binaries
                .iter()
                .any(|binary| binary.name == "buildah" && binary.present)
        );
        assert!(
            capabilities
                .required_binaries
                .iter()
                .any(|binary| binary.name == "netavark" && !binary.present)
        );
        assert!(
            capabilities
                .service_execution_blockers
                .iter()
                .any(|blocker| blocker == "missing required guest runtime binary: netavark")
        );
    }

    #[test]
    fn capability_response_reports_machine_port_forwarder_blocker_when_unreachable() {
        let temp_dir = short_socket_tempdir();
        for binary in STANDARD_CONTAINER_RUNTIME_BINARIES {
            write_fake_binary(&temp_dir, binary);
        }

        let state = MachineApiState {
            control_data_dir: temp_dir.path().join("control"),
            listen_mode: MachineApiListenMode::DirectSocket,
            binary_lookup_path: Some(fake_runtime_path(&temp_dir)),
            helper_binary_dirs: Vec::new(),
            service_backend: Some(Arc::new(ContainerSandboxBackend::new(
                ContainerSandboxBackendConfig::plan_only(
                    temp_dir.path().join("bundles"),
                    temp_dir.path().join("state"),
                ),
            ))),
            machine_port_forwarder: Some(OciMachinePortForwarderConfig {
                host: "127.0.0.1".to_owned(),
                port: 9,
                path_prefix: "/services/forwarder".to_owned(),
            }),
        };

        let capabilities = machine_api_capability_response(&state);
        assert!(!capabilities.service_execution_ready);
        assert_eq!(
            capabilities.supported_operations,
            vec![
                "healthz".to_owned(),
                "capabilities".to_owned(),
                "service-sandboxes.list".to_owned(),
                "service-sandboxes.inspect".to_owned(),
                "service-sandboxes.inspect-current".to_owned(),
                "service-sandboxes.logs".to_owned(),
                "service-sandboxes.ps".to_owned(),
            ]
        );
        assert!(
            capabilities
                .service_execution_blockers
                .iter()
                .any(|blocker| blocker
                    .contains("guest machine port forwarder is not reachable at 127.0.0.1:9")),
            "{:?}",
            capabilities.service_execution_blockers
        );
    }

    #[test]
    fn capability_response_resolves_helper_binaries_from_podman_dirs() {
        let temp_dir = short_socket_tempdir();
        let helper_dir = temp_dir.path().join("podman-helpers");
        fs::create_dir_all(&helper_dir).expect("helper dir should create");
        write_fake_binary(&temp_dir, "buildah");
        write_fake_binary(&temp_dir, "conmon");
        write_fake_binary(&temp_dir, "crun");
        write_fake_binary_at(&helper_dir, "netavark");
        write_fake_binary_at(&helper_dir, "aardvark-dns");
        write_fake_binary_at(&helper_dir, "fuse-overlayfs");

        let state = MachineApiState {
            control_data_dir: temp_dir.path().join("control"),
            listen_mode: MachineApiListenMode::DirectSocket,
            binary_lookup_path: Some(fake_runtime_path(&temp_dir)),
            helper_binary_dirs: vec![helper_dir.clone()],
            service_backend: None,
            machine_port_forwarder: None,
        };

        let capabilities = machine_api_capability_response(&state);
        let netavark_path = helper_dir.join("netavark").display().to_string();
        let aardvark_path = helper_dir.join("aardvark-dns").display().to_string();
        assert!(capabilities.required_binaries.iter().any(|binary| {
            binary.name == "netavark"
                && binary.present
                && binary.resolved_path.as_deref() == Some(netavark_path.as_str())
        }));
        assert!(capabilities.required_binaries.iter().any(|binary| {
            binary.name == "aardvark-dns"
                && binary.present
                && binary.resolved_path.as_deref() == Some(aardvark_path.as_str())
        }));
        assert!(
            !capabilities
                .service_execution_blockers
                .iter()
                .any(|blocker| blocker.contains("netavark") || blocker.contains("aardvark-dns"))
        );
    }

    #[test]
    fn capability_response_keeps_image_start_available_without_buildah() {
        let temp_dir = short_socket_tempdir();
        let helper_dir = temp_dir.path().join("podman-helpers");
        fs::create_dir_all(&helper_dir).expect("helper dir should create");
        write_fake_binary(&temp_dir, "conmon");
        write_fake_binary(&temp_dir, "crun");
        write_fake_binary_at(&helper_dir, "netavark");
        write_fake_binary_at(&helper_dir, "aardvark-dns");
        write_fake_binary_at(&helper_dir, "fuse-overlayfs");

        let state = MachineApiState {
            control_data_dir: temp_dir.path().join("control"),
            listen_mode: MachineApiListenMode::DirectSocket,
            binary_lookup_path: Some(fake_runtime_path(&temp_dir)),
            helper_binary_dirs: vec![helper_dir],
            service_backend: Some(Arc::new(ContainerSandboxBackend::new(
                ContainerSandboxBackendConfig::plan_only(
                    temp_dir.path().join("bundles"),
                    temp_dir.path().join("state"),
                ),
            ))),
            machine_port_forwarder: None,
        };

        let capabilities = machine_api_capability_response(&state);

        assert!(capabilities.service_execution_ready);
        assert!(
            capabilities
                .supported_operations
                .iter()
                .any(|operation| operation == MACHINE_API_IMAGE_START_OPERATION)
        );
        assert!(
            capabilities
                .supported_operations
                .iter()
                .all(|operation| operation != MACHINE_API_BUILD_START_OPERATION)
        );
        assert!(
            capabilities
                .required_binaries
                .iter()
                .any(|binary| binary.name == "buildah" && !binary.present)
        );
        assert!(
            capabilities
                .service_execution_blockers
                .iter()
                .all(|blocker| !blocker.contains("buildah"))
        );
    }

    #[test]
    fn apply_resolved_runtime_paths_updates_backend_config_from_helper_dirs() {
        let temp_dir = short_socket_tempdir();
        let helper_dir = temp_dir.path().join("podman-helpers");
        fs::create_dir_all(&helper_dir).expect("helper dir should create");
        write_fake_binary(&temp_dir, "buildah");
        write_fake_binary(&temp_dir, "conmon");
        write_fake_binary(&temp_dir, "crun");
        write_fake_binary_at(&helper_dir, "netavark");
        write_fake_binary_at(&helper_dir, "aardvark-dns");

        let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path().join("root"));
        let runtime_path = fake_runtime_path(&temp_dir);
        apply_resolved_runtime_paths(
            &mut config,
            Some(runtime_path.as_os_str()),
            std::slice::from_ref(&helper_dir),
        );

        assert_eq!(config.buildah_path, temp_dir.path().join("buildah"));
        assert_eq!(config.conmon_path, temp_dir.path().join("conmon"));
        assert_eq!(config.runtime_path, temp_dir.path().join("crun"));
        assert_eq!(config.netavark_path, helper_dir.join("netavark"));
        assert_eq!(config.aardvark_dns_path, helper_dir.join("aardvark-dns"));
    }

    #[test]
    fn socket_activation_listener_requires_matching_systemd_env() {
        let _guard = MachineApiEnvGuard::capture();
        set_env_var("LISTEN_PID", "999999");
        set_env_var("LISTEN_FDS", "1");

        let error = inherited_systemd_listener().expect_err("pid mismatch should fail");
        assert!(
            error
                .to_string()
                .contains("machine API socket activation expected LISTEN_PID=")
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn socket_activation_listener_accepts_one_inherited_fd() {
        let temp_dir = short_socket_tempdir();
        let socket_path = temp_dir.path().join("neovex.sock");
        let listener = StdUnixListener::bind(&socket_path).expect("listener should bind");
        let duplicated_fd = unsafe { libc::dup(listener.as_raw_fd()) };
        assert!(duplicated_fd >= 0, "listener fd should duplicate");

        let tokio_listener =
            tokio_listener_from_inherited_fd(duplicated_fd).expect("fd should convert");
        drop(tokio_listener);
    }

    fn unix_http_get(socket_path: &Path, path: &str) -> String {
        let mut stream = UnixStream::connect(socket_path).expect("unix socket should accept");
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("read timeout should set");
        write!(stream, "GET {path} HTTP/1.0\r\nHost: localhost\r\n\r\n")
            .expect("request should write");
        let mut response = Vec::new();
        let mut chunk = [0_u8; 4096];
        loop {
            match stream.read(&mut chunk) {
                Ok(0) => break,
                Ok(read) => response.extend_from_slice(&chunk[..read]),
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    break;
                }
                Err(error) => panic!("response should read: {error}"),
            }
        }
        String::from_utf8(response).expect("response should be valid utf-8")
    }

    fn wait_for_http_response_contains(socket_path: &Path, path: &str, needle: &str) -> String {
        let start = std::time::Instant::now();
        loop {
            let response = try_unix_http_get(socket_path, path).unwrap_or_default();
            if response.contains(needle) {
                return response;
            }
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "timed out waiting for machine API response on {}{}; last response: {}",
                socket_path.display(),
                path,
                response
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn try_unix_http_get(socket_path: &Path, path: &str) -> Result<String, std::io::Error> {
        let mut stream = UnixStream::connect(socket_path)?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        write!(stream, "GET {path} HTTP/1.0\r\nHost: localhost\r\n\r\n")?;
        let mut response = Vec::new();
        let mut chunk = [0_u8; 4096];
        loop {
            match stream.read(&mut chunk) {
                Ok(0) => break,
                Ok(read) => response.extend_from_slice(&chunk[..read]),
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    break;
                }
                Err(error) => return Err(error),
            }
        }
        String::from_utf8(response)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
    }

    fn wait_for_socket_path(path: &Path) {
        let start = std::time::Instant::now();
        while !path.exists() {
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "timed out waiting for socket {}",
                path.display()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    struct MachineApiEnvGuard {
        listen_pid: Option<String>,
        listen_fds: Option<String>,
    }

    impl MachineApiEnvGuard {
        fn capture() -> Self {
            Self {
                listen_pid: std::env::var("LISTEN_PID").ok(),
                listen_fds: std::env::var("LISTEN_FDS").ok(),
            }
        }
    }

    impl Drop for MachineApiEnvGuard {
        fn drop(&mut self) {
            match &self.listen_pid {
                Some(value) => set_env_var("LISTEN_PID", value),
                None => remove_env_var("LISTEN_PID"),
            }
            match &self.listen_fds {
                Some(value) => set_env_var("LISTEN_FDS", value),
                None => remove_env_var("LISTEN_FDS"),
            }
        }
    }

    fn short_socket_tempdir() -> TempDir {
        Builder::new()
            .prefix("neovex-ma-")
            .tempdir_in("/tmp")
            .expect("short temp dir should exist")
    }

    fn fake_runtime_path(temp_dir: &TempDir) -> OsString {
        temp_dir.path().as_os_str().to_owned()
    }

    fn write_fake_binary(temp_dir: &TempDir, name: &str) {
        write_fake_binary_at(temp_dir.path(), name);
    }

    fn write_fake_binary_at(root: &Path, name: &str) {
        let path = root.join(name);
        crate::test_support::write_executable_stub(&path, "#!/bin/sh\nexit 0\n");
    }
}
