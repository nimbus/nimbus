//! Pure durable teardown reducer and disposition validation.

use super::*;

pub(super) fn validate_teardown_disposition(
    record: &WorkloadSagaRecord,
) -> Result<(), WorkloadSagaError> {
    match (record.phase, record.teardown_disposition.as_deref()) {
        (phase, Some(disposition)) if phase.is_teardown() => {
            validate_disposition_for_record(record, disposition)
        }
        (WorkloadSagaPhase::CleanupPending, Some(disposition)) => {
            if !matches!(
                disposition,
                WorkloadTeardownDisposition::DefiniteFailure { .. }
            ) {
                return Err(WorkloadSagaError::InvalidTransition(
                    "teardown-owned cleanup requires exact failed claim state",
                ));
            }
            validate_disposition_for_record(record, disposition)
        }
        (phase, None) if !phase.is_teardown() => Ok(()),
        (phase, Some(_)) if !phase.is_teardown() => Err(WorkloadSagaError::InvalidTransition(
            "teardown disposition is present outside teardown-owned state",
        )),
        _ => Err(WorkloadSagaError::InvalidTransition(
            "teardown phase requires a durable teardown disposition",
        )),
    }
}

fn validate_disposition_for_record(
    record: &WorkloadSagaRecord,
    disposition: &WorkloadTeardownDisposition,
) -> Result<(), WorkloadSagaError> {
    let context = disposition.context();
    validate_context_for_record(record, context)?;
    let last_safe_phase = match &record.phase_detail {
        WorkloadPhaseDetail::CleanupPending(detail) => detail.last_safe_phase(),
        _ => record.phase,
    };
    for receipt in context.completed() {
        receipt.validate()?;
        validate_attempt_for_record(record, receipt.claim().attempt(), context)?;
        if receipt.claim().claimed_revision() >= record.revision
            || receipt.claim().attempt().target_phase().recovery_order()
                > last_safe_phase.recovery_order()
        {
            return Err(WorkloadSagaError::InvalidEvidence(
                "teardown receipt history is crossed, duplicated, or out of order",
            ));
        }
    }
    if context.completed().windows(2).any(|pair| {
        pair[0].claim().attempt().step().order() >= pair[1].claim().attempt().step().order()
    }) {
        return Err(WorkloadSagaError::InvalidEvidence(
            "teardown receipt history is duplicated or out of order",
        ));
    }
    let receipt_observations = context
        .completed()
        .iter()
        .map(|receipt| receipt.evidence().terminal_observation())
        .collect::<Vec<_>>();
    let retained_observations = match (&record.phase_detail, disposition) {
        (WorkloadPhaseDetail::Teardown(detail), _) => Some(detail.terminal_observations()),
        (
            WorkloadPhaseDetail::CleanupPending(_),
            WorkloadTeardownDisposition::DefiniteFailure {
                prior_terminal_observations,
                ..
            },
        ) => Some(prior_terminal_observations.as_slice()),
        _ => None,
    };
    if let Some(retained_observations) = retained_observations
        && receipt_observations != retained_observations
    {
        return Err(WorkloadSagaError::InvalidEvidence(
            "teardown receipt history does not match terminal observations",
        ));
    }
    if let Some(claim) = disposition.claim() {
        claim.validate()?;
        let attempt = claim.attempt();
        validate_attempt_for_record(record, attempt, context)?;
        let source_phase_matches = if record.phase == WorkloadSagaPhase::CleanupPending {
            matches!(
                &record.phase_detail,
                WorkloadPhaseDetail::CleanupPending(detail)
                    if detail.last_safe_phase() == attempt.source_phase()
            )
        } else {
            attempt.source_phase() == record.phase
        };
        let revision_matches = match disposition {
            WorkloadTeardownDisposition::DispatchPending { .. } => {
                claim.claimed_revision() == record.revision
            }
            WorkloadTeardownDisposition::InspectionRequired { .. }
            | WorkloadTeardownDisposition::DefiniteFailure { .. } => {
                claim.claimed_revision() < record.revision
            }
            WorkloadTeardownDisposition::Ready { .. } => false,
        };
        if !source_phase_matches || !revision_matches {
            return Err(WorkloadSagaError::InvalidEvidence(
                "teardown claim is crossed with the durable workload record",
            ));
        }
    }
    if let WorkloadTeardownDisposition::DefiniteFailure {
        claim,
        failure,
        confirmation,
        ..
    } = disposition
    {
        confirmation.validate_for_claim(claim)?;
        if record.failure.as_ref() != Some(failure) {
            return Err(WorkloadSagaError::InvalidEvidence(
                "teardown cleanup failure does not match record failure evidence",
            ));
        }
    }
    Ok(())
}

fn validate_attempt_for_record(
    record: &WorkloadSagaRecord,
    attempt: &WorkloadTeardownAttempt,
    context: &WorkloadTeardownContext,
) -> Result<(), WorkloadSagaError> {
    let successor_fence_matches = match (attempt.successor_fence(), context.successor_fence()) {
        (None, None) => true,
        (None, Some(_)) => matches!(
            context.cause(),
            WorkloadTeardownCause::FailedProvision { .. }
        ),
        (Some(attempt), Some(latest)) => {
            attempt.generation() < latest.generation()
                || attempt.generation() == latest.generation()
                    && attempt.desired_digest() == latest.desired_digest()
        }
        (Some(_), None) => false,
    };
    if attempt.key() != &record.key
        || attempt.saga_id() != &record.saga_id
        || attempt.generation() != record.active_intent.generation()
        || attempt.desired_digest() != record.active_intent.desired_digest()
        || attempt.required_node() != record.active_intent.admission().assigned_node()
        || attempt.source_digest() != record.active_intent.source().source_digest()
        || attempt.execution_provider_id() != record.active_intent.source().execution_provider_id()
        || attempt.network_plan_digest() != record.active_intent.network().digest()
        || attempt.selection_evidence()
            != record
                .active_intent
                .network()
                .compiled_plan()
                .content()
                .capability_selection_evidence()
        || attempt.cause() != context.cause()
        || !successor_fence_matches
    {
        return Err(WorkloadSagaError::InvalidEvidence(
            "teardown attempt is crossed with the durable workload record",
        ));
    }
    Ok(())
}

fn context_matches_except_successor_fence(
    previous: &WorkloadTeardownContext,
    next: &WorkloadTeardownContext,
) -> bool {
    previous.cause() == next.cause()
        && previous.provision_absence() == next.provision_absence()
        && previous.restart_settlement() == next.restart_settlement()
        && previous.completed() == next.completed()
}

fn success_appends_exact_receipt_and_observation(
    current: &WorkloadSagaRecord,
    candidate: &WorkloadSagaRecord,
    previous: &WorkloadTeardownContext,
    next: &WorkloadTeardownContext,
    claim: &WorkloadTeardownClaim,
) -> bool {
    let Some((receipt, prefix)) = next.completed().split_last() else {
        return false;
    };
    if previous.cause() != next.cause()
        || previous.successor_fence() != next.successor_fence()
        || previous.provision_absence() != next.provision_absence()
        || previous.restart_settlement() != next.restart_settlement()
        || prefix != previous.completed()
        || receipt.claim() != claim
        || !receipt.confirmation().matches_current(current, claim)
    {
        return false;
    }
    let (
        WorkloadPhaseDetail::Teardown(current_detail),
        WorkloadPhaseDetail::Teardown(candidate_detail),
    ) = (&current.phase_detail, &candidate.phase_detail)
    else {
        return false;
    };
    let Some((observation, observation_prefix)) =
        candidate_detail.terminal_observations().split_last()
    else {
        return false;
    };
    observation_prefix == current_detail.terminal_observations()
        && *observation == receipt.evidence().terminal_observation()
}

fn validate_context_for_record(
    record: &WorkloadSagaRecord,
    context: &WorkloadTeardownContext,
) -> Result<(), WorkloadSagaError> {
    match context.cause() {
        WorkloadTeardownCause::Successor {
            generation,
            desired_digest,
        } => {
            let successor =
                record
                    .successor_intent
                    .as_ref()
                    .ok_or(WorkloadSagaError::InvalidEvidence(
                        "successor teardown requires a queued successor",
                    ))?;
            if successor.generation() < *generation
                || (successor.generation() == *generation
                    && successor.desired_digest() != *desired_digest)
            {
                return Err(WorkloadSagaError::InvalidEvidence(
                    "teardown cause is crossed with its initiating successor",
                ));
            }
        }
        WorkloadTeardownCause::FailedProvision { claim, failure } => {
            failure.validate()?;
            if claim.attempt().generation() != record.active_intent.generation()
                || claim.attempt().desired_digest() != record.active_intent.desired_digest()
            {
                return Err(WorkloadSagaError::InvalidEvidence(
                    "failed provision cause is crossed with active generation",
                ));
            }
        }
    }
    let expected_fence = record.successor_intent.as_ref().map(|successor| {
        WorkloadTeardownSuccessorFence::new(successor.generation(), successor.desired_digest())
    });
    if context.successor_fence() != expected_fence {
        return Err(WorkloadSagaError::InvalidEvidence(
            "teardown context does not bind the latest successor fence",
        ));
    }
    if let Some(absence) = context.provision_absence() {
        absence.validate()?;
        let origin = match &record.phase_detail {
            WorkloadPhaseDetail::Teardown(detail) => Some(detail.origin()),
            _ => None,
        };
        if !matches!(context.cause(), WorkloadTeardownCause::Successor { .. })
            || absence.claim().attempt().generation() != record.active_intent.generation()
            || absence.claim().attempt().desired_digest() != record.active_intent.desired_digest()
            || origin.is_some_and(|origin| absence.claim().attempt().source_phase() != origin)
        {
            return Err(WorkloadSagaError::InvalidEvidence(
                "provision teardown absence is crossed with lifecycle origin",
            ));
        }
    }
    if let Some(settlement) = context.restart_settlement() {
        settlement.validate()?;
        let expected_source = WorkloadExecutionReference::for_restart_epoch(
            &record.active_intent,
            record.restart.completed_restart_epoch(),
        );
        if !matches!(context.cause(), WorkloadTeardownCause::Successor { .. })
            || record.restart.active().is_some()
            || settlement.source_execution() != &expected_source
        {
            return Err(WorkloadSagaError::InvalidEvidence(
                "restart teardown settlement is crossed with source lifecycle state",
            ));
        }
    }
    Ok(())
}

pub(super) fn validate_teardown_disposition_transition(
    current: &WorkloadSagaRecord,
    candidate: &WorkloadSagaRecord,
    active_changed: bool,
) -> Result<(), WorkloadSagaError> {
    if active_changed {
        return if candidate.teardown_disposition.is_none() {
            Ok(())
        } else {
            Err(WorkloadSagaError::InvalidTransition(
                "promoted generation cannot retain prior teardown disposition",
            ))
        };
    }
    match (
        current.teardown_disposition.as_deref(),
        candidate.teardown_disposition.as_deref(),
    ) {
        (None, None) => Ok(()),
        (None, Some(WorkloadTeardownDisposition::Ready { .. })) => {
            if candidate.phase != WorkloadSagaPhase::WithdrawalCommitted
                || !current.phase.is_provision()
            {
                return Err(WorkloadSagaError::InvalidTransition(
                    "teardown can start only by committing withdrawal from provision state",
                ));
            }
            Ok(())
        }
        (Some(previous), None) => {
            if current.phase == WorkloadSagaPhase::NetworkReleased
                && candidate.phase == WorkloadSagaPhase::Recorded
                && previous.context().restart_settlement().is_none()
            {
                Ok(())
            } else {
                Err(WorkloadSagaError::InvalidTransition(
                    "teardown disposition cannot disappear before terminal record",
                ))
            }
        }
        (Some(previous), Some(next)) => {
            validate_teardown_state_transition(current, candidate, previous, next)
        }
        _ => Err(WorkloadSagaError::InvalidTransition(
            "invalid teardown disposition transition",
        )),
    }
}

fn validate_teardown_state_transition(
    current: &WorkloadSagaRecord,
    candidate: &WorkloadSagaRecord,
    previous: &WorkloadTeardownDisposition,
    next: &WorkloadTeardownDisposition,
) -> Result<(), WorkloadSagaError> {
    if current.phase == candidate.phase {
        return match (previous, next) {
            (
                WorkloadTeardownDisposition::Ready { context },
                WorkloadTeardownDisposition::DispatchPending {
                    context: next_context,
                    claim,
                },
            ) if context == next_context
                && claim.attempt().issuing_revision() == current.revision
                && claim.attempt().issuing_transition_id()
                    == current.last_transition.transition_id()
                && claim.claimed_revision() == candidate.revision =>
            {
                Ok(())
            }
            (
                WorkloadTeardownDisposition::DispatchPending { context, claim },
                WorkloadTeardownDisposition::InspectionRequired {
                    context: next_context,
                    claim: next_claim,
                },
            ) if context == next_context && claim == next_claim => Ok(()),
            (
                WorkloadTeardownDisposition::InspectionRequired { context, claim },
                WorkloadTeardownDisposition::DispatchPending {
                    context: next_context,
                    claim: next_claim,
                },
            ) if context == next_context
                && claim.attempt() == next_claim.attempt()
                && claim.dispatch_epoch().checked_next() == Some(next_claim.dispatch_epoch())
                && next_claim.claimed_revision() == candidate.revision
                && matches!(
                    next_claim.authorization(),
                    WorkloadTeardownDispatchAuthorization::RetryAfterNotCompleted(evidence)
                        if evidence.matches_inspection(current, claim)
                ) =>
            {
                Ok(())
            }
            (
                WorkloadTeardownDisposition::DispatchPending {
                    context: previous_context,
                    claim: previous_claim,
                }
                | WorkloadTeardownDisposition::InspectionRequired {
                    context: previous_context,
                    claim: previous_claim,
                },
                WorkloadTeardownDisposition::InspectionRequired {
                    context: next_context,
                    claim,
                },
            ) if previous_claim == claim
                && context_matches_except_successor_fence(previous_context, next_context)
                && next.context().successor_fence()
                    == candidate.successor_intent.as_ref().map(|successor| {
                        WorkloadTeardownSuccessorFence::new(
                            successor.generation(),
                            successor.desired_digest(),
                        )
                    }) =>
            {
                Ok(())
            }
            (
                WorkloadTeardownDisposition::Ready { context },
                WorkloadTeardownDisposition::Ready {
                    context: next_context,
                },
            ) if context.cause() == next_context.cause()
                && context.completed() == next_context.completed()
                && context_matches_except_successor_fence(context, next_context)
                && context.successor_fence() != next_context.successor_fence() =>
            {
                Ok(())
            }
            _ => Err(WorkloadSagaError::InvalidTransition(
                "same-phase teardown transition is not a claim, inspection, retry, or successor fence",
            )),
        };
    }

    match (previous, next) {
        (
            WorkloadTeardownDisposition::DispatchPending { context, claim }
            | WorkloadTeardownDisposition::InspectionRequired { context, claim },
            WorkloadTeardownDisposition::Ready {
                context: next_context,
            },
        ) => {
            if !success_appends_exact_receipt_and_observation(
                current,
                candidate,
                context,
                next_context,
                claim,
            ) {
                return Err(WorkloadSagaError::InvalidTransition(
                    "teardown success must append the exact claim receipt",
                ));
            }
            Ok(())
        }
        (
            WorkloadTeardownDisposition::Ready { context },
            WorkloadTeardownDisposition::Ready {
                context: next_context,
            },
        ) if context == next_context => Ok(()),
        (
            WorkloadTeardownDisposition::DispatchPending { context, claim }
            | WorkloadTeardownDisposition::InspectionRequired { context, claim },
            WorkloadTeardownDisposition::DefiniteFailure {
                context: next_context,
                claim: next_claim,
                confirmation,
                prior_terminal_observations,
                ..
            },
        ) if context == next_context
            && claim == next_claim
            && confirmation.matches_current(current, claim)
            && matches!(
                &current.phase_detail,
                WorkloadPhaseDetail::Teardown(detail)
                    if prior_terminal_observations == detail.terminal_observations()
            )
            && candidate.phase == WorkloadSagaPhase::CleanupPending =>
        {
            Ok(())
        }
        _ => Err(WorkloadSagaError::InvalidTransition(
            "teardown phase transition lacks exact resource-free, receipt, or failure evidence",
        )),
    }
}

impl WorkloadSagaRecord {
    pub(crate) fn commit_teardown_successor(
        &self,
        successor: WorkloadSagaIntent,
        provision_absence: Option<WorkloadProvisionTeardownAbsence>,
        restart_settlement: Option<WorkloadRestartTeardownSettlement>,
    ) -> Result<Self, WorkloadSagaError> {
        if !self.phase.is_provision() || successor.generation() <= self.active_intent.generation() {
            return Err(WorkloadSagaError::InvalidTransition(
                "successor teardown requires a higher generation from provision state",
            ));
        }
        let cause = WorkloadTeardownCause::Successor {
            generation: successor.generation(),
            desired_digest: successor.desired_digest(),
        };
        let fence =
            WorkloadTeardownSuccessorFence::new(successor.generation(), successor.desired_digest());
        match (&self.provision_disposition, &provision_absence) {
            (Some(WorkloadProvisionDisposition::Ready), None) => {}
            (Some(WorkloadProvisionDisposition::InspectionRequired(claim)), None)
                if claim.attempt().target_phase() == self.phase => {}
            (Some(WorkloadProvisionDisposition::InspectionRequired(claim)), Some(absence))
                if absence.claim() == claim
                    && absence.evidence().matches_inspection(self, claim) => {}
            _ => {
                return Err(WorkloadSagaError::InvalidTransition(
                    "successor teardown requires ready provision state or exact inspected absence",
                ));
            }
        }
        let context = WorkloadTeardownContext::new(
            cause,
            Some(fence),
            provision_absence,
            restart_settlement.clone(),
        );
        let detail = WorkloadPhaseDetail::teardown(
            WorkloadSagaPhase::WithdrawalCommitted,
            &self.active_intent,
            self.phase,
            self.phase_detail.references(),
            Vec::new(),
        )?;
        let restart = if restart_settlement.is_some() {
            let mut settled = self.restart.clone();
            settled.active = None;
            settled
        } else {
            self.restart_for_outer_transition(&self.active_intent, &Some(successor.clone()))?
        };
        if restart.active().is_some() {
            return Err(WorkloadSagaError::InvalidTransition(
                "issued restart must settle before withdrawal is committed",
            ));
        }
        self.build_next_complete(
            self.active_intent.clone(),
            Some(successor),
            WorkloadSagaPhase::WithdrawalCommitted,
            detail,
            None,
            Some(WorkloadTeardownDisposition::initial(context)),
            restart,
            None,
        )
    }

    pub fn commit_teardown_cause(
        &self,
        cause: WorkloadTeardownCause,
    ) -> Result<Self, WorkloadSagaError> {
        match &cause {
            WorkloadTeardownCause::Successor {
                generation,
                desired_digest,
            } => {
                let successor =
                    self.successor_intent
                        .clone()
                        .ok_or(WorkloadSagaError::InvalidTransition(
                            "successor cause requires a queued successor",
                        ))?;
                if successor.generation() != *generation
                    || successor.desired_digest() != *desired_digest
                {
                    return Err(WorkloadSagaError::InvalidEvidence(
                        "successor cause is crossed with queued intent",
                    ));
                }
                self.commit_teardown_successor(successor, None, None)
            }
            WorkloadTeardownCause::FailedProvision { claim, failure } => {
                if !matches!(
                    &self.provision_disposition,
                    Some(WorkloadProvisionDisposition::DefiniteFailure {
                        claim: retained,
                        failure: retained_failure,
                    }) if retained == claim.as_ref() && retained_failure == failure
                ) {
                    return Err(WorkloadSagaError::InvalidEvidence(
                        "failed-provision teardown requires exact durable failure",
                    ));
                }
                let successor_fence = self.successor_intent.as_ref().map(|successor| {
                    WorkloadTeardownSuccessorFence::new(
                        successor.generation(),
                        successor.desired_digest(),
                    )
                });
                let context = WorkloadTeardownContext::new(cause, successor_fence, None, None);
                let detail = WorkloadPhaseDetail::teardown(
                    WorkloadSagaPhase::WithdrawalCommitted,
                    &self.active_intent,
                    self.phase,
                    self.phase_detail.references(),
                    Vec::new(),
                )?;
                self.build_next_complete(
                    self.active_intent.clone(),
                    self.successor_intent.clone(),
                    WorkloadSagaPhase::WithdrawalCommitted,
                    detail,
                    None,
                    Some(WorkloadTeardownDisposition::initial(context)),
                    self.restart.clone(),
                    None,
                )
            }
        }
    }

    pub(crate) fn fence_provision_for_teardown(
        &self,
        successor: WorkloadSagaIntent,
    ) -> Result<Self, WorkloadSagaError> {
        if !self.phase.is_provision() || successor.generation() <= self.active_intent.generation() {
            return Err(WorkloadSagaError::InvalidTransition(
                "provision teardown fence requires a higher successor generation",
            ));
        }
        let disposition = match self.provision_disposition.as_ref() {
            Some(WorkloadProvisionDisposition::DispatchPending(claim)) => {
                WorkloadProvisionDisposition::InspectionRequired(claim.clone())
            }
            Some(WorkloadProvisionDisposition::InspectionRequired(claim)) => {
                WorkloadProvisionDisposition::InspectionRequired(claim.clone())
            }
            Some(WorkloadProvisionDisposition::DefiniteFailure { claim, failure }) => {
                WorkloadProvisionDisposition::DefiniteFailure {
                    claim: claim.clone(),
                    failure: failure.clone(),
                }
            }
            _ => {
                return Err(WorkloadSagaError::InvalidTransition(
                    "provision teardown fence requires unresolved provision state",
                ));
            }
        };
        self.build_next_complete(
            self.active_intent.clone(),
            Some(successor),
            self.phase,
            self.phase_detail.clone(),
            Some(disposition),
            None,
            self.restart.clone(),
            self.failure.clone(),
        )
    }

    /// Commit withdrawal after exact inspection proves a fenced provision effect absent.
    pub fn provision_inspection_absence_to_teardown(
        &self,
        evidence: WorkloadProvisionAbsenceEvidence,
    ) -> Result<Self, WorkloadSagaError> {
        let claim = match self.provision_disposition.as_ref() {
            Some(WorkloadProvisionDisposition::InspectionRequired(claim))
                if claim.attempt().source_phase() == self.phase
                    && evidence.matches_inspection(self, claim) =>
            {
                claim.clone()
            }
            _ => {
                return Err(WorkloadSagaError::InvalidEvidence(
                    "provision teardown requires exact durable absence evidence",
                ));
            }
        };
        let successor =
            self.successor_intent
                .clone()
                .ok_or(WorkloadSagaError::InvalidTransition(
                    "provision absence can begin teardown only for a queued successor",
                ))?;
        let absence = WorkloadProvisionTeardownAbsence::new(claim, evidence)?;
        self.commit_teardown_successor(successor, Some(absence), None)
    }

    /// Commit withdrawal after a fenced provision success is durably recorded.
    pub fn commit_queued_successor_teardown(&self) -> Result<Self, WorkloadSagaError> {
        let successor =
            self.successor_intent
                .clone()
                .ok_or(WorkloadSagaError::InvalidTransition(
                    "queued successor teardown requires one durable successor",
                ))?;
        self.commit_teardown_successor(successor, None, None)
    }

    /// Hand a terminal successor-vetoed restart to the durable teardown reducer once.
    pub fn commit_restart_settlement_teardown(&self) -> Result<Self, WorkloadSagaError> {
        let successor =
            self.successor_intent
                .clone()
                .ok_or(WorkloadSagaError::InvalidTransition(
                    "restart teardown handoff requires one queued successor",
                ))?;
        let active = self
            .restart
            .active()
            .ok_or(WorkloadSagaError::InvalidTransition(
                "restart teardown handoff requires active restart settlement",
            ))?;
        if active.successor_veto_generation() != Some(successor.generation()) {
            return Err(WorkloadSagaError::InvalidEvidence(
                "restart teardown handoff is crossed with successor veto",
            ));
        }
        let (claim, result) = match active.disposition() {
            WorkloadRestartDisposition::SuccessorVetoed { claim, result } => {
                (claim.clone(), result.clone())
            }
            _ => {
                return Err(WorkloadSagaError::InvalidTransition(
                    "restart teardown handoff requires a terminal successor-veto result",
                ));
            }
        };
        let source_execution = WorkloadExecutionReference::for_restart_epoch(
            &self.active_intent,
            self.restart.completed_restart_epoch(),
        );
        let target_execution = WorkloadExecutionReference::for_restart_epoch(
            &self.active_intent,
            active.admission().restart_epoch(),
        );
        let settlement = WorkloadRestartTeardownSettlement::new(
            claim,
            result,
            source_execution,
            target_execution,
            active.owner_observations().to_vec(),
        )?;
        self.commit_teardown_successor(successor, None, Some(settlement))
    }

    pub(crate) fn advance_teardown_successor_fence(
        &self,
        successor: WorkloadSagaIntent,
    ) -> Result<Self, WorkloadSagaError> {
        let disposition =
            self.teardown_disposition
                .as_deref()
                .ok_or(WorkloadSagaError::InvalidTransition(
                    "teardown successor replacement requires active teardown state",
                ))?;
        let fence =
            WorkloadTeardownSuccessorFence::new(successor.generation(), successor.desired_digest());
        let context = disposition.context().with_successor_fence(fence)?;
        let next = match disposition {
            WorkloadTeardownDisposition::Ready { .. } => {
                WorkloadTeardownDisposition::Ready { context }
            }
            WorkloadTeardownDisposition::DispatchPending { claim, .. }
            | WorkloadTeardownDisposition::InspectionRequired { claim, .. } => {
                WorkloadTeardownDisposition::InspectionRequired {
                    context,
                    claim: claim.clone(),
                }
            }
            WorkloadTeardownDisposition::DefiniteFailure { .. } => {
                return Err(WorkloadSagaError::InvalidTransition(
                    "failed teardown must reconcile before successor replacement",
                ));
            }
        };
        self.build_next_complete(
            self.active_intent.clone(),
            Some(successor),
            self.phase,
            self.phase_detail.clone(),
            None,
            Some(next),
            self.restart.clone(),
            self.failure.clone(),
        )
    }

    pub fn decide_teardown(&self) -> Result<WorkloadTeardownDecision, WorkloadSagaError> {
        let disposition = match self.teardown_disposition.as_deref() {
            Some(disposition) => disposition,
            None => return Ok(WorkloadTeardownDecision::Quiescent),
        };
        match disposition {
            WorkloadTeardownDisposition::DispatchPending { claim, .. }
            | WorkloadTeardownDisposition::InspectionRequired { claim, .. } => {
                return Ok(WorkloadTeardownDecision::InspectExact(claim.clone()));
            }
            WorkloadTeardownDisposition::DefiniteFailure { claim, failure, .. } => {
                return Ok(WorkloadTeardownDecision::CleanupPending {
                    claim: claim.clone(),
                    failure: failure.clone(),
                });
            }
            WorkloadTeardownDisposition::Ready { .. } => {}
        }
        if self.phase == WorkloadSagaPhase::NetworkReleased {
            if let Some(settlement) = disposition.context().restart_settlement() {
                return Ok(WorkloadTeardownDecision::RestartSettlementPending(
                    Box::new(settlement.clone()),
                ));
            }
            return Ok(WorkloadTeardownDecision::PersistCandidate(
                ProposedWorkloadTeardownTransition::RecordTerminal,
            ));
        }
        let step =
            teardown_step_for_phase(self.phase).ok_or(WorkloadSagaError::InvalidTransition(
                "teardown disposition is crossed with a non-teardown phase",
            ))?;
        let target_phase = step.phases().1;
        let Some(subjects) = teardown_subject_for_step(self, step)? else {
            return Ok(WorkloadTeardownDecision::PersistCandidate(
                ProposedWorkloadTeardownTransition::ResourceFree { step, target_phase },
            ));
        };
        let attempt = WorkloadTeardownAttempt::new(WorkloadTeardownAttemptInput {
            key: self.key.clone(),
            saga_id: self.saga_id.clone(),
            issuing_revision: self.revision,
            issuing_transition_id: self.last_transition.transition_id.clone(),
            generation: self.active_intent.generation(),
            desired_digest: self.active_intent.desired_digest(),
            required_node: self.active_intent.admission().assigned_node().clone(),
            source_digest: self.active_intent.source().source_digest(),
            execution_provider_id: self.active_intent.source().execution_provider_id().clone(),
            network_plan_digest: self.active_intent.network().digest(),
            selection_evidence: self
                .active_intent
                .network()
                .compiled_plan()
                .content()
                .capability_selection_evidence()
                .cloned(),
            cause: disposition.cause().clone(),
            successor_fence: disposition.context().successor_fence(),
            source_phase: self.phase,
            target_phase,
            step,
            subjects,
        })?;
        let Some(provider_target) = WorkloadTeardownProviderTarget::for_attempt(&attempt)? else {
            return Ok(WorkloadTeardownDecision::PersistCandidate(
                ProposedWorkloadTeardownTransition::ResourceFree { step, target_phase },
            ));
        };
        Ok(WorkloadTeardownDecision::PersistCandidate(
            ProposedWorkloadTeardownTransition::Claim {
                attempt: Box::new(attempt),
                provider_target,
            },
        ))
    }

    pub fn claim_teardown(
        &self,
        attempt: WorkloadTeardownAttempt,
        provider_target: WorkloadTeardownProviderTarget,
    ) -> Result<Self, WorkloadSagaError> {
        let expected = self.decide_teardown()?;
        if expected
            != WorkloadTeardownDecision::PersistCandidate(
                ProposedWorkloadTeardownTransition::Claim {
                    attempt: Box::new(attempt.clone()),
                    provider_target: provider_target.clone(),
                },
            )
        {
            return Err(WorkloadSagaError::InvalidEvidence(
                "teardown claim is stale, crossed, or not the exact reducer candidate",
            ));
        }
        let context = self
            .teardown_disposition
            .as_ref()
            .expect("decision requires teardown disposition")
            .context()
            .clone();
        let claim = WorkloadTeardownClaim::initial(attempt, provider_target)?;
        self.build_teardown_transition(
            self.phase,
            self.phase_detail.clone(),
            WorkloadTeardownDisposition::DispatchPending { context, claim },
            None,
        )
    }

    pub fn teardown_dispatch_to_inspection(
        &self,
        claim: &WorkloadTeardownClaim,
    ) -> Result<Self, WorkloadSagaError> {
        let context = match self.teardown_disposition.as_deref() {
            Some(WorkloadTeardownDisposition::DispatchPending {
                context,
                claim: retained,
            }) if retained == claim => context.clone(),
            _ => {
                return Err(WorkloadSagaError::InvalidTransition(
                    "teardown inspection requires the exact pending claim",
                ));
            }
        };
        self.build_teardown_transition(
            self.phase,
            self.phase_detail.clone(),
            WorkloadTeardownDisposition::InspectionRequired {
                context,
                claim: claim.clone(),
            },
            None,
        )
    }

    pub fn apply_teardown_effect_result(
        &self,
        claim: &WorkloadTeardownClaim,
        result: WorkloadTeardownEffectResult,
    ) -> Result<Self, WorkloadSagaError> {
        let context = match self.teardown_disposition.as_deref() {
            Some(WorkloadTeardownDisposition::DispatchPending {
                context,
                claim: retained,
            }) if retained == claim => context.clone(),
            _ => {
                return Err(WorkloadSagaError::InvalidTransition(
                    "teardown effect result requires the exact pending claim",
                ));
            }
        };
        result.validate_for_claim(claim)?;
        match result {
            WorkloadTeardownEffectResult::Succeeded { evidence, .. } => self
                .advance_teardown_success(
                    context,
                    claim.clone(),
                    *evidence,
                    WorkloadTeardownResultConfirmation::dispatch(),
                ),
            WorkloadTeardownEffectResult::DefiniteFailure { failure, .. } => self
                .enter_teardown_failure(
                    context,
                    claim.clone(),
                    failure,
                    WorkloadTeardownResultConfirmation::dispatch(),
                ),
            WorkloadTeardownEffectResult::Ambiguous { .. } => self.build_teardown_transition(
                self.phase,
                self.phase_detail.clone(),
                WorkloadTeardownDisposition::InspectionRequired {
                    context,
                    claim: claim.clone(),
                },
                None,
            ),
        }
    }

    pub fn apply_teardown_inspection_result(
        &self,
        claim: &WorkloadTeardownClaim,
        result: WorkloadTeardownInspectionResult,
    ) -> Result<Self, WorkloadSagaError> {
        let context = match self.teardown_disposition.as_deref() {
            Some(WorkloadTeardownDisposition::InspectionRequired {
                context,
                claim: retained,
            }) if retained == claim => context.clone(),
            _ => {
                return Err(WorkloadSagaError::InvalidTransition(
                    "teardown inspection result requires the exact inspection claim",
                ));
            }
        };
        result.validate_for_claim(self, claim)?;
        match result {
            WorkloadTeardownInspectionResult::NotCompleted { evidence } => {
                self.teardown_inspection_to_retry(claim, evidence)
            }
            WorkloadTeardownInspectionResult::Ambiguous { .. }
            | WorkloadTeardownInspectionResult::InProgress { .. } => Ok(self.clone()),
            WorkloadTeardownInspectionResult::DefiniteFailure {
                inspection_command_id,
                failure,
                ..
            } => {
                let confirmation = WorkloadTeardownResultConfirmation::for_inspection(
                    self,
                    claim,
                    inspection_command_id,
                )?;
                self.enter_teardown_failure(context, claim.clone(), failure, confirmation)
            }
            WorkloadTeardownInspectionResult::Satisfied {
                inspection_command_id,
                evidence,
                ..
            } => {
                let confirmation = WorkloadTeardownResultConfirmation::for_inspection(
                    self,
                    claim,
                    inspection_command_id,
                )?;
                self.advance_teardown_success(context, claim.clone(), evidence, confirmation)
            }
        }
    }

    pub fn teardown_inspection_to_retry(
        &self,
        claim: &WorkloadTeardownClaim,
        evidence: WorkloadTeardownRetryEvidence,
    ) -> Result<Self, WorkloadSagaError> {
        let context = match self.teardown_disposition.as_deref() {
            Some(WorkloadTeardownDisposition::InspectionRequired {
                context,
                claim: retained,
            }) if retained == claim && evidence.matches_inspection(self, claim) => context.clone(),
            _ => {
                return Err(WorkloadSagaError::InvalidEvidence(
                    "teardown retry requires exact not-completed inspection evidence",
                ));
            }
        };
        let next_revision = self
            .revision
            .checked_next()
            .ok_or(WorkloadSagaError::RevisionOverflow)?;
        let next =
            WorkloadTeardownClaim::retry_after_not_completed(claim, next_revision, evidence)?;
        self.build_teardown_transition(
            self.phase,
            self.phase_detail.clone(),
            WorkloadTeardownDisposition::DispatchPending {
                context,
                claim: next,
            },
            None,
        )
    }

    pub fn record_resource_free_teardown_step(
        &self,
        step: WorkloadTeardownStep,
    ) -> Result<Self, WorkloadSagaError> {
        let target_phase = step.phases().1;
        if self.decide_teardown()?
            != WorkloadTeardownDecision::PersistCandidate(
                ProposedWorkloadTeardownTransition::ResourceFree { step, target_phase },
            )
        {
            return Err(WorkloadSagaError::InvalidTransition(
                "resource-free teardown advance requires the exact reducer candidate",
            ));
        }
        let context = self
            .teardown_disposition
            .as_ref()
            .expect("decision requires teardown disposition")
            .context()
            .clone();
        let detail = next_teardown_detail(self, target_phase, None)?;
        self.build_teardown_transition(
            target_phase,
            detail,
            WorkloadTeardownDisposition::Ready { context },
            None,
        )
    }

    pub fn record_terminal_teardown(&self) -> Result<Self, WorkloadSagaError> {
        if self.decide_teardown()?
            != WorkloadTeardownDecision::PersistCandidate(
                ProposedWorkloadTeardownTransition::RecordTerminal,
            )
        {
            return Err(WorkloadSagaError::InvalidTransition(
                "terminal teardown record requires exact network-released state",
            ));
        }
        let WorkloadPhaseDetail::Teardown(detail) = &self.phase_detail else {
            return Err(WorkloadSagaError::InvalidEvidence(
                "terminal teardown requires retained teardown observations",
            ));
        };
        let digest =
            WorkloadTerminalEvidenceDigest::for_observations(detail.terminal_observations())?;
        self.build_next_complete(
            self.active_intent.clone(),
            self.successor_intent.clone(),
            WorkloadSagaPhase::Recorded,
            WorkloadPhaseDetail::recorded(&self.active_intent, digest),
            None,
            None,
            self.restart.clone(),
            None,
        )
    }

    fn advance_teardown_success(
        &self,
        context: WorkloadTeardownContext,
        claim: WorkloadTeardownClaim,
        evidence: WorkloadTeardownSuccessEvidence,
        confirmation: WorkloadTeardownResultConfirmation,
    ) -> Result<Self, WorkloadSagaError> {
        let target_phase = claim.attempt().target_phase();
        let detail = next_teardown_detail(self, target_phase, Some(&evidence))?;
        let receipt = WorkloadTeardownReceipt::new(claim, evidence, confirmation)?;
        let context = context.with_receipt(receipt)?;
        self.build_teardown_transition(
            target_phase,
            detail,
            WorkloadTeardownDisposition::Ready { context },
            None,
        )
    }

    fn enter_teardown_failure(
        &self,
        context: WorkloadTeardownContext,
        claim: WorkloadTeardownClaim,
        failure: WorkloadFailureEvidence,
        confirmation: WorkloadTeardownResultConfirmation,
    ) -> Result<Self, WorkloadSagaError> {
        let references = retained_teardown_cleanup_references(self)?;
        let inspections = cleanup_inspections(self.phase, &references);
        let WorkloadPhaseDetail::Teardown(detail) = &self.phase_detail else {
            return Err(WorkloadSagaError::InvalidEvidence(
                "teardown failure requires retained terminal observations",
            ));
        };
        let prior_terminal_observations = detail.terminal_observations().to_vec();
        let detail = WorkloadPhaseDetail::cleanup_pending(
            &self.active_intent,
            self.phase,
            references,
            inspections,
        )?;
        self.build_teardown_transition(
            WorkloadSagaPhase::CleanupPending,
            detail,
            WorkloadTeardownDisposition::DefiniteFailure {
                context,
                claim,
                failure: failure.clone(),
                confirmation,
                prior_terminal_observations,
            },
            Some(failure),
        )
    }

    fn build_teardown_transition(
        &self,
        phase: WorkloadSagaPhase,
        phase_detail: WorkloadPhaseDetail,
        teardown_disposition: WorkloadTeardownDisposition,
        failure: Option<WorkloadFailureEvidence>,
    ) -> Result<Self, WorkloadSagaError> {
        self.build_next_complete(
            self.active_intent.clone(),
            self.successor_intent.clone(),
            phase,
            phase_detail,
            None,
            Some(teardown_disposition),
            self.restart.clone(),
            failure,
        )
    }
}

fn teardown_step_for_phase(phase: WorkloadSagaPhase) -> Option<WorkloadTeardownStep> {
    match phase {
        WorkloadSagaPhase::WithdrawalCommitted => Some(WorkloadTeardownStep::WithdrawPublication),
        WorkloadSagaPhase::Withdrawn => Some(WorkloadTeardownStep::DrainExecution),
        WorkloadSagaPhase::Drained => Some(WorkloadTeardownStep::StopExecution),
        WorkloadSagaPhase::WorkloadStopped => Some(WorkloadTeardownStep::DetachNetwork),
        WorkloadSagaPhase::NetworkDetached => Some(WorkloadTeardownStep::ReleaseNetwork),
        _ => None,
    }
}

fn teardown_subject_for_step(
    record: &WorkloadSagaRecord,
    step: WorkloadTeardownStep,
) -> Result<Option<WorkloadTeardownSubjects>, WorkloadSagaError> {
    let WorkloadPhaseDetail::Teardown(detail) = &record.phase_detail else {
        return Err(WorkloadSagaError::InvalidEvidence(
            "teardown decision requires teardown phase detail",
        ));
    };
    let origin_rank = detail.origin().recovery_order();
    let references = detail.retained_references();
    Ok(match step {
        WorkloadTeardownStep::WithdrawPublication
            if origin_rank >= WorkloadSagaPhase::Published.recovery_order() =>
        {
            references
                .publication()
                .cloned()
                .map(WorkloadTeardownSubjects::Publication)
        }
        WorkloadTeardownStep::DrainExecution
            if origin_rank >= WorkloadSagaPhase::WorkloadActivated.recovery_order() =>
        {
            references
                .execution()
                .cloned()
                .map(WorkloadTeardownSubjects::Execution)
        }
        WorkloadTeardownStep::StopExecution
            if origin_rank >= WorkloadSagaPhase::WorkloadPrepared.recovery_order() =>
        {
            references
                .execution()
                .cloned()
                .map(WorkloadTeardownSubjects::Execution)
        }
        WorkloadTeardownStep::DetachNetwork
            if origin_rank >= WorkloadSagaPhase::NetworkAttached.recovery_order()
                && record
                    .active_intent
                    .network()
                    .compiled_plan()
                    .content()
                    .capability_selection_evidence()
                    .is_some() =>
        {
            references
                .network()
                .cloned()
                .map(WorkloadTeardownSubjects::Network)
        }
        WorkloadTeardownStep::ReleaseNetwork
            if origin_rank >= WorkloadSagaPhase::NetworkReserved.recovery_order()
                && record
                    .active_intent
                    .network()
                    .compiled_plan()
                    .content()
                    .capability_selection_evidence()
                    .is_some() =>
        {
            references
                .network()
                .cloned()
                .map(WorkloadTeardownSubjects::Network)
        }
        _ => None,
    })
}

pub(super) fn retained_teardown_cleanup_references(
    record: &WorkloadSagaRecord,
) -> Result<WorkloadEffectReferences, WorkloadSagaError> {
    let WorkloadPhaseDetail::Teardown(detail) = &record.phase_detail else {
        return Err(WorkloadSagaError::InvalidEvidence(
            "teardown cleanup requires retained teardown detail",
        ));
    };
    let origin_rank = detail.origin().recovery_order();
    let phase_rank = record.phase.recovery_order();
    let references = detail.retained_references();
    let provider_managed_network = record
        .active_intent
        .network()
        .compiled_plan()
        .content()
        .capability_selection_evidence()
        .is_some();

    let publication = (phase_rank < WorkloadSagaPhase::Withdrawn.recovery_order()
        && origin_rank >= WorkloadSagaPhase::Published.recovery_order())
    .then(|| references.publication().cloned())
    .flatten();
    let execution = (phase_rank < WorkloadSagaPhase::WorkloadStopped.recovery_order()
        && origin_rank >= WorkloadSagaPhase::WorkloadPrepared.recovery_order())
    .then(|| references.execution().cloned())
    .flatten();
    let network = (phase_rank < WorkloadSagaPhase::NetworkReleased.recovery_order()
        && provider_managed_network
        && origin_rank >= WorkloadSagaPhase::NetworkReserved.recovery_order())
    .then(|| references.network().cloned())
    .flatten();

    let retained = WorkloadEffectReferences::new(network, execution, publication);
    if retained.is_empty() {
        return Err(WorkloadSagaError::InvalidEvidence(
            "definite teardown failure must retain an established provider resource",
        ));
    }
    Ok(retained)
}

fn next_teardown_detail(
    record: &WorkloadSagaRecord,
    target_phase: WorkloadSagaPhase,
    evidence: Option<&WorkloadTeardownSuccessEvidence>,
) -> Result<WorkloadPhaseDetail, WorkloadSagaError> {
    let WorkloadPhaseDetail::Teardown(current) = &record.phase_detail else {
        return Err(WorkloadSagaError::InvalidEvidence(
            "teardown advance requires retained teardown detail",
        ));
    };
    let mut terminal_observations = current.terminal_observations().to_vec();
    if let Some(evidence) = evidence {
        terminal_observations.push(evidence.terminal_observation());
    }
    WorkloadPhaseDetail::teardown(
        target_phase,
        &record.active_intent,
        current.origin(),
        current.retained_references().clone(),
        terminal_observations,
    )
}

fn cleanup_inspections(
    phase: WorkloadSagaPhase,
    references: &WorkloadEffectReferences,
) -> Vec<WorkloadInspectionRequirement> {
    let mut inspections = Vec::with_capacity(references.len());
    if let Some(reference) = references.network() {
        inspections.push(WorkloadInspectionRequirement::Network {
            reference: reference.clone(),
            expected_phase: phase,
        });
    }
    if let Some(reference) = references.execution() {
        inspections.push(WorkloadInspectionRequirement::Execution {
            reference: reference.clone(),
            expected_phase: phase,
        });
    }
    if let Some(reference) = references.publication() {
        inspections.push(WorkloadInspectionRequirement::Publication {
            reference: reference.clone(),
            expected_phase: phase,
        });
    }
    inspections
}
