//! Bounded compute-owned execution of one durable provision generation.
//!
//! The driver composes the existing pure reducer, sole saga coordinator, and
//! exact capability dispatcher. It never owns provider effects itself. Every
//! effect is preceded by the exact dispatch-claim CAS, and every terminal
//! provider observation is followed by one exact successor CAS before the
//! next command can be considered.

use std::sync::Arc;

use nimbus_workloads::{
    WorkloadSagaIntent, WorkloadSagaKey, WorkloadSagaPhase, WorkloadSagaRecord,
    WorkloadSagaStoreError,
};
use thiserror::Error;

use super::{
    WorkloadProvisionDecision, WorkloadProvisionDispatchError, WorkloadProvisionDispatcher,
    WorkloadSagaConfirmation, WorkloadSagaCoordinator, reduce_command_result,
};

/// Hard bound preventing one caller from turning repeated provider uncertainty
/// into a busy reconciliation loop.
const MAX_DECISIONS_PER_RUN: usize = 64;

/// Why one bounded provision run returned control to its caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkloadProvisionRunDisposition {
    /// The generation reached its requested observed state.
    Observed,
    /// The generation is valid but awaits provider progress or an external trigger.
    Waiting,
    /// An exact issued provision result is durable and the queued terminal
    /// successor can enter withdrawal without another provider call.
    SuccessorSettlementReady,
    /// A definite provider failure is durably recorded at the last completed phase.
    DefiniteFailure,
}

/// Exact durable truth at the end of one bounded provision run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadProvisionRun {
    record: WorkloadSagaRecord,
    disposition: WorkloadProvisionRunDisposition,
}

impl WorkloadProvisionRun {
    pub fn record(&self) -> &WorkloadSagaRecord {
        &self.record
    }

    pub const fn disposition(&self) -> WorkloadProvisionRunDisposition {
        self.disposition
    }
}

/// Failure before the driver can return confirmed durable truth.
#[derive(Debug, Error)]
pub enum WorkloadProvisionRunError {
    #[error("workload provision dispatch failed: {0}")]
    Dispatch(#[from] WorkloadProvisionDispatchError),
    #[error("workload provision saga failed: {0}")]
    Saga(#[from] WorkloadSagaStoreError),
    #[error("workload provision recovery record does not exist")]
    Missing,
    #[error("workload provision transition lost its exact durable confirmation")]
    UnconfirmedTransition,
    #[error("workload provision exceeded {MAX_DECISIONS_PER_RUN} bounded decisions in one run")]
    ProgressLimit,
}

/// Sole composition of durable provision decisions and exact provider dispatch.
pub struct WorkloadProvisionDriver {
    coordinator: Arc<WorkloadSagaCoordinator>,
    dispatcher: Arc<WorkloadProvisionDispatcher>,
}

impl WorkloadProvisionDriver {
    pub fn new(
        coordinator: Arc<WorkloadSagaCoordinator>,
        dispatcher: Arc<WorkloadProvisionDispatcher>,
    ) -> Self {
        Self {
            coordinator,
            dispatcher,
        }
    }

    /// Persist one complete desired intent, then advance it through the exact
    /// provider protocol until it is observed, halted, or waiting.
    pub async fn submit_and_drive(
        &self,
        key: WorkloadSagaKey,
        intent: WorkloadSagaIntent,
    ) -> Result<WorkloadProvisionRun, WorkloadProvisionRunError> {
        let confirmed = self.coordinator.submit_intent(key, intent).await?;
        self.drive(confirmed.record().clone()).await
    }

    /// Reopen only durable store truth. A pending provider claim is inspected;
    /// recovery never turns a replayed claim into execute authority.
    pub async fn resume(
        &self,
        key: &WorkloadSagaKey,
    ) -> Result<WorkloadProvisionRun, WorkloadProvisionRunError> {
        let record = self
            .coordinator
            .load(key)
            .await?
            .ok_or(WorkloadProvisionRunError::Missing)?;
        self.drive(record).await
    }

    async fn drive(
        &self,
        mut record: WorkloadSagaRecord,
    ) -> Result<WorkloadProvisionRun, WorkloadProvisionRunError> {
        if provision_successor_settlement_ready(&record) {
            return Ok(WorkloadProvisionRun {
                record,
                disposition: WorkloadProvisionRunDisposition::SuccessorSettlementReady,
            });
        }
        let mut decision = WorkloadProvisionDecision::plan(&record)
            .map_err(WorkloadSagaStoreError::InvalidTransition)?;
        let mut inspection_dispatched = false;
        let mut confirms_provider_result = false;
        let mut decisions = 0;

        loop {
            if matches!(decision, WorkloadProvisionDecision::Wait) {
                return Ok(WorkloadProvisionRun {
                    disposition: if record.phase() == WorkloadSagaPhase::Observed {
                        WorkloadProvisionRunDisposition::Observed
                    } else {
                        WorkloadProvisionRunDisposition::Waiting
                    },
                    record,
                });
            }
            if matches!(decision, WorkloadProvisionDecision::DefiniteFailure) {
                return Ok(WorkloadProvisionRun {
                    record,
                    disposition: WorkloadProvisionRunDisposition::DefiniteFailure,
                });
            }
            if decisions == MAX_DECISIONS_PER_RUN {
                return Err(WorkloadProvisionRunError::ProgressLimit);
            }
            decisions += 1;

            match decision {
                WorkloadProvisionDecision::Wait | WorkloadProvisionDecision::DefiniteFailure => {
                    unreachable!("terminal provision decisions return before budget accounting")
                }
                WorkloadProvisionDecision::InspectExact(_) if inspection_dispatched => {
                    return Ok(WorkloadProvisionRun {
                        record,
                        disposition: WorkloadProvisionRunDisposition::Waiting,
                    });
                }
                WorkloadProvisionDecision::InspectExact(_) => {
                    if decisions == MAX_DECISIONS_PER_RUN {
                        return Err(WorkloadProvisionRunError::ProgressLimit);
                    }
                    let confirmed = self
                        .coordinator
                        .inspect_confirmed_provision(record.key())
                        .await?;
                    let durable = confirmed
                        .confirmed_record()
                        .cloned()
                        .ok_or(WorkloadProvisionRunError::UnconfirmedTransition)?;
                    let command = confirmed
                        .command()
                        .cloned()
                        .ok_or(WorkloadProvisionRunError::UnconfirmedTransition)?;
                    let result = self
                        .dispatcher
                        .dispatch_confirmed(&confirmed)
                        .await?
                        .ok_or(WorkloadProvisionRunError::UnconfirmedTransition)?;
                    decision = reduce_command_result(&durable, &command, result)?;
                    record = durable;
                    inspection_dispatched = true;
                    confirms_provider_result = true;
                }
                WorkloadProvisionDecision::Proposed(proposed) => {
                    let confirmed = if confirms_provider_result {
                        self.coordinator
                            .confirm_provision_transition(&record, &proposed)
                            .await?
                    } else {
                        self.dispatcher
                            .confirm_transition(&self.coordinator, &record, &proposed)
                            .await?
                    };
                    let Some(durable) = confirmed.confirmed_record().cloned() else {
                        match confirmed.confirmation() {
                            WorkloadSagaConfirmation::Conflict { .. } => {
                                record = self
                                    .coordinator
                                    .load(record.key())
                                    .await?
                                    .ok_or(WorkloadProvisionRunError::Missing)?;
                                decision = WorkloadProvisionDecision::plan(&record)
                                    .map_err(WorkloadSagaStoreError::InvalidTransition)?;
                                inspection_dispatched = false;
                                confirms_provider_result = false;
                                continue;
                            }
                            WorkloadSagaConfirmation::UnresolvedAmbiguity => {
                                return Ok(WorkloadProvisionRun {
                                    record,
                                    disposition: WorkloadProvisionRunDisposition::Waiting,
                                });
                            }
                            _ => return Err(WorkloadProvisionRunError::UnconfirmedTransition),
                        }
                    };
                    if provision_successor_settlement_ready(&durable) {
                        return Ok(WorkloadProvisionRun {
                            record: durable,
                            disposition: WorkloadProvisionRunDisposition::SuccessorSettlementReady,
                        });
                    }
                    match confirmed.command().cloned() {
                        Some(command) => {
                            if decisions == MAX_DECISIONS_PER_RUN {
                                return Err(WorkloadProvisionRunError::ProgressLimit);
                            }
                            let result = self
                                .dispatcher
                                .dispatch_confirmed(&confirmed)
                                .await?
                                .ok_or(WorkloadProvisionRunError::UnconfirmedTransition)?;
                            inspection_dispatched =
                                command.mode() == super::WorkloadProvisionCommandMode::Inspect;
                            decision = reduce_command_result(&durable, &command, result)?;
                            confirms_provider_result = true;
                        }
                        None => {
                            inspection_dispatched = false;
                            decision = WorkloadProvisionDecision::plan(&durable)
                                .map_err(WorkloadSagaStoreError::InvalidTransition)?;
                            confirms_provider_result = false;
                        }
                    }
                    record = durable;
                }
            }
        }
    }
}

fn provision_successor_settlement_ready(record: &WorkloadSagaRecord) -> bool {
    record.successor_intent().is_some() && record.commit_queued_successor_teardown().is_ok()
}

#[cfg(test)]
#[path = "provision_driver/tests.rs"]
mod tests;
