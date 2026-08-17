//! Portable workload-restart policy, identity, state, and command vocabulary.

use super::*;

define_decimal_counter!(
    WorkloadRestartEpoch,
    "workload restart epoch must be canonical unsigned decimal text"
);
define_decimal_counter!(
    WorkloadRestartDispatchEpoch,
    "workload restart dispatch epoch must be canonical unsigned decimal text"
);
define_decimal_counter!(
    WorkloadRestartNotBeforeUnixMillis,
    "workload restart not-before time must be canonical unsigned decimal text"
);
define_sha256_digest!(
    WorkloadInspectionVersion,
    b"nimbus.workloads.inspection.version.v1\0",
    "workload inspection version must be 64 lowercase hexadecimal characters"
);
define_sha256_digest!(
    WorkloadRestartEvidenceDigest,
    b"nimbus.workloads.restart.evidence.v1\0",
    "workload restart evidence digest must be 64 lowercase hexadecimal characters"
);
define_derived_id!(WorkloadRestartRequestId, "wrr");
define_derived_id!(WorkloadExecutionAttemptId, "wea");
define_derived_id!(WorkloadRestartCommandId, "wrc");

/// Maximum durable restart receipts retained for one desired generation.
///
/// Nimbus rejects a new restart before it can create an effect when this
/// bound is exhausted. Completed receipts are never evicted.
pub const MAX_WORKLOAD_RESTART_COMPLETION_HISTORY: usize = 64;

impl WorkloadRestartRequestId {
    /// Derive the stable identity for one explicit idempotency key.
    pub fn for_explicit(
        saga_id: &WorkloadSagaId,
        source_generation: WorkloadProvisionSourceGeneration,
        idempotency_key: &str,
    ) -> Result<Self, WorkloadSagaError> {
        validate_idempotency_key(idempotency_key)?;
        let source_generation = source_generation.to_string();
        Ok(Self(derive_id(
            Self::PREFIX,
            b"nimbus.workloads.restart.request.explicit.v1",
            &[saga_id.as_str(), &source_generation, idempotency_key],
        )))
    }

    /// Derive the stable identity for one exact exit observation.
    pub fn for_automatic(
        saga_id: &WorkloadSagaId,
        inspection_version: WorkloadInspectionVersion,
    ) -> Self {
        let inspection_version = inspection_version.to_string();
        Self(derive_id(
            Self::PREFIX,
            b"nimbus.workloads.restart.request.automatic.v1",
            &[saga_id.as_str(), &inspection_version],
        ))
    }
}

fn validate_idempotency_key(value: &str) -> Result<(), WorkloadSagaError> {
    if value.is_empty()
        || value.len() > 256
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(WorkloadSagaError::InvalidIdentity(
            "restart idempotency key must be 1-256 non-control characters",
        ));
    }
    Ok(())
}

impl WorkloadExecutionAttemptId {
    /// Derive one process-incarnation identity under a stable execution owner.
    pub fn for_execution(
        execution_id: &WorkloadExecutionId,
        restart_epoch: WorkloadRestartEpoch,
    ) -> Self {
        let restart_epoch = restart_epoch.to_string();
        Self(derive_id(
            Self::PREFIX,
            b"nimbus.workloads.execution.attempt.id.v1",
            &[execution_id.as_str(), &restart_epoch],
        ))
    }
}

/// Portable desired restart policy, covered by the workload desired digest.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "policy",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum WorkloadRestartPolicy {
    #[default]
    Never,
    OnFailure {
        max_restarts: u32,
    },
    Always {
        max_restarts: u32,
    },
}

impl WorkloadRestartPolicy {
    pub fn admits_automatic(self, exit_code: i32, completed_restarts: u32) -> bool {
        match self {
            Self::Never => false,
            Self::OnFailure { max_restarts } => exit_code != 0 && completed_restarts < max_restarts,
            Self::Always { max_restarts } => completed_restarts < max_restarts,
        }
    }
}

/// Closed cause for one restart admission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "trigger",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum WorkloadRestartTrigger {
    Automatic { exit_code: i32 },
    Explicit,
}

impl WorkloadRestartTrigger {
    pub const fn is_automatic(self) -> bool {
        matches!(self, Self::Automatic { .. })
    }

    pub const fn exit_code(self) -> Option<i32> {
        match self {
            Self::Automatic { exit_code } => Some(exit_code),
            Self::Explicit => None,
        }
    }
}

/// Closed nested phase for a same-generation restart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkloadRestartPhase {
    Idle,
    Requested,
    PublicationWithdrawalPending,
    ExecutionQuiescencePending,
    Scheduled,
    PreparationPending,
    AttachmentPending,
    ActivationPrerequisitePending,
    ActivationPending,
    ReadinessPending,
    PublicationPending,
    ObservationPending,
}

impl WorkloadRestartPhase {
    pub const fn is_idle(self) -> bool {
        matches!(self, Self::Idle)
    }
}

/// One exact restart command family. Effects remain in their current owners.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkloadRestartStep {
    WithdrawPublication,
    QuiesceExecution,
    PrepareExecution,
    AttachNetwork,
    InspectActivationPrerequisites,
    ActivateExecution,
    InspectReadiness,
    Publish,
    ObservePublication,
}

impl WorkloadRestartStep {
    /// Whether this command is intrinsically a read-only observation.
    pub const fn is_inspection(self) -> bool {
        matches!(
            self,
            Self::InspectActivationPrerequisites
                | Self::InspectReadiness
                | Self::ObservePublication
        )
    }
}

/// Proof that inspection found no effect for one exact restart dispatch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WorkloadRestartAbsenceEvidence {
    request_id: WorkloadRestartRequestId,
    restart_epoch: WorkloadRestartEpoch,
    attempt_id: WorkloadExecutionAttemptId,
    step: WorkloadRestartStep,
    dispatch_epoch: WorkloadRestartDispatchEpoch,
    confirmed_revision: WorkloadSagaRevision,
    transition_id: WorkloadSagaTransitionId,
    evidence: WorkloadRestartEvidenceDigest,
}

impl WorkloadRestartAbsenceEvidence {
    /// Bind a read-only absence observation to the exact inspection state.
    pub fn for_inspection(
        record: &WorkloadSagaRecord,
        claim: &WorkloadRestartCommandClaim,
        evidence: WorkloadRestartEvidenceDigest,
    ) -> Result<Self, WorkloadSagaError> {
        let retained = record
            .restart_state()
            .active()
            .and_then(|active| active.disposition().claim());
        if !matches!(
            record
                .restart_state()
                .active()
                .map(ActiveWorkloadRestart::disposition),
            Some(WorkloadRestartDisposition::InspectionRequired { .. })
        ) || retained != Some(claim)
        {
            return Err(WorkloadSagaError::InvalidEvidence(
                "restart absence requires the exact durable inspection state",
            ));
        }
        Ok(Self {
            request_id: claim.request_id.clone(),
            restart_epoch: claim.restart_epoch,
            attempt_id: claim.attempt_id.clone(),
            step: claim.step,
            dispatch_epoch: claim.dispatch_epoch,
            confirmed_revision: record.revision(),
            transition_id: record.last_transition().transition_id().clone(),
            evidence,
        })
    }

    pub fn request_id(&self) -> &WorkloadRestartRequestId {
        &self.request_id
    }

    pub const fn restart_epoch(&self) -> WorkloadRestartEpoch {
        self.restart_epoch
    }

    pub fn attempt_id(&self) -> &WorkloadExecutionAttemptId {
        &self.attempt_id
    }

    pub const fn step(&self) -> WorkloadRestartStep {
        self.step
    }

    pub const fn dispatch_epoch(&self) -> WorkloadRestartDispatchEpoch {
        self.dispatch_epoch
    }

    pub const fn confirmed_revision(&self) -> WorkloadSagaRevision {
        self.confirmed_revision
    }

    pub fn transition_id(&self) -> &WorkloadSagaTransitionId {
        &self.transition_id
    }

    pub const fn evidence(&self) -> WorkloadRestartEvidenceDigest {
        self.evidence
    }

    fn matches_claim(&self, claim: &WorkloadRestartCommandClaim) -> bool {
        self.request_id == claim.request_id
            && self.restart_epoch == claim.restart_epoch
            && self.attempt_id == claim.attempt_id
            && self.step == claim.step
            && self.dispatch_epoch == claim.dispatch_epoch
    }

    pub(super) fn matches_inspection(
        &self,
        record: &WorkloadSagaRecord,
        claim: &WorkloadRestartCommandClaim,
    ) -> bool {
        self.matches_claim(claim)
            && self.confirmed_revision == record.revision()
            && self.transition_id == *record.last_transition().transition_id()
            && record.restart_state().active().is_some_and(|active| {
                matches!(
                    active.disposition(),
                    WorkloadRestartDisposition::InspectionRequired { claim: retained }
                        if retained == claim
                )
            })
    }
}

/// Why a durable restart dispatch may execute at its epoch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "authorization",
    content = "evidence",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum WorkloadRestartDispatchAuthorization {
    Initial,
    RetryAfterAbsence(WorkloadRestartAbsenceEvidence),
    RepublishAfterObservationAbsence(WorkloadRestartAbsenceEvidence),
}

/// Durable command claim identity. This value is not provider authority by itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadRestartCommandClaim {
    command_id: WorkloadRestartCommandId,
    request_id: WorkloadRestartRequestId,
    restart_epoch: WorkloadRestartEpoch,
    attempt_id: WorkloadExecutionAttemptId,
    step: WorkloadRestartStep,
    dispatch_epoch: WorkloadRestartDispatchEpoch,
    issuing_revision: WorkloadSagaRevision,
    authorization: WorkloadRestartDispatchAuthorization,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct WorkloadRestartCommandClaimWire {
    command_id: WorkloadRestartCommandId,
    request_id: WorkloadRestartRequestId,
    restart_epoch: WorkloadRestartEpoch,
    attempt_id: WorkloadExecutionAttemptId,
    step: WorkloadRestartStep,
    dispatch_epoch: WorkloadRestartDispatchEpoch,
    issuing_revision: WorkloadSagaRevision,
    authorization: WorkloadRestartDispatchAuthorization,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkloadRestartCommandIdentityPayload<'a> {
    request_id: &'a WorkloadRestartRequestId,
    restart_epoch: WorkloadRestartEpoch,
    attempt_id: &'a WorkloadExecutionAttemptId,
    step: WorkloadRestartStep,
    dispatch_epoch: WorkloadRestartDispatchEpoch,
    issuing_revision: WorkloadSagaRevision,
    authorization: &'a WorkloadRestartDispatchAuthorization,
}

impl<'de> Deserialize<'de> for WorkloadRestartCommandClaim {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = WorkloadRestartCommandClaimWire::deserialize(deserializer)?;
        let expected_id = wire.command_id;
        let claim = Self::new(
            wire.request_id,
            wire.restart_epoch,
            wire.attempt_id,
            wire.step,
            wire.dispatch_epoch,
            wire.issuing_revision,
            wire.authorization,
        )
        .map_err(serde::de::Error::custom)?;
        if claim.command_id != expected_id {
            return Err(serde::de::Error::custom(
                "restart command id does not bind its complete claim",
            ));
        }
        Ok(claim)
    }
}

impl WorkloadRestartCommandClaim {
    fn new(
        request_id: WorkloadRestartRequestId,
        restart_epoch: WorkloadRestartEpoch,
        attempt_id: WorkloadExecutionAttemptId,
        step: WorkloadRestartStep,
        dispatch_epoch: WorkloadRestartDispatchEpoch,
        issuing_revision: WorkloadSagaRevision,
        authorization: WorkloadRestartDispatchAuthorization,
    ) -> Result<Self, WorkloadSagaError> {
        let encoded = serde_json::to_vec(&WorkloadRestartCommandIdentityPayload {
            request_id: &request_id,
            restart_epoch,
            attempt_id: &attempt_id,
            step,
            dispatch_epoch,
            issuing_revision,
            authorization: &authorization,
        })
        .map_err(|_| {
            WorkloadSagaError::InvalidEvidence("restart command claim cannot be encoded")
        })?;
        let canonical = std::str::from_utf8(&encoded).map_err(|_| {
            WorkloadSagaError::InvalidEvidence("restart command claim is not UTF-8")
        })?;
        let command_id = WorkloadRestartCommandId(derive_id(
            WorkloadRestartCommandId::PREFIX,
            b"nimbus.workloads.restart.command.id.v2",
            &[canonical],
        ));
        let claim = Self {
            command_id,
            request_id,
            restart_epoch,
            attempt_id,
            step,
            dispatch_epoch,
            issuing_revision,
            authorization,
        };
        claim.validate()?;
        Ok(claim)
    }

    pub(super) fn initial(
        request_id: WorkloadRestartRequestId,
        restart_epoch: WorkloadRestartEpoch,
        attempt_id: WorkloadExecutionAttemptId,
        step: WorkloadRestartStep,
        issuing_revision: WorkloadSagaRevision,
    ) -> Result<Self, WorkloadSagaError> {
        Self::new(
            request_id,
            restart_epoch,
            attempt_id,
            step,
            WorkloadRestartDispatchEpoch::new(0),
            issuing_revision,
            WorkloadRestartDispatchAuthorization::Initial,
        )
    }

    pub(super) fn retry_after_absence(
        previous: &Self,
        issuing_revision: WorkloadSagaRevision,
        absence: WorkloadRestartAbsenceEvidence,
    ) -> Result<Self, WorkloadSagaError> {
        if !absence.matches_claim(previous) || absence.confirmed_revision != issuing_revision {
            return Err(WorkloadSagaError::InvalidEvidence(
                "restart retry requires exact absence at the current revision",
            ));
        }
        let dispatch_epoch =
            previous
                .dispatch_epoch
                .checked_next()
                .ok_or(WorkloadSagaError::InvalidCounter(
                    "workload restart dispatch epoch overflow",
                ))?;
        Self::new(
            previous.request_id.clone(),
            previous.restart_epoch,
            previous.attempt_id.clone(),
            previous.step,
            dispatch_epoch,
            issuing_revision,
            WorkloadRestartDispatchAuthorization::RetryAfterAbsence(absence),
        )
    }

    pub(super) fn republish_after_observation_absence(
        observation: &Self,
        issuing_revision: WorkloadSagaRevision,
        absence: WorkloadRestartAbsenceEvidence,
    ) -> Result<Self, WorkloadSagaError> {
        if observation.step != WorkloadRestartStep::ObservePublication
            || !absence.matches_claim(observation)
            || absence.confirmed_revision != issuing_revision
        {
            return Err(WorkloadSagaError::InvalidEvidence(
                "restart republish requires exact publication-observation absence",
            ));
        }
        let dispatch_epoch =
            observation
                .dispatch_epoch
                .checked_next()
                .ok_or(WorkloadSagaError::InvalidCounter(
                    "workload restart dispatch epoch overflow",
                ))?;
        Self::new(
            observation.request_id.clone(),
            observation.restart_epoch,
            observation.attempt_id.clone(),
            WorkloadRestartStep::Publish,
            dispatch_epoch,
            issuing_revision,
            WorkloadRestartDispatchAuthorization::RepublishAfterObservationAbsence(absence),
        )
    }

    pub fn command_id(&self) -> &WorkloadRestartCommandId {
        &self.command_id
    }

    pub fn request_id(&self) -> &WorkloadRestartRequestId {
        &self.request_id
    }

    pub const fn restart_epoch(&self) -> WorkloadRestartEpoch {
        self.restart_epoch
    }

    pub fn attempt_id(&self) -> &WorkloadExecutionAttemptId {
        &self.attempt_id
    }

    pub const fn step(&self) -> WorkloadRestartStep {
        self.step
    }

    pub const fn dispatch_epoch(&self) -> WorkloadRestartDispatchEpoch {
        self.dispatch_epoch
    }

    pub const fn issuing_revision(&self) -> WorkloadSagaRevision {
        self.issuing_revision
    }

    pub fn authorization(&self) -> &WorkloadRestartDispatchAuthorization {
        &self.authorization
    }

    pub(super) fn validate(&self) -> Result<(), WorkloadSagaError> {
        match &self.authorization {
            WorkloadRestartDispatchAuthorization::Initial => {
                if self.dispatch_epoch != WorkloadRestartDispatchEpoch::new(0) {
                    return Err(WorkloadSagaError::InvalidEvidence(
                        "initial restart dispatch must use epoch zero",
                    ));
                }
            }
            WorkloadRestartDispatchAuthorization::RetryAfterAbsence(absence) => {
                if absence.request_id != self.request_id
                    || absence.restart_epoch != self.restart_epoch
                    || absence.attempt_id != self.attempt_id
                    || absence.step != self.step
                    || self.step == WorkloadRestartStep::ObservePublication
                    || absence.dispatch_epoch.checked_next() != Some(self.dispatch_epoch)
                    || absence.confirmed_revision != self.issuing_revision
                {
                    return Err(WorkloadSagaError::InvalidEvidence(
                        "restart retry is not authorized by exact prior absence",
                    ));
                }
            }
            WorkloadRestartDispatchAuthorization::RepublishAfterObservationAbsence(absence) => {
                if absence.request_id != self.request_id
                    || absence.restart_epoch != self.restart_epoch
                    || absence.attempt_id != self.attempt_id
                    || absence.step != WorkloadRestartStep::ObservePublication
                    || self.step != WorkloadRestartStep::Publish
                    || absence.dispatch_epoch.checked_next() != Some(self.dispatch_epoch)
                    || absence.confirmed_revision != self.issuing_revision
                {
                    return Err(WorkloadSagaError::InvalidEvidence(
                        "restart republish is not authorized by exact publication-observation absence",
                    ));
                }
            }
        }
        Ok(())
    }
}

/// Exact portable result recorded for one restart command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "result",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum WorkloadRestartEffectResult {
    Succeeded {
        evidence: WorkloadRestartEvidenceDigest,
    },
    AuthenticatedAbsent {
        evidence: WorkloadRestartEvidenceDigest,
    },
    Failed {
        evidence: WorkloadRestartEvidenceDigest,
    },
}

impl WorkloadRestartEffectResult {
    pub const fn evidence(&self) -> WorkloadRestartEvidenceDigest {
        match self {
            Self::Succeeded { evidence }
            | Self::AuthenticatedAbsent { evidence }
            | Self::Failed { evidence } => *evidence,
        }
    }

    pub const fn is_succeeded(&self) -> bool {
        matches!(self, Self::Succeeded { .. })
    }

    pub const fn is_failed(&self) -> bool {
        matches!(self, Self::Failed { .. })
    }
}

/// Exact successful command evidence retained at the target restart phase.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WorkloadRestartCommandReceipt {
    claim: WorkloadRestartCommandClaim,
    result: WorkloadRestartEffectResult,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct WorkloadRestartCommandReceiptWire {
    claim: WorkloadRestartCommandClaim,
    result: WorkloadRestartEffectResult,
}

impl<'de> Deserialize<'de> for WorkloadRestartCommandReceipt {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = WorkloadRestartCommandReceiptWire::deserialize(deserializer)?;
        let receipt = Self {
            claim: wire.claim,
            result: wire.result,
        };
        receipt.validate().map_err(serde::de::Error::custom)?;
        Ok(receipt)
    }
}

impl WorkloadRestartCommandReceipt {
    pub(super) fn succeeded(
        claim: WorkloadRestartCommandClaim,
        result: WorkloadRestartEffectResult,
    ) -> Result<Self, WorkloadSagaError> {
        let receipt = Self { claim, result };
        receipt.validate()?;
        Ok(receipt)
    }

    pub fn claim(&self) -> &WorkloadRestartCommandClaim {
        &self.claim
    }

    pub fn result(&self) -> &WorkloadRestartEffectResult {
        &self.result
    }

    fn validate(&self) -> Result<(), WorkloadSagaError> {
        self.claim.validate()?;
        if !self.result.is_succeeded() {
            return Err(WorkloadSagaError::InvalidEvidence(
                "restart command receipt must retain exact success evidence",
            ));
        }
        Ok(())
    }
}

/// Closed command disposition for the active restart phase.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "disposition",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum WorkloadRestartDisposition {
    Ready {
        receipt: Option<WorkloadRestartCommandReceipt>,
    },
    DispatchPending {
        claim: WorkloadRestartCommandClaim,
    },
    InspectionRequired {
        claim: WorkloadRestartCommandClaim,
    },
    DefiniteFailure {
        claim: WorkloadRestartCommandClaim,
        result: WorkloadRestartEffectResult,
    },
    SuccessorVetoed {
        claim: WorkloadRestartCommandClaim,
        result: WorkloadRestartEffectResult,
    },
}

#[derive(Deserialize)]
#[serde(
    tag = "disposition",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum WorkloadRestartDispositionWire {
    Ready {
        receipt: Option<WorkloadRestartCommandReceipt>,
    },
    DispatchPending {
        claim: WorkloadRestartCommandClaim,
    },
    InspectionRequired {
        claim: WorkloadRestartCommandClaim,
    },
    DefiniteFailure {
        claim: WorkloadRestartCommandClaim,
        result: WorkloadRestartEffectResult,
    },
    SuccessorVetoed {
        claim: WorkloadRestartCommandClaim,
        result: WorkloadRestartEffectResult,
    },
}

impl<'de> Deserialize<'de> for WorkloadRestartDisposition {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = WorkloadRestartDispositionWire::deserialize(deserializer)?;
        let disposition = match wire {
            WorkloadRestartDispositionWire::Ready { receipt } => Self::Ready { receipt },
            WorkloadRestartDispositionWire::DispatchPending { claim } => {
                Self::DispatchPending { claim }
            }
            WorkloadRestartDispositionWire::InspectionRequired { claim } => {
                Self::InspectionRequired { claim }
            }
            WorkloadRestartDispositionWire::DefiniteFailure { claim, result } => {
                Self::DefiniteFailure { claim, result }
            }
            WorkloadRestartDispositionWire::SuccessorVetoed { claim, result } => {
                Self::SuccessorVetoed { claim, result }
            }
        };
        disposition.validate().map_err(serde::de::Error::custom)?;
        Ok(disposition)
    }
}

impl WorkloadRestartDisposition {
    pub(super) const fn initial_ready() -> Self {
        Self::Ready { receipt: None }
    }

    pub const fn is_ready(&self) -> bool {
        matches!(self, Self::Ready { .. })
    }

    pub fn receipt(&self) -> Option<&WorkloadRestartCommandReceipt> {
        match self {
            Self::Ready { receipt } => receipt.as_ref(),
            Self::DispatchPending { .. }
            | Self::InspectionRequired { .. }
            | Self::DefiniteFailure { .. }
            | Self::SuccessorVetoed { .. } => None,
        }
    }

    pub fn claim(&self) -> Option<&WorkloadRestartCommandClaim> {
        match self {
            Self::Ready { receipt } => receipt.as_ref().map(WorkloadRestartCommandReceipt::claim),
            Self::DispatchPending { claim }
            | Self::InspectionRequired { claim }
            | Self::DefiniteFailure { claim, .. }
            | Self::SuccessorVetoed { claim, .. } => Some(claim),
        }
    }

    pub(super) fn validate(&self) -> Result<(), WorkloadSagaError> {
        match self {
            Self::Ready { receipt } => {
                if let Some(receipt) = receipt {
                    receipt.validate()?;
                }
                Ok(())
            }
            Self::DispatchPending { claim } | Self::InspectionRequired { claim } => {
                claim.validate()
            }
            Self::DefiniteFailure { claim, result } => {
                claim.validate()?;
                if result.is_failed() {
                    Ok(())
                } else {
                    Err(WorkloadSagaError::InvalidEvidence(
                        "definite restart failure must retain failed effect evidence",
                    ))
                }
            }
            Self::SuccessorVetoed { claim, .. } => claim.validate(),
        }
    }
}

/// Every portable fence committed by restart admission.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WorkloadRestartAdmission {
    saga_id: WorkloadSagaId,
    source: WorkloadProvisionSourceEvidence,
    generation: WorkloadGeneration,
    desired_digest: WorkloadDesiredDigest,
    revision: WorkloadSagaRevision,
    trigger: WorkloadRestartTrigger,
    inspection_version: Option<WorkloadInspectionVersion>,
    provider_selection: WorkloadExecutionProviderId,
    restart_epoch: WorkloadRestartEpoch,
    policy_attempt_count: u32,
    request_id: WorkloadRestartRequestId,
    source_attempt_id: WorkloadExecutionAttemptId,
    attempt_id: WorkloadExecutionAttemptId,
    not_before_unix_millis: WorkloadRestartNotBeforeUnixMillis,
}

/// Inputs to one pure restart-admission decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadRestartAdmissionInput {
    pub expected_revision: WorkloadSagaRevision,
    pub trigger: WorkloadRestartTrigger,
    pub inspection_version: Option<WorkloadInspectionVersion>,
    pub request_id: WorkloadRestartRequestId,
    pub not_before_unix_millis: WorkloadRestartNotBeforeUnixMillis,
}

impl WorkloadRestartAdmission {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        saga_id: WorkloadSagaId,
        source: WorkloadProvisionSourceEvidence,
        generation: WorkloadGeneration,
        desired_digest: WorkloadDesiredDigest,
        revision: WorkloadSagaRevision,
        trigger: WorkloadRestartTrigger,
        inspection_version: Option<WorkloadInspectionVersion>,
        provider_selection: WorkloadExecutionProviderId,
        restart_epoch: WorkloadRestartEpoch,
        policy_attempt_count: u32,
        request_id: WorkloadRestartRequestId,
        source_attempt_id: WorkloadExecutionAttemptId,
        attempt_id: WorkloadExecutionAttemptId,
        not_before_unix_millis: WorkloadRestartNotBeforeUnixMillis,
    ) -> Result<Self, WorkloadSagaError> {
        let admission = Self {
            saga_id,
            source,
            generation,
            desired_digest,
            revision,
            trigger,
            inspection_version,
            provider_selection,
            restart_epoch,
            policy_attempt_count,
            request_id,
            source_attempt_id,
            attempt_id,
            not_before_unix_millis,
        };
        admission.validate_intrinsic()?;
        Ok(admission)
    }

    pub fn saga_id(&self) -> &WorkloadSagaId {
        &self.saga_id
    }

    pub fn source(&self) -> &WorkloadProvisionSourceEvidence {
        &self.source
    }

    pub const fn generation(&self) -> WorkloadGeneration {
        self.generation
    }

    pub const fn desired_digest(&self) -> WorkloadDesiredDigest {
        self.desired_digest
    }

    pub const fn revision(&self) -> WorkloadSagaRevision {
        self.revision
    }

    pub const fn trigger(&self) -> WorkloadRestartTrigger {
        self.trigger
    }

    pub const fn inspection_version(&self) -> Option<WorkloadInspectionVersion> {
        self.inspection_version
    }

    pub fn provider_selection(&self) -> &WorkloadExecutionProviderId {
        &self.provider_selection
    }

    pub const fn restart_epoch(&self) -> WorkloadRestartEpoch {
        self.restart_epoch
    }

    pub const fn policy_attempt_count(&self) -> u32 {
        self.policy_attempt_count
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

    pub const fn not_before_unix_millis(&self) -> WorkloadRestartNotBeforeUnixMillis {
        self.not_before_unix_millis
    }

    pub(super) fn validate_intrinsic(&self) -> Result<(), WorkloadSagaError> {
        if self.trigger.is_automatic() != self.inspection_version.is_some() {
            return Err(WorkloadSagaError::InvalidEvidence(
                "automatic restart requires one inspection version and explicit restart forbids it",
            ));
        }
        if let Some(inspection_version) = self.inspection_version
            && self.request_id
                != WorkloadRestartRequestId::for_automatic(&self.saga_id, inspection_version)
        {
            return Err(WorkloadSagaError::InvalidIdentity(
                "automatic restart request id does not bind its inspection version",
            ));
        }
        if self.source.execution_provider_id() != &self.provider_selection {
            return Err(WorkloadSagaError::InvalidEvidence(
                "restart provider selection does not match admitted source evidence",
            ));
        }
        if self.source_attempt_id == self.attempt_id {
            return Err(WorkloadSagaError::InvalidIdentity(
                "restart source and target execution attempts must differ",
            ));
        }
        Ok(())
    }
}

/// One active same-generation restart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ActiveWorkloadRestart {
    pub(super) phase: WorkloadRestartPhase,
    pub(super) admission: WorkloadRestartAdmission,
    pub(super) disposition: WorkloadRestartDisposition,
    pub(super) owner_observations: Vec<WorkloadOwnerObservation>,
    pub(super) successor_veto_generation: Option<WorkloadGeneration>,
}

impl ActiveWorkloadRestart {
    pub(super) fn requested(admission: WorkloadRestartAdmission) -> Self {
        Self {
            phase: WorkloadRestartPhase::Requested,
            admission,
            disposition: WorkloadRestartDisposition::initial_ready(),
            owner_observations: Vec::new(),
            successor_veto_generation: None,
        }
    }

    pub const fn phase(&self) -> WorkloadRestartPhase {
        self.phase
    }

    pub fn admission(&self) -> &WorkloadRestartAdmission {
        &self.admission
    }

    pub fn disposition(&self) -> &WorkloadRestartDisposition {
        &self.disposition
    }

    /// Exact role-owned evidence accumulated for the target execution attempt.
    pub fn owner_observations(&self) -> &[WorkloadOwnerObservation] {
        &self.owner_observations
    }

    /// Queued generation that permanently revokes new restart effects.
    pub const fn successor_veto_generation(&self) -> Option<WorkloadGeneration> {
        self.successor_veto_generation
    }

    pub(super) fn validate(&self) -> Result<(), WorkloadSagaError> {
        if self.phase.is_idle() {
            return Err(WorkloadSagaError::InvalidTransition(
                "an active restart cannot use the idle phase",
            ));
        }
        self.admission.validate_intrinsic()?;
        if self
            .successor_veto_generation
            .is_some_and(|generation| generation <= self.admission.generation())
        {
            return Err(WorkloadSagaError::InvalidEvidence(
                "restart successor veto must name a later desired generation",
            ));
        }
        self.disposition.validate()
    }
}

/// Durable evidence for one completed restart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WorkloadRestartHistory {
    pub(super) admission: WorkloadRestartAdmission,
    pub(super) evidence: WorkloadRestartEvidenceDigest,
}

impl WorkloadRestartHistory {
    pub fn admission(&self) -> &WorkloadRestartAdmission {
        &self.admission
    }

    pub fn request_id(&self) -> &WorkloadRestartRequestId {
        self.admission.request_id()
    }

    pub const fn restart_epoch(&self) -> WorkloadRestartEpoch {
        self.admission.restart_epoch()
    }

    pub const fn trigger(&self) -> WorkloadRestartTrigger {
        self.admission.trigger()
    }

    pub fn attempt_id(&self) -> &WorkloadExecutionAttemptId {
        self.admission.attempt_id()
    }

    pub const fn completed_automatic_restart_count(&self) -> u32 {
        self.admission.policy_attempt_count()
    }

    pub const fn not_before_unix_millis(&self) -> WorkloadRestartNotBeforeUnixMillis {
        self.admission.not_before_unix_millis()
    }

    pub const fn evidence(&self) -> WorkloadRestartEvidenceDigest {
        self.evidence
    }
}

/// Always-present nested restart state for one desired generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WorkloadRestartState {
    pub(super) current_execution_attempt_id: WorkloadExecutionAttemptId,
    pub(super) completed_restart_epoch: WorkloadRestartEpoch,
    pub(super) completed_automatic_restart_count: u32,
    pub(super) active: Option<ActiveWorkloadRestart>,
    pub(super) completion_history: Vec<WorkloadRestartHistory>,
}

impl WorkloadRestartState {
    pub(super) fn initial(execution_id: &WorkloadExecutionId) -> Self {
        let completed_restart_epoch = WorkloadRestartEpoch::new(0);
        Self {
            current_execution_attempt_id: WorkloadExecutionAttemptId::for_execution(
                execution_id,
                completed_restart_epoch,
            ),
            completed_restart_epoch,
            completed_automatic_restart_count: 0,
            active: None,
            completion_history: Vec::new(),
        }
    }

    pub fn phase(&self) -> WorkloadRestartPhase {
        self.active
            .as_ref()
            .map_or(WorkloadRestartPhase::Idle, ActiveWorkloadRestart::phase)
    }

    pub fn current_execution_attempt_id(&self) -> &WorkloadExecutionAttemptId {
        &self.current_execution_attempt_id
    }

    pub const fn completed_restart_epoch(&self) -> WorkloadRestartEpoch {
        self.completed_restart_epoch
    }

    pub const fn completed_automatic_restart_count(&self) -> u32 {
        self.completed_automatic_restart_count
    }

    pub fn active(&self) -> Option<&ActiveWorkloadRestart> {
        self.active.as_ref()
    }

    pub fn last_completed(&self) -> Option<&WorkloadRestartHistory> {
        self.completion_history.last()
    }

    /// Ordered, non-evicting receipts for this desired generation.
    pub fn completion_history(&self) -> &[WorkloadRestartHistory] {
        &self.completion_history
    }

    /// Find the original receipt for an exact completed request.
    pub fn completion_for_request(
        &self,
        request_id: &WorkloadRestartRequestId,
    ) -> Option<&WorkloadRestartHistory> {
        self.completion_history
            .iter()
            .find(|history| history.request_id() == request_id)
    }
}

/// Result of idempotent restart admission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkloadRestartAdmissionUpdate {
    Unchanged,
    Transition(Box<WorkloadSagaRecord>),
}

/// Pure recovery decision for one nested restart state at a supplied wall time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkloadRestartRecoveryDecision {
    Quiescent,
    WaitingUntil(WorkloadRestartNotBeforeUnixMillis),
    Ready,
}

#[cfg(test)]
#[path = "restart/tests.rs"]
mod tests;
