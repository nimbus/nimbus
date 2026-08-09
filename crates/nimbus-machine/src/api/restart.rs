//! Authenticated transport vocabulary for one compute-confirmed restart phase.

use std::error::Error as StdError;
use std::fmt::{self, Display, Formatter};

use nimbus_network::{NetworkPlanDigest, NetworkResourceGeneration};
use nimbus_workloads::{
    CompiledWorkloadNetworkPlan, WorkloadDesiredDigest, WorkloadExecutableIntent,
    WorkloadExecutionAttemptId, WorkloadExecutionProviderId, WorkloadExecutionReference,
    WorkloadGeneration, WorkloadInspectionVersion, WorkloadProvisionSourceEvidence,
    WorkloadRestartCommandClaim, WorkloadRestartCommandId, WorkloadRestartDispatchEpoch,
    WorkloadRestartEpoch, WorkloadRestartEvidenceDigest, WorkloadRestartRequestId,
    WorkloadRestartStep, WorkloadSagaId, WorkloadSagaKey, WorkloadSagaRevision,
    WorkloadSagaTransitionId,
};
use serde::{Deserialize, Serialize};

use crate::MachineForwarderAuthority;

const MACHINE_API_RESTART_REQUEST_DIGEST_DOMAIN: &[u8] =
    b"nimbus.machine.workload-restart.phase.request.v1\0";

/// Whether the guest may apply one exact effect or only inspect it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MachineApiWorkloadRestartCommandMode {
    Execute,
    Inspect,
}

/// Transport envelope for one restart command already confirmed by compute.
///
/// This value grants no saga, admission, scheduling, or retry authority. Its
/// constructor and deserializer reject crossed portable evidence before the
/// guest adapter can invoke one provider-owned phase.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MachineApiWorkloadRestartCommandEnvelope {
    command_id: WorkloadRestartCommandId,
    key: WorkloadSagaKey,
    saga_id: WorkloadSagaId,
    transition_id: WorkloadSagaTransitionId,
    generation: WorkloadGeneration,
    desired_digest: WorkloadDesiredDigest,
    source: WorkloadProvisionSourceEvidence,
    source_execution: WorkloadExecutionReference,
    execution: WorkloadExecutionReference,
    source_attempt_id: WorkloadExecutionAttemptId,
    attempt_id: WorkloadExecutionAttemptId,
    restart_epoch: WorkloadRestartEpoch,
    dispatch_epoch: WorkloadRestartDispatchEpoch,
    request_id: WorkloadRestartRequestId,
    issuing_revision: WorkloadSagaRevision,
    confirmed_revision: WorkloadSagaRevision,
    inspection_version: Option<WorkloadInspectionVersion>,
    provider_selection: WorkloadExecutionProviderId,
    step: WorkloadRestartStep,
    mode: MachineApiWorkloadRestartCommandMode,
    successor_veto_generation: Option<WorkloadGeneration>,
    claim: WorkloadRestartCommandClaim,
    executable: WorkloadExecutableIntent,
    network_plan_digest: NetworkPlanDigest,
    compiled_network_plan: CompiledWorkloadNetworkPlan,
    machine_forwarder_authority: MachineForwarderAuthority,
    machine_provider_generation: NetworkResourceGeneration,
}

fn deserialize_required_inspection_version<'de, D>(
    deserializer: D,
) -> Result<Option<WorkloadInspectionVersion>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::deserialize(deserializer)
}

fn deserialize_required_successor_veto_generation<'de, D>(
    deserializer: D,
) -> Result<Option<WorkloadGeneration>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::deserialize(deserializer)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MachineApiWorkloadRestartCommandEnvelopeWire {
    command_id: WorkloadRestartCommandId,
    key: WorkloadSagaKey,
    saga_id: WorkloadSagaId,
    transition_id: WorkloadSagaTransitionId,
    generation: WorkloadGeneration,
    desired_digest: WorkloadDesiredDigest,
    source: WorkloadProvisionSourceEvidence,
    source_execution: WorkloadExecutionReference,
    execution: WorkloadExecutionReference,
    source_attempt_id: WorkloadExecutionAttemptId,
    attempt_id: WorkloadExecutionAttemptId,
    restart_epoch: WorkloadRestartEpoch,
    dispatch_epoch: WorkloadRestartDispatchEpoch,
    request_id: WorkloadRestartRequestId,
    issuing_revision: WorkloadSagaRevision,
    confirmed_revision: WorkloadSagaRevision,
    #[serde(deserialize_with = "deserialize_required_inspection_version")]
    inspection_version: Option<WorkloadInspectionVersion>,
    provider_selection: WorkloadExecutionProviderId,
    step: WorkloadRestartStep,
    mode: MachineApiWorkloadRestartCommandMode,
    #[serde(deserialize_with = "deserialize_required_successor_veto_generation")]
    successor_veto_generation: Option<WorkloadGeneration>,
    claim: WorkloadRestartCommandClaim,
    executable: WorkloadExecutableIntent,
    network_plan_digest: NetworkPlanDigest,
    compiled_network_plan: CompiledWorkloadNetworkPlan,
    machine_forwarder_authority: MachineForwarderAuthority,
    machine_provider_generation: NetworkResourceGeneration,
}

impl<'de> Deserialize<'de> for MachineApiWorkloadRestartCommandEnvelope {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = MachineApiWorkloadRestartCommandEnvelopeWire::deserialize(deserializer)?;
        Self::new(
            wire.command_id,
            wire.key,
            wire.saga_id,
            wire.transition_id,
            wire.generation,
            wire.desired_digest,
            wire.source,
            wire.source_execution,
            wire.execution,
            wire.source_attempt_id,
            wire.attempt_id,
            wire.restart_epoch,
            wire.dispatch_epoch,
            wire.request_id,
            wire.issuing_revision,
            wire.confirmed_revision,
            wire.inspection_version,
            wire.provider_selection,
            wire.step,
            wire.mode,
            wire.successor_veto_generation,
            wire.claim,
            wire.executable,
            wire.network_plan_digest,
            wire.compiled_network_plan,
            wire.machine_forwarder_authority,
            wire.machine_provider_generation,
        )
        .map_err(serde::de::Error::custom)
    }
}

impl MachineApiWorkloadRestartCommandEnvelope {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        command_id: WorkloadRestartCommandId,
        key: WorkloadSagaKey,
        saga_id: WorkloadSagaId,
        transition_id: WorkloadSagaTransitionId,
        generation: WorkloadGeneration,
        desired_digest: WorkloadDesiredDigest,
        source: WorkloadProvisionSourceEvidence,
        source_execution: WorkloadExecutionReference,
        execution: WorkloadExecutionReference,
        source_attempt_id: WorkloadExecutionAttemptId,
        attempt_id: WorkloadExecutionAttemptId,
        restart_epoch: WorkloadRestartEpoch,
        dispatch_epoch: WorkloadRestartDispatchEpoch,
        request_id: WorkloadRestartRequestId,
        issuing_revision: WorkloadSagaRevision,
        confirmed_revision: WorkloadSagaRevision,
        inspection_version: Option<WorkloadInspectionVersion>,
        provider_selection: WorkloadExecutionProviderId,
        step: WorkloadRestartStep,
        mode: MachineApiWorkloadRestartCommandMode,
        successor_veto_generation: Option<WorkloadGeneration>,
        claim: WorkloadRestartCommandClaim,
        executable: WorkloadExecutableIntent,
        network_plan_digest: NetworkPlanDigest,
        compiled_network_plan: CompiledWorkloadNetworkPlan,
        machine_forwarder_authority: MachineForwarderAuthority,
        machine_provider_generation: NetworkResourceGeneration,
    ) -> Result<Self, MachineApiWorkloadRestartWireError> {
        let command = Self {
            command_id,
            key,
            saga_id,
            transition_id,
            generation,
            desired_digest,
            source,
            source_execution,
            execution,
            source_attempt_id,
            attempt_id,
            restart_epoch,
            dispatch_epoch,
            request_id,
            issuing_revision,
            confirmed_revision,
            inspection_version,
            provider_selection,
            step,
            mode,
            successor_veto_generation,
            claim,
            executable,
            network_plan_digest,
            compiled_network_plan,
            machine_forwarder_authority,
            machine_provider_generation,
        };
        command.validate()?;
        Ok(command)
    }

    fn validate(&self) -> Result<(), MachineApiWorkloadRestartWireError> {
        if self.key.saga_id() != self.saga_id {
            return Err(MachineApiWorkloadRestartWireError::SagaIdentityMismatch);
        }
        if self.command_id != *self.claim.command_id() {
            return Err(MachineApiWorkloadRestartWireError::CommandIdentityMismatch);
        }
        if self.request_id != *self.claim.request_id() {
            return Err(MachineApiWorkloadRestartWireError::RequestIdentityMismatch);
        }
        if self.attempt_id != *self.claim.attempt_id() {
            return Err(MachineApiWorkloadRestartWireError::ClaimAttemptMismatch);
        }
        if self.restart_epoch != self.claim.restart_epoch() {
            return Err(MachineApiWorkloadRestartWireError::ClaimRestartEpochMismatch);
        }
        if self.dispatch_epoch != self.claim.dispatch_epoch() {
            return Err(MachineApiWorkloadRestartWireError::ClaimDispatchEpochMismatch);
        }
        if self.issuing_revision != self.claim.issuing_revision() {
            return Err(MachineApiWorkloadRestartWireError::ClaimRevisionMismatch);
        }
        if self.step != self.claim.step() {
            return Err(MachineApiWorkloadRestartWireError::ClaimStepMismatch);
        }
        if (self.mode == MachineApiWorkloadRestartCommandMode::Execute
            && self.successor_veto_generation.is_some())
            || self
                .successor_veto_generation
                .is_some_and(|generation| generation <= self.generation)
        {
            return Err(MachineApiWorkloadRestartWireError::SuccessorVetoMismatch);
        }
        let inspection_revision = self
            .issuing_revision
            .checked_next()
            .and_then(WorkloadSagaRevision::checked_next);
        let revision_matches = match (self.mode, self.successor_veto_generation) {
            (MachineApiWorkloadRestartCommandMode::Execute, None) => {
                self.issuing_revision.checked_next() == Some(self.confirmed_revision)
            }
            (MachineApiWorkloadRestartCommandMode::Inspect, None) => {
                inspection_revision == Some(self.confirmed_revision)
            }
            (MachineApiWorkloadRestartCommandMode::Inspect, Some(_)) => {
                inspection_revision.is_some_and(|revision| revision <= self.confirmed_revision)
            }
            (MachineApiWorkloadRestartCommandMode::Execute, Some(_)) => false,
        };
        if !revision_matches {
            return Err(MachineApiWorkloadRestartWireError::ConfirmedRevisionMismatch);
        }
        if matches!(
            self.step,
            WorkloadRestartStep::InspectActivationPrerequisites
                | WorkloadRestartStep::InspectReadiness
                | WorkloadRestartStep::ObservePublication
        ) && self.mode != MachineApiWorkloadRestartCommandMode::Inspect
        {
            return Err(MachineApiWorkloadRestartWireError::InspectionOnlyStep);
        }
        if self.source.execution_provider_id() != &self.provider_selection {
            return Err(MachineApiWorkloadRestartWireError::ProviderSelectionMismatch);
        }
        if self
            .source
            .authenticate_executable(&self.executable)
            .is_err()
        {
            return Err(MachineApiWorkloadRestartWireError::ExecutableSourceMismatch);
        }
        if self.source_attempt_id != *self.source_execution.attempt_id() {
            return Err(MachineApiWorkloadRestartWireError::SourceAttemptMismatch);
        }
        if self.attempt_id != *self.execution.attempt_id() {
            return Err(MachineApiWorkloadRestartWireError::TargetAttemptMismatch);
        }
        if self.source_attempt_id == self.attempt_id
            || self.source_execution.execution_id() != self.execution.execution_id()
            || self.source_execution.workload_uid() != self.execution.workload_uid()
            || self.source_execution.node_identity() != self.execution.node_identity()
            || self.source_execution.restart_epoch().checked_next() != Some(self.restart_epoch)
            || self.execution.restart_epoch() != self.restart_epoch
        {
            return Err(MachineApiWorkloadRestartWireError::ExecutionAttemptMismatch);
        }
        if self.source_execution.generation() != self.generation
            || self.execution.generation() != self.generation
        {
            return Err(MachineApiWorkloadRestartWireError::GenerationMismatch);
        }
        if self.source_execution.desired_digest() != self.desired_digest
            || self.execution.desired_digest() != self.desired_digest
        {
            return Err(MachineApiWorkloadRestartWireError::DesiredDigestMismatch);
        }
        let plan = self.compiled_network_plan.plan();
        let plan_identity = self.compiled_network_plan.content().identity();
        if plan_identity.tenant_id() != self.key.tenant_id() {
            return Err(MachineApiWorkloadRestartWireError::TenantMismatch);
        }
        if plan.generation().as_u64() != self.generation.as_u64() {
            return Err(MachineApiWorkloadRestartWireError::GenerationMismatch);
        }
        if plan.digest() != self.network_plan_digest {
            return Err(MachineApiWorkloadRestartWireError::NetworkPlanDigestMismatch);
        }
        if let Some(inspection_version) = self.inspection_version
            && self.request_id
                != WorkloadRestartRequestId::for_automatic(&self.saga_id, inspection_version)
        {
            return Err(MachineApiWorkloadRestartWireError::InspectionVersionMismatch);
        }
        if self.machine_forwarder_authority.generation() != self.machine_provider_generation {
            return Err(MachineApiWorkloadRestartWireError::MachineProviderGenerationMismatch);
        }
        Ok(())
    }

    pub fn command_id(&self) -> &WorkloadRestartCommandId {
        &self.command_id
    }

    pub fn key(&self) -> &WorkloadSagaKey {
        &self.key
    }

    pub fn saga_id(&self) -> &WorkloadSagaId {
        &self.saga_id
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

    pub fn source(&self) -> &WorkloadProvisionSourceEvidence {
        &self.source
    }

    pub fn source_execution(&self) -> &WorkloadExecutionReference {
        &self.source_execution
    }

    pub fn execution(&self) -> &WorkloadExecutionReference {
        &self.execution
    }

    pub fn source_attempt_id(&self) -> &WorkloadExecutionAttemptId {
        &self.source_attempt_id
    }

    pub fn attempt_id(&self) -> &WorkloadExecutionAttemptId {
        &self.attempt_id
    }

    pub const fn restart_epoch(&self) -> WorkloadRestartEpoch {
        self.restart_epoch
    }

    pub const fn dispatch_epoch(&self) -> WorkloadRestartDispatchEpoch {
        self.dispatch_epoch
    }

    pub fn request_id(&self) -> &WorkloadRestartRequestId {
        &self.request_id
    }

    pub const fn issuing_revision(&self) -> WorkloadSagaRevision {
        self.issuing_revision
    }

    pub const fn confirmed_revision(&self) -> WorkloadSagaRevision {
        self.confirmed_revision
    }

    pub const fn inspection_version(&self) -> Option<WorkloadInspectionVersion> {
        self.inspection_version
    }

    pub fn provider_selection(&self) -> &WorkloadExecutionProviderId {
        &self.provider_selection
    }

    pub const fn step(&self) -> WorkloadRestartStep {
        self.step
    }

    pub const fn mode(&self) -> MachineApiWorkloadRestartCommandMode {
        self.mode
    }

    /// Later desired generation that permanently vetoed this command's effect.
    pub const fn successor_veto_generation(&self) -> Option<WorkloadGeneration> {
        self.successor_veto_generation
    }

    pub fn claim(&self) -> &WorkloadRestartCommandClaim {
        &self.claim
    }

    pub fn executable(&self) -> &WorkloadExecutableIntent {
        &self.executable
    }

    pub const fn network_plan_digest(&self) -> NetworkPlanDigest {
        self.network_plan_digest
    }

    pub fn compiled_network_plan(&self) -> &CompiledWorkloadNetworkPlan {
        &self.compiled_network_plan
    }

    pub fn machine_forwarder_authority(&self) -> &MachineForwarderAuthority {
        &self.machine_forwarder_authority
    }

    pub const fn machine_provider_generation(&self) -> NetworkResourceGeneration {
        self.machine_provider_generation
    }
}

/// Authenticated Machine API request for one exact restart phase.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MachineApiWorkloadRestartPhaseRequest {
    request_digest: MachineApiWorkloadRestartRequestDigest,
    forwarder_authority: MachineForwarderAuthority,
    command: MachineApiWorkloadRestartCommandEnvelope,
}

#[derive(Serialize)]
struct MachineApiWorkloadRestartPhaseRequestDigestPayload<'a> {
    forwarder_authority: &'a MachineForwarderAuthority,
    command: &'a MachineApiWorkloadRestartCommandEnvelope,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MachineApiWorkloadRestartPhaseRequestWire {
    request_digest: MachineApiWorkloadRestartRequestDigest,
    forwarder_authority: MachineForwarderAuthority,
    command: MachineApiWorkloadRestartCommandEnvelope,
}

impl<'de> Deserialize<'de> for MachineApiWorkloadRestartPhaseRequest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = MachineApiWorkloadRestartPhaseRequestWire::deserialize(deserializer)?;
        let expected_digest = wire.request_digest;
        let request =
            Self::new(wire.forwarder_authority, wire.command).map_err(serde::de::Error::custom)?;
        if request.request_digest != expected_digest {
            return Err(serde::de::Error::custom(
                MachineApiWorkloadRestartWireError::RequestDigestMismatch,
            ));
        }
        Ok(request)
    }
}

impl MachineApiWorkloadRestartPhaseRequest {
    pub fn new(
        forwarder_authority: MachineForwarderAuthority,
        command: MachineApiWorkloadRestartCommandEnvelope,
    ) -> Result<Self, MachineApiWorkloadRestartWireError> {
        if forwarder_authority != command.machine_forwarder_authority {
            return Err(MachineApiWorkloadRestartWireError::MachineForwarderAuthorityMismatch);
        }
        if forwarder_authority.generation() != command.machine_provider_generation {
            return Err(MachineApiWorkloadRestartWireError::MachineProviderGenerationMismatch);
        }
        let request_digest = Self::derive_request_digest(&forwarder_authority, &command)?;
        Ok(Self {
            request_digest,
            forwarder_authority,
            command,
        })
    }

    pub fn forwarder_authority(&self) -> &MachineForwarderAuthority {
        &self.forwarder_authority
    }

    pub fn command(&self) -> &MachineApiWorkloadRestartCommandEnvelope {
        &self.command
    }

    pub const fn request_digest(&self) -> MachineApiWorkloadRestartRequestDigest {
        self.request_digest
    }

    fn derive_request_digest(
        forwarder_authority: &MachineForwarderAuthority,
        command: &MachineApiWorkloadRestartCommandEnvelope,
    ) -> Result<MachineApiWorkloadRestartRequestDigest, MachineApiWorkloadRestartWireError> {
        let encoded = serde_json::to_vec(&MachineApiWorkloadRestartPhaseRequestDigestPayload {
            forwarder_authority,
            command,
        })
        .map_err(|_| MachineApiWorkloadRestartWireError::RequestEncoding)?;
        let mut preimage =
            Vec::with_capacity(MACHINE_API_RESTART_REQUEST_DIGEST_DOMAIN.len() + encoded.len());
        preimage.extend_from_slice(MACHINE_API_RESTART_REQUEST_DIGEST_DOMAIN);
        preimage.extend_from_slice(&encoded);
        Ok(MachineApiWorkloadRestartRequestDigest(
            WorkloadRestartEvidenceDigest::sha256(preimage),
        ))
    }
}

/// Stable digest of the complete authenticated restart request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MachineApiWorkloadRestartRequestDigest(WorkloadRestartEvidenceDigest);

impl Display for MachineApiWorkloadRestartRequestDigest {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Closed guest-owner observation for one exact restart command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum MachineApiWorkloadRestartObservation {
    Succeeded {
        evidence: WorkloadRestartEvidenceDigest,
    },
    AuthenticatedAbsent {
        evidence: WorkloadRestartEvidenceDigest,
    },
    DefiniteFailure {
        evidence: WorkloadRestartEvidenceDigest,
    },
    InProgress {
        evidence: WorkloadRestartEvidenceDigest,
    },
    Ambiguous,
}

/// Guest response bound to the complete authenticated restart request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MachineApiWorkloadRestartPhaseResponse {
    request_digest: MachineApiWorkloadRestartRequestDigest,
    forwarder_authority: MachineForwarderAuthority,
    command_id: WorkloadRestartCommandId,
    transition_id: WorkloadSagaTransitionId,
    request_id: WorkloadRestartRequestId,
    source_attempt_id: WorkloadExecutionAttemptId,
    attempt_id: WorkloadExecutionAttemptId,
    restart_epoch: WorkloadRestartEpoch,
    dispatch_epoch: WorkloadRestartDispatchEpoch,
    provider_selection: WorkloadExecutionProviderId,
    observation: MachineApiWorkloadRestartObservation,
}

impl MachineApiWorkloadRestartPhaseResponse {
    pub fn for_request(
        request: &MachineApiWorkloadRestartPhaseRequest,
        observation: MachineApiWorkloadRestartObservation,
    ) -> Result<Self, MachineApiWorkloadRestartWireError> {
        let command = request.command();
        Ok(Self {
            request_digest: request.request_digest(),
            forwarder_authority: request.forwarder_authority().clone(),
            command_id: command.command_id.clone(),
            transition_id: command.transition_id.clone(),
            request_id: command.request_id.clone(),
            source_attempt_id: command.source_attempt_id.clone(),
            attempt_id: command.attempt_id.clone(),
            restart_epoch: command.restart_epoch,
            dispatch_epoch: command.dispatch_epoch,
            provider_selection: command.provider_selection.clone(),
            observation,
        })
    }

    pub fn validate_for_request(
        &self,
        request: &MachineApiWorkloadRestartPhaseRequest,
    ) -> Result<(), MachineApiWorkloadRestartWireError> {
        let command = request.command();
        if self.request_digest != request.request_digest() {
            return Err(MachineApiWorkloadRestartWireError::ResponseRequestDigestMismatch);
        }
        if self.forwarder_authority != *request.forwarder_authority() {
            return Err(MachineApiWorkloadRestartWireError::ResponseAuthorityMismatch);
        }
        if self.command_id != command.command_id {
            return Err(MachineApiWorkloadRestartWireError::ResponseCommandMismatch);
        }
        if self.transition_id != command.transition_id {
            return Err(MachineApiWorkloadRestartWireError::ResponseTransitionMismatch);
        }
        if self.request_id != command.request_id {
            return Err(MachineApiWorkloadRestartWireError::ResponseRestartRequestMismatch);
        }
        if self.source_attempt_id != command.source_attempt_id {
            return Err(MachineApiWorkloadRestartWireError::ResponseSourceAttemptMismatch);
        }
        if self.attempt_id != command.attempt_id {
            return Err(MachineApiWorkloadRestartWireError::ResponseAttemptMismatch);
        }
        if self.restart_epoch != command.restart_epoch {
            return Err(MachineApiWorkloadRestartWireError::ResponseRestartEpochMismatch);
        }
        if self.dispatch_epoch != command.dispatch_epoch {
            return Err(MachineApiWorkloadRestartWireError::ResponseDispatchEpochMismatch);
        }
        if self.provider_selection != command.provider_selection {
            return Err(MachineApiWorkloadRestartWireError::ResponseProviderSelectionMismatch);
        }
        Ok(())
    }

    pub const fn request_digest(&self) -> MachineApiWorkloadRestartRequestDigest {
        self.request_digest
    }

    pub fn forwarder_authority(&self) -> &MachineForwarderAuthority {
        &self.forwarder_authority
    }

    pub fn command_id(&self) -> &WorkloadRestartCommandId {
        &self.command_id
    }

    pub fn transition_id(&self) -> &WorkloadSagaTransitionId {
        &self.transition_id
    }

    pub fn request_id(&self) -> &WorkloadRestartRequestId {
        &self.request_id
    }

    pub fn source_attempt_id(&self) -> &WorkloadExecutionAttemptId {
        &self.source_attempt_id
    }

    pub fn attempt_id(&self) -> &WorkloadExecutionAttemptId {
        &self.attempt_id
    }

    pub const fn restart_epoch(&self) -> WorkloadRestartEpoch {
        self.restart_epoch
    }

    pub const fn dispatch_epoch(&self) -> WorkloadRestartDispatchEpoch {
        self.dispatch_epoch
    }

    pub fn provider_selection(&self) -> &WorkloadExecutionProviderId {
        &self.provider_selection
    }

    pub fn observation(&self) -> &MachineApiWorkloadRestartObservation {
        &self.observation
    }
}

/// Stable failure reason for a rejected restart-phase wire value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MachineApiWorkloadRestartWireError {
    SagaIdentityMismatch,
    CommandIdentityMismatch,
    RequestIdentityMismatch,
    ClaimAttemptMismatch,
    ClaimRestartEpochMismatch,
    ClaimDispatchEpochMismatch,
    ClaimRevisionMismatch,
    ClaimStepMismatch,
    ConfirmedRevisionMismatch,
    SuccessorVetoMismatch,
    InspectionOnlyStep,
    ProviderSelectionMismatch,
    ExecutableSourceMismatch,
    SourceAttemptMismatch,
    TargetAttemptMismatch,
    ExecutionAttemptMismatch,
    GenerationMismatch,
    DesiredDigestMismatch,
    TenantMismatch,
    NetworkPlanDigestMismatch,
    InspectionVersionMismatch,
    MachineForwarderAuthorityMismatch,
    MachineProviderGenerationMismatch,
    RequestEncoding,
    RequestDigestMismatch,
    ResponseRequestDigestMismatch,
    ResponseAuthorityMismatch,
    ResponseCommandMismatch,
    ResponseTransitionMismatch,
    ResponseRestartRequestMismatch,
    ResponseSourceAttemptMismatch,
    ResponseAttemptMismatch,
    ResponseRestartEpochMismatch,
    ResponseDispatchEpochMismatch,
    ResponseProviderSelectionMismatch,
}

impl Display for MachineApiWorkloadRestartWireError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::SagaIdentityMismatch => "workload restart saga identity is crossed",
            Self::CommandIdentityMismatch => "workload restart command identity is crossed",
            Self::RequestIdentityMismatch => "workload restart request identity is crossed",
            Self::ClaimAttemptMismatch => "workload restart claim attempt is crossed",
            Self::ClaimRestartEpochMismatch => "workload restart claim epoch is crossed",
            Self::ClaimDispatchEpochMismatch => "workload restart dispatch epoch is crossed",
            Self::ClaimRevisionMismatch => "workload restart claim revision is crossed",
            Self::ClaimStepMismatch => "workload restart claim step is crossed",
            Self::ConfirmedRevisionMismatch => {
                "workload restart confirmation revision does not match command mode"
            }
            Self::SuccessorVetoMismatch => {
                "workload restart successor veto is crossed with command authority"
            }
            Self::InspectionOnlyStep => "workload restart inspection-only step cannot execute",
            Self::ProviderSelectionMismatch => {
                "workload restart provider selection is crossed with source evidence"
            }
            Self::ExecutableSourceMismatch => {
                "workload restart executable is crossed with source evidence"
            }
            Self::SourceAttemptMismatch => "workload restart source attempt is crossed",
            Self::TargetAttemptMismatch => "workload restart target attempt is crossed",
            Self::ExecutionAttemptMismatch => {
                "workload restart source and target executions are crossed"
            }
            Self::GenerationMismatch => "workload restart desired generation is crossed",
            Self::DesiredDigestMismatch => "workload restart desired digest is crossed",
            Self::TenantMismatch => "workload restart network plan belongs to another tenant",
            Self::NetworkPlanDigestMismatch => "workload restart network plan digest is crossed",
            Self::InspectionVersionMismatch => {
                "automatic workload restart request is crossed with its inspection version"
            }
            Self::MachineForwarderAuthorityMismatch => {
                "workload restart command is crossed with forwarder authority"
            }
            Self::MachineProviderGenerationMismatch => {
                "workload restart machine generation is crossed with forwarder authority"
            }
            Self::RequestEncoding => "workload restart request cannot be encoded",
            Self::RequestDigestMismatch => {
                "workload restart request digest does not bind the complete request"
            }
            Self::ResponseRequestDigestMismatch => {
                "workload restart response does not bind the complete request"
            }
            Self::ResponseAuthorityMismatch => {
                "workload restart response forwarder authority is crossed"
            }
            Self::ResponseCommandMismatch => "workload restart response command is crossed",
            Self::ResponseTransitionMismatch => "workload restart response transition is crossed",
            Self::ResponseRestartRequestMismatch => {
                "workload restart response request identity is crossed"
            }
            Self::ResponseSourceAttemptMismatch => {
                "workload restart response source attempt is crossed"
            }
            Self::ResponseAttemptMismatch => "workload restart response target attempt is crossed",
            Self::ResponseRestartEpochMismatch => {
                "workload restart response restart epoch is crossed"
            }
            Self::ResponseDispatchEpochMismatch => {
                "workload restart response dispatch epoch is crossed"
            }
            Self::ResponseProviderSelectionMismatch => {
                "workload restart response provider selection is crossed"
            }
        };
        formatter.write_str(message)
    }
}

impl StdError for MachineApiWorkloadRestartWireError {}
