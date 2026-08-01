use std::path::{Path, PathBuf};

use nimbus::{Error, SandboxId, SandboxSpec, TenantId};
use nimbus_sandbox::SandboxInspection;

use nimbus_machine::MachineForwarderAuthority;
use nimbus_machine::api::{
    MachineApiCapabilityResponse, MachineApiHealthResponse, MachineApiServiceProcessSnapshot,
    MachineApiServiceSandboxLogChunkResponse, MachineApiServiceSandboxLookupResponse,
    MachineApiServiceSandboxStartResponse, MachineApiServiceSandboxStopResponse,
    MachineApiServiceSandboxSummary,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MachineApiClient {
    socket_path: PathBuf,
    forwarder_authority: Option<MachineForwarderAuthority>,
}

impl MachineApiClient {
    pub(crate) fn new(socket_path: impl Into<PathBuf>) -> Self {
        Self {
            socket_path: socket_path.into(),
            forwarder_authority: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(socket_path: impl Into<PathBuf>) -> Self {
        Self::new(socket_path)
    }

    pub(crate) fn with_forwarder_authority(mut self, authority: MachineForwarderAuthority) -> Self {
        self.forwarder_authority = Some(authority);
        self
    }

    pub(crate) fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    pub(crate) fn health(&self) -> Result<MachineApiHealthResponse, Error> {
        Err(unsupported_machine_api_client_error(&self.socket_path))
    }

    pub(crate) fn capabilities(&self) -> Result<MachineApiCapabilityResponse, Error> {
        Err(unsupported_machine_api_client_error(&self.socket_path))
    }

    // realized by WIN2: the unix twin (`backend.rs`) forwards through these
    // methods; the non-unix stub backend (`stub/backend.rs`) returns a canned
    // error without dispatching through `client`, so these stay unused here.
    #[allow(dead_code)]
    pub(crate) fn start_service_sandbox_from_image(
        &self,
        _sandbox_id: SandboxId,
        _spec: SandboxSpec,
    ) -> Result<MachineApiServiceSandboxStartResponse, Error> {
        Err(unsupported_machine_api_client_error(&self.socket_path))
    }

    #[allow(dead_code)]
    pub(crate) fn start_service_sandbox_from_build(
        &self,
        _sandbox_id: SandboxId,
        _spec: SandboxSpec,
    ) -> Result<MachineApiServiceSandboxStartResponse, Error> {
        Err(unsupported_machine_api_client_error(&self.socket_path))
    }

    #[allow(dead_code)]
    pub(crate) fn inspect_service_sandbox(
        &self,
        _sandbox_id: &SandboxId,
    ) -> Result<Option<SandboxInspection>, Error> {
        Err(unsupported_machine_api_client_error(&self.socket_path))
    }

    #[allow(dead_code)]
    pub(crate) fn stop_service_sandbox(
        &self,
        _tenant_id: &TenantId,
        _sandbox_id: &SandboxId,
        _expected_bindings: &[nimbus::SandboxPortBinding],
    ) -> Result<MachineApiServiceSandboxStopResponse, Error> {
        Err(unsupported_machine_api_client_error(&self.socket_path))
    }

    pub(crate) fn list_service_sandboxes(
        &self,
        _tenant_id: Option<&TenantId>,
    ) -> Result<Vec<MachineApiServiceSandboxSummary>, Error> {
        Err(unsupported_machine_api_client_error(&self.socket_path))
    }

    pub(crate) fn inspect_current_service_sandbox(
        &self,
        _tenant_id: &TenantId,
        _service_name: &str,
    ) -> Result<MachineApiServiceSandboxLookupResponse, Error> {
        Err(unsupported_machine_api_client_error(&self.socket_path))
    }

    pub(crate) fn read_service_sandbox_log_chunk(
        &self,
        _sandbox_id: &SandboxId,
        _offset: u64,
    ) -> Result<MachineApiServiceSandboxLogChunkResponse, Error> {
        Err(unsupported_machine_api_client_error(&self.socket_path))
    }

    pub(crate) fn service_sandbox_process_snapshot(
        &self,
        _sandbox_id: &SandboxId,
    ) -> Result<MachineApiServiceProcessSnapshot, Error> {
        Err(unsupported_machine_api_client_error(&self.socket_path))
    }
}

fn unsupported_machine_api_client_error(socket_path: &Path) -> Error {
    Error::InvalidInput(format!(
        "machine API socket {} is unavailable because nimbus machine support currently requires a unix host",
        socket_path.display()
    ))
}
