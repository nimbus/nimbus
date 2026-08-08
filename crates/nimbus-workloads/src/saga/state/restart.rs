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
                && previous.disposition == WorkloadRestartDisposition::Ready
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
                || next.disposition != WorkloadRestartDisposition::Ready
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
            if previous.phase == next.phase {
                if previous.disposition == next.disposition {
                    return Err(WorkloadSagaError::InvalidTransition(
                        "restart transition must change semantic state",
                    ));
                }
            } else if !legal_restart_phase_edge(previous.phase, next.phase)
                || next.disposition != WorkloadRestartDisposition::Ready
            {
                return Err(WorkloadSagaError::InvalidTransition(
                    "restart transition contains an illegal phase edge",
                ));
            }
        }
        (Some(previous), None) => {
            let vetoed_for_successor = unissued_successor_veto
                && previous.phase == WorkloadRestartPhase::Requested
                && previous.disposition == WorkloadRestartDisposition::Ready
                && candidate.restart.current_execution_attempt_id
                    == current.restart.current_execution_attempt_id
                && candidate.restart.completed_restart_epoch
                    == current.restart.completed_restart_epoch
                && candidate.restart.completed_automatic_restart_count
                    == current.restart.completed_automatic_restart_count
                && candidate.restart.last_completed == current.restart.last_completed;
            let completed = previous.phase == WorkloadRestartPhase::ObservationPending
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

fn legal_restart_phase_edge(source: WorkloadRestartPhase, target: WorkloadRestartPhase) -> bool {
    matches!(
        (source, target),
        (
            WorkloadRestartPhase::Requested,
            WorkloadRestartPhase::PublicationWithdrawalPending
        ) | (
            WorkloadRestartPhase::PublicationWithdrawalPending,
            WorkloadRestartPhase::ExecutionQuiescencePending
        ) | (
            WorkloadRestartPhase::ExecutionQuiescencePending,
            WorkloadRestartPhase::Scheduled
        ) | (
            WorkloadRestartPhase::Scheduled,
            WorkloadRestartPhase::PreparationPending
        ) | (
            WorkloadRestartPhase::PreparationPending,
            WorkloadRestartPhase::AttachmentPending
        ) | (
            WorkloadRestartPhase::AttachmentPending,
            WorkloadRestartPhase::ActivationPrerequisitePending
        ) | (
            WorkloadRestartPhase::ActivationPrerequisitePending,
            WorkloadRestartPhase::ActivationPending
        ) | (
            WorkloadRestartPhase::ActivationPending,
            WorkloadRestartPhase::ReadinessPending
        ) | (
            WorkloadRestartPhase::ReadinessPending,
            WorkloadRestartPhase::PublicationPending
        ) | (
            WorkloadRestartPhase::PublicationPending,
            WorkloadRestartPhase::ObservationPending
        )
    )
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

    pub fn advance_restart_phase(
        &self,
        request_id: &WorkloadRestartRequestId,
        target: WorkloadRestartPhase,
    ) -> Result<Self, WorkloadSagaError> {
        let mut restart = self.restart.clone();
        let active = restart
            .active
            .as_mut()
            .ok_or(WorkloadSagaError::InvalidTransition(
                "restart phase advance requires an active request",
            ))?;
        if active.admission.request_id() != request_id
            || !legal_restart_phase_edge(active.phase, target)
        {
            return Err(WorkloadSagaError::InvalidTransition(
                "restart phase advance is stale, crossed, or illegal",
            ));
        }
        if active.phase == WorkloadRestartPhase::Scheduled {
            return Err(WorkloadSagaError::InvalidTransition(
                "scheduled restart requires an explicit due-time decision",
            ));
        }
        active.phase = target;
        active.disposition = WorkloadRestartDisposition::Ready;
        self.build_next_with_restart_state(restart)
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
        active.disposition = WorkloadRestartDisposition::Ready;
        self.build_next_with_restart_state(restart)
    }

    pub fn complete_restart(
        &self,
        request_id: &WorkloadRestartRequestId,
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
            || active.admission.request_id() != request_id
            || active.disposition != WorkloadRestartDisposition::Ready
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
