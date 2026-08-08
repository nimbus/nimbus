//! Bounded compute-owned execution of one durable restart epoch.
//!
//! The driver composes the pure reducer, sole saga coordinator, and exact
//! capability dispatcher. It confirms every result before it can consider a
//! later command, including the direct next-epoch retry after authenticated
//! absence.

use std::sync::Arc;

use nimbus_workloads::{
    WorkloadRestartNotBeforeUnixMillis, WorkloadSagaKey, WorkloadSagaRecord, WorkloadSagaStoreError,
};
use thiserror::Error;

use super::restart_dispatcher::{WorkloadRestartDispatchError, WorkloadRestartDispatcher};
use super::{
    ConfirmedWorkloadRestartTransition, WorkloadRestartCommandMode, WorkloadRestartDecision,
    WorkloadSagaConfirmation, WorkloadSagaCoordinator, apply_restart_result,
    decide_restart_progress,
};

/// Hard bound for one caller-owned restart convergence attempt.
const MAX_RESTART_DECISIONS_PER_RUN: usize = 64;

/// Why one bounded restart run returned control to its caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WorkloadRestartRunDisposition {
    Completed,
    Waiting,
    WaitingUntil(WorkloadRestartNotBeforeUnixMillis),
    DefiniteFailure,
}

/// Exact durable truth at the end of one bounded restart run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct WorkloadRestartRun {
    record: WorkloadSagaRecord,
    disposition: WorkloadRestartRunDisposition,
}

impl WorkloadRestartRun {
    #[cfg(test)]
    pub(super) fn record(&self) -> &WorkloadSagaRecord {
        &self.record
    }

    #[cfg(test)]
    pub(super) const fn disposition(&self) -> WorkloadRestartRunDisposition {
        self.disposition
    }
}

/// Failure before the driver can return confirmed durable truth.
#[derive(Debug, Error)]
pub(super) enum WorkloadRestartRunError {
    #[error("workload restart dispatch failed: {0}")]
    Dispatch(#[from] WorkloadRestartDispatchError),
    #[error("workload restart saga failed: {0}")]
    Saga(#[from] WorkloadSagaStoreError),
    #[error("workload restart recovery record does not exist")]
    Missing,
    #[error("workload restart transition lost its exact durable confirmation")]
    UnconfirmedTransition,
    #[error("workload restart exceeded {MAX_RESTART_DECISIONS_PER_RUN} bounded decisions")]
    ProgressLimit,
}

/// Sole composition of durable restart decisions and exact provider dispatch.
pub(super) struct WorkloadRestartDriver {
    coordinator: Arc<WorkloadSagaCoordinator>,
    dispatcher: Arc<WorkloadRestartDispatcher>,
}

impl WorkloadRestartDriver {
    pub(super) fn new(
        coordinator: Arc<WorkloadSagaCoordinator>,
        dispatcher: Arc<WorkloadRestartDispatcher>,
    ) -> Self {
        Self {
            coordinator,
            dispatcher,
        }
    }

    /// Reopen durable state and advance only its active restart epoch.
    pub(super) async fn resume(
        &self,
        key: &WorkloadSagaKey,
        now_unix_millis: WorkloadRestartNotBeforeUnixMillis,
    ) -> Result<WorkloadRestartRun, WorkloadRestartRunError> {
        let record = self
            .coordinator
            .load(key)
            .await?
            .ok_or(WorkloadRestartRunError::Missing)?;
        self.drive_confirmed_restart(record, now_unix_millis).await
    }

    /// Advance an already-confirmed restart admission under the same bound.
    pub(super) async fn drive_admitted(
        &self,
        record: WorkloadSagaRecord,
        now_unix_millis: WorkloadRestartNotBeforeUnixMillis,
    ) -> Result<WorkloadRestartRun, WorkloadRestartRunError> {
        self.drive_confirmed_restart(record, now_unix_millis).await
    }

    async fn drive_confirmed_restart(
        &self,
        mut record: WorkloadSagaRecord,
        now_unix_millis: WorkloadRestartNotBeforeUnixMillis,
    ) -> Result<WorkloadRestartRun, WorkloadRestartRunError> {
        let mut saw_active_restart = record.restart_state().active().is_some();
        let mut inspection_dispatched = false;
        let mut confirmed: Option<ConfirmedWorkloadRestartTransition> = None;
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
                                .ok_or(WorkloadRestartRunError::Missing)?;
                            continue;
                        }
                        WorkloadSagaConfirmation::UnresolvedAmbiguity => {
                            return Ok(waiting(record));
                        }
                        _ => return Err(WorkloadRestartRunError::UnconfirmedTransition),
                    }
                };
                saw_active_restart |= durable.restart_state().active().is_some();
                if let Some(command) = transition.command().cloned() {
                    require_decision_budget(&mut decisions)?;
                    let result = self
                        .dispatcher
                        .dispatch_confirmed(&transition)
                        .await?
                        .ok_or(WorkloadRestartRunError::UnconfirmedTransition)?;
                    inspection_dispatched = command.mode() == WorkloadRestartCommandMode::Inspect;
                    let result_decision = apply_restart_result(&durable, &command, result)?;
                    match result_decision {
                        WorkloadRestartDecision::Proposed(proposed) => {
                            // Confirm every received result before a budget or
                            // caller boundary can return control.
                            confirmed = Some(
                                self.coordinator
                                    .compare_and_swap_restart_result(&durable, &proposed)
                                    .await?,
                            );
                        }
                        WorkloadRestartDecision::InspectExact(_) if inspection_dispatched => {
                            return Ok(waiting(durable));
                        }
                        WorkloadRestartDecision::InspectExact(_) => {}
                        WorkloadRestartDecision::DefiniteFailure => {
                            return Ok(WorkloadRestartRun {
                                record: durable,
                                disposition: WorkloadRestartRunDisposition::DefiniteFailure,
                            });
                        }
                        WorkloadRestartDecision::WaitUntil(deadline) => {
                            return Ok(WorkloadRestartRun {
                                record: durable,
                                disposition: WorkloadRestartRunDisposition::WaitingUntil(deadline),
                            });
                        }
                        WorkloadRestartDecision::Wait => return Ok(waiting(durable)),
                    }
                    record = durable;
                    continue;
                }
                record = durable;
                inspection_dispatched = false;
            }

            let decision = decide_restart_progress(&record, now_unix_millis)
                .map_err(WorkloadSagaStoreError::InvalidTransition)?;
            match decision {
                WorkloadRestartDecision::Wait => {
                    let disposition =
                        if saw_active_restart && record.restart_state().active().is_none() {
                            WorkloadRestartRunDisposition::Completed
                        } else {
                            WorkloadRestartRunDisposition::Waiting
                        };
                    return Ok(WorkloadRestartRun {
                        record,
                        disposition,
                    });
                }
                WorkloadRestartDecision::WaitUntil(deadline) => {
                    return Ok(WorkloadRestartRun {
                        record,
                        disposition: WorkloadRestartRunDisposition::WaitingUntil(deadline),
                    });
                }
                WorkloadRestartDecision::DefiniteFailure => {
                    return Ok(WorkloadRestartRun {
                        record,
                        disposition: WorkloadRestartRunDisposition::DefiniteFailure,
                    });
                }
                WorkloadRestartDecision::InspectExact(_) if inspection_dispatched => {
                    return Ok(waiting(record));
                }
                WorkloadRestartDecision::InspectExact(_) => {
                    require_decision_budget(&mut decisions)?;
                    confirmed = Some(
                        self.coordinator
                            .inspect_confirmed_restart(record.key())
                            .await?,
                    );
                }
                WorkloadRestartDecision::Proposed(proposed) => {
                    require_decision_budget(&mut decisions)?;
                    confirmed = Some(
                        self.dispatcher
                            .confirm_transition(&self.coordinator, &record, &proposed)
                            .await?,
                    );
                }
            }
        }
    }
}

fn require_decision_budget(decisions: &mut usize) -> Result<(), WorkloadRestartRunError> {
    if *decisions == MAX_RESTART_DECISIONS_PER_RUN {
        return Err(WorkloadRestartRunError::ProgressLimit);
    }
    *decisions += 1;
    Ok(())
}

fn waiting(record: WorkloadSagaRecord) -> WorkloadRestartRun {
    WorkloadRestartRun {
        record,
        disposition: WorkloadRestartRunDisposition::Waiting,
    }
}

#[cfg(test)]
#[path = "restart_driver/tests.rs"]
mod tests;
