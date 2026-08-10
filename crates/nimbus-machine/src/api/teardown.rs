//! Authenticated transport vocabulary for one compute-confirmed teardown phase.

use std::error::Error as StdError;
use std::fmt::{self, Display, Formatter};

use nimbus_network::{NetworkPlanDigest, NetworkResourceGeneration};
use nimbus_workloads::{
    CompiledWorkloadNetworkPlan, WorkloadExecutionReference, WorkloadFailureEvidence,
    WorkloadOwnerEvidenceDigest, WorkloadProvisionSourceEvidence, WorkloadSagaRevision,
    WorkloadSagaTransitionId, WorkloadTeardownAttemptId, WorkloadTeardownClaim,
    WorkloadTeardownCommandId, WorkloadTeardownCommandMode, WorkloadTeardownDispatchEpoch,
    WorkloadTeardownProviderTarget, WorkloadTeardownReceiptPrefix, WorkloadTeardownStep,
    WorkloadTeardownSubjects, WorkloadTeardownSuccessEvidence,
};
use serde::{Deserialize, Serialize};

use crate::MachineForwarderAuthority;

const MACHINE_API_TEARDOWN_REQUEST_DIGEST_DOMAIN: &[u8] =
    b"nimbus.machine.workload-teardown.phase.request.v1\0";

/// Closed mapping from a parent provider role to one guest-owned capability.
///
/// The value intentionally contains no provider ID. Trusted guest composition
/// selects the concrete provider behind each capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MachineApiWorkloadTeardownProviderTranslation {
    GuestExecutionComposition,
    GuestContainerAttachment,
}

/// Complete input for one strict teardown command envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineApiWorkloadTeardownCommandEnvelopeInput {
    pub command_id: WorkloadTeardownCommandId,
    pub confirmed_revision: WorkloadSagaRevision,
    pub confirmed_transition_id: WorkloadSagaTransitionId,
    pub source: WorkloadProvisionSourceEvidence,
    pub compiled_network_plan: CompiledWorkloadNetworkPlan,
    pub execution_locator: WorkloadExecutionReference,
    pub prior_receipt_prefix: WorkloadTeardownReceiptPrefix,
    pub mode: WorkloadTeardownCommandMode,
    pub claim: WorkloadTeardownClaim,
    pub machine_forwarder_authority: MachineForwarderAuthority,
    pub machine_provider_generation: NetworkResourceGeneration,
    pub provider_translation: MachineApiWorkloadTeardownProviderTranslation,
}

/// Transport envelope for one teardown command already confirmed by compute.
///
/// This value grants no saga, provider-selection, retry, or storage authority.
/// Its constructor and deserializer authenticate the complete portable command
/// before a guest adapter can inspect or invoke one provider-owned phase.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct MachineApiWorkloadTeardownCommandEnvelope {
    command_id: WorkloadTeardownCommandId,
    confirmed_revision: WorkloadSagaRevision,
    confirmed_transition_id: WorkloadSagaTransitionId,
    source: WorkloadProvisionSourceEvidence,
    compiled_network_plan: CompiledWorkloadNetworkPlan,
    execution_locator: WorkloadExecutionReference,
    prior_receipt_prefix: WorkloadTeardownReceiptPrefix,
    mode: WorkloadTeardownCommandMode,
    claim: WorkloadTeardownClaim,
    machine_forwarder_authority: MachineForwarderAuthority,
    machine_provider_generation: NetworkResourceGeneration,
    provider_translation: MachineApiWorkloadTeardownProviderTranslation,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct MachineApiWorkloadTeardownCommandEnvelopeWire {
    command_id: WorkloadTeardownCommandId,
    confirmed_revision: WorkloadSagaRevision,
    confirmed_transition_id: WorkloadSagaTransitionId,
    source: WorkloadProvisionSourceEvidence,
    compiled_network_plan: CompiledWorkloadNetworkPlan,
    execution_locator: WorkloadExecutionReference,
    prior_receipt_prefix: WorkloadTeardownReceiptPrefix,
    mode: WorkloadTeardownCommandMode,
    claim: WorkloadTeardownClaim,
    machine_forwarder_authority: MachineForwarderAuthority,
    machine_provider_generation: NetworkResourceGeneration,
    provider_translation: MachineApiWorkloadTeardownProviderTranslation,
}

impl<'de> Deserialize<'de> for MachineApiWorkloadTeardownCommandEnvelope {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = MachineApiWorkloadTeardownCommandEnvelopeWire::deserialize(deserializer)?;
        Self::new(MachineApiWorkloadTeardownCommandEnvelopeInput {
            command_id: wire.command_id,
            confirmed_revision: wire.confirmed_revision,
            confirmed_transition_id: wire.confirmed_transition_id,
            source: wire.source,
            compiled_network_plan: wire.compiled_network_plan,
            execution_locator: wire.execution_locator,
            prior_receipt_prefix: wire.prior_receipt_prefix,
            mode: wire.mode,
            claim: wire.claim,
            machine_forwarder_authority: wire.machine_forwarder_authority,
            machine_provider_generation: wire.machine_provider_generation,
            provider_translation: wire.provider_translation,
        })
        .map_err(serde::de::Error::custom)
    }
}

impl MachineApiWorkloadTeardownCommandEnvelope {
    pub fn new(
        input: MachineApiWorkloadTeardownCommandEnvelopeInput,
    ) -> Result<Self, MachineApiWorkloadTeardownWireError> {
        let command = Self {
            command_id: input.command_id,
            confirmed_revision: input.confirmed_revision,
            confirmed_transition_id: input.confirmed_transition_id,
            source: input.source,
            compiled_network_plan: input.compiled_network_plan,
            execution_locator: input.execution_locator,
            prior_receipt_prefix: input.prior_receipt_prefix,
            mode: input.mode,
            claim: input.claim,
            machine_forwarder_authority: input.machine_forwarder_authority,
            machine_provider_generation: input.machine_provider_generation,
            provider_translation: input.provider_translation,
        };
        command.validate()?;
        Ok(command)
    }

    fn validate(&self) -> Result<(), MachineApiWorkloadTeardownWireError> {
        let attempt = self.claim.attempt();
        let expected_command_id = WorkloadTeardownCommandId::for_confirmed_dispatch(
            &self.claim,
            self.confirmed_revision,
            &self.confirmed_transition_id,
            self.mode,
        )
        .map_err(|_| MachineApiWorkloadTeardownWireError::CommandIdentityMismatch)?;
        if self.command_id != expected_command_id {
            return Err(MachineApiWorkloadTeardownWireError::CommandIdentityMismatch);
        }
        let revision_matches = match self.mode {
            WorkloadTeardownCommandMode::Execute => {
                self.confirmed_revision == self.claim.claimed_revision()
            }
            WorkloadTeardownCommandMode::Inspect => {
                self.claim.claimed_revision().checked_next() == Some(self.confirmed_revision)
            }
        };
        if !revision_matches {
            return Err(MachineApiWorkloadTeardownWireError::ConfirmedRevisionMismatch);
        }
        if self.source.source_digest() != attempt.source_digest() {
            return Err(MachineApiWorkloadTeardownWireError::SourceDigestMismatch);
        }
        if self.source.execution_provider_id() != attempt.execution_provider_id() {
            return Err(MachineApiWorkloadTeardownWireError::ExecutionProviderMismatch);
        }

        let plan = self.compiled_network_plan.plan();
        let content = self.compiled_network_plan.content();
        if content.identity().tenant_id() != attempt.key().tenant_id() {
            return Err(MachineApiWorkloadTeardownWireError::TenantMismatch);
        }
        if plan.generation().as_u64() != attempt.generation().as_u64()
            || content.identity().generation().as_u64() != attempt.generation().as_u64()
        {
            return Err(MachineApiWorkloadTeardownWireError::GenerationMismatch);
        }
        if plan.digest() != attempt.network_plan_digest() {
            return Err(MachineApiWorkloadTeardownWireError::NetworkPlanDigestMismatch);
        }
        let Some(selection_evidence) = attempt.selection_evidence() else {
            return Err(MachineApiWorkloadTeardownWireError::CapabilitySelectionMismatch);
        };
        if content.capability_selection_evidence() != Some(selection_evidence)
            || content.capability_selection() != Some(selection_evidence.selection())
            || selection_evidence.selection().attachment_provider_id()
                == selection_evidence.selection().ingress_provider_id()
        {
            return Err(MachineApiWorkloadTeardownWireError::CapabilitySelectionMismatch);
        }
        if self.source.attachment_provider_id()
            != selection_evidence.selection().attachment_provider_id()
        {
            return Err(MachineApiWorkloadTeardownWireError::AttachmentProviderMismatch);
        }

        if self.execution_locator.node_identity() != attempt.required_node()
            || self.execution_locator.generation() != attempt.generation()
        {
            return Err(MachineApiWorkloadTeardownWireError::ExecutionLocatorMismatch);
        }
        if self.execution_locator.desired_digest() != attempt.desired_digest() {
            return Err(MachineApiWorkloadTeardownWireError::DesiredDigestMismatch);
        }
        match (attempt.step(), attempt.subjects()) {
            (
                WorkloadTeardownStep::DrainExecution | WorkloadTeardownStep::StopExecution,
                WorkloadTeardownSubjects::Execution(reference),
            ) if reference == &self.execution_locator => {}
            (
                WorkloadTeardownStep::DetachNetwork | WorkloadTeardownStep::ReleaseNetwork,
                WorkloadTeardownSubjects::Network(reference),
            ) if reference.plan_id() == plan.plan_id()
                && reference.generation() == plan.generation()
                && reference.digest() == plan.digest() => {}
            (WorkloadTeardownStep::WithdrawPublication, _) => {
                return Err(MachineApiWorkloadTeardownWireError::UnsupportedStep);
            }
            _ => return Err(MachineApiWorkloadTeardownWireError::SubjectsMismatch),
        }

        self.prior_receipt_prefix
            .validate_for_claim(&self.claim)
            .map_err(|_| MachineApiWorkloadTeardownWireError::PriorReceiptPrefixMismatch)?;
        let required_steps: &[WorkloadTeardownStep] = match attempt.step() {
            WorkloadTeardownStep::WithdrawPublication => &[],
            WorkloadTeardownStep::DrainExecution => &[WorkloadTeardownStep::WithdrawPublication],
            WorkloadTeardownStep::StopExecution => &[
                WorkloadTeardownStep::WithdrawPublication,
                WorkloadTeardownStep::DrainExecution,
            ],
            WorkloadTeardownStep::DetachNetwork => &[
                WorkloadTeardownStep::WithdrawPublication,
                WorkloadTeardownStep::DrainExecution,
                WorkloadTeardownStep::StopExecution,
            ],
            WorkloadTeardownStep::ReleaseNetwork => &[
                WorkloadTeardownStep::WithdrawPublication,
                WorkloadTeardownStep::DrainExecution,
                WorkloadTeardownStep::StopExecution,
                WorkloadTeardownStep::DetachNetwork,
            ],
        };
        if self.prior_receipt_prefix.receipts().len() != required_steps.len()
            || required_steps
                .iter()
                .any(|step| self.prior_receipt_prefix.receipt_for(*step).is_none())
            || self
                .prior_receipt_prefix
                .receipts()
                .iter()
                .any(|receipt| !self.prior_subject_matches(receipt.claim().attempt().subjects()))
        {
            return Err(MachineApiWorkloadTeardownWireError::PriorReceiptChainIncomplete);
        }

        let expected_translation = match attempt.step() {
            WorkloadTeardownStep::DrainExecution | WorkloadTeardownStep::StopExecution
                if matches!(
                    self.claim.provider_target(),
                    WorkloadTeardownProviderTarget::Execution { .. }
                ) =>
            {
                MachineApiWorkloadTeardownProviderTranslation::GuestExecutionComposition
            }
            WorkloadTeardownStep::DetachNetwork | WorkloadTeardownStep::ReleaseNetwork
                if matches!(
                    self.claim.provider_target(),
                    WorkloadTeardownProviderTarget::Attachment { .. }
                ) =>
            {
                MachineApiWorkloadTeardownProviderTranslation::GuestContainerAttachment
            }
            WorkloadTeardownStep::WithdrawPublication => {
                return Err(MachineApiWorkloadTeardownWireError::UnsupportedStep);
            }
            _ => return Err(MachineApiWorkloadTeardownWireError::ProviderTargetMismatch),
        };
        if self.provider_translation != expected_translation {
            return Err(MachineApiWorkloadTeardownWireError::ProviderTranslationMismatch);
        }
        if self.machine_forwarder_authority.generation() != self.machine_provider_generation {
            return Err(MachineApiWorkloadTeardownWireError::MachineProviderGenerationMismatch);
        }
        Ok(())
    }

    fn prior_subject_matches(&self, subjects: &WorkloadTeardownSubjects) -> bool {
        let plan = self.compiled_network_plan.plan();
        match subjects {
            WorkloadTeardownSubjects::Publication(reference) => {
                reference.execution() == &self.execution_locator
                    && reference.network().plan_id() == plan.plan_id()
                    && reference.network().generation() == plan.generation()
                    && reference.network().digest() == plan.digest()
            }
            WorkloadTeardownSubjects::Execution(reference) => reference == &self.execution_locator,
            WorkloadTeardownSubjects::Network(reference) => {
                reference.plan_id() == plan.plan_id()
                    && reference.generation() == plan.generation()
                    && reference.digest() == plan.digest()
            }
        }
    }

    pub const fn command_id(&self) -> WorkloadTeardownCommandId {
        self.command_id
    }

    pub const fn confirmed_revision(&self) -> WorkloadSagaRevision {
        self.confirmed_revision
    }

    pub fn confirmed_transition_id(&self) -> &WorkloadSagaTransitionId {
        &self.confirmed_transition_id
    }

    pub fn source(&self) -> &WorkloadProvisionSourceEvidence {
        &self.source
    }

    pub const fn source_digest(&self) -> nimbus_workloads::WorkloadProvisionSourceDigest {
        self.source.source_digest()
    }

    pub fn compiled_network_plan(&self) -> &CompiledWorkloadNetworkPlan {
        &self.compiled_network_plan
    }

    pub fn network_plan_digest(&self) -> NetworkPlanDigest {
        self.claim.attempt().network_plan_digest()
    }

    pub fn execution_locator(&self) -> &WorkloadExecutionReference {
        &self.execution_locator
    }

    pub fn prior_receipt_prefix(&self) -> &WorkloadTeardownReceiptPrefix {
        &self.prior_receipt_prefix
    }

    pub const fn mode(&self) -> WorkloadTeardownCommandMode {
        self.mode
    }

    pub fn claim(&self) -> &WorkloadTeardownClaim {
        &self.claim
    }

    pub fn attempt_id(&self) -> &WorkloadTeardownAttemptId {
        self.claim.attempt().attempt_id()
    }

    pub const fn dispatch_epoch(&self) -> WorkloadTeardownDispatchEpoch {
        self.claim.dispatch_epoch()
    }

    pub fn step(&self) -> WorkloadTeardownStep {
        self.claim.attempt().step()
    }

    pub fn subjects(&self) -> &WorkloadTeardownSubjects {
        self.claim.attempt().subjects()
    }

    pub fn provider_target(&self) -> &WorkloadTeardownProviderTarget {
        self.claim.provider_target()
    }

    pub fn machine_forwarder_authority(&self) -> &MachineForwarderAuthority {
        &self.machine_forwarder_authority
    }

    pub const fn machine_provider_generation(&self) -> NetworkResourceGeneration {
        self.machine_provider_generation
    }

    pub const fn provider_translation(&self) -> MachineApiWorkloadTeardownProviderTranslation {
        self.provider_translation
    }
}

/// Authenticated Machine API request for one exact teardown phase.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct MachineApiWorkloadTeardownPhaseRequest {
    request_digest: MachineApiWorkloadTeardownRequestDigest,
    forwarder_authority: MachineForwarderAuthority,
    command: MachineApiWorkloadTeardownCommandEnvelope,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MachineApiWorkloadTeardownPhaseRequestDigestPayload<'a> {
    forwarder_authority: &'a MachineForwarderAuthority,
    command: &'a MachineApiWorkloadTeardownCommandEnvelope,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct MachineApiWorkloadTeardownPhaseRequestWire {
    request_digest: MachineApiWorkloadTeardownRequestDigest,
    forwarder_authority: MachineForwarderAuthority,
    command: MachineApiWorkloadTeardownCommandEnvelope,
}

impl<'de> Deserialize<'de> for MachineApiWorkloadTeardownPhaseRequest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = MachineApiWorkloadTeardownPhaseRequestWire::deserialize(deserializer)?;
        let expected_digest = wire.request_digest;
        let request =
            Self::new(wire.forwarder_authority, wire.command).map_err(serde::de::Error::custom)?;
        if request.request_digest != expected_digest {
            return Err(serde::de::Error::custom(
                MachineApiWorkloadTeardownWireError::RequestDigestMismatch,
            ));
        }
        Ok(request)
    }
}

impl MachineApiWorkloadTeardownPhaseRequest {
    pub fn new(
        forwarder_authority: MachineForwarderAuthority,
        command: MachineApiWorkloadTeardownCommandEnvelope,
    ) -> Result<Self, MachineApiWorkloadTeardownWireError> {
        if forwarder_authority != command.machine_forwarder_authority {
            return Err(MachineApiWorkloadTeardownWireError::MachineForwarderAuthorityMismatch);
        }
        if forwarder_authority.generation() != command.machine_provider_generation {
            return Err(MachineApiWorkloadTeardownWireError::MachineProviderGenerationMismatch);
        }
        let request_digest = Self::derive_request_digest(&forwarder_authority, &command)?;
        Ok(Self {
            request_digest,
            forwarder_authority,
            command,
        })
    }

    fn derive_request_digest(
        forwarder_authority: &MachineForwarderAuthority,
        command: &MachineApiWorkloadTeardownCommandEnvelope,
    ) -> Result<MachineApiWorkloadTeardownRequestDigest, MachineApiWorkloadTeardownWireError> {
        let encoded = serde_json::to_vec(&MachineApiWorkloadTeardownPhaseRequestDigestPayload {
            forwarder_authority,
            command,
        })
        .map_err(|_| MachineApiWorkloadTeardownWireError::RequestEncoding)?;
        let mut preimage =
            Vec::with_capacity(MACHINE_API_TEARDOWN_REQUEST_DIGEST_DOMAIN.len() + encoded.len());
        preimage.extend_from_slice(MACHINE_API_TEARDOWN_REQUEST_DIGEST_DOMAIN);
        preimage.extend_from_slice(&encoded);
        Ok(MachineApiWorkloadTeardownRequestDigest(
            WorkloadOwnerEvidenceDigest::sha256(preimage),
        ))
    }

    pub const fn request_digest(&self) -> MachineApiWorkloadTeardownRequestDigest {
        self.request_digest
    }

    pub fn forwarder_authority(&self) -> &MachineForwarderAuthority {
        &self.forwarder_authority
    }

    pub fn command(&self) -> &MachineApiWorkloadTeardownCommandEnvelope {
        &self.command
    }
}

/// Stable digest of the complete authenticated teardown request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MachineApiWorkloadTeardownRequestDigest(WorkloadOwnerEvidenceDigest);

impl Display for MachineApiWorkloadTeardownRequestDigest {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Closed outcomes accepted from an effect-authorized Execute command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum MachineApiWorkloadTeardownExecuteObservation {
    Succeeded {
        evidence: Box<WorkloadTeardownSuccessEvidence>,
    },
    DefiniteFailure {
        failure: WorkloadFailureEvidence,
    },
    Ambiguous,
}

/// Closed outcomes accepted from a read-only Inspect command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum MachineApiWorkloadTeardownInspectObservation {
    Satisfied {
        evidence: Box<WorkloadTeardownSuccessEvidence>,
    },
    NotCompleted {
        evidence: WorkloadOwnerEvidenceDigest,
    },
    DefiniteFailure {
        failure: WorkloadFailureEvidence,
    },
    InProgress {
        evidence: WorkloadOwnerEvidenceDigest,
    },
    Ambiguous,
}

/// Mode-tagged observation. Cross-mode outcomes are not representable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "mode",
    content = "outcome",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum MachineApiWorkloadTeardownObservation {
    Execute(MachineApiWorkloadTeardownExecuteObservation),
    Inspect(MachineApiWorkloadTeardownInspectObservation),
}

impl MachineApiWorkloadTeardownObservation {
    pub const fn mode(&self) -> WorkloadTeardownCommandMode {
        match self {
            Self::Execute(_) => WorkloadTeardownCommandMode::Execute,
            Self::Inspect(_) => WorkloadTeardownCommandMode::Inspect,
        }
    }

    fn validate_for_command(
        &self,
        command: &MachineApiWorkloadTeardownCommandEnvelope,
    ) -> Result<(), MachineApiWorkloadTeardownWireError> {
        if self.mode() != command.mode() {
            return Err(MachineApiWorkloadTeardownWireError::ObservationModeMismatch);
        }
        let success = match self {
            Self::Execute(MachineApiWorkloadTeardownExecuteObservation::Succeeded { evidence })
            | Self::Inspect(MachineApiWorkloadTeardownInspectObservation::Satisfied { evidence }) => {
                Some(evidence)
            }
            _ => None,
        };
        if success.is_some_and(|evidence| {
            !evidence.matches_step_and_subjects(command.step(), command.subjects())
        }) {
            return Err(MachineApiWorkloadTeardownWireError::SuccessEvidenceMismatch);
        }
        Ok(())
    }
}

/// Guest response bound to the complete authenticated teardown request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct MachineApiWorkloadTeardownPhaseResponse {
    request_digest: MachineApiWorkloadTeardownRequestDigest,
    forwarder_authority: MachineForwarderAuthority,
    command_id: WorkloadTeardownCommandId,
    issuing_revision: WorkloadSagaRevision,
    issuing_transition_id: WorkloadSagaTransitionId,
    confirmed_revision: WorkloadSagaRevision,
    confirmed_transition_id: WorkloadSagaTransitionId,
    attempt_id: WorkloadTeardownAttemptId,
    dispatch_epoch: WorkloadTeardownDispatchEpoch,
    provider_target: WorkloadTeardownProviderTarget,
    provider_translation: MachineApiWorkloadTeardownProviderTranslation,
    step: WorkloadTeardownStep,
    subjects: WorkloadTeardownSubjects,
    mode: WorkloadTeardownCommandMode,
    observation: MachineApiWorkloadTeardownObservation,
}

impl MachineApiWorkloadTeardownPhaseResponse {
    pub fn for_request(
        request: &MachineApiWorkloadTeardownPhaseRequest,
        observation: MachineApiWorkloadTeardownObservation,
    ) -> Result<Self, MachineApiWorkloadTeardownWireError> {
        let command = request.command();
        observation.validate_for_command(command)?;
        Ok(Self {
            request_digest: request.request_digest(),
            forwarder_authority: request.forwarder_authority().clone(),
            command_id: command.command_id(),
            issuing_revision: command.claim.attempt().issuing_revision(),
            issuing_transition_id: command.claim.attempt().issuing_transition_id().clone(),
            confirmed_revision: command.confirmed_revision,
            confirmed_transition_id: command.confirmed_transition_id.clone(),
            attempt_id: command.attempt_id().clone(),
            dispatch_epoch: command.dispatch_epoch(),
            provider_target: command.provider_target().clone(),
            provider_translation: command.provider_translation,
            step: command.step(),
            subjects: command.subjects().clone(),
            mode: command.mode,
            observation,
        })
    }

    pub fn validate_for_request(
        &self,
        request: &MachineApiWorkloadTeardownPhaseRequest,
    ) -> Result<(), MachineApiWorkloadTeardownWireError> {
        let command = request.command();
        let attempt = command.claim.attempt();
        if self.request_digest != request.request_digest() {
            return Err(MachineApiWorkloadTeardownWireError::ResponseRequestDigestMismatch);
        }
        if self.forwarder_authority != *request.forwarder_authority() {
            return Err(MachineApiWorkloadTeardownWireError::ResponseAuthorityMismatch);
        }
        if self.command_id != command.command_id {
            return Err(MachineApiWorkloadTeardownWireError::ResponseCommandMismatch);
        }
        if self.issuing_revision != attempt.issuing_revision()
            || self.issuing_transition_id != *attempt.issuing_transition_id()
        {
            return Err(MachineApiWorkloadTeardownWireError::ResponseIssuingTransitionMismatch);
        }
        if self.confirmed_revision != command.confirmed_revision
            || self.confirmed_transition_id != command.confirmed_transition_id
        {
            return Err(MachineApiWorkloadTeardownWireError::ResponseConfirmedTransitionMismatch);
        }
        if self.attempt_id != *command.attempt_id() {
            return Err(MachineApiWorkloadTeardownWireError::ResponseAttemptMismatch);
        }
        if self.dispatch_epoch != command.dispatch_epoch() {
            return Err(MachineApiWorkloadTeardownWireError::ResponseDispatchEpochMismatch);
        }
        if self.provider_target != *command.provider_target() {
            return Err(MachineApiWorkloadTeardownWireError::ResponseProviderTargetMismatch);
        }
        if self.provider_translation != command.provider_translation {
            return Err(MachineApiWorkloadTeardownWireError::ResponseProviderTranslationMismatch);
        }
        if self.step != command.step() {
            return Err(MachineApiWorkloadTeardownWireError::ResponseStepMismatch);
        }
        if self.subjects != *command.subjects() {
            return Err(MachineApiWorkloadTeardownWireError::ResponseSubjectsMismatch);
        }
        if self.mode != command.mode {
            return Err(MachineApiWorkloadTeardownWireError::ResponseModeMismatch);
        }
        self.observation.validate_for_command(command)
    }

    pub const fn request_digest(&self) -> MachineApiWorkloadTeardownRequestDigest {
        self.request_digest
    }

    pub fn forwarder_authority(&self) -> &MachineForwarderAuthority {
        &self.forwarder_authority
    }

    pub const fn command_id(&self) -> WorkloadTeardownCommandId {
        self.command_id
    }

    pub const fn issuing_revision(&self) -> WorkloadSagaRevision {
        self.issuing_revision
    }

    pub fn issuing_transition_id(&self) -> &WorkloadSagaTransitionId {
        &self.issuing_transition_id
    }

    pub const fn confirmed_revision(&self) -> WorkloadSagaRevision {
        self.confirmed_revision
    }

    pub fn confirmed_transition_id(&self) -> &WorkloadSagaTransitionId {
        &self.confirmed_transition_id
    }

    pub fn attempt_id(&self) -> &WorkloadTeardownAttemptId {
        &self.attempt_id
    }

    pub const fn dispatch_epoch(&self) -> WorkloadTeardownDispatchEpoch {
        self.dispatch_epoch
    }

    pub fn provider_target(&self) -> &WorkloadTeardownProviderTarget {
        &self.provider_target
    }

    pub const fn provider_translation(&self) -> MachineApiWorkloadTeardownProviderTranslation {
        self.provider_translation
    }

    pub const fn step(&self) -> WorkloadTeardownStep {
        self.step
    }

    pub fn subjects(&self) -> &WorkloadTeardownSubjects {
        &self.subjects
    }

    pub const fn mode(&self) -> WorkloadTeardownCommandMode {
        self.mode
    }

    pub fn observation(&self) -> &MachineApiWorkloadTeardownObservation {
        &self.observation
    }
}

/// Stable failure reason for a rejected teardown-phase wire value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MachineApiWorkloadTeardownWireError {
    CommandIdentityMismatch,
    ConfirmedRevisionMismatch,
    SourceDigestMismatch,
    ExecutionProviderMismatch,
    TenantMismatch,
    GenerationMismatch,
    DesiredDigestMismatch,
    NetworkPlanDigestMismatch,
    CapabilitySelectionMismatch,
    AttachmentProviderMismatch,
    ExecutionLocatorMismatch,
    SubjectsMismatch,
    PriorReceiptPrefixMismatch,
    PriorReceiptChainIncomplete,
    UnsupportedStep,
    ProviderTargetMismatch,
    ProviderTranslationMismatch,
    MachineForwarderAuthorityMismatch,
    MachineProviderGenerationMismatch,
    RequestEncoding,
    RequestDigestMismatch,
    ObservationModeMismatch,
    SuccessEvidenceMismatch,
    ResponseRequestDigestMismatch,
    ResponseAuthorityMismatch,
    ResponseCommandMismatch,
    ResponseIssuingTransitionMismatch,
    ResponseConfirmedTransitionMismatch,
    ResponseAttemptMismatch,
    ResponseDispatchEpochMismatch,
    ResponseProviderTargetMismatch,
    ResponseProviderTranslationMismatch,
    ResponseStepMismatch,
    ResponseSubjectsMismatch,
    ResponseModeMismatch,
}

impl Display for MachineApiWorkloadTeardownWireError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::CommandIdentityMismatch => "workload teardown command identity is crossed",
            Self::ConfirmedRevisionMismatch => {
                "workload teardown confirmation revision does not match command mode"
            }
            Self::SourceDigestMismatch => "workload teardown source digest is crossed",
            Self::ExecutionProviderMismatch => "workload teardown execution provider is crossed",
            Self::TenantMismatch => "workload teardown network plan belongs to another tenant",
            Self::GenerationMismatch => "workload teardown desired generation is crossed",
            Self::DesiredDigestMismatch => "workload teardown desired digest is crossed",
            Self::NetworkPlanDigestMismatch => "workload teardown network plan digest is crossed",
            Self::CapabilitySelectionMismatch => {
                "workload teardown capability selection is missing or crossed"
            }
            Self::AttachmentProviderMismatch => "workload teardown attachment provider is crossed",
            Self::ExecutionLocatorMismatch => "workload teardown execution locator is crossed",
            Self::SubjectsMismatch => "workload teardown typed subjects are crossed",
            Self::PriorReceiptPrefixMismatch => "workload teardown prior receipt prefix is crossed",
            Self::PriorReceiptChainIncomplete => {
                "forwarded workload teardown requires the complete prior receipt chain"
            }
            Self::UnsupportedStep => {
                "publication withdrawal is parent-local and cannot enter the guest"
            }
            Self::ProviderTargetMismatch => "workload teardown parent provider target is crossed",
            Self::ProviderTranslationMismatch => {
                "workload teardown guest provider translation is crossed"
            }
            Self::MachineForwarderAuthorityMismatch => {
                "workload teardown command is crossed with forwarder authority"
            }
            Self::MachineProviderGenerationMismatch => {
                "workload teardown machine generation is crossed with forwarder authority"
            }
            Self::RequestEncoding => "workload teardown request cannot be encoded",
            Self::RequestDigestMismatch => {
                "workload teardown request digest does not bind the complete request"
            }
            Self::ObservationModeMismatch => {
                "workload teardown observation mode is crossed with its command"
            }
            Self::SuccessEvidenceMismatch => {
                "workload teardown success evidence is crossed with its command"
            }
            Self::ResponseRequestDigestMismatch => {
                "workload teardown response does not bind the complete request"
            }
            Self::ResponseAuthorityMismatch => {
                "workload teardown response forwarder authority is crossed"
            }
            Self::ResponseCommandMismatch => "workload teardown response command is crossed",
            Self::ResponseIssuingTransitionMismatch => {
                "workload teardown response issuing transition is crossed"
            }
            Self::ResponseConfirmedTransitionMismatch => {
                "workload teardown response confirmed transition is crossed"
            }
            Self::ResponseAttemptMismatch => "workload teardown response attempt is crossed",
            Self::ResponseDispatchEpochMismatch => {
                "workload teardown response dispatch epoch is crossed"
            }
            Self::ResponseProviderTargetMismatch => {
                "workload teardown response provider target is crossed"
            }
            Self::ResponseProviderTranslationMismatch => {
                "workload teardown response provider translation is crossed"
            }
            Self::ResponseStepMismatch => "workload teardown response step is crossed",
            Self::ResponseSubjectsMismatch => "workload teardown response subjects are crossed",
            Self::ResponseModeMismatch => "workload teardown response mode is crossed",
        };
        formatter.write_str(message)
    }
}

impl StdError for MachineApiWorkloadTeardownWireError {}

#[cfg(test)]
#[path = "teardown/tests.rs"]
mod tests;
