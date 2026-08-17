//! Confirmed and fenced workload teardown commands.

use nimbus_network::{NetworkCapabilitySelectionEvidence, NetworkPlanDigest};
use nimbus_workloads::{
    CompiledWorkloadNetworkPlan, NodeIdentity, WorkloadDesiredDigest, WorkloadExecutionReference,
    WorkloadFailureEvidence, WorkloadGeneration, WorkloadOwnerEvidenceDigest,
    WorkloadProvisionSourceDigest, WorkloadProvisionSourceEvidence, WorkloadSagaId,
    WorkloadSagaKey, WorkloadSagaRecord, WorkloadSagaRevision, WorkloadSagaStoreError,
    WorkloadSagaTransitionId, WorkloadTeardownAttemptId, WorkloadTeardownClaim,
    WorkloadTeardownCommandId, WorkloadTeardownCommandMode, WorkloadTeardownDispatchEpoch,
    WorkloadTeardownDisposition, WorkloadTeardownEffectResult, WorkloadTeardownInspectionResult,
    WorkloadTeardownProviderTarget, WorkloadTeardownReceiptPrefix, WorkloadTeardownRetryEvidence,
    WorkloadTeardownStep, WorkloadTeardownSubjects, WorkloadTeardownSuccessEvidence,
};

use super::{WorkloadSagaConfirmation, WorkloadSagaCoordinator};

/// Ephemeral provider command created only from exact durable confirmation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmedWorkloadTeardownCommand {
    command_id: WorkloadTeardownCommandId,
    confirmed_revision: WorkloadSagaRevision,
    confirmed_transition_id: WorkloadSagaTransitionId,
    source: WorkloadProvisionSourceEvidence,
    compiled_network_plan: CompiledWorkloadNetworkPlan,
    execution_locator: WorkloadExecutionReference,
    prior_receipt_prefix: WorkloadTeardownReceiptPrefix,
    mode: WorkloadTeardownCommandMode,
    claim: WorkloadTeardownClaim,
}

impl ConfirmedWorkloadTeardownCommand {
    fn from_confirmation(
        record: &WorkloadSagaRecord,
        confirmation: WorkloadSagaConfirmation,
    ) -> Result<Option<Self>, WorkloadSagaStoreError> {
        record.validate()?;
        let disposition = record.teardown_disposition().ok_or(
            nimbus_workloads::WorkloadSagaError::InvalidTransition(
                "confirmed teardown command requires durable teardown state",
            ),
        )?;
        let claim =
            disposition
                .claim()
                .ok_or(nimbus_workloads::WorkloadSagaError::InvalidTransition(
                    "confirmed teardown command requires a durable claim",
                ))?;
        let mode = match (disposition, confirmation) {
            (
                WorkloadTeardownDisposition::DispatchPending { .. },
                WorkloadSagaConfirmation::AppliedByThisCall,
            ) => WorkloadTeardownCommandMode::Execute,
            (
                WorkloadTeardownDisposition::InspectionRequired { .. },
                WorkloadSagaConfirmation::AppliedByThisCall
                | WorkloadSagaConfirmation::ConfirmedAfterAmbiguity
                | WorkloadSagaConfirmation::ConfirmedReplay,
            ) => WorkloadTeardownCommandMode::Inspect,
            (
                WorkloadTeardownDisposition::DispatchPending { .. },
                WorkloadSagaConfirmation::ConfirmedAfterAmbiguity
                | WorkloadSagaConfirmation::ConfirmedReplay,
            )
            | (
                _,
                WorkloadSagaConfirmation::Conflict { .. }
                | WorkloadSagaConfirmation::UnresolvedAmbiguity,
            ) => return Ok(None),
            (WorkloadTeardownDisposition::Ready { .. }, _)
            | (WorkloadTeardownDisposition::DefiniteFailure { .. }, _) => {
                return Err(nimbus_workloads::WorkloadSagaError::InvalidTransition(
                    "confirmed teardown command requires dispatch or inspection state",
                )
                .into());
            }
        };
        authenticate_durable_claim(record, claim, mode)?;
        let compiled_network_plan = record.active_intent().network().compiled_plan().clone();
        if compiled_network_plan.plan().digest() != claim.attempt().network_plan_digest() {
            return Err(nimbus_workloads::WorkloadSagaError::InvalidEvidence(
                "confirmed teardown command network content is crossed with its durable claim",
            )
            .into());
        }
        let execution_locator = record
            .phase_detail()
            .references()
            .execution()
            .cloned()
            .ok_or(nimbus_workloads::WorkloadSagaError::InvalidEvidence(
                "effectful teardown command requires its retained execution locator",
            ))?;
        let prior_receipt_prefix = record.teardown_receipt_prefix_for_claim(claim)?;
        let command_id = WorkloadTeardownCommandId::for_confirmed_dispatch(
            claim,
            record.revision(),
            record.last_transition().transition_id(),
            mode,
        )?;
        Ok(Some(Self {
            command_id,
            confirmed_revision: record.revision(),
            confirmed_transition_id: record.last_transition().transition_id().clone(),
            source: record.active_intent().source().clone(),
            compiled_network_plan,
            execution_locator,
            prior_receipt_prefix,
            mode,
            claim: claim.clone(),
        }))
    }

    pub const fn command_id(&self) -> WorkloadTeardownCommandId {
        self.command_id
    }

    pub fn key(&self) -> &WorkloadSagaKey {
        self.claim.attempt().key()
    }

    pub fn saga_id(&self) -> &WorkloadSagaId {
        self.claim.attempt().saga_id()
    }

    pub fn issuing_revision(&self) -> WorkloadSagaRevision {
        self.claim.attempt().issuing_revision()
    }

    pub fn issuing_transition_id(&self) -> &WorkloadSagaTransitionId {
        self.claim.attempt().issuing_transition_id()
    }

    pub const fn confirmed_revision(&self) -> WorkloadSagaRevision {
        self.confirmed_revision
    }

    pub fn confirmed_transition_id(&self) -> &WorkloadSagaTransitionId {
        &self.confirmed_transition_id
    }

    pub fn generation(&self) -> WorkloadGeneration {
        self.claim.attempt().generation()
    }

    pub fn desired_digest(&self) -> WorkloadDesiredDigest {
        self.claim.attempt().desired_digest()
    }

    pub fn required_node(&self) -> &NodeIdentity {
        self.claim.attempt().required_node()
    }

    pub fn source(&self) -> &WorkloadProvisionSourceEvidence {
        &self.source
    }

    pub const fn source_digest(&self) -> WorkloadProvisionSourceDigest {
        self.source.source_digest()
    }

    pub fn network_plan_digest(&self) -> NetworkPlanDigest {
        self.claim.attempt().network_plan_digest()
    }

    /// Exact portable network content authenticated by the confirmed record.
    pub fn compiled_network_plan(&self) -> &CompiledWorkloadNetworkPlan {
        &self.compiled_network_plan
    }

    /// Exact execution identity retained from the durable teardown origin.
    pub fn execution_locator(&self) -> &WorkloadExecutionReference {
        &self.execution_locator
    }

    /// Exact ordered durable receipts committed before this command.
    pub fn prior_receipt_prefix(&self) -> &WorkloadTeardownReceiptPrefix {
        &self.prior_receipt_prefix
    }

    pub fn selection_evidence(&self) -> Option<&NetworkCapabilitySelectionEvidence> {
        self.claim.attempt().selection_evidence()
    }

    pub fn attempt_id(&self) -> &WorkloadTeardownAttemptId {
        self.claim.attempt().attempt_id()
    }

    pub const fn dispatch_epoch(&self) -> WorkloadTeardownDispatchEpoch {
        self.claim.dispatch_epoch()
    }

    pub fn provider_target(&self) -> &WorkloadTeardownProviderTarget {
        self.claim.provider_target()
    }

    pub fn step(&self) -> WorkloadTeardownStep {
        self.claim.attempt().step()
    }

    pub fn subjects(&self) -> &WorkloadTeardownSubjects {
        self.claim.attempt().subjects()
    }

    pub const fn mode(&self) -> WorkloadTeardownCommandMode {
        self.mode
    }

    pub fn claim(&self) -> &WorkloadTeardownClaim {
        &self.claim
    }
}

/// Closed outcomes accepted from an effect-authorized Execute command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkloadTeardownExecuteOutcome {
    Succeeded(Box<WorkloadTeardownSuccessEvidence>),
    DefiniteFailure(WorkloadFailureEvidence),
    Ambiguous,
}

/// Closed outcomes accepted from a read-only Inspect command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkloadTeardownInspectOutcome {
    Satisfied(Box<WorkloadTeardownSuccessEvidence>),
    NotCompleted(WorkloadOwnerEvidenceDigest),
    DefiniteFailure(WorkloadFailureEvidence),
    InProgress(WorkloadOwnerEvidenceDigest),
    Ambiguous,
}

/// Mode-tagged provider outcome. Providers cannot return an inspection-only
/// result to an Execute command or an effect result to an Inspect command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkloadTeardownProviderOutcome {
    Execute(WorkloadTeardownExecuteOutcome),
    Inspect(WorkloadTeardownInspectOutcome),
}

impl WorkloadTeardownProviderOutcome {
    pub const fn mode(&self) -> WorkloadTeardownCommandMode {
        match self {
            Self::Execute(_) => WorkloadTeardownCommandMode::Execute,
            Self::Inspect(_) => WorkloadTeardownCommandMode::Inspect,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PortableWorkloadTeardownResult {
    Execute(WorkloadTeardownEffectResult),
    Inspect(Box<WorkloadTeardownInspectionResult>),
}

/// Provider outcome correlated to every stable command fence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadTeardownCommandResult {
    command_id: WorkloadTeardownCommandId,
    key: WorkloadSagaKey,
    saga_id: WorkloadSagaId,
    confirmed_revision: WorkloadSagaRevision,
    confirmed_transition_id: WorkloadSagaTransitionId,
    generation: WorkloadGeneration,
    desired_digest: WorkloadDesiredDigest,
    required_node: NodeIdentity,
    source: WorkloadProvisionSourceEvidence,
    source_digest: WorkloadProvisionSourceDigest,
    network_plan_digest: NetworkPlanDigest,
    selection_evidence: Option<NetworkCapabilitySelectionEvidence>,
    execution_locator: WorkloadExecutionReference,
    prior_receipt_prefix: WorkloadTeardownReceiptPrefix,
    attempt_id: WorkloadTeardownAttemptId,
    dispatch_epoch: WorkloadTeardownDispatchEpoch,
    provider_target: WorkloadTeardownProviderTarget,
    subjects: WorkloadTeardownSubjects,
    step: WorkloadTeardownStep,
    mode: WorkloadTeardownCommandMode,
    portable: PortableWorkloadTeardownResult,
}

impl WorkloadTeardownCommandResult {
    pub(super) fn for_command(
        record: &WorkloadSagaRecord,
        command: &ConfirmedWorkloadTeardownCommand,
        outcome: WorkloadTeardownProviderOutcome,
    ) -> Result<Self, WorkloadSagaStoreError> {
        authenticate_confirmed_record(record, command)?;
        if outcome.mode() != command.mode() {
            return Err(nimbus_workloads::WorkloadSagaError::InvalidEvidence(
                "teardown provider outcome mode is crossed with its command",
            )
            .into());
        }
        let portable = match outcome {
            WorkloadTeardownProviderOutcome::Execute(outcome) => {
                PortableWorkloadTeardownResult::Execute(match outcome {
                    WorkloadTeardownExecuteOutcome::Succeeded(evidence) => {
                        WorkloadTeardownEffectResult::Succeeded {
                            attempt_id: command.attempt_id().clone(),
                            dispatch_epoch: command.dispatch_epoch(),
                            provider_target: command.provider_target().clone(),
                            evidence,
                        }
                    }
                    WorkloadTeardownExecuteOutcome::DefiniteFailure(failure) => {
                        WorkloadTeardownEffectResult::DefiniteFailure {
                            attempt_id: command.attempt_id().clone(),
                            dispatch_epoch: command.dispatch_epoch(),
                            provider_target: command.provider_target().clone(),
                            failure,
                        }
                    }
                    WorkloadTeardownExecuteOutcome::Ambiguous => {
                        WorkloadTeardownEffectResult::Ambiguous {
                            attempt_id: command.attempt_id().clone(),
                            dispatch_epoch: command.dispatch_epoch(),
                            provider_target: command.provider_target().clone(),
                        }
                    }
                })
            }
            WorkloadTeardownProviderOutcome::Inspect(outcome) => {
                let inspection_command_id = command.command_id();
                PortableWorkloadTeardownResult::Inspect(Box::new(match outcome {
                    WorkloadTeardownInspectOutcome::Satisfied(evidence) => {
                        WorkloadTeardownInspectionResult::Satisfied {
                            attempt_id: command.attempt_id().clone(),
                            dispatch_epoch: command.dispatch_epoch(),
                            provider_target: command.provider_target().clone(),
                            inspection_command_id,
                            evidence: *evidence,
                        }
                    }
                    WorkloadTeardownInspectOutcome::NotCompleted(evidence) => {
                        WorkloadTeardownInspectionResult::NotCompleted {
                            evidence: WorkloadTeardownRetryEvidence::for_inspection(
                                record,
                                command.claim(),
                                evidence,
                            )?,
                        }
                    }
                    WorkloadTeardownInspectOutcome::DefiniteFailure(failure) => {
                        WorkloadTeardownInspectionResult::DefiniteFailure {
                            attempt_id: command.attempt_id().clone(),
                            dispatch_epoch: command.dispatch_epoch(),
                            provider_target: command.provider_target().clone(),
                            inspection_command_id,
                            failure,
                        }
                    }
                    WorkloadTeardownInspectOutcome::InProgress(evidence) => {
                        WorkloadTeardownInspectionResult::InProgress {
                            attempt_id: command.attempt_id().clone(),
                            dispatch_epoch: command.dispatch_epoch(),
                            provider_target: command.provider_target().clone(),
                            inspection_command_id,
                            evidence,
                        }
                    }
                    WorkloadTeardownInspectOutcome::Ambiguous => {
                        WorkloadTeardownInspectionResult::Ambiguous {
                            attempt_id: command.attempt_id().clone(),
                            dispatch_epoch: command.dispatch_epoch(),
                            provider_target: command.provider_target().clone(),
                            inspection_command_id,
                        }
                    }
                }))
            }
        };
        Ok(Self {
            command_id: command.command_id(),
            key: command.key().clone(),
            saga_id: command.saga_id().clone(),
            confirmed_revision: command.confirmed_revision(),
            confirmed_transition_id: command.confirmed_transition_id().clone(),
            generation: command.generation(),
            desired_digest: command.desired_digest(),
            required_node: command.required_node().clone(),
            source: command.source().clone(),
            source_digest: command.source_digest(),
            network_plan_digest: command.network_plan_digest(),
            selection_evidence: command.selection_evidence().cloned(),
            execution_locator: command.execution_locator().clone(),
            prior_receipt_prefix: command.prior_receipt_prefix().clone(),
            attempt_id: command.attempt_id().clone(),
            dispatch_epoch: command.dispatch_epoch(),
            provider_target: command.provider_target().clone(),
            subjects: command.subjects().clone(),
            step: command.step(),
            mode: command.mode(),
            portable,
        })
    }
}

/// Durable reducer result. `Waiting` retains the exact inspection record and
/// never creates a candidate CAS from provider uncertainty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum WorkloadTeardownResultDecision {
    PersistCandidate(Box<WorkloadSagaRecord>),
    Waiting,
}

pub(super) fn apply_teardown_result(
    record: &WorkloadSagaRecord,
    command: &ConfirmedWorkloadTeardownCommand,
    result: WorkloadTeardownCommandResult,
) -> Result<WorkloadTeardownResultDecision, WorkloadSagaStoreError> {
    authenticate_confirmed_record(record, command)?;
    authenticate_command_result(command, &result)?;
    let candidate = match result.portable {
        PortableWorkloadTeardownResult::Execute(result) => {
            record.apply_teardown_effect_result(command.claim(), result)?
        }
        PortableWorkloadTeardownResult::Inspect(result) => {
            record.apply_teardown_inspection_result(command.claim(), *result)?
        }
    };
    if candidate == *record {
        Ok(WorkloadTeardownResultDecision::Waiting)
    } else {
        Ok(WorkloadTeardownResultDecision::PersistCandidate(Box::new(
            candidate,
        )))
    }
}

fn authenticate_confirmed_record(
    record: &WorkloadSagaRecord,
    command: &ConfirmedWorkloadTeardownCommand,
) -> Result<(), WorkloadSagaStoreError> {
    authenticate_durable_claim(record, command.claim(), command.mode())?;
    let durable_prefix = record.teardown_receipt_prefix_for_claim(command.claim())?;
    if record.key() != command.key()
        || record.saga_id() != command.saga_id()
        || record.revision() != command.confirmed_revision()
        || record.last_transition().transition_id() != command.confirmed_transition_id()
        || record.active_intent().source() != command.source()
        || record.active_intent().network().compiled_plan() != command.compiled_network_plan()
        || command.compiled_network_plan().plan().digest() != command.network_plan_digest()
        || record.phase_detail().references().execution() != Some(command.execution_locator())
        || durable_prefix != *command.prior_receipt_prefix()
    {
        return Err(nimbus_workloads::WorkloadSagaError::InvalidEvidence(
            "teardown command is crossed with its confirmed transition",
        )
        .into());
    }
    Ok(())
}

fn authenticate_command_result(
    command: &ConfirmedWorkloadTeardownCommand,
    result: &WorkloadTeardownCommandResult,
) -> Result<(), WorkloadSagaStoreError> {
    if result.command_id != command.command_id()
        || result.key != *command.key()
        || result.saga_id != *command.saga_id()
        || result.confirmed_revision != command.confirmed_revision()
        || result.confirmed_transition_id != *command.confirmed_transition_id()
        || result.generation != command.generation()
        || result.desired_digest != command.desired_digest()
        || result.required_node != *command.required_node()
        || result.source != *command.source()
        || result.source_digest != command.source_digest()
        || result.network_plan_digest != command.network_plan_digest()
        || result.selection_evidence.as_ref() != command.selection_evidence()
        || result.execution_locator != *command.execution_locator()
        || result.prior_receipt_prefix != *command.prior_receipt_prefix()
        || result.attempt_id != *command.attempt_id()
        || result.dispatch_epoch != command.dispatch_epoch()
        || result.provider_target != *command.provider_target()
        || result.subjects != *command.subjects()
        || result.step != command.step()
        || result.mode != command.mode()
    {
        return Err(nimbus_workloads::WorkloadSagaError::InvalidEvidence(
            "teardown result is crossed with its confirmed command",
        )
        .into());
    }
    Ok(())
}

fn authenticate_durable_claim(
    record: &WorkloadSagaRecord,
    claim: &WorkloadTeardownClaim,
    mode: WorkloadTeardownCommandMode,
) -> Result<(), WorkloadSagaStoreError> {
    let attempt = claim.attempt();
    let intent = record.active_intent();
    let exact_identity = record.key() == attempt.key()
        && record.saga_id() == attempt.saga_id()
        && intent.generation() == attempt.generation()
        && intent.desired_digest() == attempt.desired_digest()
        && intent.admission().assigned_node() == attempt.required_node()
        && intent.source().source_digest() == attempt.source_digest()
        && intent.source().execution_provider_id() == attempt.execution_provider_id()
        && intent.network().digest() == attempt.network_plan_digest()
        && attempt.selection_evidence()
            == intent
                .network()
                .compiled_plan()
                .content()
                .capability_selection_evidence();
    let revision_matches = match mode {
        WorkloadTeardownCommandMode::Execute => record.revision() == claim.claimed_revision(),
        WorkloadTeardownCommandMode::Inspect => {
            claim.claimed_revision().checked_next() == Some(record.revision())
        }
    };
    if !exact_identity || !revision_matches {
        return Err(nimbus_workloads::WorkloadSagaError::InvalidEvidence(
            "teardown command is crossed with durable workload identity",
        )
        .into());
    }
    let disposition_matches = matches!(
        (mode, record.teardown_disposition()),
        (
            WorkloadTeardownCommandMode::Execute,
            Some(WorkloadTeardownDisposition::DispatchPending { claim: retained, .. })
        ) | (
            WorkloadTeardownCommandMode::Inspect,
            Some(WorkloadTeardownDisposition::InspectionRequired { claim: retained, .. })
        ) if retained == claim
    );
    if !disposition_matches {
        return Err(nimbus_workloads::WorkloadSagaError::InvalidTransition(
            "teardown command mode does not match durable dispatch state",
        )
        .into());
    }
    Ok(())
}

/// Exact durable candidate plus any confirmation-gated provider command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmedWorkloadTeardownTransition {
    confirmed_record: Option<WorkloadSagaRecord>,
    confirmation: WorkloadSagaConfirmation,
    command: Option<ConfirmedWorkloadTeardownCommand>,
}

impl ConfirmedWorkloadTeardownTransition {
    pub fn confirmed_record(&self) -> Option<&WorkloadSagaRecord> {
        self.confirmed_record.as_ref()
    }

    pub const fn confirmation(&self) -> WorkloadSagaConfirmation {
        self.confirmation
    }

    pub fn command(&self) -> Option<&ConfirmedWorkloadTeardownCommand> {
        self.command.as_ref()
    }
}

impl WorkloadSagaCoordinator {
    /// Confirm one workloads-owned teardown candidate and grant provider
    /// authority only from the resulting durable state.
    pub(super) async fn confirm_teardown_transition(
        &self,
        loaded: &WorkloadSagaRecord,
        candidate: WorkloadSagaRecord,
    ) -> Result<ConfirmedWorkloadTeardownTransition, WorkloadSagaStoreError> {
        let confirmation = self
            .confirm_transition(Some(loaded), candidate.clone())
            .await?;
        if matches!(
            candidate.teardown_disposition(),
            Some(WorkloadTeardownDisposition::DispatchPending { .. })
        ) && matches!(
            confirmation,
            WorkloadSagaConfirmation::ConfirmedAfterAmbiguity
                | WorkloadSagaConfirmation::ConfirmedReplay
        ) {
            return self.confirm_teardown_inspection(&candidate).await;
        }
        let command = if confirmation_is_durable(confirmation)
            && matches!(
                candidate.teardown_disposition(),
                Some(
                    WorkloadTeardownDisposition::DispatchPending { .. }
                        | WorkloadTeardownDisposition::InspectionRequired { .. }
                )
            ) {
            ConfirmedWorkloadTeardownCommand::from_confirmation(&candidate, confirmation)?
        } else {
            None
        };
        Ok(ConfirmedWorkloadTeardownTransition {
            confirmed_record: confirmation_is_durable(confirmation).then_some(candidate),
            confirmation,
            command,
        })
    }

    /// Persist inspection before a recovered, replayed, or ambiguously
    /// confirmed teardown effect can be observed.
    async fn confirm_teardown_inspection(
        &self,
        pending: &WorkloadSagaRecord,
    ) -> Result<ConfirmedWorkloadTeardownTransition, WorkloadSagaStoreError> {
        let claim = pending
            .teardown_disposition()
            .and_then(WorkloadTeardownDisposition::claim)
            .ok_or(nimbus_workloads::WorkloadSagaError::InvalidTransition(
                "teardown inspection requires an exact pending claim",
            ))?;
        let inspection = pending.teardown_dispatch_to_inspection(claim)?;
        let confirmation = self
            .confirm_transition(Some(pending), inspection.clone())
            .await?;
        let command = if confirmation_is_durable(confirmation) {
            ConfirmedWorkloadTeardownCommand::from_confirmation(&inspection, confirmation)?
        } else {
            None
        };
        Ok(ConfirmedWorkloadTeardownTransition {
            confirmed_record: confirmation_is_durable(confirmation).then_some(inspection),
            confirmation,
            command,
        })
    }

    /// Load one exact recovery key and create inspection authority from
    /// durable truth only.
    pub(super) async fn inspect_confirmed_teardown(
        &self,
        key: &WorkloadSagaKey,
    ) -> Result<ConfirmedWorkloadTeardownTransition, WorkloadSagaStoreError> {
        let record = self.store.load(key).await?.ok_or(
            nimbus_workloads::WorkloadSagaError::InvalidTransition(
                "durable teardown recovery requires an existing record",
            ),
        )?;
        if record.key() != key {
            return Err(WorkloadSagaStoreError::Corrupt);
        }
        if matches!(
            record.teardown_disposition(),
            Some(WorkloadTeardownDisposition::DispatchPending { .. })
        ) {
            return self.confirm_teardown_inspection(&record).await;
        }
        let command = ConfirmedWorkloadTeardownCommand::from_confirmation(
            &record,
            WorkloadSagaConfirmation::ConfirmedReplay,
        )?
        .ok_or(nimbus_workloads::WorkloadSagaError::InvalidTransition(
            "durable teardown recovery requires an inspectable claim",
        ))?;
        Ok(ConfirmedWorkloadTeardownTransition {
            confirmed_record: Some(record),
            confirmation: WorkloadSagaConfirmation::ConfirmedReplay,
            command: Some(command),
        })
    }
}

const fn confirmation_is_durable(confirmation: WorkloadSagaConfirmation) -> bool {
    matches!(
        confirmation,
        WorkloadSagaConfirmation::AppliedByThisCall
            | WorkloadSagaConfirmation::ConfirmedAfterAmbiguity
            | WorkloadSagaConfirmation::ConfirmedReplay
    )
}

#[cfg(test)]
#[path = "teardown_command/tests.rs"]
mod tests;
