//! Intent succession for workload-saga records.
//!
//! A record applies a new intent, promotes its successor, and proves that a
//! successor record continues the current record's evidence.

use super::phase_detail_state::promoted_phase_detail;
use super::*;

impl WorkloadSagaRecord {
    pub fn apply_intent(
        &self,
        candidate: WorkloadSagaIntent,
    ) -> Result<WorkloadSagaIntentUpdate, WorkloadSagaError> {
        let current_high = self
            .successor_intent
            .as_ref()
            .map_or(self.active_intent.generation, |intent| intent.generation);
        match candidate.generation.cmp(&current_high) {
            std::cmp::Ordering::Less => Err(WorkloadSagaError::StaleGeneration {
                current: current_high,
                candidate: candidate.generation,
            }),
            std::cmp::Ordering::Equal => {
                let expected = self
                    .successor_intent
                    .as_ref()
                    .unwrap_or(&self.active_intent);
                if expected == &candidate {
                    Ok(WorkloadSagaIntentUpdate::Unchanged)
                } else {
                    Err(WorkloadSagaError::EqualGenerationConflict(
                        candidate.generation,
                    ))
                }
            }
            std::cmp::Ordering::Greater if self.phase == WorkloadSagaPhase::CleanupPending => {
                Err(WorkloadSagaError::InvalidTransition(
                    "cleanup pending must be inspected before generation replacement",
                ))
            }
            std::cmp::Ordering::Greater
                if self.phase == WorkloadSagaPhase::Recorded && self.successor_intent.is_some() =>
            {
                self.build_next(
                    self.active_intent.clone(),
                    Some(candidate),
                    self.phase,
                    self.phase_detail.clone(),
                    None,
                )
                .map(Box::new)
                .map(WorkloadSagaIntentUpdate::Transition)
            }
            std::cmp::Ordering::Greater if self.phase == WorkloadSagaPhase::Recorded => {
                let (phase, detail) = promoted_phase_detail(self, &candidate)?;
                self.build_next(candidate, None, phase, detail, None)
                    .map(Box::new)
                    .map(WorkloadSagaIntentUpdate::Transition)
            }
            std::cmp::Ordering::Greater => {
                if matches!(
                    self.provision_disposition,
                    Some(
                        WorkloadProvisionDisposition::DispatchPending(_)
                            | WorkloadProvisionDisposition::InspectionRequired(_)
                            | WorkloadProvisionDisposition::DefiniteFailure { .. }
                    )
                ) {
                    return self
                        .fence_provision_for_teardown(candidate)
                        .map(Box::new)
                        .map(WorkloadSagaIntentUpdate::Transition);
                }
                if self.restart.active.as_ref().is_some_and(|active| {
                    active.phase() != WorkloadRestartPhase::Requested
                        || !active.disposition().is_ready()
                        || active.disposition().receipt().is_some()
                }) {
                    return self
                        .build_next(
                            self.active_intent.clone(),
                            Some(candidate),
                            self.phase,
                            self.phase_detail.clone(),
                            self.failure.clone(),
                        )
                        .map(Box::new)
                        .map(WorkloadSagaIntentUpdate::Transition);
                }
                if self.phase.is_teardown() {
                    self.advance_teardown_successor_fence(candidate)
                } else {
                    self.commit_teardown_successor(candidate, None, None)
                }
                .map(Box::new)
                .map(WorkloadSagaIntentUpdate::Transition)
            }
        }
    }

    /// Promotes the exact queued successor after the active generation is recorded.
    pub fn promote_successor(&self) -> Result<Self, WorkloadSagaError> {
        if self.phase != WorkloadSagaPhase::Recorded {
            return Err(WorkloadSagaError::InvalidTransition(
                "successor promotion requires recorded active generation",
            ));
        }
        let successor =
            self.successor_intent
                .clone()
                .ok_or(WorkloadSagaError::InvalidTransition(
                    "successor promotion requires queued intent",
                ))?;
        let (phase, detail) = promoted_phase_detail(self, &successor)?;
        self.build_next(successor, None, phase, detail, None)
    }

    pub fn validate_successor(&self, candidate: &Self) -> Result<(), WorkloadSagaError> {
        candidate.validate()?;
        if candidate.saga_id != self.saga_id || candidate.key != self.key {
            return Err(WorkloadSagaError::InvalidTransition(
                "candidate belongs to another workload saga",
            ));
        }
        if candidate.revision
            != self
                .revision
                .checked_next()
                .ok_or(WorkloadSagaError::RevisionOverflow)?
        {
            return Err(WorkloadSagaError::InvalidTransition(
                "candidate revision is not the exact successor revision",
            ));
        }
        if candidate.last_transition.source_phase != Some(self.phase)
            || candidate.last_transition.target_phase != candidate.phase
        {
            return Err(WorkloadSagaError::InvalidTransition(
                "candidate transition phases do not bind the loaded record",
            ));
        }
        let active_changed = candidate.active_intent != self.active_intent;
        if active_changed {
            if self.phase != WorkloadSagaPhase::Recorded
                || candidate.active_intent.generation <= self.active_intent.generation
                || candidate.successor_intent.is_some()
                || !self.phase_detail.references().is_empty()
            {
                return Err(WorkloadSagaError::InvalidTransition(
                    "active generation can change only after recorded cleanup",
                ));
            }
            if let Some(successor) = &self.successor_intent
                && &candidate.active_intent != successor
            {
                return Err(WorkloadSagaError::InvalidTransition(
                    "promotion must consume the exact queued successor",
                ));
            }
            let (initial_phase, initial_detail) =
                promoted_phase_detail(self, &candidate.active_intent)?;
            if candidate.phase != initial_phase
                || candidate.phase_detail != initial_detail
                || candidate.provision_disposition
                    != initial_provision_disposition(&candidate.active_intent)
                || candidate.failure.is_some()
            {
                return Err(WorkloadSagaError::InvalidTransition(
                    "promoted generation must enter its exact initial phase",
                ));
            }
        } else if candidate.phase != self.phase
            && !legal_phase_edge(self.phase, candidate.phase, self.active_intent.publication)
            && !owner_reopened_publication_transition_is_exact(self, candidate)
        {
            return Err(WorkloadSagaError::InvalidTransition(
                "candidate contains an illegal phase edge",
            ));
        }
        validate_provision_disposition_transition(self, candidate, active_changed)?;
        validate_restart_state_transition(self, candidate, active_changed)?;
        validate_teardown_disposition_transition(self, candidate, active_changed)?;
        validate_successor_intent_change(self, candidate, active_changed)?;
        validate_evidence_continuity(self, candidate, active_changed)?;
        if let Some(successor) = &candidate.successor_intent {
            if successor.generation <= self.active_intent.generation {
                return Err(WorkloadSagaError::InvalidTransition(
                    "successor generation must be higher than active generation",
                ));
            }
            if let Some(previous) = &self.successor_intent
                && successor.generation < previous.generation
            {
                return Err(WorkloadSagaError::InvalidTransition(
                    "candidate replaced a successor with a stale generation",
                ));
            }
        }
        Ok(())
    }
}

fn validate_successor_intent_change(
    current: &WorkloadSagaRecord,
    candidate: &WorkloadSagaRecord,
    active_changed: bool,
) -> Result<(), WorkloadSagaError> {
    if current.phase == WorkloadSagaPhase::CleanupPending
        && current.successor_intent != candidate.successor_intent
    {
        return Err(WorkloadSagaError::InvalidTransition(
            "cleanup pending must resolve before successor replacement",
        ));
    }
    if active_changed {
        return Ok(());
    }
    match (&current.successor_intent, &candidate.successor_intent) {
        (Some(_), None) => Err(WorkloadSagaError::InvalidTransition(
            "queued successor cannot be discarded before promotion",
        )),
        (Some(previous), Some(next)) if previous != next => {
            if next.generation == previous.generation {
                return Err(WorkloadSagaError::EqualGenerationConflict(next.generation));
            }
            if next.generation < previous.generation {
                return Err(WorkloadSagaError::StaleGeneration {
                    current: previous.generation,
                    candidate: next.generation,
                });
            }
            if candidate.phase != current.phase
                || candidate.phase_detail != current.phase_detail
                || candidate.failure != current.failure
            {
                return Err(WorkloadSagaError::InvalidTransition(
                    "successor replacement cannot change active-generation lifecycle state",
                ));
            }
            Ok(())
        }
        (None, Some(_))
            if (current.phase.is_teardown()
                && candidate.phase == current.phase
                && candidate.phase_detail == current.phase_detail
                && candidate.failure == current.failure)
                || (candidate.phase == WorkloadSagaPhase::WithdrawalCommitted
                    && current.phase.is_provision())
                || (candidate.phase == current.phase
                    && candidate.phase_detail == current.phase_detail
                    && candidate.failure == current.failure
                    && matches!(
                        current.provision_disposition,
                        Some(
                            WorkloadProvisionDisposition::DispatchPending(_)
                                | WorkloadProvisionDisposition::InspectionRequired(_)
                                | WorkloadProvisionDisposition::DefiniteFailure { .. }
                        )
                    ))
                || (candidate.phase == current.phase
                    && candidate.phase_detail == current.phase_detail
                    && candidate.failure == current.failure
                    && candidate.restart.active.as_ref().is_some_and(|active| {
                        active.successor_veto_generation
                            == candidate
                                .successor_intent
                                .as_ref()
                                .map(WorkloadSagaIntent::generation)
                    })) =>
        {
            Ok(())
        }
        (None, Some(_)) => Err(WorkloadSagaError::InvalidTransition(
            "queuing a successor must preserve or withdraw active-generation state",
        )),
        _ => Ok(()),
    }
}

fn validate_evidence_continuity(
    current: &WorkloadSagaRecord,
    candidate: &WorkloadSagaRecord,
    active_changed: bool,
) -> Result<(), WorkloadSagaError> {
    if active_changed {
        return Ok(());
    }
    if owner_reopened_publication_transition_is_exact(current, candidate) {
        return Ok(());
    }
    if current.phase == candidate.phase {
        let completes_restart = current
            .restart
            .active()
            .is_some_and(|active| active.phase() == WorkloadRestartPhase::ObservationPending)
            && candidate.restart.active().is_none()
            && current.restart.current_execution_attempt_id()
                != candidate.restart.current_execution_attempt_id();
        if completes_restart {
            return Ok(());
        }
        if (current.phase_detail != candidate.phase_detail || current.failure != candidate.failure)
            && !republication_evidence_refresh_is_exact(current, candidate)
        {
            return Err(WorkloadSagaError::InvalidEvidence(
                "same-phase transition cannot rewrite lifecycle evidence",
            ));
        }
        if current.restart != candidate.restart {
            return Ok(());
        }
        if current.successor_intent == candidate.successor_intent
            && current.provision_disposition == candidate.provision_disposition
            && current.teardown_disposition == candidate.teardown_disposition
        {
            return Err(WorkloadSagaError::InvalidTransition(
                "workload saga transition must change semantic state",
            ));
        }
        return Ok(());
    }

    match (&current.phase_detail, &candidate.phase_detail) {
        (WorkloadPhaseDetail::Provision(previous), WorkloadPhaseDetail::Provision(next)) => {
            if current.phase != WorkloadSagaPhase::IntentCommitted
                && (previous.references.network != next.references.network
                    || previous.references.execution != next.references.execution
                    || previous.references.publication.is_some()
                        && previous.references.publication != next.references.publication)
            {
                return Err(WorkloadSagaError::InvalidEvidence(
                    "provision transition must retain every established effect reference",
                ));
            }
            if !next.observations.starts_with(&previous.observations) {
                return Err(WorkloadSagaError::InvalidEvidence(
                    "provision transition must retain every established owner observation",
                ));
            }
        }
        (WorkloadPhaseDetail::Intent, WorkloadPhaseDetail::Provision(_)) => {}
        (previous, WorkloadPhaseDetail::Teardown(next))
            if candidate.phase == WorkloadSagaPhase::WithdrawalCommitted =>
        {
            let expected = candidate
                .teardown_disposition
                .as_deref()
                .and_then(|disposition| disposition.context().restart_settlement())
                .map(|settlement| {
                    settlement
                        .teardown_seed_from_source(&current.active_intent, &previous.references())
                })
                .transpose()?
                .unwrap_or_else(|| (current.phase, previous.references()));
            if (next.origin, &next.retained_references) != (expected.0, &expected.1) {
                return Err(WorkloadSagaError::InvalidEvidence(
                    "withdrawal must retain the exact origin references",
                ));
            }
        }
        (WorkloadPhaseDetail::Teardown(previous), WorkloadPhaseDetail::Teardown(next)) => {
            if previous.origin != next.origin
                || previous.retained_references != next.retained_references
            {
                return Err(WorkloadSagaError::InvalidEvidence(
                    "teardown transition must retain its exact origin references",
                ));
            }
            if !next
                .terminal_observations
                .starts_with(&previous.terminal_observations)
            {
                return Err(WorkloadSagaError::InvalidEvidence(
                    "teardown transition must retain every established terminal observation",
                ));
            }
        }
        (previous, WorkloadPhaseDetail::CleanupPending(next)) => {
            let expected = if matches!(previous, WorkloadPhaseDetail::Teardown(_)) {
                teardown_state::retained_teardown_cleanup_references(current)?
            } else {
                previous.references()
            };
            if next.last_safe_phase != current.phase || next.retained_references != expected {
                return Err(WorkloadSagaError::InvalidEvidence(
                    "cleanup pending must retain the exact last-safe references",
                ));
            }
        }
        (WorkloadPhaseDetail::Teardown(previous), WorkloadPhaseDetail::Recorded(recorded)) => {
            let expected = WorkloadTerminalEvidenceDigest::for_teardown(
                &previous.terminal_observations,
                current
                    .teardown_disposition
                    .as_deref()
                    .and_then(|disposition| disposition.context().restart_settlement()),
            )?;
            if recorded.terminal_evidence_digest != expected
                || recorded.terminal_execution.as_ref() != previous.terminal_execution_reference()
            {
                return Err(WorkloadSagaError::InvalidEvidence(
                    "recorded terminal evidence does not match teardown evidence",
                ));
            }
        }
        _ => {}
    }
    Ok(())
}
