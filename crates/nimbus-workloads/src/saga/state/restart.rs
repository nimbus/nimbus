//! Intrinsic validation and pure transitions for nested workload restart state.

use super::*;

pub(super) fn initial_restart_state(intent: &WorkloadSagaIntent) -> WorkloadRestartState {
    let execution_id = WorkloadExecutionId::for_execution(
        intent.admission().workload_uid(),
        intent.admission().assigned_node(),
        intent.generation(),
    );
    WorkloadRestartState::initial(&execution_id)
}

pub(super) fn validate_restart_state(record: &WorkloadSagaRecord) -> Result<(), WorkloadSagaError> {
    let state = &record.restart;
    let intent = &record.active_intent;
    let execution_id = WorkloadExecutionId::for_execution(
        intent.admission().workload_uid(),
        intent.admission().assigned_node(),
        intent.generation(),
    );
    let expected_current =
        WorkloadExecutionAttemptId::for_execution(&execution_id, state.completed_restart_epoch);
    if state.current_execution_attempt_id != expected_current {
        return Err(WorkloadSagaError::InvalidIdentity(
            "current execution attempt does not match the completed restart epoch",
        ));
    }
    if let Some(reference) = record.phase_detail.references().execution()
        && (reference.attempt_id() != &state.current_execution_attempt_id
            || reference.restart_epoch() != state.completed_restart_epoch)
    {
        return Err(WorkloadSagaError::InvalidEvidence(
            "lifecycle execution evidence is crossed with the current execution attempt",
        ));
    }
    if let Some(reference) = record.phase_detail.references().publication()
        && reference.execution().attempt_id() != &state.current_execution_attempt_id
    {
        return Err(WorkloadSagaError::InvalidEvidence(
            "publication evidence is crossed with the current execution attempt",
        ));
    }
    if state.completed_restart_epoch.as_u64() == 0 {
        if state.last_completed.is_some() {
            return Err(WorkloadSagaError::InvalidEvidence(
                "initial execution attempt cannot retain completed restart history",
            ));
        }
    } else {
        let history = state
            .last_completed
            .as_ref()
            .ok_or(WorkloadSagaError::InvalidEvidence(
                "completed restart epoch requires exact last-completed history",
            ))?;
        let source_epoch = WorkloadRestartEpoch::new(
            state
                .completed_restart_epoch
                .as_u64()
                .checked_sub(1)
                .ok_or(WorkloadSagaError::InvalidEvidence(
                    "completed restart history cannot use epoch zero",
                ))?,
        );
        let expected_source_attempt =
            WorkloadExecutionAttemptId::for_execution(&execution_id, source_epoch);
        validate_restart_admission_for_record(
            record,
            history.admission(),
            state.completed_restart_epoch,
            &expected_source_attempt,
            &state.current_execution_attempt_id,
            state.completed_automatic_restart_count,
        )?;
    }

    if let Some(active) = &state.active {
        active.validate()?;
        if record.phase != WorkloadSagaPhase::Observed
            || record.active_intent.desired_state() != DesiredWorkloadState::Running
            || record.successor_intent.is_some()
            || record.failure.is_some()
        {
            return Err(WorkloadSagaError::InvalidTransition(
                "active restart requires an observed running generation without withdrawal",
            ));
        }
        let admission = &active.admission;
        let expected_epoch = state
            .completed_restart_epoch
            .checked_next()
            .ok_or(WorkloadSagaError::RestartEpochOverflow)?;
        let expected_attempt =
            WorkloadExecutionAttemptId::for_execution(&execution_id, expected_epoch);
        validate_restart_admission_for_record(
            record,
            admission,
            expected_epoch,
            &state.current_execution_attempt_id,
            &expected_attempt,
            state.completed_automatic_restart_count,
        )?;
        validate_restart_disposition_for_record(record, active)?;
    }
    Ok(())
}

fn validate_restart_admission_for_record(
    record: &WorkloadSagaRecord,
    admission: &WorkloadRestartAdmission,
    expected_epoch: WorkloadRestartEpoch,
    expected_source_attempt: &WorkloadExecutionAttemptId,
    expected_target_attempt: &WorkloadExecutionAttemptId,
    expected_policy_count: u32,
) -> Result<(), WorkloadSagaError> {
    admission.validate_intrinsic()?;
    let intent = &record.active_intent;
    if admission.saga_id() != &record.saga_id
        || admission.source() != intent.source()
        || admission.generation() != intent.generation()
        || admission.desired_digest() != intent.desired_digest()
        || admission.provider_selection() != intent.source().execution_provider_id()
        || admission.restart_epoch() != expected_epoch
        || admission.source_attempt_id() != expected_source_attempt
        || admission.attempt_id() != expected_target_attempt
        || admission.revision() >= record.revision
        || admission.policy_attempt_count() != expected_policy_count
    {
        return Err(WorkloadSagaError::InvalidEvidence(
            "restart admission is crossed with the durable workload record",
        ));
    }
    if let WorkloadRestartTrigger::Automatic { exit_code } = admission.trigger() {
        let previous_count =
            expected_policy_count
                .checked_sub(1)
                .ok_or(WorkloadSagaError::InvalidEvidence(
                    "automatic restart admission must consume exactly one policy attempt",
                ))?;
        if !intent
            .restart_policy()
            .admits_automatic(exit_code, previous_count)
        {
            return Err(WorkloadSagaError::InvalidTransition(
                "portable restart policy does not admit this automatic restart",
            ));
        }
    }
    Ok(())
}

fn restart_step_for_phase(phase: WorkloadRestartPhase) -> Option<WorkloadRestartStep> {
    match phase {
        WorkloadRestartPhase::PublicationWithdrawalPending => {
            Some(WorkloadRestartStep::WithdrawPublication)
        }
        WorkloadRestartPhase::ExecutionQuiescencePending => {
            Some(WorkloadRestartStep::QuiesceExecution)
        }
        WorkloadRestartPhase::PreparationPending => Some(WorkloadRestartStep::PrepareExecution),
        WorkloadRestartPhase::AttachmentPending => Some(WorkloadRestartStep::AttachNetwork),
        WorkloadRestartPhase::ActivationPrerequisitePending => {
            Some(WorkloadRestartStep::InspectActivationPrerequisites)
        }
        WorkloadRestartPhase::ActivationPending => Some(WorkloadRestartStep::ActivateExecution),
        WorkloadRestartPhase::ReadinessPending => Some(WorkloadRestartStep::InspectReadiness),
        WorkloadRestartPhase::PublicationPending => Some(WorkloadRestartStep::Publish),
        WorkloadRestartPhase::ObservationPending => Some(WorkloadRestartStep::ObservePublication),
        WorkloadRestartPhase::Idle
        | WorkloadRestartPhase::Requested
        | WorkloadRestartPhase::Scheduled => None,
    }
}

fn restart_target_for_step(step: WorkloadRestartStep) -> Option<WorkloadRestartPhase> {
    match step {
        WorkloadRestartStep::WithdrawPublication => {
            Some(WorkloadRestartPhase::ExecutionQuiescencePending)
        }
        WorkloadRestartStep::QuiesceExecution => Some(WorkloadRestartPhase::Scheduled),
        WorkloadRestartStep::PrepareExecution => Some(WorkloadRestartPhase::AttachmentPending),
        WorkloadRestartStep::AttachNetwork => {
            Some(WorkloadRestartPhase::ActivationPrerequisitePending)
        }
        WorkloadRestartStep::InspectActivationPrerequisites => {
            Some(WorkloadRestartPhase::ActivationPending)
        }
        WorkloadRestartStep::ActivateExecution => Some(WorkloadRestartPhase::ReadinessPending),
        WorkloadRestartStep::InspectReadiness => Some(WorkloadRestartPhase::PublicationPending),
        WorkloadRestartStep::Publish => Some(WorkloadRestartPhase::ObservationPending),
        WorkloadRestartStep::ObservePublication => None,
    }
}

fn claim_matches_active_restart(
    claim: &WorkloadRestartCommandClaim,
    active: &ActiveWorkloadRestart,
) -> bool {
    claim.request_id() == active.admission.request_id()
        && claim.restart_epoch() == active.admission.restart_epoch()
        && claim.attempt_id() == active.admission.attempt_id()
}

fn revision_after(source: WorkloadSagaRevision, count: usize) -> Option<WorkloadSagaRevision> {
    (0..count).try_fold(source, |revision, _| revision.checked_next())
}

fn validate_restart_disposition_for_record(
    record: &WorkloadSagaRecord,
    active: &ActiveWorkloadRestart,
) -> Result<(), WorkloadSagaError> {
    let validate_claim = |claim: &WorkloadRestartCommandClaim,
                          expected_step: WorkloadRestartStep|
     -> Result<(), WorkloadSagaError> {
        if !claim_matches_active_restart(claim, active)
            || claim.step() != expected_step
            || claim.issuing_revision() <= active.admission.revision()
            || claim.issuing_revision() >= record.revision
        {
            return Err(WorkloadSagaError::InvalidEvidence(
                "restart command claim is crossed with its active restart",
            ));
        }
        Ok(())
    };

    match &active.disposition {
        WorkloadRestartDisposition::Ready { receipt: None } => {
            let no_effect_ready = matches!(
                active.phase,
                WorkloadRestartPhase::Requested
                    | WorkloadRestartPhase::PublicationWithdrawalPending
                    | WorkloadRestartPhase::PreparationPending
            ) || (active.phase == WorkloadRestartPhase::ObservationPending
                && record.active_intent.publication == WorkloadPublicationIntent::Withheld);
            if !no_effect_ready {
                return Err(WorkloadSagaError::InvalidEvidence(
                    "restart ready state is missing exact prior command evidence",
                ));
            }
        }
        WorkloadRestartDisposition::Ready {
            receipt: Some(receipt),
        } => {
            let claim = receipt.claim();
            let target =
                restart_target_for_step(claim.step()).ok_or(WorkloadSagaError::InvalidEvidence(
                    "terminal restart observation cannot remain active as ready",
                ))?;
            validate_claim(claim, claim.step())?;
            if target != active.phase
                || !matches!(
                    revision_after(claim.issuing_revision(), 2),
                    Some(revision) if revision == record.revision
                ) && !matches!(
                    revision_after(claim.issuing_revision(), 3),
                    Some(revision) if revision == record.revision
                )
            {
                return Err(WorkloadSagaError::InvalidEvidence(
                    "restart success receipt is crossed with its target phase or revision",
                ));
            }
        }
        WorkloadRestartDisposition::DispatchPending { claim } => {
            let expected_step =
                restart_step_for_phase(active.phase).ok_or(WorkloadSagaError::InvalidEvidence(
                    "restart dispatch claim cannot exist at a non-effect phase",
                ))?;
            validate_claim(claim, expected_step)?;
            if revision_after(claim.issuing_revision(), 1) != Some(record.revision) {
                return Err(WorkloadSagaError::InvalidEvidence(
                    "restart dispatch claim does not bind its confirmed revision",
                ));
            }
        }
        WorkloadRestartDisposition::InspectionRequired { claim } => {
            let expected_step =
                restart_step_for_phase(active.phase).ok_or(WorkloadSagaError::InvalidEvidence(
                    "restart inspection claim cannot exist at a non-effect phase",
                ))?;
            validate_claim(claim, expected_step)?;
            if revision_after(claim.issuing_revision(), 2) != Some(record.revision) {
                return Err(WorkloadSagaError::InvalidEvidence(
                    "restart inspection state does not exactly follow its dispatch claim",
                ));
            }
        }
        WorkloadRestartDisposition::DefiniteFailure { claim, .. } => {
            let expected_step =
                restart_step_for_phase(active.phase).ok_or(WorkloadSagaError::InvalidEvidence(
                    "restart failure claim cannot exist at a non-effect phase",
                ))?;
            validate_claim(claim, expected_step)?;
            if !matches!(
                revision_after(claim.issuing_revision(), 2),
                Some(revision) if revision == record.revision
            ) && !matches!(
                revision_after(claim.issuing_revision(), 3),
                Some(revision) if revision == record.revision
            ) {
                return Err(WorkloadSagaError::InvalidEvidence(
                    "restart failure does not exactly follow its command claim",
                ));
            }
        }
    }
    Ok(())
}

pub(super) fn validate_restart_state_transition(
    current: &WorkloadSagaRecord,
    candidate: &WorkloadSagaRecord,
    active_generation_changed: bool,
) -> Result<(), WorkloadSagaError> {
    if active_generation_changed {
        if candidate.restart != initial_restart_state(&candidate.active_intent) {
            return Err(WorkloadSagaError::InvalidTransition(
                "promoted generation must start with exact initial restart state",
            ));
        }
        return Ok(());
    }
    if current.restart == candidate.restart {
        return Ok(());
    }
    let unissued_successor_veto = matches!(
        (&current.restart.active, &candidate.restart.active),
        (Some(previous), None)
            if candidate.successor_intent.is_some()
                && previous.phase == WorkloadRestartPhase::Requested
                && previous.disposition.is_ready()
                && previous.disposition.receipt().is_none()
    );
    let restart_completion = matches!(
        (&current.restart.active, &candidate.restart.active),
        (Some(previous), None)
            if previous.phase == WorkloadRestartPhase::ObservationPending
                && candidate.phase == WorkloadSagaPhase::Observed
    );
    if !unissued_successor_veto
        && (current.active_intent != candidate.active_intent
            || current.phase != candidate.phase
            || (!restart_completion && current.phase_detail != candidate.phase_detail)
            || current.provision_disposition != candidate.provision_disposition
            || current.failure != candidate.failure)
    {
        return Err(WorkloadSagaError::InvalidTransition(
            "restart transition cannot rewrite outer lifecycle state",
        ));
    }

    match (&current.restart.active, &candidate.restart.active) {
        (None, Some(next)) => {
            let expected_automatic_count = match next.admission.trigger() {
                WorkloadRestartTrigger::Automatic { .. } => current
                    .restart
                    .completed_automatic_restart_count
                    .checked_add(1)
                    .ok_or(WorkloadSagaError::InvalidCounter(
                        "automatic restart count overflow",
                    ))?,
                WorkloadRestartTrigger::Explicit => {
                    current.restart.completed_automatic_restart_count
                }
            };
            if next.phase != WorkloadRestartPhase::Requested
                || !next.disposition.is_ready()
                || next.disposition.receipt().is_some()
                || next.admission.revision() != current.revision
                || candidate.restart.current_execution_attempt_id
                    != current.restart.current_execution_attempt_id
                || candidate.restart.completed_restart_epoch
                    != current.restart.completed_restart_epoch
                || candidate.restart.completed_automatic_restart_count != expected_automatic_count
                || candidate.restart.last_completed != current.restart.last_completed
            {
                return Err(WorkloadSagaError::InvalidTransition(
                    "restart admission must create one exact requested state",
                ));
            }
        }
        (Some(previous), Some(next)) => {
            if previous.admission != next.admission
                || current.restart.current_execution_attempt_id
                    != candidate.restart.current_execution_attempt_id
                || current.restart.completed_restart_epoch
                    != candidate.restart.completed_restart_epoch
                || current.restart.completed_automatic_restart_count
                    != candidate.restart.completed_automatic_restart_count
                || current.restart.last_completed != candidate.restart.last_completed
            {
                return Err(WorkloadSagaError::InvalidTransition(
                    "active restart transition must retain identity, count, and schedule",
                ));
            }
            validate_active_restart_transition(current, candidate, previous, next)?;
        }
        (Some(previous), None) => {
            let vetoed_for_successor = unissued_successor_veto
                && previous.phase == WorkloadRestartPhase::Requested
                && previous.disposition.is_ready()
                && previous.disposition.receipt().is_none()
                && candidate.restart.current_execution_attempt_id
                    == current.restart.current_execution_attempt_id
                && candidate.restart.completed_restart_epoch
                    == current.restart.completed_restart_epoch
                && candidate.restart.completed_automatic_restart_count
                    == current.restart.completed_automatic_restart_count
                && candidate.restart.last_completed == current.restart.last_completed;
            let completed = previous.phase == WorkloadRestartPhase::ObservationPending
                && matches!(
                    previous.disposition,
                    WorkloadRestartDisposition::DispatchPending { ref claim }
                        | WorkloadRestartDisposition::InspectionRequired { ref claim }
                        if claim.step() == WorkloadRestartStep::ObservePublication
                )
                && candidate.restart.current_execution_attempt_id
                    == *previous.admission.attempt_id()
                && candidate.restart.completed_restart_epoch == previous.admission.restart_epoch()
                && candidate.restart.completed_automatic_restart_count
                    == previous.admission.policy_attempt_count()
                && candidate
                    .restart
                    .last_completed
                    .as_ref()
                    .is_some_and(|history| history.admission == previous.admission);
            if !vetoed_for_successor && !completed {
                return Err(WorkloadSagaError::InvalidTransition(
                    "active restart can end only after observation or an unissued successor veto",
                ));
            }
        }
        (None, None) => {
            return Err(WorkloadSagaError::InvalidTransition(
                "restart transition cannot rewrite inactive history",
            ));
        }
    }
    Ok(())
}

fn validate_active_restart_transition(
    current: &WorkloadSagaRecord,
    candidate: &WorkloadSagaRecord,
    previous: &ActiveWorkloadRestart,
    next: &ActiveWorkloadRestart,
) -> Result<(), WorkloadSagaError> {
    if previous.phase == next.phase {
        return match (&previous.disposition, &next.disposition) {
            (
                WorkloadRestartDisposition::Ready { .. },
                WorkloadRestartDisposition::DispatchPending { claim },
            ) if claim.issuing_revision() == current.revision
                && claim.dispatch_epoch() == WorkloadRestartDispatchEpoch::new(0)
                && matches!(
                    claim.authorization(),
                    WorkloadRestartDispatchAuthorization::Initial
                ) =>
            {
                Ok(())
            }
            (
                WorkloadRestartDisposition::DispatchPending { claim: previous },
                WorkloadRestartDisposition::InspectionRequired { claim: next },
            ) if previous == next => Ok(()),
            (
                WorkloadRestartDisposition::DispatchPending { claim: previous }
                | WorkloadRestartDisposition::InspectionRequired { claim: previous },
                WorkloadRestartDisposition::DefiniteFailure {
                    claim: next,
                    result,
                },
            ) if previous == next && result.is_failed() => Ok(()),
            (
                WorkloadRestartDisposition::InspectionRequired { claim: previous },
                WorkloadRestartDisposition::DispatchPending { claim: next },
            ) if retry_claim_follows_inspection(current, previous, next, candidate.revision) => {
                Ok(())
            }
            _ => Err(WorkloadSagaError::InvalidTransition(
                "restart disposition transition is not legal",
            )),
        };
    }

    let exact_success = matches!(
        (&previous.disposition, &next.disposition),
        (
            WorkloadRestartDisposition::DispatchPending { claim: previous_claim }
                | WorkloadRestartDisposition::InspectionRequired { claim: previous_claim },
            WorkloadRestartDisposition::Ready {
                receipt: Some(receipt),
            },
        ) if receipt.claim() == previous_claim
            && receipt.result().is_succeeded()
            && restart_target_for_step(previous_claim.step()) == Some(next.phase)
    );
    let requested_without_effect = previous.phase == WorkloadRestartPhase::Requested
        && next.phase == WorkloadRestartPhase::PublicationWithdrawalPending
        && previous.disposition.is_ready()
        && previous.disposition.receipt().is_none()
        && next.disposition.is_ready()
        && next.disposition.receipt().is_none();
    let scheduled_due = previous.phase == WorkloadRestartPhase::Scheduled
        && next.phase == WorkloadRestartPhase::PreparationPending
        && previous.disposition.is_ready()
        && previous
            .disposition
            .receipt()
            .is_some_and(|receipt| receipt.claim().step() == WorkloadRestartStep::QuiesceExecution)
        && next.disposition.is_ready()
        && next.disposition.receipt().is_none();
    let withheld_publication = previous.phase == WorkloadRestartPhase::PublicationPending
        && next.phase == WorkloadRestartPhase::ObservationPending
        && current.active_intent.publication == WorkloadPublicationIntent::Withheld
        && previous.disposition.is_ready()
        && previous.disposition.receipt().is_some()
        && next.disposition.is_ready()
        && next.disposition.receipt().is_none();
    if exact_success || requested_without_effect || scheduled_due || withheld_publication {
        Ok(())
    } else {
        Err(WorkloadSagaError::InvalidTransition(
            "restart phase transition lacks exact command or no-effect authority",
        ))
    }
}

fn retry_claim_follows_inspection(
    current: &WorkloadSagaRecord,
    previous: &WorkloadRestartCommandClaim,
    next: &WorkloadRestartCommandClaim,
    resulting_revision: WorkloadSagaRevision,
) -> bool {
    let WorkloadRestartDispatchAuthorization::RetryAfterAbsence(absence) = next.authorization()
    else {
        return false;
    };
    previous.request_id() == next.request_id()
        && previous.restart_epoch() == next.restart_epoch()
        && previous.attempt_id() == next.attempt_id()
        && previous.step() == next.step()
        && previous.dispatch_epoch().checked_next() == Some(next.dispatch_epoch())
        && next.issuing_revision() == current.revision
        && current.revision.checked_next() == Some(resulting_revision)
        && absence.matches_inspection(current, previous)
}

impl WorkloadSagaRecord {
    pub fn restart_state(&self) -> &WorkloadRestartState {
        &self.restart
    }

    pub fn current_execution_reference(&self) -> WorkloadExecutionReference {
        WorkloadExecutionReference::for_restart_epoch(
            &self.active_intent,
            self.restart.completed_restart_epoch,
        )
    }

    pub fn restart_recovery_decision(
        &self,
        now_unix_millis: WorkloadRestartNotBeforeUnixMillis,
    ) -> WorkloadRestartRecoveryDecision {
        let Some(active) = &self.restart.active else {
            return WorkloadRestartRecoveryDecision::Quiescent;
        };
        if active.phase == WorkloadRestartPhase::Scheduled
            && now_unix_millis < active.admission.not_before_unix_millis()
        {
            WorkloadRestartRecoveryDecision::WaitingUntil(active.admission.not_before_unix_millis())
        } else {
            WorkloadRestartRecoveryDecision::Ready
        }
    }

    /// Whether the global restart watch must discover this record.
    pub fn requires_restart_watch(&self) -> bool {
        if self.restart.active.is_some() {
            return true;
        }
        self.phase == WorkloadSagaPhase::Observed
            && self.active_intent.desired_state() == DesiredWorkloadState::Running
            && self.active_intent.restart_policy() != WorkloadRestartPolicy::Never
            && self.successor_intent.is_none()
            && self.failure.is_none()
    }

    pub fn admit_restart(
        &self,
        input: WorkloadRestartAdmissionInput,
    ) -> Result<WorkloadRestartAdmissionUpdate, WorkloadSagaError> {
        if let Some(active) = &self.restart.active {
            let admission = &active.admission;
            return if admission.request_id() == &input.request_id
                && admission.trigger() == input.trigger
                && admission.inspection_version() == input.inspection_version
                && admission.not_before_unix_millis() == input.not_before_unix_millis
            {
                Ok(WorkloadRestartAdmissionUpdate::Unchanged)
            } else if admission.request_id() == &input.request_id {
                Err(WorkloadSagaError::InvalidTransition(
                    "duplicate restart request has crossed admission content",
                ))
            } else {
                Err(WorkloadSagaError::InvalidTransition(
                    "another restart request is already active",
                ))
            };
        }
        if input.expected_revision != self.revision {
            return Err(WorkloadSagaError::InvalidTransition(
                "restart admission revision is stale or crossed",
            ));
        }
        if self.phase != WorkloadSagaPhase::Observed
            || self.active_intent.desired_state() != DesiredWorkloadState::Running
            || self.successor_intent.is_some()
            || self.failure.is_some()
            || self.provision_disposition != Some(WorkloadProvisionDisposition::Ready)
        {
            return Err(WorkloadSagaError::InvalidTransition(
                "restart admission requires an observed running generation without withdrawal",
            ));
        }
        let next_epoch = self
            .restart
            .completed_restart_epoch
            .checked_next()
            .ok_or(WorkloadSagaError::RestartEpochOverflow)?;
        let execution_id = WorkloadExecutionId::for_execution(
            self.active_intent.admission().workload_uid(),
            self.active_intent.admission().assigned_node(),
            self.active_intent.generation(),
        );
        let target_attempt = WorkloadExecutionAttemptId::for_execution(&execution_id, next_epoch);
        let policy_attempt_count = match input.trigger {
            WorkloadRestartTrigger::Automatic { exit_code } => {
                if !self
                    .active_intent
                    .restart_policy()
                    .admits_automatic(exit_code, self.restart.completed_automatic_restart_count)
                {
                    return Err(WorkloadSagaError::InvalidTransition(
                        "portable restart policy rejects this automatic restart",
                    ));
                }
                self.restart
                    .completed_automatic_restart_count
                    .checked_add(1)
                    .ok_or(WorkloadSagaError::InvalidCounter(
                        "automatic restart count overflow",
                    ))?
            }
            WorkloadRestartTrigger::Explicit => self.restart.completed_automatic_restart_count,
        };
        let admission = WorkloadRestartAdmission::new(
            self.saga_id.clone(),
            self.active_intent.source().clone(),
            self.active_intent.generation(),
            self.active_intent.desired_digest(),
            self.revision,
            input.trigger,
            input.inspection_version,
            self.active_intent.source().execution_provider_id().clone(),
            next_epoch,
            policy_attempt_count,
            input.request_id,
            self.restart.current_execution_attempt_id.clone(),
            target_attempt,
            input.not_before_unix_millis,
        )?;
        let mut restart = self.restart.clone();
        restart.completed_automatic_restart_count = policy_attempt_count;
        restart.active = Some(ActiveWorkloadRestart::requested(admission));
        self.build_next_with_restart_state(restart)
            .map(Box::new)
            .map(WorkloadRestartAdmissionUpdate::Transition)
    }

    /// Advance an exact restart edge that has no provider effect.
    pub fn advance_restart_without_effect(
        &self,
        request_id: &WorkloadRestartRequestId,
    ) -> Result<Self, WorkloadSagaError> {
        let mut restart = self.restart.clone();
        let active = restart
            .active
            .as_mut()
            .ok_or(WorkloadSagaError::InvalidTransition(
                "restart phase advance requires an active request",
            ))?;
        if active.admission.request_id() != request_id || !active.disposition.is_ready() {
            return Err(WorkloadSagaError::InvalidTransition(
                "restart no-effect advance is stale, crossed, or unresolved",
            ));
        }
        let target = match active.phase {
            WorkloadRestartPhase::Requested if active.disposition.receipt().is_none() => {
                WorkloadRestartPhase::PublicationWithdrawalPending
            }
            WorkloadRestartPhase::PublicationPending
                if self.active_intent.publication == WorkloadPublicationIntent::Withheld
                    && active.disposition.receipt().is_some() =>
            {
                WorkloadRestartPhase::ObservationPending
            }
            _ => {
                return Err(WorkloadSagaError::InvalidTransition(
                    "restart phase requires an exact command result or due-time decision",
                ));
            }
        };
        active.phase = target;
        active.disposition = WorkloadRestartDisposition::initial_ready();
        self.build_next_with_restart_state(restart)
    }

    /// Persist the first dispatch epoch for the effect owned by the current phase.
    pub fn claim_restart_command(
        &self,
        request_id: &WorkloadRestartRequestId,
    ) -> Result<Self, WorkloadSagaError> {
        let mut restart = self.restart.clone();
        let active = restart
            .active
            .as_mut()
            .ok_or(WorkloadSagaError::InvalidTransition(
                "restart command claim requires an active request",
            ))?;
        if active.admission.request_id() != request_id || !active.disposition.is_ready() {
            return Err(WorkloadSagaError::InvalidTransition(
                "restart command claim is stale, crossed, or unresolved",
            ));
        }
        let step = restart_step_for_phase(active.phase).ok_or(
            WorkloadSagaError::InvalidTransition("current restart phase has no provider command"),
        )?;
        let claim = WorkloadRestartCommandClaim::initial(
            active.admission.request_id().clone(),
            active.admission.restart_epoch(),
            active.admission.attempt_id().clone(),
            step,
            self.revision,
        )?;
        active.disposition = WorkloadRestartDisposition::DispatchPending { claim };
        self.build_next_with_restart_state(restart)
    }

    /// Require side-effect-free inspection after an uncertain restart dispatch.
    pub fn restart_dispatch_to_inspection(
        &self,
        claim: &WorkloadRestartCommandClaim,
    ) -> Result<Self, WorkloadSagaError> {
        let mut restart = self.restart.clone();
        let active = restart
            .active
            .as_mut()
            .ok_or(WorkloadSagaError::InvalidTransition(
                "restart inspection requires an active request",
            ))?;
        if !matches!(
            &active.disposition,
            WorkloadRestartDisposition::DispatchPending { claim: retained }
                if retained == claim
        ) {
            return Err(WorkloadSagaError::InvalidTransition(
                "restart inspection requires the exact pending claim",
            ));
        }
        active.disposition = WorkloadRestartDisposition::InspectionRequired {
            claim: claim.clone(),
        };
        self.build_next_with_restart_state(restart)
    }

    /// Authorize the same attempt at the next epoch after exact absence.
    pub fn restart_inspection_to_retry(
        &self,
        claim: &WorkloadRestartCommandClaim,
        absence: WorkloadRestartAbsenceEvidence,
    ) -> Result<Self, WorkloadSagaError> {
        let mut restart = self.restart.clone();
        let active = restart
            .active
            .as_mut()
            .ok_or(WorkloadSagaError::InvalidTransition(
                "restart retry requires an active request",
            ))?;
        if !matches!(
            &active.disposition,
            WorkloadRestartDisposition::InspectionRequired { claim: retained }
                if retained == claim
        ) || !absence.matches_inspection(self, claim)
        {
            return Err(WorkloadSagaError::InvalidEvidence(
                "restart retry requires exact durable absence evidence",
            ));
        }
        let next = WorkloadRestartCommandClaim::retry_after_absence(claim, self.revision, absence)?;
        active.disposition = WorkloadRestartDisposition::DispatchPending { claim: next };
        self.build_next_with_restart_state(restart)
    }

    /// Persist one exact restart effect result.
    pub fn apply_restart_effect_result(
        &self,
        claim: &WorkloadRestartCommandClaim,
        result: WorkloadRestartEffectResult,
        observed_detail: Option<WorkloadPhaseDetail>,
    ) -> Result<Self, WorkloadSagaError> {
        let active = self
            .restart
            .active
            .as_ref()
            .ok_or(WorkloadSagaError::InvalidTransition(
                "restart effect result requires an active request",
            ))?;
        if !matches!(
            &active.disposition,
            WorkloadRestartDisposition::DispatchPending { claim: retained }
                | WorkloadRestartDisposition::InspectionRequired { claim: retained }
                if retained == claim
        ) {
            return Err(WorkloadSagaError::InvalidTransition(
                "restart effect result is stale or crossed",
            ));
        }
        match result {
            WorkloadRestartEffectResult::AuthenticatedAbsent { .. } => {
                Err(WorkloadSagaError::InvalidTransition(
                    "authenticated absence must authorize an inspected retry",
                ))
            }
            result @ WorkloadRestartEffectResult::Failed { .. } => {
                if observed_detail.is_some() {
                    return Err(WorkloadSagaError::InvalidEvidence(
                        "failed restart effect cannot carry observed detail",
                    ));
                }
                let mut restart = self.restart.clone();
                restart
                    .active
                    .as_mut()
                    .expect("active restart checked above")
                    .disposition = WorkloadRestartDisposition::DefiniteFailure {
                    claim: claim.clone(),
                    result,
                };
                self.build_next_with_restart_state(restart)
            }
            result @ WorkloadRestartEffectResult::Succeeded { .. } => {
                if claim.step() == WorkloadRestartStep::ObservePublication {
                    let observed_detail =
                        observed_detail.ok_or(WorkloadSagaError::InvalidEvidence(
                            "restart observation success requires exact new-attempt detail",
                        ))?;
                    return self.complete_restart_after_observation(
                        claim,
                        observed_detail,
                        result.evidence(),
                    );
                }
                if observed_detail.is_some() {
                    return Err(WorkloadSagaError::InvalidEvidence(
                        "non-observation restart success cannot rewrite outer detail",
                    ));
                }
                let target = restart_target_for_step(claim.step()).ok_or(
                    WorkloadSagaError::InvalidTransition(
                        "restart command has no active target phase",
                    ),
                )?;
                let receipt = WorkloadRestartCommandReceipt::succeeded(claim.clone(), result)?;
                let mut restart = self.restart.clone();
                let active = restart
                    .active
                    .as_mut()
                    .expect("active restart checked above");
                active.phase = target;
                active.disposition = WorkloadRestartDisposition::Ready {
                    receipt: Some(receipt),
                };
                self.build_next_with_restart_state(restart)
            }
        }
    }

    pub fn advance_scheduled_restart(
        &self,
        request_id: &WorkloadRestartRequestId,
        now_unix_millis: WorkloadRestartNotBeforeUnixMillis,
    ) -> Result<Self, WorkloadSagaError> {
        let mut restart = self.restart.clone();
        let active = restart
            .active
            .as_mut()
            .ok_or(WorkloadSagaError::InvalidTransition(
                "scheduled restart advance requires an active request",
            ))?;
        if active.admission.request_id() != request_id
            || active.phase != WorkloadRestartPhase::Scheduled
            || !active.disposition.is_ready()
            || !active.disposition.receipt().is_some_and(|receipt| {
                receipt.claim().step() == WorkloadRestartStep::QuiesceExecution
            })
        {
            return Err(WorkloadSagaError::InvalidTransition(
                "scheduled restart advance is stale or crossed",
            ));
        }
        if now_unix_millis < active.admission.not_before_unix_millis() {
            return Err(WorkloadSagaError::InvalidTransition(
                "scheduled restart is not due",
            ));
        }
        active.phase = WorkloadRestartPhase::PreparationPending;
        active.disposition = WorkloadRestartDisposition::initial_ready();
        self.build_next_with_restart_state(restart)
    }

    fn complete_restart_after_observation(
        &self,
        claim: &WorkloadRestartCommandClaim,
        observed_detail: WorkloadPhaseDetail,
        evidence: WorkloadRestartEvidenceDigest,
    ) -> Result<Self, WorkloadSagaError> {
        let active = self
            .restart
            .active
            .as_ref()
            .ok_or(WorkloadSagaError::InvalidTransition(
                "restart completion requires an active request",
            ))?;
        if active.phase != WorkloadRestartPhase::ObservationPending
            || !matches!(
                &active.disposition,
                WorkloadRestartDisposition::DispatchPending { claim: retained }
                    | WorkloadRestartDisposition::InspectionRequired { claim: retained }
                    if retained == claim
            )
            || claim.step() != WorkloadRestartStep::ObservePublication
        {
            return Err(WorkloadSagaError::InvalidTransition(
                "restart completion is stale, crossed, or not observed",
            ));
        }
        validate_phase_detail(
            WorkloadSagaPhase::Observed,
            &self.active_intent,
            &observed_detail,
        )?;
        if observed_detail
            .references()
            .execution()
            .map(WorkloadExecutionReference::attempt_id)
            != Some(active.admission.attempt_id())
            || observed_detail
                .references()
                .publication()
                .is_some_and(|publication| {
                    publication.execution().attempt_id() != active.admission.attempt_id()
                })
        {
            return Err(WorkloadSagaError::InvalidEvidence(
                "restart completion requires exact new-attempt execution and publication evidence",
            ));
        }
        let admission = active.admission.clone();
        let mut restart = self.restart.clone();
        restart.current_execution_attempt_id = admission.attempt_id().clone();
        restart.completed_restart_epoch = admission.restart_epoch();
        restart.completed_automatic_restart_count = admission.policy_attempt_count();
        restart.last_completed = Some(WorkloadRestartHistory {
            admission,
            evidence,
        });
        restart.active = None;
        self.build_next_with_restart_state_and_detail(restart, observed_detail)
    }
}
