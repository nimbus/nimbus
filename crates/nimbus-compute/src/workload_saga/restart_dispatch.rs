//! Confirmation-gated restart commands and exact result reduction.
//!
//! A portable restart claim is durable intent, not provider authority. The
//! sole saga coordinator first confirms the claim. Only the direct CAS winner
//! receives execute authority. Replay, uncertain commit, and fresh-process
//! recovery persist inspection state before any provider read.

use nimbus_workloads::{
    CompiledWorkloadNetworkPlan, WorkloadDesiredDigest, WorkloadExecutableIntent,
    WorkloadExecutionAttemptId, WorkloadExecutionProviderId, WorkloadGeneration,
    WorkloadInspectionVersion, WorkloadPhaseDetail, WorkloadProvisionSourceEvidence,
    WorkloadRestartAbsenceEvidence, WorkloadRestartCommandClaim, WorkloadRestartCommandId,
    WorkloadRestartDispatchEpoch, WorkloadRestartEffectResult, WorkloadRestartEpoch,
    WorkloadRestartEvidenceDigest, WorkloadRestartRequestId, WorkloadRestartStep, WorkloadSagaId,
    WorkloadSagaKey, WorkloadSagaRecord, WorkloadSagaRevision, WorkloadSagaStoreError,
    WorkloadSagaTransitionId,
};

use super::{
    ProposedWorkloadRestartTransition, WorkloadRestartDecision, WorkloadRestartSymbolicAction,
    WorkloadSagaConfirmation, WorkloadSagaCoordinator,
};

/// Whether one confirmed command may execute an effect or only inspect it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkloadRestartCommandMode {
    Execute,
    Inspect,
}

/// Ephemeral provider command created only from an exact durable confirmation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmedWorkloadRestartCommand {
    command_id: WorkloadRestartCommandId,
    key: WorkloadSagaKey,
    saga_id: WorkloadSagaId,
    transition_id: WorkloadSagaTransitionId,
    generation: WorkloadGeneration,
    desired_digest: WorkloadDesiredDigest,
    source: WorkloadProvisionSourceEvidence,
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
    mode: WorkloadRestartCommandMode,
    claim: WorkloadRestartCommandClaim,
    executable: WorkloadExecutableIntent,
    compiled_network_plan: CompiledWorkloadNetworkPlan,
}

impl ConfirmedWorkloadRestartCommand {
    fn from_confirmation(
        record: &WorkloadSagaRecord,
        action: WorkloadRestartSymbolicAction,
        confirmation: WorkloadSagaConfirmation,
    ) -> Result<Option<Self>, WorkloadSagaStoreError> {
        record.validate()?;
        let active = record.restart_state().active().ok_or(
            nimbus_workloads::WorkloadSagaError::InvalidTransition(
                "confirmed restart command requires an active restart",
            ),
        )?;
        let claim = active.disposition().claim().ok_or(
            nimbus_workloads::WorkloadSagaError::InvalidTransition(
                "confirmed restart command requires a durable claim",
            ),
        )?;

        let mode = match (confirmation, action) {
            (
                WorkloadSagaConfirmation::AppliedByThisCall,
                WorkloadRestartSymbolicAction::StartExactAttempt,
            ) => WorkloadRestartCommandMode::Execute,
            (
                WorkloadSagaConfirmation::AppliedByThisCall
                | WorkloadSagaConfirmation::ConfirmedAfterAmbiguity
                | WorkloadSagaConfirmation::ConfirmedReplay,
                WorkloadRestartSymbolicAction::InspectExactAttempt,
            )
            | (
                WorkloadSagaConfirmation::ConfirmedAfterAmbiguity
                | WorkloadSagaConfirmation::ConfirmedReplay,
                WorkloadRestartSymbolicAction::StartExactAttempt,
            ) => WorkloadRestartCommandMode::Inspect,
            (
                WorkloadSagaConfirmation::Conflict { .. }
                | WorkloadSagaConfirmation::UnresolvedAmbiguity,
                _,
            ) => return Ok(None),
        };

        authenticate_exact_restart_confirmation(record, claim, mode)?;
        let admission = active.admission();
        Ok(Some(Self {
            command_id: claim.command_id().clone(),
            key: record.key().clone(),
            saga_id: record.saga_id().clone(),
            transition_id: record.last_transition().transition_id().clone(),
            generation: admission.generation(),
            desired_digest: admission.desired_digest(),
            source: admission.source().clone(),
            source_attempt_id: admission.source_attempt_id().clone(),
            attempt_id: admission.attempt_id().clone(),
            restart_epoch: admission.restart_epoch(),
            dispatch_epoch: claim.dispatch_epoch(),
            request_id: admission.request_id().clone(),
            issuing_revision: claim.issuing_revision(),
            confirmed_revision: record.revision(),
            inspection_version: admission.inspection_version(),
            provider_selection: admission.provider_selection().clone(),
            step: claim.step(),
            mode,
            claim: claim.clone(),
            executable: record.active_intent().executable().clone(),
            compiled_network_plan: record.active_intent().network().compiled_plan().clone(),
        }))
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

    pub const fn mode(&self) -> WorkloadRestartCommandMode {
        self.mode
    }

    pub fn claim(&self) -> &WorkloadRestartCommandClaim {
        &self.claim
    }

    pub fn executable(&self) -> &WorkloadExecutableIntent {
        &self.executable
    }

    pub fn compiled_network_plan(&self) -> &CompiledWorkloadNetworkPlan {
        &self.compiled_network_plan
    }
}

fn authenticate_exact_restart_confirmation(
    record: &WorkloadSagaRecord,
    claim: &WorkloadRestartCommandClaim,
    mode: WorkloadRestartCommandMode,
) -> Result<(), WorkloadSagaStoreError> {
    let active = record.restart_state().active().ok_or(
        nimbus_workloads::WorkloadSagaError::InvalidTransition(
            "restart confirmation lost its active request",
        ),
    )?;
    let admission = active.admission();
    let claim_revision_matches = match mode {
        WorkloadRestartCommandMode::Execute => {
            claim.issuing_revision().checked_next() == Some(record.revision())
        }
        WorkloadRestartCommandMode::Inspect => {
            claim
                .issuing_revision()
                .checked_next()
                .and_then(WorkloadSagaRevision::checked_next)
                == Some(record.revision())
        }
    };
    let exact = record.saga_id() == admission.saga_id()
        && record.active_intent().generation() == admission.generation()
        && record.active_intent().desired_digest() == admission.desired_digest()
        && record.active_intent().source() == admission.source()
        && admission.source().execution_provider_id() == admission.provider_selection()
        && claim.request_id() == admission.request_id()
        && claim.restart_epoch() == admission.restart_epoch()
        && claim.attempt_id() == admission.attempt_id()
        && claim_revision_matches;
    if !exact {
        return Err(nimbus_workloads::WorkloadSagaError::InvalidEvidence(
            "restart command confirmation is crossed with its durable admission",
        )
        .into());
    }

    let disposition_matches = match (mode, active.disposition()) {
        (
            WorkloadRestartCommandMode::Execute,
            nimbus_workloads::WorkloadRestartDisposition::DispatchPending { claim: retained },
        )
        | (
            WorkloadRestartCommandMode::Inspect,
            nimbus_workloads::WorkloadRestartDisposition::InspectionRequired { claim: retained },
        ) => retained == claim,
        _ => false,
    };
    if !disposition_matches {
        return Err(nimbus_workloads::WorkloadSagaError::InvalidTransition(
            "restart command mode does not match durable dispatch state",
        )
        .into());
    }
    Ok(())
}

/// Closed provider observation for one exact restart command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkloadRestartCommandOutcome {
    AuthenticatedAbsent {
        evidence: WorkloadRestartEvidenceDigest,
    },
    Ambiguous,
    InProgress {
        evidence: WorkloadRestartEvidenceDigest,
    },
    DefiniteFailure {
        evidence: WorkloadRestartEvidenceDigest,
    },
    Succeeded {
        evidence: WorkloadRestartEvidenceDigest,
        observed_detail: Option<Box<WorkloadPhaseDetail>>,
    },
}

/// Provider result correlated to every stable command fence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadRestartCommandResult {
    command_id: WorkloadRestartCommandId,
    transition_id: WorkloadSagaTransitionId,
    attempt_id: WorkloadExecutionAttemptId,
    dispatch_epoch: WorkloadRestartDispatchEpoch,
    provider_selection: WorkloadExecutionProviderId,
    outcome: WorkloadRestartCommandOutcome,
}

impl WorkloadRestartCommandResult {
    pub fn for_command(
        command: &ConfirmedWorkloadRestartCommand,
        outcome: WorkloadRestartCommandOutcome,
    ) -> Self {
        Self {
            command_id: command.command_id.clone(),
            transition_id: command.transition_id.clone(),
            attempt_id: command.attempt_id.clone(),
            dispatch_epoch: command.dispatch_epoch,
            provider_selection: command.provider_selection.clone(),
            outcome,
        }
    }

    pub fn outcome(&self) -> &WorkloadRestartCommandOutcome {
        &self.outcome
    }
}

/// Reduce one exactly correlated restart result without invoking a provider.
pub fn apply_restart_result(
    record: &WorkloadSagaRecord,
    command: &ConfirmedWorkloadRestartCommand,
    result: WorkloadRestartCommandResult,
) -> Result<WorkloadRestartDecision, WorkloadSagaStoreError> {
    authenticate_result_transition(record, command, &result)?;
    authenticate_result_attempt(record, command, &result)?;
    authenticate_result_dispatch_epoch(record, command, &result)?;

    match result.outcome {
        WorkloadRestartCommandOutcome::AuthenticatedAbsent { evidence } => {
            retry_after_authenticated_absence(record, command, evidence)
        }
        WorkloadRestartCommandOutcome::Ambiguous
        | WorkloadRestartCommandOutcome::InProgress { .. } => {
            if command.mode == WorkloadRestartCommandMode::Execute {
                let candidate = record.restart_dispatch_to_inspection(command.claim())?;
                Ok(WorkloadRestartDecision::Proposed(
                    ProposedWorkloadRestartTransition::new(
                        candidate,
                        Some(WorkloadRestartSymbolicAction::InspectExactAttempt),
                    ),
                ))
            } else {
                Ok(retain_restart_inspection(command))
            }
        }
        WorkloadRestartCommandOutcome::DefiniteFailure { evidence } => {
            let candidate = record.apply_restart_effect_result(
                command.claim(),
                WorkloadRestartEffectResult::Failed { evidence },
                None,
            )?;
            Ok(stop_restart_dispatch(candidate))
        }
        WorkloadRestartCommandOutcome::Succeeded {
            evidence,
            observed_detail,
        } => {
            let candidate = record.apply_restart_effect_result(
                command.claim(),
                WorkloadRestartEffectResult::Succeeded { evidence },
                observed_detail.map(|detail| *detail),
            )?;
            Ok(WorkloadRestartDecision::Proposed(
                ProposedWorkloadRestartTransition::new(candidate, None),
            ))
        }
    }
}

fn authenticate_result_transition(
    record: &WorkloadSagaRecord,
    command: &ConfirmedWorkloadRestartCommand,
    result: &WorkloadRestartCommandResult,
) -> Result<(), WorkloadSagaStoreError> {
    if result.command_id != command.command_id
        || result.transition_id != command.transition_id
        || record.key() != command.key()
        || record.saga_id() != command.saga_id()
        || record.revision() != command.confirmed_revision
        || record.last_transition().transition_id() != command.transition_id()
    {
        return Err(nimbus_workloads::WorkloadSagaError::InvalidEvidence(
            "restart result is crossed with its confirmed transition",
        )
        .into());
    }
    Ok(())
}

fn authenticate_result_attempt(
    record: &WorkloadSagaRecord,
    command: &ConfirmedWorkloadRestartCommand,
    result: &WorkloadRestartCommandResult,
) -> Result<(), WorkloadSagaStoreError> {
    let active = record.restart_state().active().ok_or(
        nimbus_workloads::WorkloadSagaError::InvalidTransition(
            "restart result requires an active durable request",
        ),
    )?;
    if result.attempt_id != command.attempt_id
        || result.provider_selection != command.provider_selection
        || active.admission().attempt_id() != command.attempt_id()
        || active.admission().source_attempt_id() != command.source_attempt_id()
        || active.admission().generation() != command.generation()
        || active.admission().desired_digest() != command.desired_digest()
        || active.admission().source() != command.source()
        || active.admission().provider_selection() != command.provider_selection()
    {
        return Err(nimbus_workloads::WorkloadSagaError::InvalidEvidence(
            "restart result is crossed with its execution attempt",
        )
        .into());
    }
    Ok(())
}

fn authenticate_result_dispatch_epoch(
    record: &WorkloadSagaRecord,
    command: &ConfirmedWorkloadRestartCommand,
    result: &WorkloadRestartCommandResult,
) -> Result<(), WorkloadSagaStoreError> {
    let claim = record
        .restart_state()
        .active()
        .and_then(|active| active.disposition().claim());
    if result.dispatch_epoch != command.dispatch_epoch
        || claim != Some(command.claim())
        || command.claim().dispatch_epoch() != command.dispatch_epoch()
        || command.claim().command_id() != command.command_id()
        || command.claim().request_id() != command.request_id()
        || command.claim().restart_epoch() != command.restart_epoch()
        || command.claim().step() != command.step()
    {
        return Err(nimbus_workloads::WorkloadSagaError::InvalidEvidence(
            "restart result is crossed with its dispatch epoch",
        )
        .into());
    }
    Ok(())
}

fn retain_restart_inspection(command: &ConfirmedWorkloadRestartCommand) -> WorkloadRestartDecision {
    WorkloadRestartDecision::InspectExact(Box::new(command.claim.clone()))
}

fn retry_after_authenticated_absence(
    record: &WorkloadSagaRecord,
    command: &ConfirmedWorkloadRestartCommand,
    evidence: WorkloadRestartEvidenceDigest,
) -> Result<WorkloadRestartDecision, WorkloadSagaStoreError> {
    if command.mode != WorkloadRestartCommandMode::Inspect {
        return Err(nimbus_workloads::WorkloadSagaError::InvalidEvidence(
            "only exact inspection may authorize restart retry",
        )
        .into());
    }
    let absence =
        WorkloadRestartAbsenceEvidence::for_inspection(record, command.claim(), evidence)?;
    let candidate = record.restart_inspection_to_retry(command.claim(), absence)?;
    Ok(WorkloadRestartDecision::Proposed(
        ProposedWorkloadRestartTransition::new(
            candidate,
            Some(WorkloadRestartSymbolicAction::StartExactAttempt),
        ),
    ))
}

fn stop_restart_dispatch(candidate: WorkloadSagaRecord) -> WorkloadRestartDecision {
    WorkloadRestartDecision::Proposed(ProposedWorkloadRestartTransition::new(candidate, None))
}

/// Exact durable candidate plus its provenance-gated restart command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmedWorkloadRestartTransition {
    confirmed_record: Option<WorkloadSagaRecord>,
    confirmation: WorkloadSagaConfirmation,
    command: Option<ConfirmedWorkloadRestartCommand>,
}

impl ConfirmedWorkloadRestartTransition {
    pub fn confirmed_record(&self) -> Option<&WorkloadSagaRecord> {
        self.confirmed_record.as_ref()
    }

    pub const fn confirmation(&self) -> WorkloadSagaConfirmation {
        self.confirmation
    }

    pub fn command(&self) -> Option<&ConfirmedWorkloadRestartCommand> {
        self.command.as_ref()
    }
}

impl WorkloadSagaCoordinator {
    /// Confirm one restart candidate and grant provider authority when safe.
    pub async fn claim_restart_command(
        &self,
        loaded: &WorkloadSagaRecord,
        proposed: &ProposedWorkloadRestartTransition,
    ) -> Result<ConfirmedWorkloadRestartTransition, WorkloadSagaStoreError> {
        let candidate = proposed.candidate().clone();
        let confirmation = self
            .confirm_transition(Some(loaded), candidate.clone())
            .await?;
        if proposed.action_after_confirmation()
            == Some(WorkloadRestartSymbolicAction::StartExactAttempt)
            && matches!(
                confirmation,
                WorkloadSagaConfirmation::ConfirmedAfterAmbiguity
                    | WorkloadSagaConfirmation::ConfirmedReplay
            )
        {
            return self.inspect_ambiguous_restart(&candidate).await;
        }
        let command = match proposed.action_after_confirmation() {
            Some(action) => ConfirmedWorkloadRestartCommand::from_confirmation(
                &candidate,
                action,
                confirmation,
            )?,
            None => None,
        };
        let confirmed_record = confirmation_is_durable(confirmation).then_some(candidate);
        Ok(ConfirmedWorkloadRestartTransition {
            confirmed_record,
            confirmation,
            command,
        })
    }

    /// Persist inspection before a replay, recovered claim, or uncertain effect read.
    async fn inspect_ambiguous_restart(
        &self,
        pending: &WorkloadSagaRecord,
    ) -> Result<ConfirmedWorkloadRestartTransition, WorkloadSagaStoreError> {
        let claim = pending
            .restart_state()
            .active()
            .and_then(|active| active.disposition().claim())
            .ok_or(nimbus_workloads::WorkloadSagaError::InvalidTransition(
                "restart inspection requires an exact pending claim",
            ))?;
        let inspection = pending.restart_dispatch_to_inspection(claim)?;
        let confirmation = self
            .confirm_transition(Some(pending), inspection.clone())
            .await?;
        let command = ConfirmedWorkloadRestartCommand::from_confirmation(
            &inspection,
            WorkloadRestartSymbolicAction::InspectExactAttempt,
            confirmation,
        )?;
        let confirmed_record = confirmation_is_durable(confirmation).then_some(inspection);
        Ok(ConfirmedWorkloadRestartTransition {
            confirmed_record,
            confirmation,
            command,
        })
    }

    /// Load exact durable restart recovery state and emit inspection only.
    pub async fn inspect_confirmed_restart(
        &self,
        key: &WorkloadSagaKey,
    ) -> Result<ConfirmedWorkloadRestartTransition, WorkloadSagaStoreError> {
        let record = self.store.load(key).await?.ok_or(
            nimbus_workloads::WorkloadSagaError::InvalidTransition(
                "durable restart recovery requires an existing record",
            ),
        )?;
        if record.key() != key {
            return Err(WorkloadSagaStoreError::Corrupt);
        }
        if matches!(
            record
                .restart_state()
                .active()
                .map(|active| active.disposition()),
            Some(nimbus_workloads::WorkloadRestartDisposition::DispatchPending { .. })
        ) {
            return self.inspect_ambiguous_restart(&record).await;
        }
        let command = ConfirmedWorkloadRestartCommand::from_confirmation(
            &record,
            WorkloadRestartSymbolicAction::InspectExactAttempt,
            WorkloadSagaConfirmation::ConfirmedReplay,
        )?
        .ok_or(nimbus_workloads::WorkloadSagaError::InvalidTransition(
            "durable restart recovery requires an inspectable claim",
        ))?;
        Ok(ConfirmedWorkloadRestartTransition {
            confirmed_record: Some(record),
            confirmation: WorkloadSagaConfirmation::ConfirmedReplay,
            command: Some(command),
        })
    }

    /// Confirm a pure restart-result candidate through the sole saga store.
    pub async fn compare_and_swap_restart_result(
        &self,
        loaded: &WorkloadSagaRecord,
        proposed: &ProposedWorkloadRestartTransition,
    ) -> Result<WorkloadSagaConfirmation, WorkloadSagaStoreError> {
        self.confirm_transition(Some(loaded), proposed.candidate().clone())
            .await
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
#[path = "restart_dispatch/tests.rs"]
mod tests;
