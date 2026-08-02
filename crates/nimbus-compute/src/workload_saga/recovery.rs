//! Pure recovery decisions derived from durable workload-saga records.

use nimbus_workloads::{
    DesiredWorkloadState, WorkloadActivationIntent, WorkloadEffectReferences,
    WorkloadExecutionReference, WorkloadGeneration, WorkloadInspectionRequirement,
    WorkloadNetworkReference, WorkloadPhaseDetail, WorkloadPublicationIntent,
    WorkloadPublicationReference, WorkloadSagaError, WorkloadSagaId, WorkloadSagaIntent,
    WorkloadSagaKey, WorkloadSagaPageRequest, WorkloadSagaPhase, WorkloadSagaRecord,
    WorkloadSagaRecoveryCursor, WorkloadSagaRevision, WorkloadSagaStoreError,
    WorkloadTerminalEvidenceDigest,
};

use super::WorkloadSagaCoordinator;

/// One provider-neutral operation required to recover a durable workload saga.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkloadSagaAction {
    ReserveNetwork {
        reference: WorkloadNetworkReference,
    },
    PrepareWorkload {
        reference: WorkloadExecutionReference,
    },
    AttachNetwork {
        reference: WorkloadNetworkReference,
    },
    ActivateWorkload {
        reference: WorkloadExecutionReference,
    },
    InspectReadiness {
        network: WorkloadNetworkReference,
        execution: WorkloadExecutionReference,
    },
    Publish {
        reference: WorkloadPublicationReference,
    },
    ObservePublication {
        reference: WorkloadPublicationReference,
    },
    WithdrawPublication {
        reference: WorkloadPublicationReference,
    },
    DrainWorkload {
        reference: WorkloadExecutionReference,
    },
    StopWorkload {
        reference: WorkloadExecutionReference,
    },
    DetachNetwork {
        reference: WorkloadNetworkReference,
    },
    ReleaseNetwork {
        reference: WorkloadNetworkReference,
    },
    RecordTerminalEvidence {
        digest: WorkloadTerminalEvidenceDigest,
    },
    PromoteSuccessor {
        intent: WorkloadSagaIntent,
    },
    InspectCleanup {
        last_safe_phase: WorkloadSagaPhase,
        retained_references: WorkloadEffectReferences,
        inspections: Vec<WorkloadInspectionRequirement>,
    },
    AdvanceWithoutEffect,
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
        let references = record.phase_detail().references();
        let (target_phase, action) = match record.phase() {
            WorkloadSagaPhase::IntentCommitted => (
                WorkloadSagaPhase::NetworkReserved,
                WorkloadSagaAction::ReserveNetwork {
                    reference: WorkloadNetworkReference::for_intent(record.active_intent()),
                },
            ),
            WorkloadSagaPhase::NetworkReserved => (
                WorkloadSagaPhase::WorkloadPrepared,
                WorkloadSagaAction::PrepareWorkload {
                    reference: required_execution(&references)?,
                },
            ),
            WorkloadSagaPhase::WorkloadPrepared => (
                WorkloadSagaPhase::NetworkAttached,
                WorkloadSagaAction::AttachNetwork {
                    reference: required_network(&references)?,
                },
            ),
            WorkloadSagaPhase::NetworkAttached
                if record.active_intent().activation() == WorkloadActivationIntent::PrepareOnly =>
            {
                (
                    WorkloadSagaPhase::NetworkAttached,
                    WorkloadSagaAction::Quiescent,
                )
            }
            WorkloadSagaPhase::NetworkAttached => (
                WorkloadSagaPhase::WorkloadActivated,
                WorkloadSagaAction::ActivateWorkload {
                    reference: required_execution(&references)?,
                },
            ),
            WorkloadSagaPhase::WorkloadActivated => (
                WorkloadSagaPhase::Ready,
                WorkloadSagaAction::InspectReadiness {
                    network: required_network(&references)?,
                    execution: required_execution(&references)?,
                },
            ),
            WorkloadSagaPhase::Ready
                if record.active_intent().publication() == WorkloadPublicationIntent::Withheld =>
            {
                (
                    WorkloadSagaPhase::Observed,
                    WorkloadSagaAction::AdvanceWithoutEffect,
                )
            }
            WorkloadSagaPhase::Ready => (
                WorkloadSagaPhase::Published,
                WorkloadSagaAction::Publish {
                    reference: required_publication(&references)?,
                },
            ),
            WorkloadSagaPhase::Published => (
                WorkloadSagaPhase::Observed,
                WorkloadSagaAction::ObservePublication {
                    reference: required_publication(&references)?,
                },
            ),
            WorkloadSagaPhase::Observed => {
                (WorkloadSagaPhase::Observed, WorkloadSagaAction::Quiescent)
            }
            WorkloadSagaPhase::WithdrawalCommitted => match references.publication() {
                Some(reference) => (
                    WorkloadSagaPhase::Withdrawn,
                    WorkloadSagaAction::WithdrawPublication {
                        reference: reference.clone(),
                    },
                ),
                None => (
                    WorkloadSagaPhase::Withdrawn,
                    WorkloadSagaAction::AdvanceWithoutEffect,
                ),
            },
            WorkloadSagaPhase::Withdrawn => match references.execution() {
                Some(reference) => (
                    WorkloadSagaPhase::Drained,
                    WorkloadSagaAction::DrainWorkload {
                        reference: reference.clone(),
                    },
                ),
                None => (
                    WorkloadSagaPhase::Drained,
                    WorkloadSagaAction::AdvanceWithoutEffect,
                ),
            },
            WorkloadSagaPhase::Drained => match references.execution() {
                Some(reference) => (
                    WorkloadSagaPhase::WorkloadStopped,
                    WorkloadSagaAction::StopWorkload {
                        reference: reference.clone(),
                    },
                ),
                None => (
                    WorkloadSagaPhase::WorkloadStopped,
                    WorkloadSagaAction::AdvanceWithoutEffect,
                ),
            },
            WorkloadSagaPhase::WorkloadStopped => match references.network() {
                Some(reference) => (
                    WorkloadSagaPhase::NetworkDetached,
                    WorkloadSagaAction::DetachNetwork {
                        reference: reference.clone(),
                    },
                ),
                None => (
                    WorkloadSagaPhase::NetworkDetached,
                    WorkloadSagaAction::AdvanceWithoutEffect,
                ),
            },
            WorkloadSagaPhase::NetworkDetached => match references.network() {
                Some(reference) => (
                    WorkloadSagaPhase::NetworkReleased,
                    WorkloadSagaAction::ReleaseNetwork {
                        reference: reference.clone(),
                    },
                ),
                None => (
                    WorkloadSagaPhase::NetworkReleased,
                    WorkloadSagaAction::AdvanceWithoutEffect,
                ),
            },
            WorkloadSagaPhase::NetworkReleased => {
                let WorkloadPhaseDetail::Teardown(detail) = record.phase_detail() else {
                    return Err(WorkloadSagaError::InvalidEvidence(
                        "network-released recovery requires teardown evidence",
                    ));
                };
                (
                    WorkloadSagaPhase::Recorded,
                    WorkloadSagaAction::RecordTerminalEvidence {
                        digest: WorkloadTerminalEvidenceDigest::for_observations(
                            detail.terminal_observations(),
                        )?,
                    },
                )
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
                            intent: intent.clone(),
                        },
                    )
                }
                None => (WorkloadSagaPhase::Recorded, WorkloadSagaAction::Quiescent),
            },
            WorkloadSagaPhase::CleanupPending => {
                let WorkloadPhaseDetail::CleanupPending(detail) = record.phase_detail() else {
                    return Err(WorkloadSagaError::InvalidEvidence(
                        "cleanup recovery requires cleanup-pending evidence",
                    ));
                };
                (
                    WorkloadSagaPhase::CleanupPending,
                    WorkloadSagaAction::InspectCleanup {
                        last_safe_phase: detail.last_safe_phase(),
                        retained_references: detail.retained_references().clone(),
                        inspections: detail.inspections().to_vec(),
                    },
                )
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

fn required_network(
    references: &WorkloadEffectReferences,
) -> Result<WorkloadNetworkReference, WorkloadSagaError> {
    references
        .network()
        .cloned()
        .ok_or(WorkloadSagaError::InvalidEvidence(
            "recovery action requires an exact network reference",
        ))
}

fn required_execution(
    references: &WorkloadEffectReferences,
) -> Result<WorkloadExecutionReference, WorkloadSagaError> {
    references
        .execution()
        .cloned()
        .ok_or(WorkloadSagaError::InvalidEvidence(
            "recovery action requires an exact execution reference",
        ))
}

fn required_publication(
    references: &WorkloadEffectReferences,
) -> Result<WorkloadPublicationReference, WorkloadSagaError> {
    references
        .publication()
        .cloned()
        .ok_or(WorkloadSagaError::InvalidEvidence(
            "recovery action requires an exact publication reference",
        ))
}

#[cfg(test)]
#[path = "recovery/tests.rs"]
mod tests;
