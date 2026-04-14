use neovex::{
    SandboxBackendKind, SandboxBuildLaunchSpec, SandboxHandle, SandboxId, SandboxImageLaunchSpec,
};
use serde::{Deserialize, Serialize};

pub(crate) const PROTOCOL_VERSION: &str = "v1alpha1";
pub(crate) const MACHINE_API_ROLE: &str = "guest-machine-api";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MachineApiHealthResponse {
    pub(crate) status: String,
    pub(crate) role: String,
    pub(crate) protocol_version: String,
    pub(crate) listen_mode: String,
    pub(crate) control_data_dir: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MachineApiCapabilityResponse {
    pub(crate) protocol_version: String,
    pub(crate) service_execution_ready: bool,
    pub(crate) service_execution_mode: MachineApiServiceExecutionMode,
    pub(crate) supported_service_backends: Vec<SandboxBackendKind>,
    pub(crate) supported_operations: Vec<String>,
    pub(crate) required_binaries: Vec<MachineApiRequiredBinaryStatus>,
    pub(crate) service_execution_blockers: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MachineApiServiceExecutionMode {
    StandardContainers,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MachineApiRequiredBinaryStatus {
    pub(crate) name: String,
    pub(crate) present: bool,
    pub(crate) resolved_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MachineApiServiceSandboxImageStartRequest {
    pub(crate) launch: SandboxImageLaunchSpec,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MachineApiServiceSandboxBuildStartRequest {
    pub(crate) launch: SandboxBuildLaunchSpec,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MachineApiServiceSandboxStartResponse {
    pub(crate) handle: SandboxHandle,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MachineApiServiceSandboxInspectResponse {
    pub(crate) sandbox_id: SandboxId,
    pub(crate) handle: Option<SandboxHandle>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MachineApiServiceSandboxStopResponse {
    pub(crate) sandbox_id: SandboxId,
    pub(crate) stopped: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MachineApiErrorResponse {
    pub(crate) error: String,
}
