use nimbus::SandboxBackendKind;
use nimbus::{
    PublishedEndpoint, SandboxId, SandboxLifecycleSpec, SandboxPortBinding, SandboxResourceLimits,
    SandboxStatus, TenantId,
};
#[cfg(unix)]
use nimbus::{SandboxBuildLaunchSpec, SandboxHandle, SandboxImageLaunchSpec};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::PathBuf;

#[cfg(unix)]
pub(crate) const PROTOCOL_VERSION: &str = "v1alpha2";
#[cfg(unix)]
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
    pub(crate) binary_statuses: Vec<MachineApiBinaryStatus>,
    pub(crate) operation_statuses: Vec<MachineApiOperationStatus>,
    pub(crate) service_execution_blockers: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MachineApiServiceExecutionMode {
    StandardContainers,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MachineApiBinaryStatus {
    pub(crate) name: String,
    pub(crate) present: bool,
    pub(crate) resolved_path: Option<String>,
    pub(crate) required_for_operations: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MachineApiOperationStatus {
    pub(crate) name: String,
    pub(crate) available: bool,
    pub(crate) blockers: Vec<String>,
}

#[cfg(unix)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MachineApiBootcStatusResponse {
    pub(crate) status: serde_json::Value,
    pub(crate) booted_image: Option<String>,
    pub(crate) booted_digest: Option<String>,
    pub(crate) staged_image: Option<String>,
    pub(crate) staged_digest: Option<String>,
    pub(crate) rollback_image: Option<String>,
    pub(crate) rollback_digest: Option<String>,
}

#[cfg(unix)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MachineApiBootcSwitchRequest {
    pub(crate) image: String,
    #[serde(default)]
    pub(crate) transport: Option<String>,
}

#[cfg(unix)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MachineApiBootcUpgradeRequest {
    #[serde(default)]
    pub(crate) check: bool,
    #[serde(default)]
    pub(crate) tag: Option<String>,
}

#[cfg(unix)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MachineApiBootcRollbackRequest {}

#[cfg(unix)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MachineApiBootcOperationResponse {
    pub(crate) before: MachineApiBootcStatusResponse,
    pub(crate) after: MachineApiBootcStatusResponse,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
}

impl MachineApiCapabilityResponse {
    pub(crate) fn blockers_for_operations<'a>(
        &self,
        required_operations: impl IntoIterator<Item = &'a str>,
    ) -> Vec<String> {
        let mut blockers = BTreeSet::new();
        for required_operation in required_operations {
            if let Some(status) = self
                .operation_statuses
                .iter()
                .find(|status| status.name == required_operation)
            {
                for blocker in &status.blockers {
                    blockers.insert(blocker.clone());
                }
            }
        }
        blockers.into_iter().collect()
    }
}

#[cfg(unix)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MachineApiServiceSandboxImageStartRequest {
    pub(crate) launch: SandboxImageLaunchSpec,
}

#[cfg(unix)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MachineApiServiceSandboxBuildStartRequest {
    pub(crate) launch: SandboxBuildLaunchSpec,
}

#[cfg(unix)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MachineApiServiceSandboxStartResponse {
    pub(crate) handle: SandboxHandle,
}

#[cfg(unix)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MachineApiServiceSandboxInspectResponse {
    pub(crate) sandbox_id: SandboxId,
    pub(crate) handle: Option<SandboxHandle>,
}

#[cfg(unix)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MachineApiServiceSandboxStopResponse {
    pub(crate) sandbox_id: SandboxId,
    pub(crate) stopped: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MachineApiServiceSandboxSummary {
    pub(crate) sandbox_id: SandboxId,
    pub(crate) tenant_id: TenantId,
    pub(crate) service_name: String,
    pub(crate) status: SandboxStatus,
    pub(crate) published_endpoints: Vec<PublishedEndpoint>,
    pub(crate) restart_count: u32,
    pub(crate) last_exit_code: Option<i32>,
    pub(crate) shutdown_requested: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MachineApiServiceSandboxLogPaths {
    pub(crate) ctr_log: PathBuf,
    pub(crate) oci_log: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MachineApiServiceSandboxDetails {
    pub(crate) summary: MachineApiServiceSandboxSummary,
    pub(crate) resources: SandboxResourceLimits,
    pub(crate) lifecycle: SandboxLifecycleSpec,
    pub(crate) port_bindings: Vec<SandboxPortBinding>,
    pub(crate) log_paths: MachineApiServiceSandboxLogPaths,
    pub(crate) state_dir: PathBuf,
    pub(crate) manifest_path: PathBuf,
}

#[cfg(unix)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MachineApiServiceSandboxListResponse {
    pub(crate) sandboxes: Vec<MachineApiServiceSandboxSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MachineApiServiceSandboxLookupResponse {
    pub(crate) tenant_id: TenantId,
    pub(crate) service_name: String,
    pub(crate) details: Option<MachineApiServiceSandboxDetails>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MachineApiServiceSandboxLogChunkResponse {
    pub(crate) sandbox_id: SandboxId,
    pub(crate) offset: u64,
    pub(crate) next_offset: u64,
    pub(crate) chunk: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MachineApiServiceProcessSnapshot {
    pub(crate) sandbox_id: SandboxId,
    pub(crate) tenant_id: TenantId,
    pub(crate) service_name: String,
    pub(crate) status: SandboxStatus,
    pub(crate) runtime_pidfile: PathBuf,
    pub(crate) conmon_pidfile: PathBuf,
    pub(crate) runtime_pid: Option<u32>,
    pub(crate) conmon_pid: Option<u32>,
    pub(crate) process_rows: Vec<MachineApiServiceProcessRow>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MachineApiServiceProcessRow {
    pub(crate) pid: u32,
    pub(crate) ppid: u32,
    pub(crate) command: String,
}

#[cfg(unix)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MachineApiServiceProcessSnapshotResponse {
    pub(crate) snapshot: MachineApiServiceProcessSnapshot,
}

#[cfg(unix)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MachineApiErrorResponse {
    pub(crate) error: String,
}
