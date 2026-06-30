use nimbus_core::TenantId;
use nimbus_sandbox::{
    PublishedEndpoint, SandboxBackendKind, SandboxId, SandboxLifecycleSpec, SandboxPortBinding,
    SandboxResourceLimits, SandboxStatus,
};
#[cfg(unix)]
use nimbus_sandbox::{SandboxHandle, SandboxSpec};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::PathBuf;

#[cfg(unix)]
pub const MACHINE_API_PROTOCOL_VERSION: &str = "v1alpha2";
#[cfg(unix)]
pub const PROTOCOL_VERSION: &str = MACHINE_API_PROTOCOL_VERSION;
#[cfg(unix)]
pub const MACHINE_API_ROLE: &str = "guest-machine-api";
pub const MACHINE_API_HEALTH_PATH: &str = "/healthz";
pub const MACHINE_API_CAPABILITIES_PATH: &str = "/v1/machine-api/capabilities";
pub const MACHINE_API_BOOTC_STATUS_PATH: &str = "/v1/machine-api/os/bootc/status";
pub const MACHINE_API_BOOTC_SWITCH_PATH: &str = "/v1/machine-api/os/bootc/switch";
pub const MACHINE_API_BOOTC_UPGRADE_PATH: &str = "/v1/machine-api/os/bootc/upgrade";
pub const MACHINE_API_BOOTC_ROLLBACK_PATH: &str = "/v1/machine-api/os/bootc/rollback";
pub const MACHINE_API_SERVICE_SANDBOX_IMAGE_START_PATH: &str =
    "/v1/machine-api/service-sandboxes/image-start";
pub const MACHINE_API_SERVICE_SANDBOX_BUILD_START_PATH: &str =
    "/v1/machine-api/service-sandboxes/build-start";
pub const MACHINE_API_SERVICE_SANDBOXES_PATH: &str = "/v1/machine-api/service-sandboxes";
pub const MACHINE_API_CURRENT_SERVICE_SANDBOX_PATH: &str =
    "/v1/machine-api/service-sandboxes/current";
pub const MACHINE_API_SERVICE_SANDBOX_PATH: &str = "/v1/machine-api/service-sandboxes/{sandbox_id}";
pub const MACHINE_API_SERVICE_SANDBOX_LOGS_PATH: &str =
    "/v1/machine-api/service-sandboxes/{sandbox_id}/logs";
pub const MACHINE_API_SERVICE_SANDBOX_PROCESS_SNAPSHOT_PATH: &str =
    "/v1/machine-api/service-sandboxes/{sandbox_id}/ps";
pub const MACHINE_API_SERVICE_SANDBOX_STOP_PATH: &str =
    "/v1/machine-api/service-sandboxes/{sandbox_id}/stop";
pub const MACHINE_API_IMAGE_START_OPERATION: &str = "service-sandboxes.image-start";
pub const MACHINE_API_BUILD_START_OPERATION: &str = "service-sandboxes.build-start";
pub const MACHINE_API_LIST_OPERATION: &str = "service-sandboxes.list";
pub const MACHINE_API_INSPECT_OPERATION: &str = "service-sandboxes.inspect";
pub const MACHINE_API_INSPECT_CURRENT_OPERATION: &str = "service-sandboxes.inspect-current";
pub const MACHINE_API_LOGS_OPERATION: &str = "service-sandboxes.logs";
pub const MACHINE_API_PS_OPERATION: &str = "service-sandboxes.ps";
pub const MACHINE_API_STOP_OPERATION: &str = "service-sandboxes.stop";
pub const MACHINE_API_BOOTC_STATUS_OPERATION: &str = "os.bootc.status";
pub const MACHINE_API_BOOTC_SWITCH_OPERATION: &str = "os.bootc.switch";
pub const MACHINE_API_BOOTC_UPGRADE_OPERATION: &str = "os.bootc.upgrade";
pub const MACHINE_API_BOOTC_ROLLBACK_OPERATION: &str = "os.bootc.rollback";

pub fn machine_api_service_sandbox_path(sandbox_id: &str) -> String {
    format!(
        "/v1/machine-api/service-sandboxes/{}",
        machine_api_path_segment(sandbox_id)
    )
}

pub fn machine_api_service_sandbox_stop_path(sandbox_id: &str) -> String {
    format!("{}/stop", machine_api_service_sandbox_path(sandbox_id))
}

pub fn machine_api_service_sandbox_logs_path(sandbox_id: &str, offset: u64) -> String {
    format!(
        "{}/logs?offset={offset}",
        machine_api_service_sandbox_path(sandbox_id)
    )
}

pub fn machine_api_service_sandbox_process_snapshot_path(sandbox_id: &str) -> String {
    format!("{}/ps", machine_api_service_sandbox_path(sandbox_id))
}

pub fn machine_api_service_sandbox_list_path(tenant_id: Option<&str>) -> String {
    tenant_id
        .map(|tenant_id| {
            machine_api_query_path(
                MACHINE_API_SERVICE_SANDBOXES_PATH,
                &[("tenant_id", tenant_id)],
            )
        })
        .unwrap_or_else(|| MACHINE_API_SERVICE_SANDBOXES_PATH.to_owned())
}

pub fn machine_api_current_service_sandbox_path(tenant_id: &str, service_name: &str) -> String {
    machine_api_query_path(
        MACHINE_API_CURRENT_SERVICE_SANDBOX_PATH,
        &[("tenant_id", tenant_id), ("service_name", service_name)],
    )
}

pub fn machine_api_query_path(path: &str, params: &[(&str, &str)]) -> String {
    let mut encoded = String::from(path);
    for (index, (name, value)) in params.iter().enumerate() {
        encoded.push(if index == 0 { '?' } else { '&' });
        encoded.push_str(name);
        encoded.push('=');
        percent_encode_query_value_into(value, &mut encoded);
    }
    encoded
}

/// Encode `id` into a single URL path segment, percent-escaping every byte
/// outside the RFC 3986 unreserved set so reserved/structural characters
/// (`/`, `..`, `%`, space, `?`, `#`, ...) cannot break out of the segment
/// and alter the request line's path structure.
pub fn machine_api_path_segment(id: &str) -> String {
    let mut out = String::with_capacity(id.len());
    percent_encode_path_segment_into(id, &mut out);
    out
}

fn percent_encode_query_value_into(value: &str, encoded: &mut String) {
    percent_encode_into(value, encoded, is_unreserved_query_byte);
}

fn percent_encode_path_segment_into(value: &str, encoded: &mut String) {
    percent_encode_into(value, encoded, is_unreserved_path_segment_byte);
}

fn percent_encode_into(value: &str, encoded: &mut String, is_unreserved: fn(u8) -> bool) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";

    for byte in value.bytes() {
        if is_unreserved(byte) {
            encoded.push(byte as char);
        } else {
            encoded.push('%');
            encoded.push(HEX[(byte >> 4) as usize] as char);
            encoded.push(HEX[(byte & 0x0F) as usize] as char);
        }
    }
}

fn is_unreserved_query_byte(byte: u8) -> bool {
    matches!(
        byte,
        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~'
    )
}

fn is_unreserved_path_segment_byte(byte: u8) -> bool {
    is_unreserved_query_byte(byte)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineApiHealthResponse {
    pub status: String,
    pub role: String,
    pub protocol_version: String,
    pub listen_mode: String,
    pub control_data_dir: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineApiCapabilityResponse {
    pub protocol_version: String,
    pub service_execution_ready: bool,
    pub service_execution_mode: MachineApiServiceExecutionMode,
    #[serde(default)]
    pub service_execution_driver: MachineApiServiceExecutionDriver,
    pub supported_service_backends: Vec<SandboxBackendKind>,
    pub supported_operations: Vec<String>,
    pub binary_statuses: Vec<MachineApiBinaryStatus>,
    pub operation_statuses: Vec<MachineApiOperationStatus>,
    pub service_execution_blockers: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MachineApiServiceExecutionMode {
    StandardContainers,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MachineApiServiceExecutionDriver {
    #[default]
    Unavailable,
    GuestNodeAgentSystemdTransientUnit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineApiBinaryStatus {
    pub name: String,
    pub present: bool,
    pub resolved_path: Option<String>,
    pub required_for_operations: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineApiOperationStatus {
    pub name: String,
    pub available: bool,
    pub blockers: Vec<String>,
}

#[cfg(unix)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineApiBootcStatusResponse {
    pub status: serde_json::Value,
    pub booted_image: Option<String>,
    pub booted_digest: Option<String>,
    pub staged_image: Option<String>,
    pub staged_digest: Option<String>,
    pub rollback_image: Option<String>,
    pub rollback_digest: Option<String>,
}

#[cfg(unix)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineApiBootcSwitchRequest {
    pub image: String,
    #[serde(default)]
    pub transport: Option<String>,
}

#[cfg(unix)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineApiBootcUpgradeRequest {
    #[serde(default)]
    pub check: bool,
    #[serde(default)]
    pub tag: Option<String>,
}

#[cfg(unix)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineApiBootcRollbackRequest {}

#[cfg(unix)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineApiBootcOperationResponse {
    pub before: MachineApiBootcStatusResponse,
    pub after: MachineApiBootcStatusResponse,
    pub stdout: String,
    pub stderr: String,
}

impl MachineApiCapabilityResponse {
    pub fn blockers_for_operations<'a>(
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
pub struct MachineApiServiceSandboxImageStartRequest {
    pub spec: SandboxSpec,
}

#[cfg(unix)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineApiServiceSandboxBuildStartRequest {
    pub spec: SandboxSpec,
}

#[cfg(unix)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineApiServiceSandboxStartResponse {
    pub handle: SandboxHandle,
}

#[cfg(unix)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineApiServiceSandboxInspectResponse {
    pub sandbox_id: SandboxId,
    pub handle: Option<SandboxHandle>,
}

#[cfg(unix)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineApiServiceSandboxStopResponse {
    pub sandbox_id: SandboxId,
    pub stopped: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineApiServiceSandboxSummary {
    pub sandbox_id: SandboxId,
    pub tenant_id: TenantId,
    pub service_name: String,
    pub status: SandboxStatus,
    pub published_endpoints: Vec<PublishedEndpoint>,
    pub restart_count: u32,
    pub last_exit_code: Option<i32>,
    pub shutdown_requested: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineApiServiceSandboxLogPaths {
    pub ctr_log: PathBuf,
    pub oci_log: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineApiServiceSandboxDetails {
    pub summary: MachineApiServiceSandboxSummary,
    pub resources: SandboxResourceLimits,
    pub lifecycle: SandboxLifecycleSpec,
    pub port_bindings: Vec<SandboxPortBinding>,
    pub log_paths: MachineApiServiceSandboxLogPaths,
    pub state_dir: PathBuf,
    pub manifest_path: PathBuf,
}

#[cfg(unix)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineApiServiceSandboxListResponse {
    pub sandboxes: Vec<MachineApiServiceSandboxSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineApiServiceSandboxLookupResponse {
    pub tenant_id: TenantId,
    pub service_name: String,
    pub details: Option<MachineApiServiceSandboxDetails>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineApiServiceSandboxLogChunkResponse {
    pub sandbox_id: SandboxId,
    pub offset: u64,
    pub next_offset: u64,
    pub chunk: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineApiServiceProcessSnapshot {
    pub sandbox_id: SandboxId,
    pub tenant_id: TenantId,
    pub service_name: String,
    pub status: SandboxStatus,
    pub runtime_pidfile: PathBuf,
    pub conmon_pidfile: PathBuf,
    pub runtime_pid: Option<u32>,
    pub conmon_pid: Option<u32>,
    pub process_rows: Vec<MachineApiServiceProcessRow>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineApiServiceProcessRow {
    pub pid: u32,
    pub ppid: u32,
    pub command: String,
}

#[cfg(unix)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineApiServiceProcessSnapshotResponse {
    pub snapshot: MachineApiServiceProcessSnapshot,
}

#[cfg(unix)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineApiErrorResponse {
    pub error: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn machine_api_query_path_percent_encodes_query_delimiters() {
        let path = machine_api_query_path(
            MACHINE_API_CURRENT_SERVICE_SANDBOX_PATH,
            &[
                ("tenant_id", "tenant"),
                ("service_name", "db & cache=1/path☁"),
            ],
        );

        assert_eq!(
            path,
            "/v1/machine-api/service-sandboxes/current?tenant_id=tenant&service_name=db%20%26%20cache%3D1%2Fpath%E2%98%81"
        );
    }

    #[test]
    fn machine_api_path_segment_encodes_reserved_and_structural_characters() {
        assert_eq!(machine_api_path_segment("db-1"), "db-1");
        assert_eq!(machine_api_path_segment("../etc"), "..%2Fetc");
        assert_eq!(machine_api_path_segment("a/b"), "a%2Fb");
        assert_eq!(machine_api_path_segment("a b"), "a%20b");
        assert_eq!(machine_api_path_segment("50%off"), "50%25off");
        assert_eq!(machine_api_path_segment("q?x#y"), "q%3Fx%23y");
    }

    #[test]
    fn machine_api_service_sandbox_paths_use_encoded_single_segments() {
        assert_eq!(
            machine_api_service_sandbox_path("x/y"),
            "/v1/machine-api/service-sandboxes/x%2Fy"
        );
        assert_eq!(
            machine_api_service_sandbox_logs_path("x/y", 7),
            "/v1/machine-api/service-sandboxes/x%2Fy/logs?offset=7"
        );
        assert_eq!(
            machine_api_service_sandbox_process_snapshot_path("p#q"),
            "/v1/machine-api/service-sandboxes/p%23q/ps"
        );
        assert_eq!(
            machine_api_service_sandbox_stop_path("a b%c"),
            "/v1/machine-api/service-sandboxes/a%20b%25c/stop"
        );
    }
}
