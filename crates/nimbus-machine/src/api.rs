use nimbus_core::TenantId;
use nimbus_network::PublishedEndpoint;
#[cfg(unix)]
use nimbus_sandbox::SandboxInspection;
use nimbus_sandbox::{
    MachinePortForwardReceipt, SandboxBackendKind, SandboxId, SandboxLifecycleSpec,
    SandboxPortBinding, SandboxResourceLimits, SandboxStatus,
};
#[cfg(unix)]
use nimbus_workloads::{
    CompiledWorkloadNetworkPlan, WorkloadDesiredDigest, WorkloadExecutableIntent,
    WorkloadExecutionReference, WorkloadGeneration, WorkloadProvisionAttemptId,
    WorkloadProvisionCommandId, WorkloadProvisionCommandMode, WorkloadProvisionDispatchClaim,
    WorkloadProvisionDispatchEpoch, WorkloadProvisionProviderTarget, WorkloadProvisionSourceDigest,
    WorkloadProvisionSourceEvidence, WorkloadProvisionStep, WorkloadProvisionSubjects,
    WorkloadSagaRevision, WorkloadSagaTransitionId,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
#[cfg(unix)]
use std::error::Error as StdError;
#[cfg(unix)]
use std::fmt::{self, Display, Formatter};
use std::path::PathBuf;

#[cfg(unix)]
use crate::MachineForwarderAuthority;

#[cfg(unix)]
mod restart;
#[cfg(unix)]
pub use restart::{
    MachineApiWorkloadRestartCommandEnvelope, MachineApiWorkloadRestartCommandMode,
    MachineApiWorkloadRestartObservation, MachineApiWorkloadRestartPhaseRequest,
    MachineApiWorkloadRestartPhaseResponse, MachineApiWorkloadRestartRequestDigest,
    MachineApiWorkloadRestartWireError,
};

#[cfg(unix)]
mod teardown;
#[cfg(unix)]
pub use teardown::{
    MachineApiNetworkReleaseAbsenceEvidence, MachineApiWorkloadTeardownCommandEnvelope,
    MachineApiWorkloadTeardownCommandEnvelopeInput, MachineApiWorkloadTeardownExecuteObservation,
    MachineApiWorkloadTeardownInspectObservation, MachineApiWorkloadTeardownObservation,
    MachineApiWorkloadTeardownPhaseRequest, MachineApiWorkloadTeardownPhaseResponse,
    MachineApiWorkloadTeardownPhaseResult, MachineApiWorkloadTeardownProviderTranslation,
    MachineApiWorkloadTeardownRequestDigest, MachineApiWorkloadTeardownWireError,
};

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
pub const MACHINE_API_WORKLOAD_PROVISION_PHASE_PATH: &str =
    "/v1/machine-api/workload-provision/phase";
pub const MACHINE_API_WORKLOAD_RESTART_PHASE_PATH: &str = "/v1/machine-api/workload-restart/phase";
pub const MACHINE_API_WORKLOAD_TEARDOWN_PHASE_PATH: &str =
    "/v1/machine-api/workload-teardown/phase";
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
pub const MACHINE_API_WORKLOAD_PROVISION_PHASE_OPERATION: &str = "workload-provision.phase";
pub const MACHINE_API_WORKLOAD_RESTART_PHASE_OPERATION: &str = "workload-restart.phase";
pub const MACHINE_API_WORKLOAD_TEARDOWN_PHASE_OPERATION: &str = "workload-teardown.phase";
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

/// Maximum opaque evidence retained in one provision-phase response.
#[cfg(unix)]
pub const MAX_MACHINE_API_PROVISION_EVIDENCE_BYTES: usize = 64 * 1024;

/// Transport envelope for one command already confirmed by the compute owner.
///
/// This value does not grant confirmation authority. Its constructor and
/// deserializer reject crossed portable evidence before a guest adapter acts.
#[cfg(unix)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MachineApiWorkloadProvisionCommandEnvelope {
    command_id: WorkloadProvisionCommandId,
    attempt_id: WorkloadProvisionAttemptId,
    dispatch_epoch: WorkloadProvisionDispatchEpoch,
    provider_target: WorkloadProvisionProviderTarget,
    claim: WorkloadProvisionDispatchClaim,
    confirmed_revision: WorkloadSagaRevision,
    transition_id: WorkloadSagaTransitionId,
    generation: WorkloadGeneration,
    desired_digest: WorkloadDesiredDigest,
    source: WorkloadProvisionSourceEvidence,
    network_plan_digest: nimbus_network::NetworkPlanDigest,
    execution: WorkloadExecutionReference,
    executable: WorkloadExecutableIntent,
    compiled_network_plan: CompiledWorkloadNetworkPlan,
    machine_provider_generation: nimbus_network::NetworkResourceGeneration,
    mode: WorkloadProvisionCommandMode,
}

#[cfg(unix)]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MachineApiWorkloadProvisionCommandEnvelopeWire {
    command_id: WorkloadProvisionCommandId,
    attempt_id: WorkloadProvisionAttemptId,
    dispatch_epoch: WorkloadProvisionDispatchEpoch,
    provider_target: WorkloadProvisionProviderTarget,
    claim: WorkloadProvisionDispatchClaim,
    confirmed_revision: WorkloadSagaRevision,
    transition_id: WorkloadSagaTransitionId,
    generation: WorkloadGeneration,
    desired_digest: WorkloadDesiredDigest,
    source: WorkloadProvisionSourceEvidence,
    network_plan_digest: nimbus_network::NetworkPlanDigest,
    execution: WorkloadExecutionReference,
    executable: WorkloadExecutableIntent,
    compiled_network_plan: CompiledWorkloadNetworkPlan,
    machine_provider_generation: nimbus_network::NetworkResourceGeneration,
    mode: WorkloadProvisionCommandMode,
}

#[cfg(unix)]
impl<'de> Deserialize<'de> for MachineApiWorkloadProvisionCommandEnvelope {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = MachineApiWorkloadProvisionCommandEnvelopeWire::deserialize(deserializer)?;
        Self::new(
            wire.command_id,
            wire.attempt_id,
            wire.dispatch_epoch,
            wire.provider_target,
            wire.claim,
            wire.confirmed_revision,
            wire.transition_id,
            wire.generation,
            wire.desired_digest,
            wire.source,
            wire.network_plan_digest,
            wire.execution,
            wire.executable,
            wire.compiled_network_plan,
            wire.machine_provider_generation,
            wire.mode,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[cfg(unix)]
impl MachineApiWorkloadProvisionCommandEnvelope {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        command_id: WorkloadProvisionCommandId,
        attempt_id: WorkloadProvisionAttemptId,
        dispatch_epoch: WorkloadProvisionDispatchEpoch,
        provider_target: WorkloadProvisionProviderTarget,
        claim: WorkloadProvisionDispatchClaim,
        confirmed_revision: WorkloadSagaRevision,
        transition_id: WorkloadSagaTransitionId,
        generation: WorkloadGeneration,
        desired_digest: WorkloadDesiredDigest,
        source: WorkloadProvisionSourceEvidence,
        network_plan_digest: nimbus_network::NetworkPlanDigest,
        execution: WorkloadExecutionReference,
        executable: WorkloadExecutableIntent,
        compiled_network_plan: CompiledWorkloadNetworkPlan,
        machine_provider_generation: nimbus_network::NetworkResourceGeneration,
        mode: WorkloadProvisionCommandMode,
    ) -> Result<Self, MachineApiWorkloadProvisionWireError> {
        let command = Self {
            command_id,
            attempt_id,
            dispatch_epoch,
            provider_target,
            claim,
            confirmed_revision,
            transition_id,
            generation,
            desired_digest,
            source,
            network_plan_digest,
            execution,
            executable,
            compiled_network_plan,
            machine_provider_generation,
            mode,
        };
        command.validate()?;
        Ok(command)
    }

    fn validate(&self) -> Result<(), MachineApiWorkloadProvisionWireError> {
        let attempt = self.claim.attempt();
        if self.attempt_id != *attempt.attempt_id() {
            return Err(MachineApiWorkloadProvisionWireError::AttemptMismatch);
        }
        if self.dispatch_epoch != self.claim.dispatch_epoch() {
            return Err(MachineApiWorkloadProvisionWireError::DispatchEpochMismatch);
        }
        if self.provider_target != *self.claim.provider_target() {
            return Err(MachineApiWorkloadProvisionWireError::ProviderTargetMismatch);
        }
        if self.confirmed_revision < self.claim.claimed_revision()
            || (self.mode == WorkloadProvisionCommandMode::Execute
                && self.confirmed_revision != self.claim.claimed_revision())
        {
            return Err(MachineApiWorkloadProvisionWireError::ConfirmedRevisionMismatch);
        }
        let expected_command_id = WorkloadProvisionCommandId::for_confirmed_dispatch(
            &self.claim,
            self.confirmed_revision,
            &self.transition_id,
            &self.execution,
            self.mode,
        )
        .map_err(|_| MachineApiWorkloadProvisionWireError::CommandIdentityEncoding)?;
        if self.command_id != expected_command_id {
            return Err(MachineApiWorkloadProvisionWireError::CommandIdentityMismatch);
        }
        if self.generation != attempt.generation() {
            return Err(MachineApiWorkloadProvisionWireError::GenerationMismatch);
        }
        if self.desired_digest != attempt.desired_digest() {
            return Err(MachineApiWorkloadProvisionWireError::DesiredDigestMismatch);
        }
        if self.source.source_digest() != attempt.source_digest() {
            return Err(MachineApiWorkloadProvisionWireError::SourceDigestMismatch);
        }
        if self
            .source
            .authenticate_executable(&self.executable)
            .is_err()
        {
            return Err(MachineApiWorkloadProvisionWireError::ExecutableSourceMismatch);
        }
        if self.network_plan_digest != attempt.network_plan_digest() {
            return Err(MachineApiWorkloadProvisionWireError::NetworkPlanDigestMismatch);
        }
        if self.execution.generation() != self.generation
            || self.execution.desired_digest() != self.desired_digest
            || self.execution.node_identity() != attempt.required_node()
        {
            return Err(MachineApiWorkloadProvisionWireError::ExecutionMismatch);
        }
        let plan = self.compiled_network_plan.plan();
        let plan_identity = self.compiled_network_plan.content().identity();
        if plan_identity.tenant_id() != attempt.key().tenant_id() {
            return Err(MachineApiWorkloadProvisionWireError::TenantMismatch);
        }
        if plan.generation().as_u64() != self.generation.as_u64() {
            return Err(MachineApiWorkloadProvisionWireError::GenerationMismatch);
        }
        if plan.digest() != self.network_plan_digest {
            return Err(MachineApiWorkloadProvisionWireError::NetworkPlanDigestMismatch);
        }
        let network_matches = |reference: &nimbus_workloads::WorkloadNetworkReference| {
            reference.plan_id() == plan.plan_id()
                && reference.generation() == plan.generation()
                && reference.digest() == plan.digest()
        };
        let subjects_match = match attempt.subjects() {
            WorkloadProvisionSubjects::Network(network) => network_matches(network),
            WorkloadProvisionSubjects::Execution(execution) => execution == &self.execution,
            WorkloadProvisionSubjects::Readiness { network, execution } => {
                network_matches(network) && execution == &self.execution
            }
            WorkloadProvisionSubjects::Publication(publication) => {
                network_matches(publication.network())
            }
        };
        if !subjects_match {
            return Err(MachineApiWorkloadProvisionWireError::SubjectMismatch);
        }
        if self.mode == WorkloadProvisionCommandMode::Execute
            && matches!(
                attempt.step(),
                WorkloadProvisionStep::InspectActivationPrerequisites
                    | WorkloadProvisionStep::InspectWorkloadReadiness
                    | WorkloadProvisionStep::ObservePublication
            )
        {
            return Err(MachineApiWorkloadProvisionWireError::InspectionOnlyStep);
        }
        Ok(())
    }

    pub fn command_id(&self) -> &WorkloadProvisionCommandId {
        &self.command_id
    }

    pub fn attempt_id(&self) -> &WorkloadProvisionAttemptId {
        &self.attempt_id
    }

    pub const fn dispatch_epoch(&self) -> WorkloadProvisionDispatchEpoch {
        self.dispatch_epoch
    }

    pub fn provider_target(&self) -> &WorkloadProvisionProviderTarget {
        &self.provider_target
    }

    pub fn claim(&self) -> &WorkloadProvisionDispatchClaim {
        &self.claim
    }

    pub const fn confirmed_revision(&self) -> WorkloadSagaRevision {
        self.confirmed_revision
    }

    pub fn transition_id(&self) -> &WorkloadSagaTransitionId {
        &self.transition_id
    }

    pub const fn generation(&self) -> WorkloadGeneration {
        self.generation
    }

    pub const fn desired_digest(&self) -> WorkloadDesiredDigest {
        self.desired_digest
    }

    pub const fn source_digest(&self) -> WorkloadProvisionSourceDigest {
        self.source.source_digest()
    }

    pub fn source(&self) -> &WorkloadProvisionSourceEvidence {
        &self.source
    }

    pub const fn network_plan_digest(&self) -> nimbus_network::NetworkPlanDigest {
        self.network_plan_digest
    }

    pub fn execution(&self) -> &WorkloadExecutionReference {
        &self.execution
    }

    pub fn executable(&self) -> &WorkloadExecutableIntent {
        &self.executable
    }

    pub fn compiled_network_plan(&self) -> &CompiledWorkloadNetworkPlan {
        &self.compiled_network_plan
    }

    pub const fn machine_provider_generation(&self) -> nimbus_network::NetworkResourceGeneration {
        self.machine_provider_generation
    }

    pub const fn mode(&self) -> WorkloadProvisionCommandMode {
        self.mode
    }
}

/// Authenticated Machine API request for one exact provision phase.
#[cfg(unix)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MachineApiWorkloadProvisionPhaseRequest {
    forwarder_authority: MachineForwarderAuthority,
    command: MachineApiWorkloadProvisionCommandEnvelope,
}

#[cfg(unix)]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MachineApiWorkloadProvisionPhaseRequestWire {
    forwarder_authority: MachineForwarderAuthority,
    command: MachineApiWorkloadProvisionCommandEnvelope,
}

#[cfg(unix)]
impl<'de> Deserialize<'de> for MachineApiWorkloadProvisionPhaseRequest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = MachineApiWorkloadProvisionPhaseRequestWire::deserialize(deserializer)?;
        Self::new(wire.forwarder_authority, wire.command).map_err(serde::de::Error::custom)
    }
}

#[cfg(unix)]
impl MachineApiWorkloadProvisionPhaseRequest {
    pub fn new(
        forwarder_authority: MachineForwarderAuthority,
        command: MachineApiWorkloadProvisionCommandEnvelope,
    ) -> Result<Self, MachineApiWorkloadProvisionWireError> {
        if forwarder_authority.generation() != command.machine_provider_generation {
            return Err(MachineApiWorkloadProvisionWireError::MachineProviderGenerationMismatch);
        }
        Ok(Self {
            forwarder_authority,
            command,
        })
    }

    pub fn forwarder_authority(&self) -> &MachineForwarderAuthority {
        &self.forwarder_authority
    }

    pub fn command(&self) -> &MachineApiWorkloadProvisionCommandEnvelope {
        &self.command
    }
}

/// Closed guest-owner observation with provider-specific opaque evidence.
#[cfg(unix)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum MachineApiWorkloadProvisionObservation {
    Succeeded { evidence: Vec<u8> },
    DefiniteFailure { evidence: Vec<u8> },
    Absent { evidence: Vec<u8> },
    InProgress { evidence: Vec<u8> },
    Ambiguous { evidence: Vec<u8> },
}

#[cfg(unix)]
impl MachineApiWorkloadProvisionObservation {
    pub fn evidence(&self) -> &[u8] {
        match self {
            Self::Succeeded { evidence }
            | Self::DefiniteFailure { evidence }
            | Self::Absent { evidence }
            | Self::InProgress { evidence }
            | Self::Ambiguous { evidence } => evidence,
        }
    }

    fn validate(&self) -> Result<(), MachineApiWorkloadProvisionWireError> {
        let size = self.evidence().len();
        if size > MAX_MACHINE_API_PROVISION_EVIDENCE_BYTES {
            return Err(MachineApiWorkloadProvisionWireError::EvidenceTooLarge {
                size,
                max: MAX_MACHINE_API_PROVISION_EVIDENCE_BYTES,
            });
        }
        Ok(())
    }
}

/// Guest response correlated to the complete command fence.
#[cfg(unix)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MachineApiWorkloadProvisionPhaseResponse {
    forwarder_authority: MachineForwarderAuthority,
    command_id: WorkloadProvisionCommandId,
    attempt_id: WorkloadProvisionAttemptId,
    dispatch_epoch: WorkloadProvisionDispatchEpoch,
    provider_target: WorkloadProvisionProviderTarget,
    observation: MachineApiWorkloadProvisionObservation,
}

#[cfg(unix)]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MachineApiWorkloadProvisionPhaseResponseWire {
    forwarder_authority: MachineForwarderAuthority,
    command_id: WorkloadProvisionCommandId,
    attempt_id: WorkloadProvisionAttemptId,
    dispatch_epoch: WorkloadProvisionDispatchEpoch,
    provider_target: WorkloadProvisionProviderTarget,
    observation: MachineApiWorkloadProvisionObservation,
}

#[cfg(unix)]
impl<'de> Deserialize<'de> for MachineApiWorkloadProvisionPhaseResponse {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = MachineApiWorkloadProvisionPhaseResponseWire::deserialize(deserializer)?;
        wire.observation
            .validate()
            .map_err(serde::de::Error::custom)?;
        Ok(Self {
            forwarder_authority: wire.forwarder_authority,
            command_id: wire.command_id,
            attempt_id: wire.attempt_id,
            dispatch_epoch: wire.dispatch_epoch,
            provider_target: wire.provider_target,
            observation: wire.observation,
        })
    }
}

#[cfg(unix)]
impl MachineApiWorkloadProvisionPhaseResponse {
    pub fn for_request(
        request: &MachineApiWorkloadProvisionPhaseRequest,
        observation: MachineApiWorkloadProvisionObservation,
    ) -> Result<Self, MachineApiWorkloadProvisionWireError> {
        observation.validate()?;
        let command = request.command();
        Ok(Self {
            forwarder_authority: request.forwarder_authority().clone(),
            command_id: command.command_id,
            attempt_id: command.attempt_id.clone(),
            dispatch_epoch: command.dispatch_epoch,
            provider_target: command.provider_target.clone(),
            observation,
        })
    }

    pub fn validate_for_request(
        &self,
        request: &MachineApiWorkloadProvisionPhaseRequest,
    ) -> Result<(), MachineApiWorkloadProvisionWireError> {
        let command = request.command();
        if self.forwarder_authority != *request.forwarder_authority() {
            return Err(MachineApiWorkloadProvisionWireError::ResponseAuthorityMismatch);
        }
        if self.command_id != command.command_id {
            return Err(MachineApiWorkloadProvisionWireError::ResponseCommandMismatch);
        }
        if self.attempt_id != command.attempt_id {
            return Err(MachineApiWorkloadProvisionWireError::ResponseAttemptMismatch);
        }
        if self.dispatch_epoch != command.dispatch_epoch {
            return Err(MachineApiWorkloadProvisionWireError::ResponseEpochMismatch);
        }
        if self.provider_target != command.provider_target {
            return Err(MachineApiWorkloadProvisionWireError::ResponseProviderTargetMismatch);
        }
        self.observation.validate()
    }

    pub fn command_id(&self) -> &WorkloadProvisionCommandId {
        &self.command_id
    }

    pub fn forwarder_authority(&self) -> &MachineForwarderAuthority {
        &self.forwarder_authority
    }

    pub fn attempt_id(&self) -> &WorkloadProvisionAttemptId {
        &self.attempt_id
    }

    pub const fn dispatch_epoch(&self) -> WorkloadProvisionDispatchEpoch {
        self.dispatch_epoch
    }

    pub fn provider_target(&self) -> &WorkloadProvisionProviderTarget {
        &self.provider_target
    }

    pub fn observation(&self) -> &MachineApiWorkloadProvisionObservation {
        &self.observation
    }
}

/// Stable failure reason for a rejected provision-phase wire value.
#[cfg(unix)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MachineApiWorkloadProvisionWireError {
    AttemptMismatch,
    DispatchEpochMismatch,
    ProviderTargetMismatch,
    ConfirmedRevisionMismatch,
    CommandIdentityMismatch,
    CommandIdentityEncoding,
    TenantMismatch,
    ExecutionMismatch,
    GenerationMismatch,
    DesiredDigestMismatch,
    SourceDigestMismatch,
    ExecutableSourceMismatch,
    NetworkPlanDigestMismatch,
    SubjectMismatch,
    MachineProviderGenerationMismatch,
    InspectionOnlyStep,
    EvidenceTooLarge { size: usize, max: usize },
    ResponseAuthorityMismatch,
    ResponseCommandMismatch,
    ResponseAttemptMismatch,
    ResponseEpochMismatch,
    ResponseProviderTargetMismatch,
}

#[cfg(unix)]
impl Display for MachineApiWorkloadProvisionWireError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::AttemptMismatch => "workload provision command attempt is crossed",
            Self::DispatchEpochMismatch => "workload provision command dispatch epoch is crossed",
            Self::ProviderTargetMismatch => "workload provision command provider target is crossed",
            Self::ConfirmedRevisionMismatch => {
                "workload provision command revision is crossed with its durable claim"
            }
            Self::CommandIdentityMismatch => {
                "workload provision command id does not bind its complete command"
            }
            Self::CommandIdentityEncoding => {
                "workload provision command identity cannot be encoded"
            }
            Self::TenantMismatch => "workload provision network plan belongs to another tenant",
            Self::ExecutionMismatch => "workload provision execution reference is crossed",
            Self::GenerationMismatch => "workload provision generation is crossed",
            Self::DesiredDigestMismatch => "workload provision desired digest is crossed",
            Self::SourceDigestMismatch => "workload provision source digest is crossed",
            Self::ExecutableSourceMismatch => {
                "workload provision executable is crossed with admitted source evidence"
            }
            Self::NetworkPlanDigestMismatch => "workload provision network plan digest is crossed",
            Self::SubjectMismatch => "workload provision subjects are crossed",
            Self::MachineProviderGenerationMismatch => {
                "workload provision machine provider generation is crossed"
            }
            Self::InspectionOnlyStep => {
                "workload provision inspection-only step cannot use execute mode"
            }
            Self::EvidenceTooLarge { size, max } => {
                return write!(
                    formatter,
                    "workload provision evidence contains {size} bytes; the limit is {max} bytes"
                );
            }
            Self::ResponseAuthorityMismatch => {
                "workload provision response forwarder authority is crossed"
            }
            Self::ResponseCommandMismatch => "workload provision response command id is crossed",
            Self::ResponseAttemptMismatch => "workload provision response attempt is crossed",
            Self::ResponseEpochMismatch => "workload provision response epoch is crossed",
            Self::ResponseProviderTargetMismatch => {
                "workload provision response provider target is crossed"
            }
        };
        formatter.write_str(message)
    }
}

#[cfg(unix)]
impl StdError for MachineApiWorkloadProvisionWireError {}

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
#[path = "api/tests.rs"]
mod tests;
