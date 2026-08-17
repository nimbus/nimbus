//! Confirmation-gated workload-provision commands and result reduction.
//!
//! A pure candidate is never a provider command. The sole saga coordinator
//! first confirms the candidate through the durable store; this module then
//! preserves how that confirmation happened and grants execute authority only
//! to the direct compare-and-swap winner.

use nimbus_network::NetworkPlanDigest;
use nimbus_workloads::{
    CompiledWorkloadNetworkPlan, WorkloadDesiredDigest, WorkloadExecutableIntent,
    WorkloadExecutionReference, WorkloadGeneration, WorkloadOwnerEvidenceDigest,
    WorkloadProvisionAbsenceEvidence, WorkloadProvisionAttemptId, WorkloadProvisionCommandId,
    WorkloadProvisionCommandMode, WorkloadProvisionDispatchAuthorization,
    WorkloadProvisionDispatchClaim, WorkloadProvisionDispatchEpoch, WorkloadProvisionEffectResult,
    WorkloadProvisionInspectionResult, WorkloadProvisionPrerequisiteEvidence,
    WorkloadProvisionProviderTarget, WorkloadProvisionSourceDigest,
    WorkloadProvisionSourceEvidence, WorkloadProvisionStep, WorkloadProvisionSubjects,
    WorkloadSagaCommit, WorkloadSagaExpected, WorkloadSagaId, WorkloadSagaKey, WorkloadSagaRecord,
    WorkloadSagaRevision, WorkloadSagaStoreError, WorkloadSagaTransitionId,
};

use super::{
    ProposedWorkloadProvisionTransition, WorkloadProvisionDecision,
    WorkloadProvisionSymbolicAction, WorkloadSagaCoordinator,
};

/// Provenance of one exact candidate confirmation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkloadSagaConfirmation {
    AppliedByThisCall,
    ConfirmedAfterAmbiguity,
    ConfirmedReplay,
    Conflict {
        expected: WorkloadSagaExpected,
        observed: Option<WorkloadSagaRevision>,
    },
    UnresolvedAmbiguity,
}

/// Ephemeral command created only from an exact coordinator confirmation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmedWorkloadProvisionCommand {
    command_id: WorkloadProvisionCommandId,
    key: WorkloadSagaKey,
    saga_id: WorkloadSagaId,
    attempt_id: WorkloadProvisionAttemptId,
    issuing_revision: WorkloadSagaRevision,
    confirmed_revision: WorkloadSagaRevision,
    transition_id: WorkloadSagaTransitionId,
    generation: WorkloadGeneration,
    desired_digest: WorkloadDesiredDigest,
    source: WorkloadProvisionSourceEvidence,
    network_plan_digest: NetworkPlanDigest,
    provider_target: WorkloadProvisionProviderTarget,
    execution: WorkloadExecutionReference,
    source_phase: nimbus_workloads::WorkloadSagaPhase,
    target_phase: nimbus_workloads::WorkloadSagaPhase,
    step: WorkloadProvisionStep,
    subjects: WorkloadProvisionSubjects,
    prerequisite: Option<WorkloadProvisionPrerequisiteEvidence>,
    dispatch_epoch: WorkloadProvisionDispatchEpoch,
    authorization: WorkloadProvisionDispatchAuthorization,
    mode: WorkloadProvisionCommandMode,
    claim: WorkloadProvisionDispatchClaim,
    executable: WorkloadExecutableIntent,
    compiled_network_plan: CompiledWorkloadNetworkPlan,
}

impl ConfirmedWorkloadProvisionCommand {
    fn from_confirmation(
        record: &WorkloadSagaRecord,
        action: WorkloadProvisionSymbolicAction,
        confirmation: WorkloadSagaConfirmation,
    ) -> Result<Option<Self>, WorkloadSagaStoreError> {
        record.validate()?;
        let Some(claim) = record
            .provision_disposition()
            .and_then(nimbus_workloads::WorkloadProvisionDisposition::claim)
        else {
            return Err(nimbus_workloads::WorkloadSagaError::InvalidTransition(
                "confirmed provision command requires a durable dispatch claim",
            )
            .into());
        };

        let inspection_only = matches!(
            claim.attempt().step(),
            WorkloadProvisionStep::InspectActivationPrerequisites
                | WorkloadProvisionStep::InspectWorkloadReadiness
                | WorkloadProvisionStep::ObservePublication
        );
        let mode = match (confirmation, action, inspection_only) {
            (
                WorkloadSagaConfirmation::AppliedByThisCall,
                WorkloadProvisionSymbolicAction::StartExactAttempt,
                false,
            ) => WorkloadProvisionCommandMode::Execute,
            (
                WorkloadSagaConfirmation::AppliedByThisCall,
                WorkloadProvisionSymbolicAction::StartExactAttempt,
                true,
            ) => WorkloadProvisionCommandMode::Inspect,
            (
                WorkloadSagaConfirmation::AppliedByThisCall
                | WorkloadSagaConfirmation::ConfirmedAfterAmbiguity
                | WorkloadSagaConfirmation::ConfirmedReplay,
                WorkloadProvisionSymbolicAction::InspectExactAttempt,
                _,
            )
            | (
                WorkloadSagaConfirmation::ConfirmedAfterAmbiguity
                | WorkloadSagaConfirmation::ConfirmedReplay,
                WorkloadProvisionSymbolicAction::StartExactAttempt,
                _,
            ) => WorkloadProvisionCommandMode::Inspect,
            (
                WorkloadSagaConfirmation::Conflict { .. }
                | WorkloadSagaConfirmation::UnresolvedAmbiguity,
                _,
                _,
            ) => return Ok(None),
        };

        if action == WorkloadProvisionSymbolicAction::StartExactAttempt
            && !matches!(
                record.provision_disposition(),
                Some(nimbus_workloads::WorkloadProvisionDisposition::DispatchPending(_))
            )
        {
            return Err(nimbus_workloads::WorkloadSagaError::InvalidTransition(
                "execute authorization requires an exact pending dispatch claim",
            )
            .into());
        }

        let attempt = claim.attempt();
        let execution = record.current_execution_reference();
        let command_id = WorkloadProvisionCommandId::for_confirmed_dispatch(
            claim,
            record.revision(),
            record.last_transition().transition_id(),
            &execution,
            mode,
        )?;
        let prerequisite = attempt.prerequisite().cloned();

        Ok(Some(Self {
            command_id,
            key: attempt.key().clone(),
            saga_id: attempt.saga_id().clone(),
            attempt_id: attempt.attempt_id().clone(),
            issuing_revision: attempt.issuing_revision(),
            confirmed_revision: record.revision(),
            transition_id: record.last_transition().transition_id().clone(),
            generation: attempt.generation(),
            desired_digest: attempt.desired_digest(),
            source: record.active_intent().source().clone(),
            network_plan_digest: attempt.network_plan_digest(),
            provider_target: claim.provider_target().clone(),
            execution,
            source_phase: attempt.source_phase(),
            target_phase: attempt.target_phase(),
            step: attempt.step(),
            subjects: attempt.subjects().clone(),
            prerequisite,
            dispatch_epoch: claim.dispatch_epoch(),
            authorization: claim.authorization().clone(),
            mode,
            claim: claim.clone(),
            executable: record.active_intent().executable().clone(),
            compiled_network_plan: record.active_intent().network().compiled_plan().clone(),
        }))
    }

    pub const fn command_id(&self) -> WorkloadProvisionCommandId {
        self.command_id
    }

    pub fn key(&self) -> &WorkloadSagaKey {
        &self.key
    }

    pub fn saga_id(&self) -> &WorkloadSagaId {
        &self.saga_id
    }

    pub fn attempt_id(&self) -> &WorkloadProvisionAttemptId {
        &self.attempt_id
    }

    pub const fn issuing_revision(&self) -> WorkloadSagaRevision {
        self.issuing_revision
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

    /// Complete source-owner evidence that authenticates this executable.
    pub fn source(&self) -> &WorkloadProvisionSourceEvidence {
        &self.source
    }

    pub const fn network_plan_digest(&self) -> NetworkPlanDigest {
        self.network_plan_digest
    }

    pub fn provider_target(&self) -> &WorkloadProvisionProviderTarget {
        &self.provider_target
    }

    /// Exact generation-scoped execution identity shared by network and
    /// execution provider phases.
    pub fn execution(&self) -> &WorkloadExecutionReference {
        &self.execution
    }

    pub const fn step(&self) -> WorkloadProvisionStep {
        self.step
    }

    pub fn subjects(&self) -> &WorkloadProvisionSubjects {
        &self.subjects
    }

    pub const fn dispatch_epoch(&self) -> WorkloadProvisionDispatchEpoch {
        self.dispatch_epoch
    }

    pub const fn mode(&self) -> WorkloadProvisionCommandMode {
        self.mode
    }

    pub fn claim(&self) -> &WorkloadProvisionDispatchClaim {
        &self.claim
    }

    /// Exact source-owned executable authenticated by this command's source digest.
    pub fn executable(&self) -> &WorkloadExecutableIntent {
        &self.executable
    }

    /// Complete provider-neutral network plan authenticated by this command's plan digest.
    pub fn compiled_network_plan(&self) -> &CompiledWorkloadNetworkPlan {
        &self.compiled_network_plan
    }

    pub fn absence_evidence(
        &self,
        evidence: WorkloadOwnerEvidenceDigest,
    ) -> WorkloadProvisionAbsenceEvidence {
        WorkloadProvisionAbsenceEvidence::for_confirmation(
            &self.claim,
            self.confirmed_revision,
            self.transition_id.clone(),
            evidence,
        )
    }
}

/// Closed provider result retaining the exact command fence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadProvisionCommandResult {
    command_id: WorkloadProvisionCommandId,
    attempt_id: WorkloadProvisionAttemptId,
    dispatch_epoch: WorkloadProvisionDispatchEpoch,
    provider_target: WorkloadProvisionProviderTarget,
    outcome: WorkloadProvisionInspectionResult,
}

impl WorkloadProvisionCommandResult {
    pub fn for_command(
        command: &ConfirmedWorkloadProvisionCommand,
        outcome: WorkloadProvisionInspectionResult,
    ) -> Result<Self, WorkloadSagaStoreError> {
        if !outcome_matches_command(command, &outcome) {
            return Err(nimbus_workloads::WorkloadSagaError::InvalidEvidence(
                "provision result is crossed with its confirmed command",
            )
            .into());
        }
        if command.mode == WorkloadProvisionCommandMode::Execute
            && matches!(outcome, WorkloadProvisionInspectionResult::Absent { .. })
        {
            return Err(nimbus_workloads::WorkloadSagaError::InvalidEvidence(
                "only an inspection command may report provider absence",
            )
            .into());
        }
        if matches!(outcome, WorkloadProvisionInspectionResult::Absent { .. })
            && matches!(
                command.step,
                WorkloadProvisionStep::InspectActivationPrerequisites
                    | WorkloadProvisionStep::InspectWorkloadReadiness
            )
        {
            return Err(nimbus_workloads::WorkloadSagaError::InvalidEvidence(
                "inspection-only provision steps cannot authorize execution retry",
            )
            .into());
        }
        Ok(Self {
            command_id: command.command_id,
            attempt_id: command.attempt_id.clone(),
            dispatch_epoch: command.dispatch_epoch,
            provider_target: command.provider_target.clone(),
            outcome,
        })
    }

    pub const fn command_id(&self) -> WorkloadProvisionCommandId {
        self.command_id
    }

    pub fn outcome(&self) -> &WorkloadProvisionInspectionResult {
        &self.outcome
    }
}

fn outcome_matches_command(
    command: &ConfirmedWorkloadProvisionCommand,
    outcome: &WorkloadProvisionInspectionResult,
) -> bool {
    match outcome {
        WorkloadProvisionInspectionResult::Absent { evidence } => {
            evidence.attempt_id() == command.attempt_id()
                && evidence.dispatch_epoch() == command.dispatch_epoch()
                && evidence.confirmed_revision() == command.confirmed_revision()
                && evidence.transition_id() == command.transition_id()
                && evidence.provider_target() == command.provider_target()
                && evidence.step() == command.step()
        }
        WorkloadProvisionInspectionResult::Ambiguous {
            attempt_id,
            dispatch_epoch,
            provider_target,
        }
        | WorkloadProvisionInspectionResult::DefiniteFailure {
            attempt_id,
            dispatch_epoch,
            provider_target,
            ..
        }
        | WorkloadProvisionInspectionResult::InProgress {
            attempt_id,
            dispatch_epoch,
            provider_target,
            ..
        }
        | WorkloadProvisionInspectionResult::Succeeded {
            attempt_id,
            dispatch_epoch,
            provider_target,
            ..
        } => {
            attempt_id == command.attempt_id()
                && *dispatch_epoch == command.dispatch_epoch()
                && provider_target == command.provider_target()
        }
    }
}

/// Reduce one exactly correlated command result without invoking a provider.
pub fn reduce_command_result(
    record: &WorkloadSagaRecord,
    command: &ConfirmedWorkloadProvisionCommand,
    result: WorkloadProvisionCommandResult,
) -> Result<WorkloadProvisionDecision, WorkloadSagaStoreError> {
    if result.command_id != command.command_id
        || result.attempt_id != command.attempt_id
        || result.dispatch_epoch != command.dispatch_epoch
        || result.provider_target != command.provider_target
        || record.key() != command.key()
        || record.saga_id() != command.saga_id()
        || record.revision() != command.confirmed_revision
        || record.last_transition().transition_id() != command.transition_id()
        || record
            .provision_disposition()
            .and_then(nimbus_workloads::WorkloadProvisionDisposition::claim)
            != Some(command.claim())
    {
        return Err(nimbus_workloads::WorkloadSagaError::InvalidEvidence(
            "provision command result is crossed with durable state",
        )
        .into());
    }
    result
        .outcome
        .validate_for_record(record, command.claim())?;

    match result.outcome {
        WorkloadProvisionInspectionResult::Absent { evidence } => {
            if command.mode != WorkloadProvisionCommandMode::Inspect {
                return Err(nimbus_workloads::WorkloadSagaError::InvalidEvidence(
                    "execute result cannot authorize absence retry",
                )
                .into());
            }
            let candidate = match (command.step, record.successor_intent().is_some()) {
                (_, true) => record.provision_inspection_absence_to_teardown(evidence)?,
                (WorkloadProvisionStep::ObservePublication, false) => {
                    record.publication_observation_absence_to_republication(evidence)?
                }
                (_, _) => record.inspection_to_retry_dispatch(evidence)?,
            };
            Ok(WorkloadProvisionDecision::Proposed(
                ProposedWorkloadProvisionTransition::new(
                    candidate,
                    (record.successor_intent().is_none())
                        .then_some(WorkloadProvisionSymbolicAction::StartExactAttempt),
                ),
            ))
        }
        WorkloadProvisionInspectionResult::Ambiguous { .. }
        | WorkloadProvisionInspectionResult::InProgress { .. } => {
            if command.mode == WorkloadProvisionCommandMode::Execute {
                WorkloadProvisionDecision::reduce(
                    record,
                    WorkloadProvisionEffectResult::Ambiguous {
                        attempt_id: command.attempt_id.clone(),
                    },
                )
                .map_err(Into::into)
            } else {
                Ok(WorkloadProvisionDecision::InspectExact(Box::new(
                    command.claim.clone(),
                )))
            }
        }
        WorkloadProvisionInspectionResult::DefiniteFailure { failure, .. } => {
            WorkloadProvisionDecision::reduce(
                record,
                WorkloadProvisionEffectResult::DefiniteFailure {
                    attempt_id: command.attempt_id.clone(),
                    failure,
                },
            )
            .map_err(Into::into)
        }
        WorkloadProvisionInspectionResult::Succeeded { evidence, .. } => {
            WorkloadProvisionDecision::reduce(
                record,
                WorkloadProvisionEffectResult::Succeeded {
                    attempt_id: command.attempt_id.clone(),
                    evidence,
                },
            )
            .map_err(Into::into)
        }
    }
}

/// Exact durable candidate plus its provenance-gated provider command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmedWorkloadProvisionTransition {
    confirmed_record: Option<WorkloadSagaRecord>,
    confirmation: WorkloadSagaConfirmation,
    command: Option<ConfirmedWorkloadProvisionCommand>,
}

impl ConfirmedWorkloadProvisionTransition {
    /// Return durable candidate truth only when the store confirmed it.
    pub fn confirmed_record(&self) -> Option<&WorkloadSagaRecord> {
        self.confirmed_record.as_ref()
    }

    pub const fn confirmation(&self) -> WorkloadSagaConfirmation {
        self.confirmation
    }

    pub fn command(&self) -> Option<&ConfirmedWorkloadProvisionCommand> {
        self.command.as_ref()
    }
}

impl WorkloadSagaCoordinator {
    /// Confirm a pure candidate and gate any resulting provider command.
    pub(super) async fn confirm_provision_transition(
        &self,
        loaded: &WorkloadSagaRecord,
        proposed: &ProposedWorkloadProvisionTransition,
    ) -> Result<ConfirmedWorkloadProvisionTransition, WorkloadSagaStoreError> {
        let candidate = proposed.candidate().clone();
        let confirmation = self
            .confirm_transition(Some(loaded), candidate.clone())
            .await?;
        if proposed.action_after_confirmation()
            == Some(WorkloadProvisionSymbolicAction::StartExactAttempt)
            && matches!(
                confirmation,
                WorkloadSagaConfirmation::ConfirmedAfterAmbiguity
                    | WorkloadSagaConfirmation::ConfirmedReplay
            )
        {
            return self.confirm_inspection_transition(&candidate).await;
        }
        let command = match proposed.action_after_confirmation() {
            Some(action) => ConfirmedWorkloadProvisionCommand::from_confirmation(
                &candidate,
                action,
                confirmation,
            )?,
            None => None,
        };
        let confirmed_record = matches!(
            confirmation,
            WorkloadSagaConfirmation::AppliedByThisCall
                | WorkloadSagaConfirmation::ConfirmedAfterAmbiguity
                | WorkloadSagaConfirmation::ConfirmedReplay
        )
        .then_some(candidate);
        Ok(ConfirmedWorkloadProvisionTransition {
            confirmed_record,
            confirmation,
            command,
        })
    }

    /// Persist the exact inspection-required boundary before provider reads.
    ///
    /// A replayed or recovered pending claim is evidence that an effect may
    /// already have happened. The inspection result cannot authorize a retry
    /// until this transition is durable; otherwise a crash can skip the
    /// inspected-claim witness and turn absence into fresh effect authority.
    async fn confirm_inspection_transition(
        &self,
        pending: &WorkloadSagaRecord,
    ) -> Result<ConfirmedWorkloadProvisionTransition, WorkloadSagaStoreError> {
        let inspection = pending.dispatch_to_inspection()?;
        let confirmation = self
            .confirm_transition(Some(pending), inspection.clone())
            .await?;
        let command = ConfirmedWorkloadProvisionCommand::from_confirmation(
            &inspection,
            WorkloadProvisionSymbolicAction::InspectExactAttempt,
            confirmation,
        )?;
        let confirmed_record = matches!(
            confirmation,
            WorkloadSagaConfirmation::AppliedByThisCall
                | WorkloadSagaConfirmation::ConfirmedAfterAmbiguity
                | WorkloadSagaConfirmation::ConfirmedReplay
        )
        .then_some(inspection);
        Ok(ConfirmedWorkloadProvisionTransition {
            confirmed_record,
            confirmation,
            command,
        })
    }

    /// Load exact durable recovery state before creating an inspection command.
    pub(super) async fn inspect_confirmed_provision(
        &self,
        key: &WorkloadSagaKey,
    ) -> Result<ConfirmedWorkloadProvisionTransition, WorkloadSagaStoreError> {
        let record = self.store.load(key).await?.ok_or({
            nimbus_workloads::WorkloadSagaError::InvalidTransition(
                "durable provision recovery requires an existing record",
            )
        })?;
        if record.key() != key {
            return Err(WorkloadSagaStoreError::Corrupt);
        }
        if matches!(
            record.provision_disposition(),
            Some(nimbus_workloads::WorkloadProvisionDisposition::DispatchPending(_))
        ) {
            return self.confirm_inspection_transition(&record).await;
        }
        let command = ConfirmedWorkloadProvisionCommand::from_confirmation(
            &record,
            WorkloadProvisionSymbolicAction::InspectExactAttempt,
            WorkloadSagaConfirmation::ConfirmedReplay,
        )?
        .ok_or({
            WorkloadSagaStoreError::InvalidTransition(
                nimbus_workloads::WorkloadSagaError::InvalidTransition(
                    "durable provision recovery requires an inspectable claim",
                ),
            )
        })?;
        Ok(ConfirmedWorkloadProvisionTransition {
            confirmed_record: Some(record),
            confirmation: WorkloadSagaConfirmation::ConfirmedReplay,
            command: Some(command),
        })
    }

    pub(super) async fn confirm_transition(
        &self,
        loaded: Option<&WorkloadSagaRecord>,
        next: WorkloadSagaRecord,
    ) -> Result<WorkloadSagaConfirmation, WorkloadSagaStoreError> {
        let expected = match loaded {
            Some(current) => {
                current.validate_successor(&next)?;
                WorkloadSagaExpected::Revision(current.revision())
            }
            None => {
                next.validate()?;
                if next.revision().as_u64() != 0 || next.last_transition().source_phase().is_some()
                {
                    return Err(nimbus_workloads::WorkloadSagaError::InvalidTransition(
                        "missing-store creation requires the initial revision",
                    )
                    .into());
                }
                WorkloadSagaExpected::Missing
            }
        };

        match self.store.compare_and_swap(expected, next.clone()).await {
            Ok(WorkloadSagaCommit::Applied) => Ok(WorkloadSagaConfirmation::AppliedByThisCall),
            Ok(WorkloadSagaCommit::Unchanged) => Ok(WorkloadSagaConfirmation::ConfirmedReplay),
            Err(WorkloadSagaStoreError::Conflict { expected, observed }) => {
                Ok(WorkloadSagaConfirmation::Conflict { expected, observed })
            }
            Err(WorkloadSagaStoreError::Ambiguous) => {
                self.resolve_ambiguous_confirmation(loaded, expected, &next)
                    .await
            }
            Err(error) => Err(error),
        }
    }

    async fn resolve_ambiguous_confirmation(
        &self,
        loaded: Option<&WorkloadSagaRecord>,
        expected: WorkloadSagaExpected,
        next: &WorkloadSagaRecord,
    ) -> Result<WorkloadSagaConfirmation, WorkloadSagaStoreError> {
        let observed = self.store.load(next.key()).await?;
        if observed
            .as_ref()
            .is_some_and(|record| record.key() != next.key())
        {
            return Err(WorkloadSagaStoreError::Corrupt);
        }
        if observed.as_ref() == Some(next) {
            return Ok(WorkloadSagaConfirmation::ConfirmedAfterAmbiguity);
        }
        if observed.is_none() || observed.as_ref() == loaded {
            return Ok(WorkloadSagaConfirmation::UnresolvedAmbiguity);
        }
        Ok(WorkloadSagaConfirmation::Conflict {
            expected,
            observed: observed.as_ref().map(WorkloadSagaRecord::revision),
        })
    }
}

#[cfg(test)]
#[path = "provision_dispatch/tests.rs"]
mod tests;
