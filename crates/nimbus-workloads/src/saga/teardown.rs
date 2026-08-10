//! Portable teardown intent, evidence, and reducer vocabulary.

use nimbus_network::{NetworkCapabilitySelectionEvidence, NetworkPlanDigest};
use serde::{Deserialize, Deserializer, Serialize};

use super::*;

mod dispatch;

pub use dispatch::{
    WorkloadTeardownClaim, WorkloadTeardownCommandId, WorkloadTeardownCommandMode,
    WorkloadTeardownDispatchAuthorization, WorkloadTeardownDispatchEpoch,
    WorkloadTeardownInspectionResult, WorkloadTeardownProviderTarget,
    WorkloadTeardownRetryEvidence,
};

/// Durable proof of how one terminal teardown result was confirmed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum WorkloadTeardownResultConfirmation {
    Dispatch,
    Inspection {
        inspected_revision: WorkloadSagaRevision,
        inspected_transition_id: WorkloadSagaTransitionId,
        inspection_command_id: WorkloadTeardownCommandId,
    },
}

impl WorkloadTeardownResultConfirmation {
    pub(crate) const fn dispatch() -> Self {
        Self::Dispatch
    }

    pub(crate) fn for_inspection(
        record: &WorkloadSagaRecord,
        claim: &WorkloadTeardownClaim,
        inspection_command_id: WorkloadTeardownCommandId,
    ) -> Result<Self, WorkloadSagaError> {
        let confirmation = Self::Inspection {
            inspected_revision: record.revision(),
            inspected_transition_id: record.last_transition().transition_id().clone(),
            inspection_command_id,
        };
        if confirmation.matches_current(record, claim) {
            Ok(confirmation)
        } else {
            Err(WorkloadSagaError::InvalidEvidence(
                "teardown result confirmation is crossed with the current inspection command",
            ))
        }
    }

    pub(crate) fn validate_for_claim(
        &self,
        claim: &WorkloadTeardownClaim,
    ) -> Result<(), WorkloadSagaError> {
        match self {
            Self::Dispatch => Ok(()),
            Self::Inspection {
                inspected_revision,
                inspected_transition_id,
                inspection_command_id,
            } if *inspected_revision > claim.claimed_revision()
                && WorkloadTeardownCommandId::for_confirmed_dispatch(
                    claim,
                    *inspected_revision,
                    inspected_transition_id,
                    WorkloadTeardownCommandMode::Inspect,
                )
                .is_ok_and(|expected| expected == *inspection_command_id) =>
            {
                Ok(())
            }
            Self::Inspection { .. } => Err(WorkloadSagaError::InvalidEvidence(
                "teardown inspection confirmation is crossed with its durable claim",
            )),
        }
    }

    pub(crate) fn matches_current(
        &self,
        record: &WorkloadSagaRecord,
        claim: &WorkloadTeardownClaim,
    ) -> bool {
        match self {
            Self::Dispatch => matches!(
                record.teardown_disposition(),
                Some(WorkloadTeardownDisposition::DispatchPending { claim: retained, .. })
                    if retained == claim
            ),
            Self::Inspection {
                inspected_revision,
                inspected_transition_id,
                inspection_command_id,
            } => {
                matches!(
                    record.teardown_disposition(),
                    Some(WorkloadTeardownDisposition::InspectionRequired { claim: retained, .. })
                        if retained == claim
                ) && *inspected_revision == record.revision()
                    && inspected_transition_id == record.last_transition().transition_id()
                    && WorkloadTeardownCommandId::for_confirmed_dispatch(
                        claim,
                        record.revision(),
                        record.last_transition().transition_id(),
                        WorkloadTeardownCommandMode::Inspect,
                    )
                    .is_ok_and(|expected| expected == *inspection_command_id)
            }
        }
    }
}

/// Stable reason why an active workload generation must retire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum WorkloadTeardownCause {
    Successor {
        generation: WorkloadGeneration,
        desired_digest: WorkloadDesiredDigest,
    },
    FailedProvision {
        claim: Box<WorkloadProvisionDispatchClaim>,
        failure: WorkloadFailureEvidence,
    },
}

impl WorkloadTeardownCause {
    pub const fn successor_generation(&self) -> Option<WorkloadGeneration> {
        match self {
            Self::Successor { generation, .. } => Some(*generation),
            Self::FailedProvision { .. } => None,
        }
    }

    pub const fn successor_desired_digest(&self) -> Option<WorkloadDesiredDigest> {
        match self {
            Self::Successor { desired_digest, .. } => Some(*desired_digest),
            Self::FailedProvision { .. } => None,
        }
    }
}

/// Latest queued successor fence, distinct from the stable initiating cause.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WorkloadTeardownSuccessorFence {
    generation: WorkloadGeneration,
    desired_digest: WorkloadDesiredDigest,
}

impl WorkloadTeardownSuccessorFence {
    pub const fn new(
        generation: WorkloadGeneration,
        desired_digest: WorkloadDesiredDigest,
    ) -> Self {
        Self {
            generation,
            desired_digest,
        }
    }

    pub const fn generation(&self) -> WorkloadGeneration {
        self.generation
    }

    pub const fn desired_digest(&self) -> WorkloadDesiredDigest {
        self.desired_digest
    }
}

/// Exact issued restart result retained before teardown begins.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WorkloadRestartTeardownSettlement {
    claim: WorkloadRestartCommandClaim,
    result: WorkloadRestartEffectResult,
    source_execution: WorkloadExecutionReference,
    target_execution: WorkloadExecutionReference,
    owner_observations: Vec<WorkloadOwnerObservation>,
}

impl WorkloadRestartTeardownSettlement {
    pub fn new(
        claim: WorkloadRestartCommandClaim,
        result: WorkloadRestartEffectResult,
        source_execution: WorkloadExecutionReference,
        target_execution: WorkloadExecutionReference,
        owner_observations: Vec<WorkloadOwnerObservation>,
    ) -> Result<Self, WorkloadSagaError> {
        let settlement = Self {
            claim,
            result,
            source_execution,
            target_execution,
            owner_observations,
        };
        settlement.validate()?;
        Ok(settlement)
    }

    pub fn claim(&self) -> &WorkloadRestartCommandClaim {
        &self.claim
    }

    pub fn result(&self) -> &WorkloadRestartEffectResult {
        &self.result
    }

    pub fn source_execution(&self) -> &WorkloadExecutionReference {
        &self.source_execution
    }

    pub fn target_execution(&self) -> &WorkloadExecutionReference {
        &self.target_execution
    }

    pub fn owner_observations(&self) -> &[WorkloadOwnerObservation] {
        &self.owner_observations
    }

    pub(crate) fn validate(&self) -> Result<(), WorkloadSagaError> {
        self.claim.validate()?;
        if self.target_execution.attempt_id() != self.claim.attempt_id()
            || self.source_execution.attempt_id() == self.target_execution.attempt_id()
        {
            return Err(WorkloadSagaError::InvalidEvidence(
                "restart teardown settlement has crossed source or target execution identity",
            ));
        }
        if !matches!(
            self.result,
            WorkloadRestartEffectResult::Succeeded { .. }
                | WorkloadRestartEffectResult::AuthenticatedAbsent { .. }
                | WorkloadRestartEffectResult::Failed { .. }
        ) {
            return Err(WorkloadSagaError::InvalidEvidence(
                "restart teardown settlement requires a terminal result",
            ));
        }
        Ok(())
    }
}

/// Closed teardown operation selected by the pure workloads reducer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkloadTeardownStep {
    WithdrawPublication,
    DrainExecution,
    StopExecution,
    DetachNetwork,
    ReleaseNetwork,
}

impl WorkloadTeardownStep {
    pub const fn phases(self) -> (WorkloadSagaPhase, WorkloadSagaPhase) {
        match self {
            Self::WithdrawPublication => (
                WorkloadSagaPhase::WithdrawalCommitted,
                WorkloadSagaPhase::Withdrawn,
            ),
            Self::DrainExecution => (WorkloadSagaPhase::Withdrawn, WorkloadSagaPhase::Drained),
            Self::StopExecution => (
                WorkloadSagaPhase::Drained,
                WorkloadSagaPhase::WorkloadStopped,
            ),
            Self::DetachNetwork => (
                WorkloadSagaPhase::WorkloadStopped,
                WorkloadSagaPhase::NetworkDetached,
            ),
            Self::ReleaseNetwork => (
                WorkloadSagaPhase::NetworkDetached,
                WorkloadSagaPhase::NetworkReleased,
            ),
        }
    }

    pub(crate) const fn order(self) -> u8 {
        match self {
            Self::WithdrawPublication => 0,
            Self::DrainExecution => 1,
            Self::StopExecution => 2,
            Self::DetachNetwork => 3,
            Self::ReleaseNetwork => 4,
        }
    }
}

/// Exact typed subject for one teardown operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum WorkloadTeardownSubjects {
    Publication(WorkloadPublicationReference),
    Execution(WorkloadExecutionReference),
    Network(WorkloadNetworkReference),
}

/// Exact successful provider observation for one teardown step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum WorkloadTeardownSuccessEvidence {
    PublicationAbsent {
        reference: WorkloadPublicationReference,
        evidence: WorkloadOwnerEvidenceDigest,
    },
    ExecutionDrained {
        reference: WorkloadExecutionReference,
        evidence: WorkloadOwnerEvidenceDigest,
    },
    ExecutionStopped {
        reference: WorkloadExecutionReference,
        evidence: WorkloadOwnerEvidenceDigest,
    },
    NetworkDetached {
        reference: WorkloadNetworkReference,
        evidence: WorkloadOwnerEvidenceDigest,
    },
    NetworkReleased {
        reference: WorkloadNetworkReference,
        evidence: WorkloadOwnerEvidenceDigest,
    },
}

impl WorkloadTeardownSuccessEvidence {
    pub const fn step(&self) -> WorkloadTeardownStep {
        match self {
            Self::PublicationAbsent { .. } => WorkloadTeardownStep::WithdrawPublication,
            Self::ExecutionDrained { .. } => WorkloadTeardownStep::DrainExecution,
            Self::ExecutionStopped { .. } => WorkloadTeardownStep::StopExecution,
            Self::NetworkDetached { .. } => WorkloadTeardownStep::DetachNetwork,
            Self::NetworkReleased { .. } => WorkloadTeardownStep::ReleaseNetwork,
        }
    }

    pub(crate) fn matches_subjects(&self, subjects: &WorkloadTeardownSubjects) -> bool {
        matches!(
            (self, subjects),
            (
                Self::PublicationAbsent { reference, .. },
                WorkloadTeardownSubjects::Publication(expected)
            ) if reference == expected
        ) || matches!(
            (self, subjects),
            (
                Self::ExecutionDrained { reference, .. }
                    | Self::ExecutionStopped { reference, .. },
                WorkloadTeardownSubjects::Execution(expected)
            ) if reference == expected
        ) || matches!(
            (self, subjects),
            (
                Self::NetworkDetached { reference, .. }
                    | Self::NetworkReleased { reference, .. },
                WorkloadTeardownSubjects::Network(expected)
            ) if reference == expected
        )
    }

    pub(crate) fn terminal_observation(&self) -> WorkloadTerminalObservation {
        match self {
            Self::PublicationAbsent {
                reference,
                evidence,
            } => WorkloadTerminalObservation::PublicationAbsent {
                reference: reference.clone(),
                evidence: *evidence,
            },
            Self::ExecutionDrained {
                reference,
                evidence,
            } => WorkloadTerminalObservation::ExecutionDrained {
                reference: reference.clone(),
                evidence: *evidence,
            },
            Self::ExecutionStopped {
                reference,
                evidence,
            } => WorkloadTerminalObservation::ExecutionStopped {
                reference: reference.clone(),
                evidence: *evidence,
            },
            Self::NetworkDetached {
                reference,
                evidence,
            } => WorkloadTerminalObservation::NetworkDetached {
                reference: reference.clone(),
                evidence: *evidence,
            },
            Self::NetworkReleased {
                reference,
                evidence,
            } => WorkloadTerminalObservation::NetworkReleased {
                reference: reference.clone(),
                evidence: *evidence,
            },
        }
    }
}

/// Complete semantic payload from which a teardown attempt ID is derived.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadTeardownAttemptInput {
    pub key: WorkloadSagaKey,
    pub saga_id: WorkloadSagaId,
    pub issuing_revision: WorkloadSagaRevision,
    pub issuing_transition_id: WorkloadSagaTransitionId,
    pub generation: WorkloadGeneration,
    pub desired_digest: WorkloadDesiredDigest,
    pub required_node: NodeIdentity,
    pub source_digest: WorkloadProvisionSourceDigest,
    pub execution_provider_id: WorkloadExecutionProviderId,
    pub network_plan_digest: NetworkPlanDigest,
    pub selection_evidence: Option<NetworkCapabilitySelectionEvidence>,
    pub cause: WorkloadTeardownCause,
    pub successor_fence: Option<WorkloadTeardownSuccessorFence>,
    pub source_phase: WorkloadSagaPhase,
    pub target_phase: WorkloadSagaPhase,
    pub step: WorkloadTeardownStep,
    pub subjects: WorkloadTeardownSubjects,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkloadTeardownAttemptIdentity<'a> {
    key: &'a WorkloadSagaKey,
    saga_id: &'a WorkloadSagaId,
    issuing_revision: WorkloadSagaRevision,
    issuing_transition_id: &'a WorkloadSagaTransitionId,
    generation: WorkloadGeneration,
    desired_digest: WorkloadDesiredDigest,
    required_node: &'a NodeIdentity,
    source_digest: WorkloadProvisionSourceDigest,
    execution_provider_id: &'a WorkloadExecutionProviderId,
    network_plan_digest: NetworkPlanDigest,
    selection_evidence: &'a Option<NetworkCapabilitySelectionEvidence>,
    cause: &'a WorkloadTeardownCause,
    successor_fence: &'a Option<WorkloadTeardownSuccessorFence>,
    source_phase: WorkloadSagaPhase,
    target_phase: WorkloadSagaPhase,
    step: WorkloadTeardownStep,
    subjects: &'a WorkloadTeardownSubjects,
}

/// Stable identity of one exact teardown attempt.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct WorkloadTeardownAttemptId(String);

impl WorkloadTeardownAttemptId {
    const PREFIX: &'static str = "wtd";

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for WorkloadTeardownAttemptId {
    type Error = WorkloadSagaError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        validate_id(&value, Self::PREFIX)?;
        Ok(Self(value))
    }
}

impl From<WorkloadTeardownAttemptId> for String {
    fn from(value: WorkloadTeardownAttemptId) -> Self {
        value.0
    }
}

/// Durable compute-proposed teardown attempt. The value grants no effect authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadTeardownAttempt {
    attempt_id: WorkloadTeardownAttemptId,
    key: WorkloadSagaKey,
    saga_id: WorkloadSagaId,
    issuing_revision: WorkloadSagaRevision,
    issuing_transition_id: WorkloadSagaTransitionId,
    generation: WorkloadGeneration,
    desired_digest: WorkloadDesiredDigest,
    required_node: NodeIdentity,
    source_digest: WorkloadProvisionSourceDigest,
    execution_provider_id: WorkloadExecutionProviderId,
    network_plan_digest: NetworkPlanDigest,
    selection_evidence: Option<NetworkCapabilitySelectionEvidence>,
    cause: WorkloadTeardownCause,
    successor_fence: Option<WorkloadTeardownSuccessorFence>,
    source_phase: WorkloadSagaPhase,
    target_phase: WorkloadSagaPhase,
    step: WorkloadTeardownStep,
    subjects: WorkloadTeardownSubjects,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct WorkloadTeardownAttemptWire {
    attempt_id: WorkloadTeardownAttemptId,
    key: WorkloadSagaKey,
    saga_id: WorkloadSagaId,
    issuing_revision: WorkloadSagaRevision,
    issuing_transition_id: WorkloadSagaTransitionId,
    generation: WorkloadGeneration,
    desired_digest: WorkloadDesiredDigest,
    required_node: NodeIdentity,
    source_digest: WorkloadProvisionSourceDigest,
    execution_provider_id: WorkloadExecutionProviderId,
    network_plan_digest: NetworkPlanDigest,
    selection_evidence: Option<NetworkCapabilitySelectionEvidence>,
    cause: WorkloadTeardownCause,
    successor_fence: Option<WorkloadTeardownSuccessorFence>,
    source_phase: WorkloadSagaPhase,
    target_phase: WorkloadSagaPhase,
    step: WorkloadTeardownStep,
    subjects: WorkloadTeardownSubjects,
}

impl<'de> Deserialize<'de> for WorkloadTeardownAttempt {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = WorkloadTeardownAttemptWire::deserialize(deserializer)?;
        let expected = wire.attempt_id;
        let attempt = Self::new(WorkloadTeardownAttemptInput {
            key: wire.key,
            saga_id: wire.saga_id,
            issuing_revision: wire.issuing_revision,
            issuing_transition_id: wire.issuing_transition_id,
            generation: wire.generation,
            desired_digest: wire.desired_digest,
            required_node: wire.required_node,
            source_digest: wire.source_digest,
            execution_provider_id: wire.execution_provider_id,
            network_plan_digest: wire.network_plan_digest,
            selection_evidence: wire.selection_evidence,
            cause: wire.cause,
            successor_fence: wire.successor_fence,
            source_phase: wire.source_phase,
            target_phase: wire.target_phase,
            step: wire.step,
            subjects: wire.subjects,
        })
        .map_err(serde::de::Error::custom)?;
        if attempt.attempt_id != expected {
            return Err(serde::de::Error::custom(
                "workload teardown attempt id does not bind its complete payload",
            ));
        }
        Ok(attempt)
    }
}

impl WorkloadTeardownAttempt {
    pub fn new(input: WorkloadTeardownAttemptInput) -> Result<Self, WorkloadSagaError> {
        validate_attempt_input(&input)?;
        let encoded = serde_json::to_vec(&WorkloadTeardownAttemptIdentity {
            key: &input.key,
            saga_id: &input.saga_id,
            issuing_revision: input.issuing_revision,
            issuing_transition_id: &input.issuing_transition_id,
            generation: input.generation,
            desired_digest: input.desired_digest,
            required_node: &input.required_node,
            source_digest: input.source_digest,
            execution_provider_id: &input.execution_provider_id,
            network_plan_digest: input.network_plan_digest,
            selection_evidence: &input.selection_evidence,
            cause: &input.cause,
            successor_fence: &input.successor_fence,
            source_phase: input.source_phase,
            target_phase: input.target_phase,
            step: input.step,
            subjects: &input.subjects,
        })
        .map_err(|_| WorkloadSagaError::InvalidEvidence("teardown attempt cannot be encoded"))?;
        let canonical = std::str::from_utf8(&encoded)
            .map_err(|_| WorkloadSagaError::InvalidEvidence("teardown attempt is not UTF-8"))?;
        Ok(Self {
            attempt_id: WorkloadTeardownAttemptId(derive_id(
                WorkloadTeardownAttemptId::PREFIX,
                b"nimbus.workloads.teardown.attempt.id.v1",
                &[canonical],
            )),
            key: input.key,
            saga_id: input.saga_id,
            issuing_revision: input.issuing_revision,
            issuing_transition_id: input.issuing_transition_id,
            generation: input.generation,
            desired_digest: input.desired_digest,
            required_node: input.required_node,
            source_digest: input.source_digest,
            execution_provider_id: input.execution_provider_id,
            network_plan_digest: input.network_plan_digest,
            selection_evidence: input.selection_evidence,
            cause: input.cause,
            successor_fence: input.successor_fence,
            source_phase: input.source_phase,
            target_phase: input.target_phase,
            step: input.step,
            subjects: input.subjects,
        })
    }

    pub fn attempt_id(&self) -> &WorkloadTeardownAttemptId {
        &self.attempt_id
    }

    pub fn key(&self) -> &WorkloadSagaKey {
        &self.key
    }

    pub fn saga_id(&self) -> &WorkloadSagaId {
        &self.saga_id
    }

    pub const fn issuing_revision(&self) -> WorkloadSagaRevision {
        self.issuing_revision
    }

    pub fn issuing_transition_id(&self) -> &WorkloadSagaTransitionId {
        &self.issuing_transition_id
    }

    pub const fn generation(&self) -> WorkloadGeneration {
        self.generation
    }

    pub const fn desired_digest(&self) -> WorkloadDesiredDigest {
        self.desired_digest
    }

    pub fn required_node(&self) -> &NodeIdentity {
        &self.required_node
    }

    pub const fn source_digest(&self) -> WorkloadProvisionSourceDigest {
        self.source_digest
    }

    pub fn execution_provider_id(&self) -> &WorkloadExecutionProviderId {
        &self.execution_provider_id
    }

    pub const fn network_plan_digest(&self) -> NetworkPlanDigest {
        self.network_plan_digest
    }

    pub fn selection_evidence(&self) -> Option<&NetworkCapabilitySelectionEvidence> {
        self.selection_evidence.as_ref()
    }

    pub fn cause(&self) -> &WorkloadTeardownCause {
        &self.cause
    }

    pub const fn successor_fence(&self) -> Option<WorkloadTeardownSuccessorFence> {
        self.successor_fence
    }

    pub const fn source_phase(&self) -> WorkloadSagaPhase {
        self.source_phase
    }

    pub const fn target_phase(&self) -> WorkloadSagaPhase {
        self.target_phase
    }

    pub const fn step(&self) -> WorkloadTeardownStep {
        self.step
    }

    pub fn subjects(&self) -> &WorkloadTeardownSubjects {
        &self.subjects
    }
}

fn validate_attempt_input(input: &WorkloadTeardownAttemptInput) -> Result<(), WorkloadSagaError> {
    if input.saga_id != input.key.saga_id() {
        return Err(WorkloadSagaError::InvalidIdentity(
            "teardown attempt saga id does not match its workload key",
        ));
    }
    if input.step.phases() != (input.source_phase, input.target_phase) {
        return Err(WorkloadSagaError::InvalidTransition(
            "teardown step does not match its source and target phases",
        ));
    }
    let subjects_valid = matches!(
        (input.step, &input.subjects),
        (
            WorkloadTeardownStep::WithdrawPublication,
            WorkloadTeardownSubjects::Publication(_)
        ) | (
            WorkloadTeardownStep::DrainExecution | WorkloadTeardownStep::StopExecution,
            WorkloadTeardownSubjects::Execution(_)
        ) | (
            WorkloadTeardownStep::DetachNetwork | WorkloadTeardownStep::ReleaseNetwork,
            WorkloadTeardownSubjects::Network(_)
        )
    );
    if !subjects_valid {
        return Err(WorkloadSagaError::InvalidEvidence(
            "teardown step does not match its typed subject",
        ));
    }
    match &input.subjects {
        WorkloadTeardownSubjects::Publication(reference) => {
            if reference.execution().generation() != input.generation
                || reference.execution().desired_digest() != input.desired_digest
                || reference.execution().node_identity() != &input.required_node
                || reference.network().digest() != input.network_plan_digest
            {
                return Err(WorkloadSagaError::InvalidEvidence(
                    "teardown publication subject is crossed with active identity",
                ));
            }
        }
        WorkloadTeardownSubjects::Execution(reference) => {
            if reference.generation() != input.generation
                || reference.desired_digest() != input.desired_digest
                || reference.node_identity() != &input.required_node
            {
                return Err(WorkloadSagaError::InvalidEvidence(
                    "teardown execution subject is crossed with active identity",
                ));
            }
        }
        WorkloadTeardownSubjects::Network(reference) => {
            if reference.digest() != input.network_plan_digest {
                return Err(WorkloadSagaError::InvalidEvidence(
                    "teardown network subject is crossed with active plan",
                ));
            }
        }
    }
    Ok(())
}

/// Exact successful receipt retained across later teardown steps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WorkloadTeardownReceipt {
    claim: WorkloadTeardownClaim,
    evidence: WorkloadTeardownSuccessEvidence,
    confirmation: WorkloadTeardownResultConfirmation,
}

/// Exact provision absence that fenced a pending effect before teardown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WorkloadProvisionTeardownAbsence {
    claim: WorkloadProvisionDispatchClaim,
    evidence: WorkloadProvisionAbsenceEvidence,
}

impl WorkloadProvisionTeardownAbsence {
    pub(crate) fn new(
        claim: WorkloadProvisionDispatchClaim,
        evidence: WorkloadProvisionAbsenceEvidence,
    ) -> Result<Self, WorkloadSagaError> {
        let absence = Self { claim, evidence };
        absence.validate()?;
        Ok(absence)
    }

    pub fn claim(&self) -> &WorkloadProvisionDispatchClaim {
        &self.claim
    }

    pub fn evidence(&self) -> &WorkloadProvisionAbsenceEvidence {
        &self.evidence
    }

    pub(crate) fn validate(&self) -> Result<(), WorkloadSagaError> {
        if self.evidence.matches_claim(&self.claim) {
            Ok(())
        } else {
            Err(WorkloadSagaError::InvalidEvidence(
                "provision teardown absence is crossed with its inspected claim",
            ))
        }
    }
}

impl WorkloadTeardownReceipt {
    pub(crate) fn new(
        claim: WorkloadTeardownClaim,
        evidence: WorkloadTeardownSuccessEvidence,
        confirmation: WorkloadTeardownResultConfirmation,
    ) -> Result<Self, WorkloadSagaError> {
        let receipt = Self {
            claim,
            evidence,
            confirmation,
        };
        receipt.validate()?;
        Ok(receipt)
    }

    pub fn claim(&self) -> &WorkloadTeardownClaim {
        &self.claim
    }

    pub fn evidence(&self) -> &WorkloadTeardownSuccessEvidence {
        &self.evidence
    }

    pub fn confirmation(&self) -> &WorkloadTeardownResultConfirmation {
        &self.confirmation
    }

    pub(crate) fn validate(&self) -> Result<(), WorkloadSagaError> {
        self.claim.validate()?;
        self.confirmation.validate_for_claim(&self.claim)?;
        if self.evidence.step() != self.claim.attempt().step()
            || !self
                .evidence
                .matches_subjects(self.claim.attempt().subjects())
        {
            return Err(WorkloadSagaError::InvalidEvidence(
                "teardown receipt is crossed with its durable claim",
            ));
        }
        Ok(())
    }
}

/// Portable projection of the exact ordered receipts committed before one
/// teardown command.
///
/// The durable authority remains [`WorkloadTeardownContext`]. This bounded
/// value carries only the authenticated history that an effect provider needs
/// to enforce cross-phase ordering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadTeardownReceiptPrefix {
    receipts: Vec<WorkloadTeardownReceipt>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct WorkloadTeardownReceiptPrefixWire {
    receipts: Vec<WorkloadTeardownReceipt>,
}

impl<'de> Deserialize<'de> for WorkloadTeardownReceiptPrefix {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = WorkloadTeardownReceiptPrefixWire::deserialize(deserializer)?;
        Self::from_receipts(wire.receipts).map_err(serde::de::Error::custom)
    }
}

impl WorkloadTeardownReceiptPrefix {
    pub(crate) fn for_claim(
        receipts: &[WorkloadTeardownReceipt],
        claim: &WorkloadTeardownClaim,
    ) -> Result<Self, WorkloadSagaError> {
        let prefix = Self::from_receipts(receipts.to_vec())?;
        prefix.validate_for_claim(claim)?;
        Ok(prefix)
    }

    fn from_receipts(receipts: Vec<WorkloadTeardownReceipt>) -> Result<Self, WorkloadSagaError> {
        let prefix = Self { receipts };
        prefix.validate_ordered_history()?;
        Ok(prefix)
    }

    pub fn receipts(&self) -> &[WorkloadTeardownReceipt] {
        &self.receipts
    }

    pub fn receipt_for(&self, step: WorkloadTeardownStep) -> Option<&WorkloadTeardownReceipt> {
        self.receipts
            .iter()
            .find(|receipt| receipt.claim().attempt().step() == step)
    }

    /// Validate this history as the ordered prefix for `claim`.
    ///
    /// Resource-free phases intentionally leave gaps. Exact equality with the
    /// durable context is checked by the record projection and command fence.
    pub fn validate_for_claim(
        &self,
        claim: &WorkloadTeardownClaim,
    ) -> Result<(), WorkloadSagaError> {
        claim.validate()?;
        self.validate_ordered_history()?;
        let current = claim.attempt();
        for receipt in &self.receipts {
            let prior_claim = receipt.claim();
            if prior_claim.attempt().step().order() >= current.step().order()
                || prior_claim.claimed_revision() >= claim.claimed_revision()
                || prior_claim.attempt().issuing_revision() >= current.issuing_revision()
                || prior_claim.claimed_revision() >= current.issuing_revision()
                || !same_teardown_lifecycle(prior_claim.attempt(), current)
                || !successor_fence_precedes_or_equals(
                    prior_claim.attempt().successor_fence(),
                    current.successor_fence(),
                )
            {
                return Err(WorkloadSagaError::InvalidEvidence(
                    "teardown receipt prefix is stale, crossed, or not prior to its claim",
                ));
            }
        }
        Ok(())
    }

    fn validate_ordered_history(&self) -> Result<(), WorkloadSagaError> {
        if self.receipts.len() > WorkloadTeardownStep::ReleaseNetwork.order() as usize {
            return Err(WorkloadSagaError::InvalidEvidence(
                "teardown receipt prefix exceeds the closed teardown step set",
            ));
        }
        for receipt in &self.receipts {
            receipt.validate()?;
        }
        for pair in self.receipts.windows(2) {
            let previous = pair[0].claim();
            let next = pair[1].claim();
            if previous.attempt().step().order() >= next.attempt().step().order()
                || previous.claimed_revision() >= next.claimed_revision()
                || previous.attempt().issuing_revision() >= next.attempt().issuing_revision()
                || previous.claimed_revision() >= next.attempt().issuing_revision()
                || !same_teardown_lifecycle(previous.attempt(), next.attempt())
                || !successor_fence_precedes_or_equals(
                    previous.attempt().successor_fence(),
                    next.attempt().successor_fence(),
                )
            {
                return Err(WorkloadSagaError::InvalidEvidence(
                    "teardown receipt prefix is duplicated, reordered, stale, or crossed",
                ));
            }
        }
        Ok(())
    }
}

fn same_teardown_lifecycle(
    previous: &WorkloadTeardownAttempt,
    next: &WorkloadTeardownAttempt,
) -> bool {
    previous.key() == next.key()
        && previous.saga_id() == next.saga_id()
        && previous.generation() == next.generation()
        && previous.desired_digest() == next.desired_digest()
        && previous.required_node() == next.required_node()
        && previous.source_digest() == next.source_digest()
        && previous.execution_provider_id() == next.execution_provider_id()
        && previous.network_plan_digest() == next.network_plan_digest()
        && previous.selection_evidence() == next.selection_evidence()
        && previous.cause() == next.cause()
}

fn successor_fence_precedes_or_equals(
    previous: Option<WorkloadTeardownSuccessorFence>,
    next: Option<WorkloadTeardownSuccessorFence>,
) -> bool {
    match (previous, next) {
        (None, None | Some(_)) => true,
        (Some(_), None) => false,
        (Some(previous), Some(next)) => {
            previous.generation() < next.generation()
                || previous.generation() == next.generation()
                    && previous.desired_digest() == next.desired_digest()
        }
    }
}

/// Immutable teardown context retained through every nonterminal step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WorkloadTeardownContext {
    cause: WorkloadTeardownCause,
    successor_fence: Option<WorkloadTeardownSuccessorFence>,
    provision_absence: Option<Box<WorkloadProvisionTeardownAbsence>>,
    restart_settlement: Option<Box<WorkloadRestartTeardownSettlement>>,
    completed: Vec<WorkloadTeardownReceipt>,
}

impl WorkloadTeardownContext {
    pub(crate) fn new(
        cause: WorkloadTeardownCause,
        successor_fence: Option<WorkloadTeardownSuccessorFence>,
        provision_absence: Option<WorkloadProvisionTeardownAbsence>,
        restart_settlement: Option<WorkloadRestartTeardownSettlement>,
    ) -> Self {
        Self {
            cause,
            successor_fence,
            provision_absence: provision_absence.map(Box::new),
            restart_settlement: restart_settlement.map(Box::new),
            completed: Vec::new(),
        }
    }

    pub fn cause(&self) -> &WorkloadTeardownCause {
        &self.cause
    }

    pub const fn successor_fence(&self) -> Option<WorkloadTeardownSuccessorFence> {
        self.successor_fence
    }

    pub fn provision_absence(&self) -> Option<&WorkloadProvisionTeardownAbsence> {
        self.provision_absence.as_deref()
    }

    pub fn restart_settlement(&self) -> Option<&WorkloadRestartTeardownSettlement> {
        self.restart_settlement.as_deref()
    }

    pub fn completed(&self) -> &[WorkloadTeardownReceipt] {
        &self.completed
    }

    pub(crate) fn with_successor_fence(
        &self,
        successor_fence: WorkloadTeardownSuccessorFence,
    ) -> Result<Self, WorkloadSagaError> {
        if self
            .successor_fence
            .is_some_and(|current| successor_fence.generation() <= current.generation())
        {
            return Err(WorkloadSagaError::InvalidTransition(
                "teardown successor fence must advance monotonically",
            ));
        }
        let mut context = self.clone();
        context.successor_fence = Some(successor_fence);
        Ok(context)
    }

    pub(crate) fn with_receipt(
        &self,
        receipt: WorkloadTeardownReceipt,
    ) -> Result<Self, WorkloadSagaError> {
        if self
            .completed
            .iter()
            .any(|completed| completed.claim().attempt().step() == receipt.claim().attempt().step())
        {
            return Err(WorkloadSagaError::InvalidTransition(
                "teardown step already has a durable receipt",
            ));
        }
        let mut context = self.clone();
        context.completed.push(receipt);
        Ok(context)
    }
}

/// Durable provider-effect outcome accepted by the pure reducer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum WorkloadTeardownEffectResult {
    Succeeded {
        attempt_id: WorkloadTeardownAttemptId,
        dispatch_epoch: WorkloadTeardownDispatchEpoch,
        provider_target: WorkloadTeardownProviderTarget,
        evidence: Box<WorkloadTeardownSuccessEvidence>,
    },
    DefiniteFailure {
        attempt_id: WorkloadTeardownAttemptId,
        dispatch_epoch: WorkloadTeardownDispatchEpoch,
        provider_target: WorkloadTeardownProviderTarget,
        failure: WorkloadFailureEvidence,
    },
    Ambiguous {
        attempt_id: WorkloadTeardownAttemptId,
        dispatch_epoch: WorkloadTeardownDispatchEpoch,
        provider_target: WorkloadTeardownProviderTarget,
    },
}

impl WorkloadTeardownEffectResult {
    pub(crate) fn validate_for_claim(
        &self,
        claim: &WorkloadTeardownClaim,
    ) -> Result<(), WorkloadSagaError> {
        let matches = match self {
            Self::Succeeded {
                attempt_id,
                dispatch_epoch,
                provider_target,
                evidence,
            } => {
                attempt_id == claim.attempt().attempt_id()
                    && *dispatch_epoch == claim.dispatch_epoch()
                    && provider_target == claim.provider_target()
                    && evidence.step() == claim.attempt().step()
                    && evidence.matches_subjects(claim.attempt().subjects())
            }
            Self::DefiniteFailure {
                attempt_id,
                dispatch_epoch,
                provider_target,
                ..
            }
            | Self::Ambiguous {
                attempt_id,
                dispatch_epoch,
                provider_target,
            } => {
                attempt_id == claim.attempt().attempt_id()
                    && *dispatch_epoch == claim.dispatch_epoch()
                    && provider_target == claim.provider_target()
            }
        };
        if matches {
            Ok(())
        } else {
            Err(WorkloadSagaError::InvalidEvidence(
                "teardown effect result is crossed with the durable claim",
            ))
        }
    }
}

/// Durable teardown state orthogonal to the current lifecycle phase.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "disposition",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum WorkloadTeardownDisposition {
    Ready {
        context: WorkloadTeardownContext,
    },
    DispatchPending {
        context: WorkloadTeardownContext,
        claim: WorkloadTeardownClaim,
    },
    InspectionRequired {
        context: WorkloadTeardownContext,
        claim: WorkloadTeardownClaim,
    },
    DefiniteFailure {
        context: WorkloadTeardownContext,
        claim: WorkloadTeardownClaim,
        failure: WorkloadFailureEvidence,
        confirmation: WorkloadTeardownResultConfirmation,
        prior_terminal_observations: Vec<WorkloadTerminalObservation>,
    },
}

impl WorkloadTeardownDisposition {
    pub(crate) fn initial(context: WorkloadTeardownContext) -> Self {
        Self::Ready { context }
    }

    pub fn context(&self) -> &WorkloadTeardownContext {
        match self {
            Self::Ready { context }
            | Self::DispatchPending { context, .. }
            | Self::InspectionRequired { context, .. }
            | Self::DefiniteFailure { context, .. } => context,
        }
    }

    pub fn cause(&self) -> &WorkloadTeardownCause {
        self.context().cause()
    }

    pub fn claim(&self) -> Option<&WorkloadTeardownClaim> {
        match self {
            Self::Ready { .. } => None,
            Self::DispatchPending { claim, .. }
            | Self::InspectionRequired { claim, .. }
            | Self::DefiniteFailure { claim, .. } => Some(claim),
        }
    }

    pub const fn requires_inspection(&self) -> bool {
        matches!(
            self,
            Self::DispatchPending { .. } | Self::InspectionRequired { .. }
        )
    }
}

/// Pure candidate returned by the workloads reducer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProposedWorkloadTeardownTransition {
    Claim {
        attempt: Box<WorkloadTeardownAttempt>,
        provider_target: WorkloadTeardownProviderTarget,
    },
    ResourceFree {
        step: WorkloadTeardownStep,
        target_phase: WorkloadSagaPhase,
    },
    RecordTerminal,
}

/// Side-effect-free next action for a confirmed durable record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkloadTeardownDecision {
    Quiescent,
    PersistCandidate(ProposedWorkloadTeardownTransition),
    InspectExact(WorkloadTeardownClaim),
    RestartSettlementPending(Box<WorkloadRestartTeardownSettlement>),
    CleanupPending {
        claim: WorkloadTeardownClaim,
        failure: WorkloadFailureEvidence,
    },
}
