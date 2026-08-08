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

/// Durable command claim identity. This value is not provider authority by itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WorkloadRestartCommandClaim {
    command_id: WorkloadRestartCommandId,
    request_id: WorkloadRestartRequestId,
    restart_epoch: WorkloadRestartEpoch,
    attempt_id: WorkloadExecutionAttemptId,
    step: WorkloadRestartStep,
    dispatch_epoch: WorkloadRestartDispatchEpoch,
    issuing_revision: WorkloadSagaRevision,
}

impl WorkloadRestartCommandClaim {
    pub(crate) fn new(
        request_id: WorkloadRestartRequestId,
        restart_epoch: WorkloadRestartEpoch,
        attempt_id: WorkloadExecutionAttemptId,
        step: WorkloadRestartStep,
        dispatch_epoch: WorkloadRestartDispatchEpoch,
        issuing_revision: WorkloadSagaRevision,
    ) -> Self {
        let restart_epoch_text = restart_epoch.to_string();
        let dispatch_epoch_text = dispatch_epoch.to_string();
        let issuing_revision_text = issuing_revision.to_string();
        let command_id = WorkloadRestartCommandId(derive_id(
            WorkloadRestartCommandId::PREFIX,
            b"nimbus.workloads.restart.command.id.v1",
            &[
                request_id.as_str(),
                &restart_epoch_text,
                attempt_id.as_str(),
                restart_step_name(step),
                &dispatch_epoch_text,
                &issuing_revision_text,
            ],
        ));
        Self {
            command_id,
            request_id,
            restart_epoch,
            attempt_id,
            step,
            dispatch_epoch,
            issuing_revision,
        }
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

    pub(super) fn validate(&self) -> Result<(), WorkloadSagaError> {
        if self.command_id
            != Self::new(
                self.request_id.clone(),
                self.restart_epoch,
                self.attempt_id.clone(),
                self.step,
                self.dispatch_epoch,
                self.issuing_revision,
            )
            .command_id
        {
            return Err(WorkloadSagaError::InvalidIdentity(
                "restart command id does not bind its complete claim",
            ));
        }
        Ok(())
    }
}

fn restart_step_name(step: WorkloadRestartStep) -> &'static str {
    match step {
        WorkloadRestartStep::WithdrawPublication => "withdraw_publication",
        WorkloadRestartStep::QuiesceExecution => "quiesce_execution",
        WorkloadRestartStep::PrepareExecution => "prepare_execution",
        WorkloadRestartStep::AttachNetwork => "attach_network",
        WorkloadRestartStep::InspectActivationPrerequisites => "inspect_activation_prerequisites",
        WorkloadRestartStep::ActivateExecution => "activate_execution",
        WorkloadRestartStep::InspectReadiness => "inspect_readiness",
        WorkloadRestartStep::Publish => "publish",
        WorkloadRestartStep::ObservePublication => "observe_publication",
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

/// Closed command disposition for the active restart phase.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "disposition",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum WorkloadRestartDisposition {
    Ready,
    DispatchPending {
        claim: WorkloadRestartCommandClaim,
    },
    InspectionRequired {
        claim: WorkloadRestartCommandClaim,
    },
    DefiniteFailure {
        claim: Option<WorkloadRestartCommandClaim>,
        result: WorkloadRestartEffectResult,
    },
}

impl WorkloadRestartDisposition {
    pub(super) fn validate(&self) -> Result<(), WorkloadSagaError> {
        match self {
            Self::Ready => Ok(()),
            Self::DispatchPending { claim } | Self::InspectionRequired { claim } => {
                claim.validate()
            }
            Self::DefiniteFailure { claim, .. } => {
                if let Some(claim) = claim {
                    claim.validate()?;
                }
                Ok(())
            }
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
}

impl ActiveWorkloadRestart {
    pub(super) fn requested(admission: WorkloadRestartAdmission) -> Self {
        Self {
            phase: WorkloadRestartPhase::Requested,
            admission,
            disposition: WorkloadRestartDisposition::Ready,
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

    pub(super) fn validate(&self) -> Result<(), WorkloadSagaError> {
        if self.phase.is_idle() {
            return Err(WorkloadSagaError::InvalidTransition(
                "an active restart cannot use the idle phase",
            ));
        }
        self.admission.validate_intrinsic()?;
        self.disposition.validate()
    }
}

/// Durable evidence for the last completed restart.
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
    pub(super) last_completed: Option<WorkloadRestartHistory>,
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
            last_completed: None,
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
        self.last_completed.as_ref()
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
