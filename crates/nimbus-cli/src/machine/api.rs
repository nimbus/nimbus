use std::ffi::{OsStr, OsString};
use std::fs;
use std::fs::File;
use std::future::Future;
use std::io::{Read, Seek, SeekFrom};
use std::net::{TcpStream, ToSocketAddrs};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener as StdUnixListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::extract::{DefaultBodyLimit, Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use nimbus::{Error, SandboxBackendKind, SandboxError, SandboxStatus, TenantId};
use nimbus_node::SystemdTransientUnitBackend;
use nimbus_sandbox::backends::container::{
    ContainerSandboxBackendConfig, ContainerSandboxStateView, OciMachinePortForwarderConfig,
};
use nimbus_workloads::NodeIdentity;
use serde::Deserialize;

use super::{MachineApiCommand, MachineRootLayout};
use nimbus_machine::MachineForwarderAuthority;
use nimbus_machine::api::{
    MACHINE_API_BOOTC_ROLLBACK_OPERATION, MACHINE_API_BOOTC_STATUS_OPERATION,
    MACHINE_API_BOOTC_SWITCH_OPERATION, MACHINE_API_BOOTC_UPGRADE_OPERATION,
    MACHINE_API_INSPECT_CURRENT_OPERATION, MACHINE_API_INSPECT_OPERATION,
    MACHINE_API_LIST_OPERATION, MACHINE_API_LOGS_OPERATION, MACHINE_API_PS_OPERATION,
    MACHINE_API_ROLE, MACHINE_API_WORKLOAD_PROVISION_PHASE_OPERATION,
    MACHINE_API_WORKLOAD_RESTART_PHASE_OPERATION, MACHINE_API_WORKLOAD_TEARDOWN_PHASE_OPERATION,
    MachineApiBinaryStatus, MachineApiBootcOperationResponse, MachineApiBootcRollbackRequest,
    MachineApiBootcStatusResponse, MachineApiBootcSwitchRequest, MachineApiBootcUpgradeRequest,
    MachineApiCapabilityResponse, MachineApiErrorResponse, MachineApiHealthResponse,
    MachineApiOperationStatus, MachineApiServiceExecutionDriver, MachineApiServiceExecutionMode,
    MachineApiServiceProcessRow, MachineApiServiceProcessSnapshot,
    MachineApiServiceProcessSnapshotResponse, MachineApiServiceSandboxDetails,
    MachineApiServiceSandboxInspectResponse, MachineApiServiceSandboxListResponse,
    MachineApiServiceSandboxLogChunkResponse, MachineApiServiceSandboxLogPaths,
    MachineApiServiceSandboxLookupResponse, MachineApiServiceSandboxSummary, PROTOCOL_VERSION,
};

mod binaries;
mod bootc;
mod capabilities;
mod listener;
mod logs;
mod network_composition;
mod process;
mod routes;
mod service_workloads;
mod state;
#[cfg(test)]
mod tests;

pub(crate) use self::binaries::default_guest_helper_binary_dirs;
#[cfg(test)]
pub(crate) use self::listener::bind_direct_listener;

use self::binaries::apply_resolved_runtime_paths;
use self::listener::resolve_machine_api_listener;
use self::network_composition::{GuestMachineNetworkComposition, load_parent_forwarder_authority};
use self::routes::machine_api_router;
#[cfg(test)]
pub(crate) use self::service_workloads::teardown::tests::acceptance::{
    teardown_wire_fixture, teardown_wire_fixture_for_forwarder,
};
pub(crate) use self::service_workloads::{GuestNodeWorkloadService, MachineApiNodeWorkloadFacade};
#[cfg(test)]
pub(crate) use self::service_workloads::{
    guest_teardown_provider_claim_for_test, machine_api_node_workload_facade_from_sandbox_backend,
    sandbox_network_plan_for_teardown_test,
};
#[cfg(target_os = "linux")]
use self::state::machine_systemd_teardown_state_root;

const MACHINE_API_OPERATION_BLOCKER: &str =
    "guest machine API does not yet expose service lifecycle operations";
const MACHINE_PORT_FORWARDER_TIMEOUT: Duration = Duration::from_millis(200);
#[derive(Clone)]
pub(crate) struct MachineApiState {
    pub(crate) control_data_dir: PathBuf,
    pub(crate) listen_mode: MachineApiListenMode,
    pub(crate) binary_lookup_path: Option<OsString>,
    pub(crate) helper_binary_dirs: Vec<PathBuf>,
    pub(crate) service_workloads: Option<Arc<dyn MachineApiNodeWorkloadFacade>>,
    pub(crate) machine_port_forwarder: Option<OciMachinePortForwarderConfig>,
    pub(crate) forwarder_authority: Option<MachineForwarderAuthority>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MachineApiListenMode {
    DirectSocket,
}

impl MachineApiListenMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::DirectSocket => "direct-socket",
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
    let (forwarder_authority, machine_port_forwarder) =
        load_parent_forwarder_authority(&control_data_dir)?;
    let binary_lookup_path = std::env::var_os("PATH");
    let helper_binary_dirs = default_guest_helper_binary_dirs();
    let container_root = control_data_dir.join("service-sandboxes").join("container");
    let mut container_config = ContainerSandboxBackendConfig::plan_only(
        container_root.join("bundles"),
        container_root.join("state"),
    );
    apply_resolved_runtime_paths(
        &mut container_config,
        binary_lookup_path.as_deref(),
        &helper_binary_dirs,
    );
    container_config.machine_port_forwarder = Some(machine_port_forwarder.clone());
    let node_id = NodeIdentity::new(&command.guest_node_id).map_err(|error| {
        Error::Internal(format!(
            "failed to build machine API guest node identity: {error}"
        ))
    })?;
    let network_composition =
        GuestMachineNetworkComposition::claim(&control_data_dir, container_config)?;
    let (listener, listen_mode) = resolve_machine_api_listener(&command)?;
    let bundle_materializer = network_composition.backend();
    #[cfg(target_os = "linux")]
    let node_lifecycle_backend = {
        let teardown_state_root = machine_systemd_teardown_state_root(&control_data_dir);
        SystemdTransientUnitBackend::linux_systemd_default(&teardown_state_root)
            .await
            .map_err(|error| {
                Error::Internal(format!(
                    "machine API service workloads require guest systemd transient unit support: {error}"
                ))
            })?
    };
    #[cfg(not(target_os = "linux"))]
    let node_lifecycle_backend = SystemdTransientUnitBackend::unavailable(
        "machine API service workloads require a Linux guest systemd manager",
    );
    let service_workloads = Arc::new(GuestNodeWorkloadService::new(
        node_id,
        node_lifecycle_backend,
        bundle_materializer,
        container_root.join("state"),
    ));
    let state = MachineApiState {
        service_workloads: Some(service_workloads),
        control_data_dir,
        listen_mode,
        binary_lookup_path,
        helper_binary_dirs,
        machine_port_forwarder: Some(machine_port_forwarder),
        forwarder_authority: Some(forwarder_authority),
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

fn require_service_workloads(
    state: &MachineApiState,
) -> Result<&Arc<dyn MachineApiNodeWorkloadFacade>, MachineApiHttpError> {
    state
        .service_workloads
        .as_ref()
        .ok_or_else(|| MachineApiHttpError {
            status: StatusCode::SERVICE_UNAVAILABLE,
            message: MACHINE_API_OPERATION_BLOCKER.to_owned(),
        })
}

fn require_forwarder_authority(
    state: &MachineApiState,
    presented: &MachineForwarderAuthority,
) -> Result<(), MachineApiHttpError> {
    let expected = state
        .forwarder_authority
        .as_ref()
        .ok_or_else(|| MachineApiHttpError {
            status: StatusCode::SERVICE_UNAVAILABLE,
            message: "machine API has no boot-authenticated forwarder authority".to_owned(),
        })?;
    expected
        .authenticate(presented)
        .map_err(|error| MachineApiHttpError {
            status: StatusCode::CONFLICT,
            message: error.to_string(),
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
        SandboxError::NetworkSubnetExhausted { subnet } => MachineApiHttpError {
            // The node's per-tenant network-segment pool cannot host another
            // sandbox — a capacity limit, not a client error.
            status: StatusCode::SERVICE_UNAVAILABLE,
            message: format!("network subnet {subnet} is exhausted"),
        },
    }
}

#[derive(Debug)]
pub(crate) struct MachineApiHttpError {
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
