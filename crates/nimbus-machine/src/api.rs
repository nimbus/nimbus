use nimbus_core::TenantId;
use nimbus_network::PublishedEndpoint;
use nimbus_sandbox::{
    MachinePortForwardReceipt, SandboxBackendKind, SandboxId, SandboxLifecycleSpec,
    SandboxPortBinding, SandboxResourceLimits, SandboxStatus,
};
#[cfg(unix)]
use nimbus_sandbox::{SandboxHandle, SandboxInspection, SandboxSpec};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::PathBuf;

#[cfg(unix)]
use crate::MachineForwarderAuthority;

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
#[serde(deny_unknown_fields)]
pub struct MachineApiBootcSwitchRequest {
    pub forwarder_authority: MachineForwarderAuthority,
    pub image: String,
    #[serde(default)]
    pub transport: Option<String>,
}

#[cfg(unix)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MachineApiBootcUpgradeRequest {
    pub forwarder_authority: MachineForwarderAuthority,
    #[serde(default)]
    pub check: bool,
    #[serde(default)]
    pub tag: Option<String>,
}

#[cfg(unix)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MachineApiBootcRollbackRequest {
    pub forwarder_authority: MachineForwarderAuthority,
}

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
#[serde(deny_unknown_fields)]
pub struct MachineApiServiceSandboxImageStartRequest {
    pub sandbox_id: SandboxId,
    pub forwarder_authority: MachineForwarderAuthority,
    pub spec: SandboxSpec,
}

#[cfg(unix)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MachineApiServiceSandboxBuildStartRequest {
    pub sandbox_id: SandboxId,
    pub forwarder_authority: MachineForwarderAuthority,
    pub spec: SandboxSpec,
}

#[cfg(unix)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MachineApiServiceSandboxStartResponse {
    pub handle: SandboxHandle,
    pub forwarder_authority: MachineForwarderAuthority,
    pub publication_evidence: Vec<MachinePortForwardReceipt>,
}

#[cfg(unix)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MachineApiServiceSandboxInspectResponse {
    pub sandbox_id: SandboxId,
    pub inspection: Option<SandboxInspection>,
}

#[cfg(unix)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MachineApiServiceSandboxStopRequest {
    pub forwarder_authority: MachineForwarderAuthority,
}

#[cfg(unix)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MachineApiServiceSandboxStopResponse {
    pub tenant_id: TenantId,
    pub sandbox_id: SandboxId,
    pub stopped: bool,
    pub forwarder_authority: MachineForwarderAuthority,
    pub confirmed_absent_evidence: Vec<MachinePortForwardReceipt>,
}

#[cfg(unix)]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MachineApiServiceSandboxStartResponseWire {
    handle: SandboxHandle,
    forwarder_authority: MachineForwarderAuthority,
    publication_evidence: Vec<MachinePortForwardReceipt>,
}

#[cfg(unix)]
impl<'de> Deserialize<'de> for MachineApiServiceSandboxStartResponse {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = MachineApiServiceSandboxStartResponseWire::deserialize(deserializer)?;
        let mut seen_bindings = Vec::with_capacity(wire.publication_evidence.len());
        for (index, receipt) in wire.publication_evidence.iter().enumerate() {
            if receipt.outcome != nimbus_sandbox::MachinePortForwardOutcome::Exposed
                || receipt.tenant_id != wire.handle.tenant_id
                || receipt.sandbox_id != wire.handle.id
                || receipt.provider_instance != *wire.forwarder_authority.provider_instance()
                || receipt.provider_generation != wire.forwarder_authority.generation()
            {
                return Err(serde::de::Error::custom(format!(
                    "start publication evidence member {index} is crossed, stale, or not an \
                     exact exposed receipt for the response identity"
                )));
            }
            if seen_bindings
                .iter()
                .any(|binding| binding == &receipt.binding)
            {
                return Err(serde::de::Error::custom(format!(
                    "start publication evidence member {index} duplicates a binding already \
                     present in the exact response set"
                )));
            }
            seen_bindings.push(receipt.binding.clone());
        }
        Ok(Self {
            handle: wire.handle,
            forwarder_authority: wire.forwarder_authority,
            publication_evidence: wire.publication_evidence,
        })
    }
}

#[cfg(unix)]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MachineApiServiceSandboxStopResponseWire {
    tenant_id: TenantId,
    sandbox_id: SandboxId,
    stopped: bool,
    forwarder_authority: MachineForwarderAuthority,
    confirmed_absent_evidence: Vec<MachinePortForwardReceipt>,
}

#[cfg(unix)]
impl<'de> Deserialize<'de> for MachineApiServiceSandboxStopResponse {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = MachineApiServiceSandboxStopResponseWire::deserialize(deserializer)?;
        let mut seen_bindings = Vec::with_capacity(wire.confirmed_absent_evidence.len());
        for (index, receipt) in wire.confirmed_absent_evidence.iter().enumerate() {
            if !matches!(
                receipt.outcome,
                nimbus_sandbox::MachinePortForwardOutcome::Withdrawn
                    | nimbus_sandbox::MachinePortForwardOutcome::ExactAlreadyAbsent
            ) || receipt.tenant_id != wire.tenant_id
                || receipt.sandbox_id != wire.sandbox_id
                || receipt.provider_instance != *wire.forwarder_authority.provider_instance()
                || receipt.provider_generation != wire.forwarder_authority.generation()
            {
                return Err(serde::de::Error::custom(format!(
                    "stop absence evidence member {index} is crossed, stale, or not an exact \
                     withdrawn/already-absent receipt for the response identity"
                )));
            }
            if seen_bindings
                .iter()
                .any(|binding| binding == &receipt.binding)
            {
                return Err(serde::de::Error::custom(format!(
                    "stop absence evidence member {index} duplicates a binding already present \
                     in the exact response set"
                )));
            }
            seen_bindings.push(receipt.binding.clone());
        }
        Ok(Self {
            tenant_id: wire.tenant_id,
            sandbox_id: wire.sandbox_id,
            stopped: wire.stopped,
            forwarder_authority: wire.forwarder_authority,
            confirmed_absent_evidence: wire.confirmed_absent_evidence,
        })
    }
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
    #[cfg(unix)]
    use nimbus_network::{NetworkProviderHandle, NetworkProviderId, NetworkResourceGeneration};
    #[cfg(unix)]
    use nimbus_sandbox::{
        MachinePortForwardOutcome, SandboxCleanupObservation, SandboxExecutionObservation,
        SandboxOwnerSpec, SandboxProcessSpec, SandboxRestartAssessment, SandboxRestartBlocker,
        SandboxRootSpec,
    };

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

    #[cfg(unix)]
    #[test]
    fn service_sandbox_mutation_dtos_are_strict_and_preserve_exact_evidence() {
        let sandbox_id = SandboxId::new("sandbox-machine-api-01");
        let authority = MachineForwarderAuthority::new(
            NetworkProviderHandle::new(
                NetworkProviderId::for_registration_key("machine-gvproxy"),
                "machine-config-01",
            )
            .expect("provider fixture should validate"),
            NetworkResourceGeneration::new(11),
        );
        let bindings = vec![
            SandboxPortBinding::tcp("http", 18_080, 8_080),
            SandboxPortBinding::tcp("metrics", 19_090, 9_090),
        ];
        let spec = SandboxSpec::new(
            TenantId::new("tenant-machine-api").expect("tenant fixture should validate"),
            SandboxOwnerSpec::service("api"),
            SandboxBackendKind::Container,
            SandboxRootSpec::rootfs("/tmp/rootfs"),
            SandboxProcessSpec::new(["/bin/service"]),
        )
        .with_port_bindings(bindings.clone());

        let image_request = MachineApiServiceSandboxImageStartRequest {
            sandbox_id: sandbox_id.clone(),
            forwarder_authority: authority.clone(),
            spec: spec.clone(),
        };
        let build_request = MachineApiServiceSandboxBuildStartRequest {
            sandbox_id: sandbox_id.clone(),
            forwarder_authority: authority.clone(),
            spec: spec.clone(),
        };
        assert_strict_authority_request(&image_request, "image start request");
        assert_strict_authority_request(&build_request, "build start request");
        let stop_request = MachineApiServiceSandboxStopRequest {
            forwarder_authority: authority.clone(),
        };
        assert_strict_authority_request(&stop_request, "stop request");
        assert_strict_authority_request(
            &MachineApiBootcSwitchRequest {
                forwarder_authority: authority.clone(),
                image: "ghcr.io/nimbus/machine-os:next".to_owned(),
                transport: Some("registry".to_owned()),
            },
            "bootc switch request",
        );
        assert_strict_authority_request(
            &MachineApiBootcUpgradeRequest {
                forwarder_authority: authority.clone(),
                check: false,
                tag: None,
            },
            "bootc upgrade request",
        );
        assert_strict_authority_request(
            &MachineApiBootcRollbackRequest {
                forwarder_authority: authority.clone(),
            },
            "bootc rollback request",
        );

        let handle = SandboxHandle::new(
            spec.tenant_id.clone(),
            sandbox_id.clone(),
            "api",
            SandboxBackendKind::Container,
            SandboxStatus::Ready,
            Vec::new(),
        );
        let inspection = SandboxInspection::provider_reported(handle.clone())
            .with_provider_projection(
                handle.clone(),
                SandboxExecutionObservation::Exited { exit_code: 42 },
                SandboxRestartAssessment::Candidate {
                    exit_code: 42,
                    completed_restarts: 1,
                    retry_delay_millis: 2_000,
                    persisted_not_before_millis: Some(9_000),
                    blocker: Some(SandboxRestartBlocker::StartupReconciliationUnavailable),
                },
                SandboxCleanupObservation::Retained,
            );
        let inspect_response = MachineApiServiceSandboxInspectResponse {
            sandbox_id: sandbox_id.clone(),
            inspection: Some(inspection.clone()),
        };
        let inspect_value =
            serde_json::to_value(&inspect_response).expect("inspection response should serialize");
        assert_eq!(
            serde_json::from_value::<MachineApiServiceSandboxInspectResponse>(
                inspect_value.clone()
            )
            .expect("inspection response should deserialize"),
            inspect_response,
            "every typed inspection field and exact version must round trip"
        );
        assert_eq!(
            inspect_response
                .inspection
                .as_ref()
                .expect("inspection should remain present")
                .version,
            inspection.version
        );
        assert_unknown_field_rejected::<MachineApiServiceSandboxInspectResponse>(
            inspect_value,
            "inspection response",
        );
        let exposed = bindings
            .iter()
            .map(|binding| MachinePortForwardReceipt {
                outcome: MachinePortForwardOutcome::Exposed,
                tenant_id: spec.tenant_id.clone(),
                sandbox_id: sandbox_id.clone(),
                binding: binding.clone(),
                provider_instance: authority.provider_instance().clone(),
                provider_generation: authority.generation(),
            })
            .collect::<Vec<_>>();
        let start = MachineApiServiceSandboxStartResponse {
            handle: handle.clone(),
            forwarder_authority: authority.clone(),
            publication_evidence: exposed.clone(),
        };
        let start_value = serde_json::to_value(&start).expect("start response should serialize");
        assert_eq!(
            serde_json::from_value::<MachineApiServiceSandboxStartResponse>(start_value.clone())
                .expect("start response should deserialize"),
            start
        );
        assert_eq!(start.handle.id, sandbox_id);
        assert_eq!(start.forwarder_authority, authority);
        assert_eq!(start.publication_evidence, exposed);
        assert_unknown_field_rejected::<MachineApiServiceSandboxStartResponse>(
            start_value,
            "start response",
        );
        let mut nested_unknown =
            serde_json::to_value(&start).expect("nested unknown fixture should serialize");
        nested_unknown["publication_evidence"][0]["unexpected"] = serde_json::json!(true);
        assert!(
            serde_json::from_value::<MachineApiServiceSandboxStartResponse>(nested_unknown)
                .is_err(),
            "strict receipt DTOs must reject unknown provider evidence fields"
        );
        let mut crossed_start =
            serde_json::to_value(&start).expect("crossed start fixture should serialize");
        crossed_start["publication_evidence"][0]["outcome"] = serde_json::json!("withdrawn");
        assert!(
            serde_json::from_value::<MachineApiServiceSandboxStartResponse>(crossed_start).is_err(),
            "the strict response DTO must reject crossed start outcomes"
        );
        let mut duplicate_start =
            serde_json::to_value(&start).expect("duplicate start fixture should serialize");
        duplicate_start["publication_evidence"][1] =
            duplicate_start["publication_evidence"][0].clone();
        assert!(
            serde_json::from_value::<MachineApiServiceSandboxStartResponse>(duplicate_start)
                .is_err(),
            "the strict response DTO must reject a duplicate binding that substitutes for an \
             omitted member"
        );

        let absent = bindings
            .iter()
            .map(|binding| MachinePortForwardReceipt {
                outcome: MachinePortForwardOutcome::ExactAlreadyAbsent,
                tenant_id: spec.tenant_id.clone(),
                sandbox_id: sandbox_id.clone(),
                binding: binding.clone(),
                provider_instance: authority.provider_instance().clone(),
                provider_generation: authority.generation(),
            })
            .collect::<Vec<_>>();
        let stop = MachineApiServiceSandboxStopResponse {
            tenant_id: spec.tenant_id.clone(),
            sandbox_id: SandboxId::new("sandbox-machine-api-01"),
            stopped: true,
            forwarder_authority: authority.clone(),
            confirmed_absent_evidence: absent.clone(),
        };
        let stop_value = serde_json::to_value(&stop).expect("stop response should serialize");
        assert_eq!(
            serde_json::from_value::<MachineApiServiceSandboxStopResponse>(stop_value.clone())
                .expect("stop response should deserialize"),
            stop
        );
        assert_eq!(stop.forwarder_authority, authority);
        assert_eq!(stop.confirmed_absent_evidence, absent);
        assert_unknown_field_rejected::<MachineApiServiceSandboxStopResponse>(
            stop_value,
            "stop response",
        );
        let mut stale_stop =
            serde_json::to_value(&stop).expect("stale stop fixture should serialize");
        stale_stop["confirmed_absent_evidence"][0]["provider_generation"] =
            serde_json::json!(authority.generation().as_u64() + 1);
        assert!(
            serde_json::from_value::<MachineApiServiceSandboxStopResponse>(stale_stop).is_err(),
            "the strict response DTO must reject stale stop provider generations"
        );
        let mut duplicate_stop =
            serde_json::to_value(&stop).expect("duplicate stop fixture should serialize");
        duplicate_stop["confirmed_absent_evidence"][1] =
            duplicate_stop["confirmed_absent_evidence"][0].clone();
        assert!(
            serde_json::from_value::<MachineApiServiceSandboxStopResponse>(duplicate_stop).is_err(),
            "the strict response DTO must reject duplicate absence evidence that substitutes for \
             an omitted member"
        );
    }

    #[cfg(unix)]
    fn assert_strict_authority_request<T>(request: &T, label: &str)
    where
        T: Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug,
    {
        let value = serde_json::to_value(request).expect("request should serialize");
        let round_trip =
            serde_json::from_value::<T>(value.clone()).expect("request should deserialize");
        assert_eq!(&round_trip, request, "{label} must round trip exactly");

        let mut missing = value.clone();
        missing
            .as_object_mut()
            .expect("request wire should be an object")
            .remove("forwarder_authority");
        assert!(
            serde_json::from_value::<T>(missing).is_err(),
            "{label} must reject a missing authority"
        );
        assert_unknown_field_rejected::<T>(value, label);
    }

    #[cfg(unix)]
    fn assert_unknown_field_rejected<T>(mut value: serde_json::Value, label: &str)
    where
        T: for<'de> Deserialize<'de>,
    {
        value
            .as_object_mut()
            .expect("wire should be an object")
            .insert("unexpected".to_owned(), serde_json::json!(true));
        assert!(
            serde_json::from_value::<T>(value).is_err(),
            "{label} must reject unknown fields"
        );
    }
}
