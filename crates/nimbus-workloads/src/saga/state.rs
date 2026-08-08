//! Validated workload-saga record and state machine.

use super::*;

#[path = "state/provision.rs"]
mod provision_state;

use provision_state::{
    initial_provision_disposition, validate_provision_disposition,
    validate_provision_disposition_transition,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WorkloadSagaTransition {
    transition_id: WorkloadSagaTransitionId,
    source_phase: Option<WorkloadSagaPhase>,
    target_phase: WorkloadSagaPhase,
    active_generation: WorkloadGeneration,
    successor_generation: Option<WorkloadGeneration>,
    resulting_revision: WorkloadSagaRevision,
}

impl WorkloadSagaTransition {
    pub fn transition_id(&self) -> &WorkloadSagaTransitionId {
        &self.transition_id
    }

    pub fn source_phase(&self) -> Option<WorkloadSagaPhase> {
        self.source_phase
    }

    pub fn target_phase(&self) -> WorkloadSagaPhase {
        self.target_phase
    }

    pub fn resulting_revision(&self) -> WorkloadSagaRevision {
        self.resulting_revision
    }
}

/// Complete portable state for one logical workload across generations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadSagaRecord {
    format_version: u32,
    saga_id: WorkloadSagaId,
    key: WorkloadSagaKey,
    active_intent: WorkloadSagaIntent,
    successor_intent: Option<WorkloadSagaIntent>,
    revision: WorkloadSagaRevision,
    phase: WorkloadSagaPhase,
    phase_detail: WorkloadPhaseDetail,
    provision_disposition: Option<WorkloadProvisionDisposition>,
    last_transition: WorkloadSagaTransition,
    failure: Option<WorkloadFailureEvidence>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct WorkloadSagaRecordWire {
    format_version: u32,
    saga_id: WorkloadSagaId,
    key: WorkloadSagaKey,
    active_intent: WorkloadSagaIntent,
    successor_intent: Option<WorkloadSagaIntent>,
    revision: WorkloadSagaRevision,
    phase: WorkloadSagaPhase,
    phase_detail: WorkloadPhaseDetail,
    provision_disposition: Option<WorkloadProvisionDisposition>,
    last_transition: WorkloadSagaTransition,
    failure: Option<WorkloadFailureEvidence>,
}

impl<'de> Deserialize<'de> for WorkloadSagaRecord {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = WorkloadSagaRecordWire::deserialize(deserializer)?;
        let record = Self {
            format_version: wire.format_version,
            saga_id: wire.saga_id,
            key: wire.key,
            active_intent: wire.active_intent,
            successor_intent: wire.successor_intent,
            revision: wire.revision,
            phase: wire.phase,
            phase_detail: wire.phase_detail,
            provision_disposition: wire.provision_disposition,
            last_transition: wire.last_transition,
            failure: wire.failure,
        };
        record.validate().map_err(serde::de::Error::custom)?;
        Ok(record)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TransitionIdentityPayload<'a> {
    saga_id: &'a WorkloadSagaId,
    expected_revision: Option<WorkloadSagaRevision>,
    resulting_revision: WorkloadSagaRevision,
    source_phase: Option<WorkloadSagaPhase>,
    target_phase: WorkloadSagaPhase,
    active_intent: &'a WorkloadSagaIntent,
    successor_intent: &'a Option<WorkloadSagaIntent>,
    phase_detail: &'a WorkloadPhaseDetail,
    provision_disposition: &'a Option<WorkloadProvisionDisposition>,
    failure: &'a Option<WorkloadFailureEvidence>,
}

fn transition_id(payload: &TransitionIdentityPayload<'_>) -> WorkloadSagaTransitionId {
    let encoded = serde_json::to_vec(payload)
        .expect("closed validated workload transition payload always serializes");
    WorkloadSagaTransitionId(derive_id(
        WorkloadSagaTransitionId::PREFIX,
        b"nimbus.workloads.saga.transition.v3",
        &[std::str::from_utf8(&encoded).expect("JSON is valid UTF-8")],
    ))
}

/// Result of applying desired intent to one logical saga.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkloadSagaIntentUpdate {
    Unchanged,
    Transition(Box<WorkloadSagaRecord>),
}

impl WorkloadSagaRecord {
    pub fn new(
        key: WorkloadSagaKey,
        active_intent: WorkloadSagaIntent,
    ) -> Result<Self, WorkloadSagaError> {
        let saga_id = key.saga_id();
        let revision = WorkloadSagaRevision::new(0);
        let (phase, phase_detail) = initial_phase_detail(&active_intent)?;
        let successor_intent = None;
        let failure = None;
        let provision_disposition = initial_provision_disposition(&active_intent);
        let payload = TransitionIdentityPayload {
            saga_id: &saga_id,
            expected_revision: None,
            resulting_revision: revision,
            source_phase: None,
            target_phase: phase,
            active_intent: &active_intent,
            successor_intent: &successor_intent,
            phase_detail: &phase_detail,
            provision_disposition: &provision_disposition,
            failure: &failure,
        };
        let transition_id = transition_id(&payload);
        let active_generation = active_intent.generation;
        let record = Self {
            format_version: WORKLOAD_SAGA_FORMAT_VERSION,
            saga_id,
            key,
            active_intent,
            successor_intent,
            revision,
            phase,
            phase_detail,
            provision_disposition,
            last_transition: WorkloadSagaTransition {
                transition_id,
                source_phase: None,
                target_phase: phase,
                active_generation,
                successor_generation: None,
                resulting_revision: revision,
            },
            failure,
        };
        record.validate()?;
        Ok(record)
    }

    pub fn format_version(&self) -> u32 {
        self.format_version
    }

    pub fn saga_id(&self) -> &WorkloadSagaId {
        &self.saga_id
    }

    pub fn key(&self) -> &WorkloadSagaKey {
        &self.key
    }

    pub fn active_intent(&self) -> &WorkloadSagaIntent {
        &self.active_intent
    }

    pub fn successor_intent(&self) -> Option<&WorkloadSagaIntent> {
        self.successor_intent.as_ref()
    }

    pub fn revision(&self) -> WorkloadSagaRevision {
        self.revision
    }

    pub fn phase(&self) -> WorkloadSagaPhase {
        self.phase
    }

    pub fn phase_detail(&self) -> &WorkloadPhaseDetail {
        &self.phase_detail
    }

    /// Provision outcome state, present only for a running provision phase.
    pub fn provision_disposition(&self) -> Option<&WorkloadProvisionDisposition> {
        self.provision_disposition.as_ref()
    }

    pub fn last_transition(&self) -> &WorkloadSagaTransition {
        &self.last_transition
    }

    pub fn failure(&self) -> Option<&WorkloadFailureEvidence> {
        self.failure.as_ref()
    }

    pub fn recovery_key(&self) -> (u8, &WorkloadSagaId) {
        (self.phase.recovery_order(), &self.saga_id)
    }

    pub fn requires_recovery(&self) -> bool {
        if self
            .provision_disposition
            .as_ref()
            .is_some_and(WorkloadProvisionDisposition::is_definite_failure)
        {
            return false;
        }
        (self.phase == WorkloadSagaPhase::Recorded && self.successor_intent.is_some())
            || (self.phase.is_recoverable()
                && !(self.phase == WorkloadSagaPhase::NetworkAttached
                    && self.active_intent.activation == WorkloadActivationIntent::PrepareOnly))
    }

    pub fn advance(
        &self,
        phase: WorkloadSagaPhase,
        phase_detail: WorkloadPhaseDetail,
        failure: Option<WorkloadFailureEvidence>,
    ) -> Result<Self, WorkloadSagaError> {
        if self.phase.is_provision()
            && self.provision_disposition != Some(WorkloadProvisionDisposition::Ready)
        {
            return Err(WorkloadSagaError::InvalidTransition(
                "provision attempt must resolve before ordinary lifecycle advance",
            ));
        }
        if self.phase.is_provision()
            && phase.is_provision()
            && !(self.phase == WorkloadSagaPhase::Ready
                && phase == WorkloadSagaPhase::Observed
                && self.active_intent.publication == WorkloadPublicationIntent::Withheld)
        {
            return Err(WorkloadSagaError::InvalidTransition(
                "effectful provision advance requires the exact persisted attempt protocol",
            ));
        }
        if self.phase == WorkloadSagaPhase::CleanupPending {
            return Err(WorkloadSagaError::InvalidTransition(
                "cleanup pending cannot advance without later inspection-result authority",
            ));
        }
        if !legal_phase_edge(self.phase, phase, self.active_intent.publication) {
            return Err(WorkloadSagaError::InvalidTransition(
                "workload saga phase edge is not legal",
            ));
        }
        if phase == WorkloadSagaPhase::WorkloadActivated
            && self.active_intent.activation == WorkloadActivationIntent::PrepareOnly
        {
            return Err(WorkloadSagaError::InvalidTransition(
                "prepare-only intent cannot activate",
            ));
        }
        if failure.is_some() && phase != WorkloadSagaPhase::CleanupPending {
            return Err(WorkloadSagaError::InvalidTransition(
                "failure evidence is valid only for cleanup pending",
            ));
        }
        if phase == WorkloadSagaPhase::WithdrawalCommitted {
            let expected = self.phase_detail.references();
            let WorkloadPhaseDetail::Teardown(detail) = &phase_detail else {
                return Err(WorkloadSagaError::InvalidEvidence(
                    "withdrawal committed requires teardown detail",
                ));
            };
            if detail.origin != self.phase || detail.retained_references != expected {
                return Err(WorkloadSagaError::InvalidEvidence(
                    "withdrawal must retain the exact origin references",
                ));
            }
        }
        if phase == WorkloadSagaPhase::CleanupPending {
            let expected = self.phase_detail.references();
            let WorkloadPhaseDetail::CleanupPending(detail) = &phase_detail else {
                return Err(WorkloadSagaError::InvalidEvidence(
                    "cleanup pending requires cleanup detail",
                ));
            };
            if detail.last_safe_phase != self.phase || detail.retained_references != expected {
                return Err(WorkloadSagaError::InvalidEvidence(
                    "cleanup pending must retain the exact last-safe references",
                ));
            }
        }
        if phase == WorkloadSagaPhase::Recorded {
            let WorkloadPhaseDetail::Teardown(current) = &self.phase_detail else {
                return Err(WorkloadSagaError::InvalidEvidence(
                    "recorded transition requires terminal teardown evidence",
                ));
            };
            let WorkloadPhaseDetail::Recorded(recorded) = &phase_detail else {
                return Err(WorkloadSagaError::InvalidEvidence(
                    "recorded phase requires recorded detail",
                ));
            };
            let expected =
                WorkloadTerminalEvidenceDigest::for_observations(&current.terminal_observations)?;
            if recorded.terminal_evidence_digest != expected {
                return Err(WorkloadSagaError::InvalidEvidence(
                    "recorded terminal evidence digest does not match teardown evidence",
                ));
            }
        }
        self.build_next(
            self.active_intent.clone(),
            self.successor_intent.clone(),
            phase,
            phase_detail,
            failure,
        )
    }

    /// Claim the first dispatch epoch for one exact provision attempt.
    pub fn ready_to_initial_dispatch(
        &self,
        attempt: WorkloadProvisionAttempt,
        provider_target: WorkloadProvisionProviderTarget,
    ) -> Result<Self, WorkloadSagaError> {
        if self.provision_disposition != Some(WorkloadProvisionDisposition::Ready)
            || attempt.issuing_revision() != self.revision
        {
            return Err(WorkloadSagaError::InvalidTransition(
                "initial dispatch requires ready state and the exact current revision",
            ));
        }
        let claim = WorkloadProvisionDispatchClaim::initial(attempt, provider_target)?;
        self.build_provision_transition(
            self.phase,
            self.phase_detail.clone(),
            WorkloadProvisionDisposition::DispatchPending(claim),
            None,
        )
    }

    /// Record a reserve or attach edge for a plan with no provider-owned resources.
    pub fn record_resource_free_network_step(
        &self,
        step: WorkloadProvisionStep,
        phase: WorkloadSagaPhase,
        phase_detail: WorkloadPhaseDetail,
    ) -> Result<Self, WorkloadSagaError> {
        if self.provision_disposition != Some(WorkloadProvisionDisposition::Ready)
            || !matches!(
                step,
                WorkloadProvisionStep::ReserveNetwork | WorkloadProvisionStep::AttachNetwork
            )
            || self
                .active_intent
                .network
                .compiled_plan()
                .content()
                .capability_selection_evidence()
                .is_some()
        {
            return Err(WorkloadSagaError::InvalidTransition(
                "resource-free network transition requires ready state and no provider selection",
            ));
        }
        let expected_edge = matches!(
            (step, self.phase, phase),
            (
                WorkloadProvisionStep::ReserveNetwork,
                WorkloadSagaPhase::IntentCommitted,
                WorkloadSagaPhase::NetworkReserved
            ) | (
                WorkloadProvisionStep::AttachNetwork,
                WorkloadSagaPhase::WorkloadPrepared,
                WorkloadSagaPhase::NetworkAttached
            )
        );
        if !expected_edge {
            return Err(WorkloadSagaError::InvalidTransition(
                "resource-free network step does not match its lifecycle edge",
            ));
        }
        self.build_provision_transition(
            phase,
            phase_detail,
            WorkloadProvisionDisposition::Ready,
            None,
        )
    }

    /// Require side-effect-free inspection after an uncertain dispatch.
    pub fn dispatch_to_inspection(&self) -> Result<Self, WorkloadSagaError> {
        let Some(WorkloadProvisionDisposition::DispatchPending(claim)) =
            self.provision_disposition.as_ref()
        else {
            return Err(WorkloadSagaError::InvalidTransition(
                "dispatch inspection requires an exact pending claim",
            ));
        };
        self.build_provision_transition(
            self.phase,
            self.phase_detail.clone(),
            WorkloadProvisionDisposition::InspectionRequired(claim.clone()),
            None,
        )
    }

    /// Authorize the same stable attempt at the next epoch after exact absence.
    pub fn inspection_to_retry_dispatch(
        &self,
        absence: WorkloadProvisionAbsenceEvidence,
    ) -> Result<Self, WorkloadSagaError> {
        let previous = match self.provision_disposition.as_ref() {
            Some(WorkloadProvisionDisposition::InspectionRequired(previous)) => previous,
            _ => {
                return Err(WorkloadSagaError::InvalidTransition(
                    "dispatch retry requires an exact inspected claim",
                ));
            }
        };
        if !absence.matches_inspection(self, previous) {
            return Err(WorkloadSagaError::InvalidEvidence(
                "retry absence evidence is crossed with the inspected dispatch claim",
            ));
        }
        let claimed_revision = self
            .revision
            .checked_next()
            .ok_or(WorkloadSagaError::RevisionOverflow)?;
        let next = WorkloadProvisionDispatchClaim::retry_after_absence(
            previous,
            claimed_revision,
            absence,
        )?;
        self.build_provision_transition(
            self.phase,
            self.phase_detail.clone(),
            WorkloadProvisionDisposition::DispatchPending(next),
            None,
        )
    }

    /// Persist one exact successful dispatch and return to ready disposition.
    pub fn dispatch_to_success(
        &self,
        phase: WorkloadSagaPhase,
        phase_detail: WorkloadPhaseDetail,
    ) -> Result<Self, WorkloadSagaError> {
        let claim = match self.provision_disposition.as_ref() {
            Some(
                WorkloadProvisionDisposition::DispatchPending(claim)
                | WorkloadProvisionDisposition::InspectionRequired(claim),
            ) => claim,
            _ => {
                return Err(WorkloadSagaError::InvalidTransition(
                    "dispatch success requires an exact unresolved claim",
                ));
            }
        };
        if claim.attempt().step() == WorkloadProvisionStep::InspectActivationPrerequisites {
            return Err(WorkloadSagaError::InvalidTransition(
                "activation-prerequisite success requires a distinct activation dispatch",
            ));
        }
        if phase != claim.attempt().target_phase() {
            return Err(WorkloadSagaError::InvalidTransition(
                "dispatch success must advance to the attempted target phase",
            ));
        }
        self.build_provision_transition(
            phase,
            phase_detail,
            WorkloadProvisionDisposition::Ready,
            None,
        )
    }

    /// Persist prerequisite success as the distinct activation dispatch claim.
    pub fn dispatch_to_activation(
        &self,
        attempt: WorkloadProvisionAttempt,
        provider_target: WorkloadProvisionProviderTarget,
    ) -> Result<Self, WorkloadSagaError> {
        let previous = match self.provision_disposition.as_ref() {
            Some(
                WorkloadProvisionDisposition::DispatchPending(claim)
                | WorkloadProvisionDisposition::InspectionRequired(claim),
            ) => claim,
            _ => {
                return Err(WorkloadSagaError::InvalidTransition(
                    "activation dispatch requires a retained prerequisite claim",
                ));
            }
        };
        if previous.attempt().step() != WorkloadProvisionStep::InspectActivationPrerequisites
            || attempt.issuing_revision() != self.revision
        {
            return Err(WorkloadSagaError::InvalidTransition(
                "activation dispatch must follow exact prerequisite inspection",
            ));
        }
        let claim = WorkloadProvisionDispatchClaim::initial(attempt, provider_target)?;
        self.build_provision_transition(
            self.phase,
            self.phase_detail.clone(),
            WorkloadProvisionDisposition::DispatchPending(claim),
            None,
        )
    }

    /// Halt this generation at its last completed phase after definite failure.
    pub fn dispatch_to_definite_failure(
        &self,
        failure: WorkloadFailureEvidence,
    ) -> Result<Self, WorkloadSagaError> {
        let claim = match self.provision_disposition.as_ref() {
            Some(
                WorkloadProvisionDisposition::DispatchPending(claim)
                | WorkloadProvisionDisposition::InspectionRequired(claim),
            ) => claim.clone(),
            _ => {
                return Err(WorkloadSagaError::InvalidTransition(
                    "dispatch failure requires an exact unresolved claim",
                ));
            }
        };
        self.build_provision_transition(
            self.phase,
            self.phase_detail.clone(),
            WorkloadProvisionDisposition::DefiniteFailure { claim, failure },
            None,
        )
    }

    fn build_provision_transition(
        &self,
        phase: WorkloadSagaPhase,
        phase_detail: WorkloadPhaseDetail,
        provision_disposition: WorkloadProvisionDisposition,
        failure: Option<WorkloadFailureEvidence>,
    ) -> Result<Self, WorkloadSagaError> {
        if self.active_intent.desired_state != DesiredWorkloadState::Running
            || !self.phase.is_provision()
            || !phase.is_provision()
        {
            return Err(WorkloadSagaError::InvalidTransition(
                "provision disposition transition requires a running provision phase",
            ));
        }
        if phase != self.phase
            && !legal_phase_edge(self.phase, phase, self.active_intent.publication)
        {
            return Err(WorkloadSagaError::InvalidTransition(
                "provision disposition transition contains an illegal phase edge",
            ));
        }
        self.build_next_with_provision_disposition(
            self.active_intent.clone(),
            self.successor_intent.clone(),
            phase,
            phase_detail,
            Some(provision_disposition),
            failure,
        )
    }

    pub fn apply_intent(
        &self,
        candidate: WorkloadSagaIntent,
    ) -> Result<WorkloadSagaIntentUpdate, WorkloadSagaError> {
        let current_high = self
            .successor_intent
            .as_ref()
            .map_or(self.active_intent.generation, |intent| intent.generation);
        if candidate.generation > current_high
            && matches!(
                self.provision_disposition,
                Some(
                    WorkloadProvisionDisposition::DispatchPending(_)
                        | WorkloadProvisionDisposition::InspectionRequired(_)
                        | WorkloadProvisionDisposition::DefiniteFailure { .. }
                )
            )
        {
            return Err(WorkloadSagaError::InvalidTransition(
                "unresolved provision state must resolve before generation replacement",
            ));
        }
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
                let (phase, detail) = initial_phase_detail(&candidate)?;
                self.build_next(candidate, None, phase, detail, None)
                    .map(Box::new)
                    .map(WorkloadSagaIntentUpdate::Transition)
            }
            std::cmp::Ordering::Greater => {
                let (phase, detail) = if self.phase.is_teardown() {
                    (self.phase, self.phase_detail.clone())
                } else {
                    let references = self.phase_detail.references();
                    (
                        WorkloadSagaPhase::WithdrawalCommitted,
                        WorkloadPhaseDetail::teardown(
                            WorkloadSagaPhase::WithdrawalCommitted,
                            &self.active_intent,
                            self.phase,
                            references,
                            Vec::new(),
                        )?,
                    )
                };
                self.build_next(
                    self.active_intent.clone(),
                    Some(candidate),
                    phase,
                    detail,
                    None,
                )
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
        let (phase, detail) = initial_phase_detail(&successor)?;
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
            let (initial_phase, initial_detail) = initial_phase_detail(&candidate.active_intent)?;
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
        {
            return Err(WorkloadSagaError::InvalidTransition(
                "candidate contains an illegal phase edge",
            ));
        }
        validate_provision_disposition_transition(self, candidate, active_changed)?;
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

    fn build_next(
        &self,
        active_intent: WorkloadSagaIntent,
        successor_intent: Option<WorkloadSagaIntent>,
        phase: WorkloadSagaPhase,
        phase_detail: WorkloadPhaseDetail,
        failure: Option<WorkloadFailureEvidence>,
    ) -> Result<Self, WorkloadSagaError> {
        let provision_disposition = if phase.is_provision()
            && active_intent.desired_state == DesiredWorkloadState::Running
        {
            Some(WorkloadProvisionDisposition::Ready)
        } else {
            None
        };
        self.build_next_with_provision_disposition(
            active_intent,
            successor_intent,
            phase,
            phase_detail,
            provision_disposition,
            failure,
        )
    }

    fn build_next_with_provision_disposition(
        &self,
        active_intent: WorkloadSagaIntent,
        successor_intent: Option<WorkloadSagaIntent>,
        phase: WorkloadSagaPhase,
        phase_detail: WorkloadPhaseDetail,
        provision_disposition: Option<WorkloadProvisionDisposition>,
        failure: Option<WorkloadFailureEvidence>,
    ) -> Result<Self, WorkloadSagaError> {
        let revision = self
            .revision
            .checked_next()
            .ok_or(WorkloadSagaError::RevisionOverflow)?;
        let payload = TransitionIdentityPayload {
            saga_id: &self.saga_id,
            expected_revision: Some(self.revision),
            resulting_revision: revision,
            source_phase: Some(self.phase),
            target_phase: phase,
            active_intent: &active_intent,
            successor_intent: &successor_intent,
            phase_detail: &phase_detail,
            provision_disposition: &provision_disposition,
            failure: &failure,
        };
        let candidate = Self {
            format_version: WORKLOAD_SAGA_FORMAT_VERSION,
            saga_id: self.saga_id.clone(),
            key: self.key.clone(),
            last_transition: WorkloadSagaTransition {
                transition_id: transition_id(&payload),
                source_phase: Some(self.phase),
                target_phase: phase,
                active_generation: active_intent.generation,
                successor_generation: successor_intent.as_ref().map(|intent| intent.generation),
                resulting_revision: revision,
            },
            active_intent,
            successor_intent,
            revision,
            phase,
            phase_detail,
            provision_disposition,
            failure,
        };
        self.validate_successor(&candidate)?;
        Ok(candidate)
    }

    pub fn validate(&self) -> Result<(), WorkloadSagaError> {
        if self.format_version != WORKLOAD_SAGA_FORMAT_VERSION {
            return Err(WorkloadSagaError::InvalidTransition(
                "unsupported workload saga format version",
            ));
        }
        if self.saga_id != self.key.saga_id() {
            return Err(WorkloadSagaError::InvalidIdentity(
                "workload saga id does not match its tenant-qualified key",
            ));
        }
        self.active_intent.validate()?;
        if self
            .active_intent
            .network()
            .compiled_plan()
            .content()
            .identity()
            .tenant_id()
            != self.key.tenant_id()
        {
            return Err(WorkloadSagaError::InvalidIntent(
                "active network plan tenant must match workload saga tenant",
            ));
        }
        if let Some(successor) = &self.successor_intent {
            successor.validate()?;
            if successor
                .network()
                .compiled_plan()
                .content()
                .identity()
                .tenant_id()
                != self.key.tenant_id()
            {
                return Err(WorkloadSagaError::InvalidIntent(
                    "successor network plan tenant must match workload saga tenant",
                ));
            }
            if successor.generation <= self.active_intent.generation {
                return Err(WorkloadSagaError::InvalidIntent(
                    "successor generation must be higher than active generation",
                ));
            }
            if self.phase.is_provision() {
                return Err(WorkloadSagaError::InvalidTransition(
                    "queued successor requires active-generation withdrawal",
                ));
            }
        }
        validate_phase_detail(self.phase, &self.active_intent, &self.phase_detail)?;
        validate_provision_disposition(self)?;
        if let Some(failure) = &self.failure {
            failure.validate()?;
            if self.phase != WorkloadSagaPhase::CleanupPending {
                return Err(WorkloadSagaError::InvalidEvidence(
                    "failure evidence is valid only for cleanup pending",
                ));
            }
        }
        if self.last_transition.target_phase != self.phase
            || self.last_transition.active_generation != self.active_intent.generation
            || self.last_transition.successor_generation
                != self
                    .successor_intent
                    .as_ref()
                    .map(|intent| intent.generation)
            || self.last_transition.resulting_revision != self.revision
        {
            return Err(WorkloadSagaError::InvalidTransition(
                "last transition does not describe the current record",
            ));
        }
        let expected_revision = if self.revision.as_u64() == 0 {
            if self.last_transition.source_phase.is_some() {
                return Err(WorkloadSagaError::InvalidTransition(
                    "initial transition cannot contain a source phase",
                ));
            }
            let (initial_phase, initial_detail) = initial_phase_detail(&self.active_intent)?;
            if self.phase != initial_phase
                || self.phase_detail != initial_detail
                || self.successor_intent.is_some()
                || self.failure.is_some()
                || self.provision_disposition != initial_provision_disposition(&self.active_intent)
            {
                return Err(WorkloadSagaError::InvalidTransition(
                    "initial revision must contain exact initial intent state",
                ));
            }
            None
        } else {
            let source_phase =
                self.last_transition
                    .source_phase
                    .ok_or(WorkloadSagaError::InvalidTransition(
                        "noninitial transition requires a source phase",
                    ))?;
            match (&self.phase_detail, self.phase) {
                (WorkloadPhaseDetail::Teardown(detail), WorkloadSagaPhase::WithdrawalCommitted)
                    if source_phase != WorkloadSagaPhase::WithdrawalCommitted
                        && detail.origin != source_phase =>
                {
                    return Err(WorkloadSagaError::InvalidEvidence(
                        "withdrawal origin does not match the transition source phase",
                    ));
                }
                (
                    WorkloadPhaseDetail::CleanupPending(detail),
                    WorkloadSagaPhase::CleanupPending,
                ) if detail.last_safe_phase != source_phase => {
                    return Err(WorkloadSagaError::InvalidEvidence(
                        "cleanup last-safe phase does not match the transition source phase",
                    ));
                }
                _ => {}
            }
            if source_phase != self.phase
                && !legal_phase_edge(source_phase, self.phase, self.active_intent.publication)
            {
                let (initial_phase, initial_detail) = initial_phase_detail(&self.active_intent)?;
                let is_generation_promotion = source_phase == WorkloadSagaPhase::Recorded
                    && self.phase == initial_phase
                    && self.phase_detail == initial_detail
                    && self.provision_disposition
                        == initial_provision_disposition(&self.active_intent);
                if !is_generation_promotion {
                    return Err(WorkloadSagaError::InvalidTransition(
                        "last transition source and target do not form a legal phase edge",
                    ));
                }
            }
            Some(WorkloadSagaRevision::new(self.revision.as_u64() - 1))
        };
        let payload = TransitionIdentityPayload {
            saga_id: &self.saga_id,
            expected_revision,
            resulting_revision: self.revision,
            source_phase: self.last_transition.source_phase,
            target_phase: self.phase,
            active_intent: &self.active_intent,
            successor_intent: &self.successor_intent,
            phase_detail: &self.phase_detail,
            provision_disposition: &self.provision_disposition,
            failure: &self.failure,
        };
        if self.last_transition.transition_id != transition_id(&payload) {
            return Err(WorkloadSagaError::InvalidTransition(
                "last transition id does not bind the complete semantic payload",
            ));
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
                    && current.phase.is_provision()) =>
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
    if current.phase == candidate.phase {
        if current.phase_detail != candidate.phase_detail || current.failure != candidate.failure {
            return Err(WorkloadSagaError::InvalidEvidence(
                "same-phase transition cannot rewrite lifecycle evidence",
            ));
        }
        if current.successor_intent == candidate.successor_intent
            && current.provision_disposition == candidate.provision_disposition
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
            if next.origin != current.phase || next.retained_references != previous.references() {
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
            if next.last_safe_phase != current.phase
                || next.retained_references != previous.references()
            {
                return Err(WorkloadSagaError::InvalidEvidence(
                    "cleanup pending must retain the exact last-safe references",
                ));
            }
        }
        (WorkloadPhaseDetail::Teardown(previous), WorkloadPhaseDetail::Recorded(recorded)) => {
            let expected =
                WorkloadTerminalEvidenceDigest::for_observations(&previous.terminal_observations)?;
            if recorded.terminal_evidence_digest != expected {
                return Err(WorkloadSagaError::InvalidEvidence(
                    "recorded terminal evidence digest does not match teardown evidence",
                ));
            }
        }
        _ => {}
    }
    Ok(())
}

fn initial_phase_detail(
    intent: &WorkloadSagaIntent,
) -> Result<(WorkloadSagaPhase, WorkloadPhaseDetail), WorkloadSagaError> {
    match intent.desired_state {
        DesiredWorkloadState::Running => Ok((
            WorkloadSagaPhase::IntentCommitted,
            WorkloadPhaseDetail::Intent,
        )),
        DesiredWorkloadState::Stopped => Ok((
            WorkloadSagaPhase::Recorded,
            WorkloadPhaseDetail::recorded(
                intent,
                WorkloadTerminalEvidenceDigest::for_observations(&[])?,
            ),
        )),
    }
}

fn legal_phase_edge(
    source: WorkloadSagaPhase,
    target: WorkloadSagaPhase,
    publication: WorkloadPublicationIntent,
) -> bool {
    if target == WorkloadSagaPhase::CleanupPending {
        return source != WorkloadSagaPhase::IntentCommitted
            && source != WorkloadSagaPhase::Recorded
            && source != WorkloadSagaPhase::CleanupPending;
    }
    if target == WorkloadSagaPhase::WithdrawalCommitted && source.is_provision() {
        return true;
    }
    matches!(
        (source, target),
        (
            WorkloadSagaPhase::IntentCommitted,
            WorkloadSagaPhase::NetworkReserved
        ) | (
            WorkloadSagaPhase::NetworkReserved,
            WorkloadSagaPhase::WorkloadPrepared
        ) | (
            WorkloadSagaPhase::WorkloadPrepared,
            WorkloadSagaPhase::NetworkAttached
        ) | (
            WorkloadSagaPhase::NetworkAttached,
            WorkloadSagaPhase::WorkloadActivated
        ) | (
            WorkloadSagaPhase::WorkloadActivated,
            WorkloadSagaPhase::Ready
        ) | (WorkloadSagaPhase::Ready, WorkloadSagaPhase::Published)
            | (WorkloadSagaPhase::Published, WorkloadSagaPhase::Observed)
            | (
                WorkloadSagaPhase::WithdrawalCommitted,
                WorkloadSagaPhase::Withdrawn
            )
            | (WorkloadSagaPhase::Withdrawn, WorkloadSagaPhase::Drained)
            | (
                WorkloadSagaPhase::Drained,
                WorkloadSagaPhase::WorkloadStopped
            )
            | (
                WorkloadSagaPhase::WorkloadStopped,
                WorkloadSagaPhase::NetworkDetached
            )
            | (
                WorkloadSagaPhase::NetworkDetached,
                WorkloadSagaPhase::NetworkReleased
            )
            | (
                WorkloadSagaPhase::NetworkReleased,
                WorkloadSagaPhase::Recorded
            )
    ) || (source == WorkloadSagaPhase::Ready
        && target == WorkloadSagaPhase::Observed
        && publication == WorkloadPublicationIntent::Withheld)
}

pub(super) fn validate_phase_detail(
    phase: WorkloadSagaPhase,
    intent: &WorkloadSagaIntent,
    detail: &WorkloadPhaseDetail,
) -> Result<(), WorkloadSagaError> {
    match (phase, detail) {
        (WorkloadSagaPhase::IntentCommitted, WorkloadPhaseDetail::Intent)
            if intent.desired_state == DesiredWorkloadState::Running =>
        {
            Ok(())
        }
        (phase, WorkloadPhaseDetail::Provision(detail)) if phase.is_provision() => {
            validate_provision_detail(phase, intent, detail)
        }
        (phase, WorkloadPhaseDetail::Teardown(detail)) if phase.is_teardown() => {
            validate_teardown_detail(phase, intent, detail)
        }
        (WorkloadSagaPhase::CleanupPending, WorkloadPhaseDetail::CleanupPending(detail)) => {
            validate_cleanup_detail(intent, detail)
        }
        (WorkloadSagaPhase::Recorded, WorkloadPhaseDetail::Recorded(detail)) => {
            if detail.completed_generation != intent.generation
                || detail.desired_digest != intent.desired_digest
            {
                return Err(WorkloadSagaError::InvalidEvidence(
                    "recorded detail is crossed with another desired generation",
                ));
            }
            Ok(())
        }
        _ => Err(WorkloadSagaError::InvalidEvidence(
            "phase detail tag is not valid for the workload saga phase",
        )),
    }
}

fn validate_provision_detail(
    phase: WorkloadSagaPhase,
    intent: &WorkloadSagaIntent,
    detail: &WorkloadProvisionDetail,
) -> Result<(), WorkloadSagaError> {
    if matches!(phase, WorkloadSagaPhase::IntentCommitted) {
        return Err(WorkloadSagaError::InvalidEvidence(
            "intent committed cannot carry provision evidence",
        ));
    }
    if intent.desired_state != DesiredWorkloadState::Running {
        return Err(WorkloadSagaError::InvalidIntent(
            "stopped intent cannot enter provision",
        ));
    }
    if intent.activation == WorkloadActivationIntent::PrepareOnly
        && matches!(
            phase,
            WorkloadSagaPhase::WorkloadActivated
                | WorkloadSagaPhase::Ready
                | WorkloadSagaPhase::Published
                | WorkloadSagaPhase::Observed
        )
    {
        return Err(WorkloadSagaError::InvalidEvidence(
            "prepare-only intent cannot carry activated evidence",
        ));
    }
    detail.references.validate_for(intent)?;
    if detail.references.network.is_none() || detail.references.execution.is_none() {
        return Err(WorkloadSagaError::InvalidEvidence(
            "provision phase requires network and execution references",
        ));
    }

    let publication_required = intent.publication == WorkloadPublicationIntent::PublishWhenReady
        && matches!(
            phase,
            WorkloadSagaPhase::Ready | WorkloadSagaPhase::Published | WorkloadSagaPhase::Observed
        );
    if detail.references.publication.is_some() != publication_required {
        return Err(WorkloadSagaError::InvalidEvidence(
            "publication reference presence does not match phase and publication intent",
        ));
    }
    if phase == WorkloadSagaPhase::Published
        && intent.publication != WorkloadPublicationIntent::PublishWhenReady
    {
        return Err(WorkloadSagaError::InvalidEvidence(
            "published phase requires publish-when-ready intent",
        ));
    }

    let expected = expected_owner_observations(phase, intent.publication)?;
    if detail.observations.len() != expected.len()
        || detail
            .observations
            .iter()
            .zip(expected)
            .any(|(observation, expected)| {
                observation.kind() != expected || !observation.matches(&detail.references)
            })
    {
        return Err(WorkloadSagaError::InvalidEvidence(
            "provision observations are missing, extra, duplicated, crossed, or out of order",
        ));
    }
    Ok(())
}

fn expected_owner_observations(
    phase: WorkloadSagaPhase,
    publication: WorkloadPublicationIntent,
) -> Result<Vec<OwnerObservationKind>, WorkloadSagaError> {
    let mut expected = Vec::new();
    let rank = match phase {
        WorkloadSagaPhase::NetworkReserved => 1,
        WorkloadSagaPhase::WorkloadPrepared => 2,
        WorkloadSagaPhase::NetworkAttached => 3,
        WorkloadSagaPhase::WorkloadActivated => 4,
        WorkloadSagaPhase::Ready => 5,
        WorkloadSagaPhase::Published => 6,
        WorkloadSagaPhase::Observed => match publication {
            WorkloadPublicationIntent::Withheld => 5,
            WorkloadPublicationIntent::PublishWhenReady => 7,
        },
        _ => {
            return Err(WorkloadSagaError::InvalidEvidence(
                "phase has no provision observation matrix",
            ));
        }
    };
    if rank >= 1 {
        expected.push(OwnerObservationKind::NetworkReserved);
    }
    if rank >= 2 {
        expected.push(OwnerObservationKind::ExecutionPrepared);
    }
    if rank >= 3 {
        expected.push(OwnerObservationKind::NetworkAttached);
    }
    if rank >= 4 {
        expected.push(OwnerObservationKind::ExecutionActivated);
    }
    if rank >= 5 {
        expected.push(OwnerObservationKind::Ready);
    }
    if rank >= 6 {
        expected.push(OwnerObservationKind::PublicationPresent);
    }
    if rank >= 7 {
        expected.push(OwnerObservationKind::PublicationObserved);
    }
    Ok(expected)
}

fn validate_teardown_detail(
    phase: WorkloadSagaPhase,
    intent: &WorkloadSagaIntent,
    detail: &WorkloadTeardownDetail,
) -> Result<(), WorkloadSagaError> {
    if !detail.origin.is_provision() {
        return Err(WorkloadSagaError::InvalidEvidence(
            "teardown origin must be a provision phase",
        ));
    }
    detail.retained_references.validate_for(intent)?;
    validate_origin_references(detail.origin, intent, &detail.retained_references)?;
    let expected = expected_terminal_observations(phase, &detail.retained_references);
    if detail.terminal_observations.len() != expected.len()
        || detail
            .terminal_observations
            .iter()
            .zip(expected)
            .any(|(observation, expected)| {
                observation.kind() != expected || !observation.matches(&detail.retained_references)
            })
    {
        return Err(WorkloadSagaError::InvalidEvidence(
            "teardown observations are missing, extra, duplicated, crossed, or out of order",
        ));
    }
    Ok(())
}

fn validate_origin_references(
    origin: WorkloadSagaPhase,
    intent: &WorkloadSagaIntent,
    references: &WorkloadEffectReferences,
) -> Result<(), WorkloadSagaError> {
    if origin == WorkloadSagaPhase::IntentCommitted {
        return if references.is_empty() {
            Ok(())
        } else {
            Err(WorkloadSagaError::InvalidEvidence(
                "intent origin cannot retain effect references",
            ))
        };
    }
    if references.network.is_none() || references.execution.is_none() {
        return Err(WorkloadSagaError::InvalidEvidence(
            "effect-bearing origin must retain network and execution references",
        ));
    }
    let publication_required = intent.publication == WorkloadPublicationIntent::PublishWhenReady
        && matches!(
            origin,
            WorkloadSagaPhase::Ready | WorkloadSagaPhase::Published | WorkloadSagaPhase::Observed
        );
    if references.publication.is_some() != publication_required {
        return Err(WorkloadSagaError::InvalidEvidence(
            "teardown retained publication reference does not match its origin",
        ));
    }
    Ok(())
}

fn expected_terminal_observations(
    phase: WorkloadSagaPhase,
    references: &WorkloadEffectReferences,
) -> Vec<TerminalObservationKind> {
    let rank = match phase {
        WorkloadSagaPhase::WithdrawalCommitted => 0,
        WorkloadSagaPhase::Withdrawn => 1,
        WorkloadSagaPhase::Drained => 2,
        WorkloadSagaPhase::WorkloadStopped => 3,
        WorkloadSagaPhase::NetworkDetached => 4,
        WorkloadSagaPhase::NetworkReleased => 5,
        _ => 0,
    };
    let mut expected = Vec::new();
    if rank >= 1 && references.publication.is_some() {
        expected.push(TerminalObservationKind::PublicationAbsent);
    }
    if rank >= 2 && references.execution.is_some() {
        expected.push(TerminalObservationKind::ExecutionDrained);
    }
    if rank >= 3 && references.execution.is_some() {
        expected.push(TerminalObservationKind::ExecutionStopped);
    }
    if rank >= 4 && references.network.is_some() {
        expected.push(TerminalObservationKind::NetworkDetached);
    }
    if rank >= 5 && references.network.is_some() {
        expected.push(TerminalObservationKind::NetworkReleased);
    }
    expected
}

fn validate_cleanup_detail(
    intent: &WorkloadSagaIntent,
    detail: &WorkloadCleanupPendingDetail,
) -> Result<(), WorkloadSagaError> {
    if matches!(
        detail.last_safe_phase,
        WorkloadSagaPhase::IntentCommitted
            | WorkloadSagaPhase::Recorded
            | WorkloadSagaPhase::CleanupPending
    ) || detail.retained_references.is_empty()
    {
        return Err(WorkloadSagaError::InvalidEvidence(
            "cleanup pending requires an effect-bearing last-safe phase and retained reference",
        ));
    }
    detail.retained_references.validate_for(intent)?;
    if detail.inspections.len() != detail.retained_references.len()
        || detail
            .inspections
            .iter()
            .enumerate()
            .any(|(index, inspection)| {
                !inspection.matches_index(index, &detail.retained_references)
                    || match inspection {
                        WorkloadInspectionRequirement::Network { expected_phase, .. }
                        | WorkloadInspectionRequirement::Execution { expected_phase, .. }
                        | WorkloadInspectionRequirement::Publication { expected_phase, .. } => {
                            *expected_phase != detail.last_safe_phase
                        }
                    }
            })
    {
        return Err(WorkloadSagaError::InvalidEvidence(
            "cleanup inspection set must match every retained subject exactly once in N/E/P order",
        ));
    }
    Ok(())
}
