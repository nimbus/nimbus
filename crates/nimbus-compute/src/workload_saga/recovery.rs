//! Pure recovery decisions derived from durable workload-saga records.

use nimbus_workloads::{
    DesiredWorkloadState, ProposedWorkloadTeardownTransition, WorkloadGeneration,
    WorkloadSagaError, WorkloadSagaId, WorkloadSagaIntent, WorkloadSagaKey,
    WorkloadSagaPageRequest, WorkloadSagaPhase, WorkloadSagaRecord, WorkloadSagaRecoveryCursor,
    WorkloadSagaRevision, WorkloadSagaStoreError, WorkloadTeardownDecision,
};

use super::{WorkloadProvisionDecision, WorkloadSagaCoordinator};

/// One provider-neutral operation required to recover a durable workload saga.
#[expect(
    clippy::large_enum_variant,
    reason = "complete provider-neutral evidence stays inline at this low-rate pure decision seam"
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkloadSagaAction {
    /// The one pure provision reducer owns every provision-phase decision.
    Provision(WorkloadProvisionDecision),
    /// The portable workloads reducer owns all teardown decisions.
    Teardown(WorkloadTeardownDecision),
    PromoteSuccessor {
        intent: Box<WorkloadSagaIntent>,
    },
    Quiescent,
}

/// Pure recovery decision bound to exact durable identity and fencing state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadSagaDecision {
    key: WorkloadSagaKey,
    saga_id: WorkloadSagaId,
    revision: WorkloadSagaRevision,
    active_generation: WorkloadGeneration,
    target_phase: WorkloadSagaPhase,
    action: WorkloadSagaAction,
}

impl WorkloadSagaDecision {
    /// Selects an action without reading a store or invoking an effect owner.
    pub fn for_record(record: &WorkloadSagaRecord) -> Result<Self, WorkloadSagaError> {
        record.validate()?;
        let (target_phase, action) = match record.phase() {
            phase if phase.is_provision() => {
                let decision = WorkloadProvisionDecision::plan(record)?;
                let target_phase = decision.target_phase(record.phase());
                (target_phase, WorkloadSagaAction::Provision(decision))
            }
            WorkloadSagaPhase::Recorded => match record.successor_intent() {
                Some(intent) => {
                    let target_phase = match intent.desired_state() {
                        DesiredWorkloadState::Running => WorkloadSagaPhase::IntentCommitted,
                        DesiredWorkloadState::Stopped => WorkloadSagaPhase::Recorded,
                    };
                    (
                        target_phase,
                        WorkloadSagaAction::PromoteSuccessor {
                            intent: Box::new(intent.clone()),
                        },
                    )
                }
                None => (WorkloadSagaPhase::Recorded, WorkloadSagaAction::Quiescent),
            },
            _ => {
                let decision = record.decide_teardown()?;
                let target_phase = teardown_target_phase(record, &decision);
                (target_phase, WorkloadSagaAction::Teardown(decision))
            }
        };

        Ok(Self::new(record, target_phase, action))
    }

    fn new(
        record: &WorkloadSagaRecord,
        target_phase: WorkloadSagaPhase,
        action: WorkloadSagaAction,
    ) -> Self {
        Self {
            key: record.key().clone(),
            saga_id: record.saga_id().clone(),
            revision: record.revision(),
            active_generation: record.active_intent().generation(),
            target_phase,
            action,
        }
    }

    pub fn key(&self) -> &WorkloadSagaKey {
        &self.key
    }

    pub fn saga_id(&self) -> &WorkloadSagaId {
        &self.saga_id
    }

    pub fn revision(&self) -> WorkloadSagaRevision {
        self.revision
    }

    pub fn active_generation(&self) -> WorkloadGeneration {
        self.active_generation
    }

    pub fn target_phase(&self) -> WorkloadSagaPhase {
        self.target_phase
    }

    pub fn action(&self) -> &WorkloadSagaAction {
        &self.action
    }
}

fn teardown_target_phase(
    record: &WorkloadSagaRecord,
    decision: &WorkloadTeardownDecision,
) -> WorkloadSagaPhase {
    match decision {
        WorkloadTeardownDecision::PersistCandidate(ProposedWorkloadTeardownTransition::Claim {
            attempt,
            ..
        }) => attempt.target_phase(),
        WorkloadTeardownDecision::PersistCandidate(
            ProposedWorkloadTeardownTransition::ResourceFree { target_phase, .. },
        ) => *target_phase,
        WorkloadTeardownDecision::PersistCandidate(
            ProposedWorkloadTeardownTransition::RecordTerminal,
        ) => WorkloadSagaPhase::Recorded,
        WorkloadTeardownDecision::InspectExact(claim) => claim.attempt().target_phase(),
        WorkloadTeardownDecision::Quiescent
        | WorkloadTeardownDecision::RestartSettlementPending(_)
        | WorkloadTeardownDecision::CleanupPending { .. } => record.phase(),
    }
}

/// One bounded, ordered page of pure recovery decisions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadSagaDecisionPage {
    decisions: Vec<WorkloadSagaDecision>,
    next_cursor: Option<WorkloadSagaRecoveryCursor>,
}

impl WorkloadSagaDecisionPage {
    pub fn decisions(&self) -> &[WorkloadSagaDecision] {
        &self.decisions
    }

    pub fn next_cursor(&self) -> Option<&WorkloadSagaRecoveryCursor> {
        self.next_cursor.as_ref()
    }
}

impl WorkloadSagaCoordinator {
    /// Reads exactly one bounded recovery page and derives ordered pure decisions.
    pub async fn plan_recoverable_page(
        &self,
        request: WorkloadSagaPageRequest,
    ) -> Result<WorkloadSagaDecisionPage, WorkloadSagaStoreError> {
        let page = self.store.list_recoverable(request).await?;
        let mut decisions = Vec::with_capacity(page.records().len());
        for record in page.records() {
            decisions.push(WorkloadSagaDecision::for_record(record)?);
        }
        Ok(WorkloadSagaDecisionPage {
            decisions,
            next_cursor: page.next_cursor().cloned(),
        })
    }
}

#[cfg(test)]
#[path = "recovery/tests.rs"]
pub(crate) mod tests;
