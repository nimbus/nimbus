//! Bounded durable driver for one workload teardown generation.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use nimbus_workloads::{
    WorkloadSagaKey, WorkloadSagaRecord, WorkloadSagaStoreError, WorkloadTeardownDecision,
};
use thiserror::Error;

use super::teardown_command::{
    ConfirmedWorkloadTeardownTransition, WorkloadTeardownResultDecision, apply_teardown_result,
};
use super::teardown_decision::materialize_teardown_candidate;
use super::teardown_dispatch::{WorkloadTeardownDispatchError, WorkloadTeardownDispatcher};
use super::{WorkloadSagaConfirmation, WorkloadSagaCoordinator};

const MAX_TEARDOWN_DECISIONS_PER_RUN: usize = 64;

type WorkloadTeardownRunFuture<'a> = Pin<
    Box<dyn Future<Output = Result<WorkloadTeardownRun, WorkloadTeardownRunError>> + Send + 'a>,
>;

/// Why one bounded teardown run returned control to its caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkloadTeardownRunDisposition {
    Completed,
    Waiting,
    RestartSettlementPending,
    CleanupPending,
}

/// Exact durable truth at the end of one bounded teardown run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadTeardownRun {
    record: WorkloadSagaRecord,
    disposition: WorkloadTeardownRunDisposition,
}

impl WorkloadTeardownRun {
    pub fn record(&self) -> &WorkloadSagaRecord {
        &self.record
    }

    pub const fn disposition(&self) -> WorkloadTeardownRunDisposition {
        self.disposition
    }
}

/// Failure before the driver can return confirmed durable truth.
#[derive(Debug, Error)]
pub enum WorkloadTeardownRunError {
    #[error("workload teardown dispatch failed: {0}")]
    Dispatch(#[from] WorkloadTeardownDispatchError),
    #[error("workload teardown saga failed: {0}")]
    Saga(#[from] WorkloadSagaStoreError),
    #[error("workload teardown recovery record does not exist")]
    Missing,
    #[error("workload teardown transition lost its exact durable confirmation")]
    UnconfirmedTransition,
    #[error("workload teardown exceeded {MAX_TEARDOWN_DECISIONS_PER_RUN} bounded decisions")]
    ProgressLimit,
}

/// Sole composition of portable teardown decisions and exact provider dispatch.
pub struct WorkloadTeardownDriver {
    coordinator: Arc<WorkloadSagaCoordinator>,
    dispatcher: Arc<WorkloadTeardownDispatcher>,
}

impl WorkloadTeardownDriver {
    pub fn new(
        coordinator: Arc<WorkloadSagaCoordinator>,
        dispatcher: Arc<WorkloadTeardownDispatcher>,
    ) -> Self {
        Self {
            coordinator,
            dispatcher,
        }
    }

    /// Reopen durable state for one exact key and advance only its teardown.
    pub fn resume<'a>(&'a self, key: &'a WorkloadSagaKey) -> WorkloadTeardownRunFuture<'a> {
        Box::pin(async move {
            let record = self
                .coordinator
                .load(key)
                .await?
                .ok_or(WorkloadTeardownRunError::Missing)?;
            self.drive(record).await
        })
    }

    fn drive<'a>(&'a self, mut record: WorkloadSagaRecord) -> WorkloadTeardownRunFuture<'a> {
        Box::pin(async move {
            let mut confirmed: Option<ConfirmedWorkloadTeardownTransition> = None;
            let mut decisions = 0;

            loop {
                if let Some(transition) = confirmed.take() {
                    let Some(durable) = transition.confirmed_record().cloned() else {
                        match transition.confirmation() {
                            WorkloadSagaConfirmation::Conflict { .. } => {
                                record = self
                                    .coordinator
                                    .load(record.key())
                                    .await?
                                    .ok_or(WorkloadTeardownRunError::Missing)?;
                                continue;
                            }
                            WorkloadSagaConfirmation::UnresolvedAmbiguity => {
                                return Ok(waiting(record));
                            }
                            _ => return Err(WorkloadTeardownRunError::UnconfirmedTransition),
                        }
                    };
                    if let Some(command) = transition.command().cloned() {
                        require_decision_budget(&mut decisions)?;
                        let result = self
                            .dispatcher
                            .dispatch_confirmed(&transition)
                            .await?
                            .ok_or(WorkloadTeardownRunError::UnconfirmedTransition)?;
                        match apply_teardown_result(&durable, &command, result)? {
                            WorkloadTeardownResultDecision::PersistCandidate(candidate) => {
                                // A received provider result is always followed by
                                // its durable CAS before any budget or caller boundary.
                                confirmed = Some(
                                    self.coordinator
                                        .confirm_teardown_transition(&durable, *candidate)
                                        .await?,
                                );
                            }
                            WorkloadTeardownResultDecision::Waiting => {
                                return Ok(waiting(durable));
                            }
                        }
                        record = durable;
                        continue;
                    }
                    record = durable;
                }

                match record
                    .decide_teardown()
                    .map_err(WorkloadSagaStoreError::InvalidTransition)?
                {
                    WorkloadTeardownDecision::Quiescent => {
                        return Ok(WorkloadTeardownRun {
                            record,
                            disposition: WorkloadTeardownRunDisposition::Completed,
                        });
                    }
                    WorkloadTeardownDecision::RestartSettlementPending(_) => {
                        return Ok(WorkloadTeardownRun {
                            record,
                            disposition: WorkloadTeardownRunDisposition::RestartSettlementPending,
                        });
                    }
                    WorkloadTeardownDecision::CleanupPending { .. } => {
                        return Ok(WorkloadTeardownRun {
                            record,
                            disposition: WorkloadTeardownRunDisposition::CleanupPending,
                        });
                    }
                    WorkloadTeardownDecision::InspectExact(_) => {
                        require_decision_budget(&mut decisions)?;
                        confirmed = Some(
                            self.coordinator
                                .inspect_confirmed_teardown(record.key())
                                .await?,
                        );
                    }
                    WorkloadTeardownDecision::PersistCandidate(proposed) => {
                        require_decision_budget(&mut decisions)?;
                        let candidate = materialize_teardown_candidate(&record, &proposed)?;
                        confirmed = Some(
                            self.coordinator
                                .confirm_teardown_transition(&record, candidate)
                                .await?,
                        );
                    }
                }
            }
        })
    }
}

fn require_decision_budget(decisions: &mut usize) -> Result<(), WorkloadTeardownRunError> {
    if *decisions == MAX_TEARDOWN_DECISIONS_PER_RUN {
        return Err(WorkloadTeardownRunError::ProgressLimit);
    }
    *decisions += 1;
    Ok(())
}

fn waiting(record: WorkloadSagaRecord) -> WorkloadTeardownRun {
    WorkloadTeardownRun {
        record,
        disposition: WorkloadTeardownRunDisposition::Waiting,
    }
}

#[cfg(test)]
#[path = "teardown_driver/tests.rs"]
mod tests;
