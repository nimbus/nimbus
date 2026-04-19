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
use neovex::{Error, SandboxBackend, SandboxBackendKind, SandboxError, SandboxStatus, TenantId};
use neovex_sandbox::backends::container::{
    ContainerSandboxBackend, ContainerSandboxBackendConfig, ContainerSandboxStateView,
    OciMachinePortForwarderConfig,
};
use serde::Deserialize;

use super::protocol::{
    MACHINE_API_ROLE, MachineApiBinaryStatus, MachineApiCapabilityResponse,
    MachineApiErrorResponse, MachineApiHealthResponse, MachineApiOperationStatus,
    MachineApiServiceExecutionMode, MachineApiServiceProcessRow, MachineApiServiceProcessSnapshot,
    MachineApiServiceProcessSnapshotResponse, MachineApiServiceSandboxBuildStartRequest,
    MachineApiServiceSandboxDetails, MachineApiServiceSandboxImageStartRequest,
    MachineApiServiceSandboxInspectResponse, MachineApiServiceSandboxListResponse,
    MachineApiServiceSandboxLogChunkResponse, MachineApiServiceSandboxLogPaths,
    MachineApiServiceSandboxLookupResponse, MachineApiServiceSandboxStartResponse,
    MachineApiServiceSandboxStopResponse, MachineApiServiceSandboxSummary, PROTOCOL_VERSION,
};
use super::{MachineApiCommand, MachineRootLayout};

mod binaries;
mod capabilities;
mod listener;
mod logs;
mod process;
mod routes;
mod state;
#[cfg(test)]
mod tests;

pub(crate) use self::binaries::default_guest_helper_binary_dirs;
#[cfg(test)]
pub(crate) use self::listener::bind_direct_listener;

use self::binaries::apply_resolved_runtime_paths;
use self::listener::resolve_machine_api_listener;
use self::routes::machine_api_router;

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
