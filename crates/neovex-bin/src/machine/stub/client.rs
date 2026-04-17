use std::path::{Path, PathBuf};

use neovex::{
    Error, SandboxBuildLaunchSpec, SandboxHandle, SandboxId, SandboxImageLaunchSpec, TenantId,
};

use super::protocol::{
    MachineApiCapabilityResponse, MachineApiHealthResponse, MachineApiServiceProcessSnapshot,
    MachineApiServiceSandboxLogChunkResponse, MachineApiServiceSandboxLookupResponse,
    MachineApiServiceSandboxSummary,
};

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) struct MachineApiClient {
    socket_path: PathBuf,
}

#[allow(dead_code)]
impl MachineApiClient {
    pub(crate) fn new(socket_path: impl Into<PathBuf>) -> Self {
        Self {
            socket_path: socket_path.into(),
        }
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(socket_path: impl Into<PathBuf>) -> Self {
        Self::new(socket_path)
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

    pub(crate) fn start_service_sandbox_from_image(
        &self,
        _launch: SandboxImageLaunchSpec,
    ) -> Result<SandboxHandle, Error> {
        Err(unsupported_machine_api_client_error(&self.socket_path))
    }

    pub(crate) fn start_service_sandbox_from_build(
        &self,
        _launch: SandboxBuildLaunchSpec,
    ) -> Result<SandboxHandle, Error> {
        Err(unsupported_machine_api_client_error(&self.socket_path))
    }

    pub(crate) fn inspect_service_sandbox(
        &self,
        _sandbox_id: &SandboxId,
    ) -> Result<Option<SandboxHandle>, Error> {
        Err(unsupported_machine_api_client_error(&self.socket_path))
    }

    pub(crate) fn stop_service_sandbox(&self, _sandbox_id: &SandboxId) -> Result<(), Error> {
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

    pub(crate) fn service_process_snapshot(
        &self,
        _sandbox_id: &SandboxId,
    ) -> Result<MachineApiServiceProcessSnapshot, Error> {
        Err(unsupported_machine_api_client_error(&self.socket_path))
    }
}

fn unsupported_machine_api_client_error(socket_path: &Path) -> Error {
    Error::InvalidInput(format!(
        "machine API socket {} is unavailable because neovex machine support currently requires a unix host",
        socket_path.display()
    ))
}
