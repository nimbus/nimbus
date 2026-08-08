//! Explicit restart submission through the sole durable workload saga.
//!
//! Callers supply stable logical identity, the exact source generation, and
//! one idempotency key. This seam admits durable work before handing an active
//! record to the retained supervisor. It does not wait for provider effects.

use std::sync::Arc;

use nimbus_workloads::{
    WorkloadProvisionSourceGeneration, WorkloadProvisionSourceIdentity, WorkloadRestartEpoch,
    WorkloadRestartNotBeforeUnixMillis, WorkloadRestartRequestId, WorkloadSagaKey,
    WorkloadSagaRecord,
};
use thiserror::Error;

use super::restart_supervisor::RetainedRestartSupervisor;
use super::restart_watch::RestartSupervisor;
use super::{
    WorkloadRestartAdmissionDisposition, WorkloadRestartAdmissionError,
    WorkloadRestartAdmissionRequest, WorkloadRestartCancellationToken, WorkloadSagaCoordinator,
};

/// Exact caller-owned fences for one immediate explicit restart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExplicitWorkloadRestartRequest {
    key: WorkloadSagaKey,
    source_identity: WorkloadProvisionSourceIdentity,
    source_generation: WorkloadProvisionSourceGeneration,
    idempotency_key: String,
}

impl ExplicitWorkloadRestartRequest {
    pub(crate) fn new(
        key: WorkloadSagaKey,
        source_identity: WorkloadProvisionSourceIdentity,
        source_generation: WorkloadProvisionSourceGeneration,
        idempotency_key: impl Into<String>,
    ) -> Self {
        Self {
            key,
            source_identity,
            source_generation,
            idempotency_key: idempotency_key.into(),
        }
    }
}

/// Durable disposition of one explicit restart submission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExplicitWorkloadRestartDisposition {
    Applied,
    Replayed,
}

/// Stable durable receipt returned before provider convergence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExplicitWorkloadRestartSubmission {
    request_id: WorkloadRestartRequestId,
    restart_epoch: WorkloadRestartEpoch,
    disposition: ExplicitWorkloadRestartDisposition,
}

impl ExplicitWorkloadRestartSubmission {
    pub(crate) fn request_id(&self) -> &WorkloadRestartRequestId {
        &self.request_id
    }

    pub(crate) const fn restart_epoch(&self) -> WorkloadRestartEpoch {
        self.restart_epoch
    }

    pub(crate) const fn disposition(&self) -> ExplicitWorkloadRestartDisposition {
        self.disposition
    }
}

#[derive(Debug, Error)]
pub(crate) enum ExplicitWorkloadRestartError {
    #[error("workload restart was cancelled before durable submission")]
    Cancelled,
    #[error("explicit restart requires an existing workload saga")]
    WorkloadNotFound,
    #[error("explicit restart source identity is stale or crossed")]
    SourceIdentityMismatch,
    #[error("explicit restart source generation is stale or crossed")]
    SourceGenerationMismatch,
    #[error("explicit restart admission failed: {0}")]
    Admission(WorkloadRestartAdmissionError),
    #[error("durable restart admission did not retain the submitted request")]
    MissingDurableReceipt,
    #[error("durable restart supervision failed: {0}")]
    Supervision(String),
}

/// Compute-owned explicit submission capability sharing the automatic watch's
/// coordinator and retained supervisor.
pub(super) struct ExplicitWorkloadRestartSubmitter {
    coordinator: Arc<WorkloadSagaCoordinator>,
    supervisor: Arc<RetainedRestartSupervisor>,
}

impl ExplicitWorkloadRestartSubmitter {
    pub(super) fn new(
        coordinator: Arc<WorkloadSagaCoordinator>,
        supervisor: Arc<RetainedRestartSupervisor>,
    ) -> Self {
        Self {
            coordinator,
            supervisor,
        }
    }

    pub(super) async fn submit(
        &self,
        request: &ExplicitWorkloadRestartRequest,
        cancellation: &WorkloadRestartCancellationToken,
    ) -> Result<ExplicitWorkloadRestartSubmission, ExplicitWorkloadRestartError> {
        if cancellation.is_cancelled() {
            return Err(ExplicitWorkloadRestartError::Cancelled);
        }
        let current = self
            .coordinator
            .load(&request.key)
            .await
            .map_err(|error| {
                ExplicitWorkloadRestartError::Admission(WorkloadRestartAdmissionError::Saga(error))
            })?
            .ok_or(ExplicitWorkloadRestartError::WorkloadNotFound)?;
        if current.active_intent().source().source_identity() != &request.source_identity {
            return Err(ExplicitWorkloadRestartError::SourceIdentityMismatch);
        }
        if current.active_intent().source().source_generation() != request.source_generation {
            return Err(ExplicitWorkloadRestartError::SourceGenerationMismatch);
        }

        // Immediate explicit requests use a stable zero deadline. Wall-clock
        // time would make an exact replay cross its original admission body.
        let admission = WorkloadRestartAdmissionRequest::for_explicit(
            &current,
            &request.idempotency_key,
            WorkloadRestartNotBeforeUnixMillis::new(0),
        )
        .map_err(|error| {
            ExplicitWorkloadRestartError::Admission(WorkloadRestartAdmissionError::Saga(
                nimbus_workloads::WorkloadSagaStoreError::InvalidTransition(error),
            ))
        })?;
        let submitted_request_id = admission.request_id().clone();
        let confirmed = self
            .coordinator
            .compare_and_swap_restart_admission(&admission, cancellation)
            .await
            .map_err(|error| match error {
                WorkloadRestartAdmissionError::Cancelled => ExplicitWorkloadRestartError::Cancelled,
                other => ExplicitWorkloadRestartError::Admission(other),
            })?;
        let (restart_epoch, active) = durable_receipt(confirmed.record(), &submitted_request_id)
            .ok_or(ExplicitWorkloadRestartError::MissingDurableReceipt)?;

        // Tracking is synchronous and follows the durable CAS without an
        // await point. Caller cancellation can drop only its waiter; the task
        // and the bounded durable watch retain recovery authority.
        if active {
            self.supervisor
                .track(confirmed.record().clone())
                .map_err(ExplicitWorkloadRestartError::Supervision)?;
        }

        Ok(ExplicitWorkloadRestartSubmission {
            request_id: submitted_request_id,
            restart_epoch,
            disposition: match confirmed.disposition() {
                WorkloadRestartAdmissionDisposition::Applied => {
                    ExplicitWorkloadRestartDisposition::Applied
                }
                WorkloadRestartAdmissionDisposition::ConfirmedReplay => {
                    ExplicitWorkloadRestartDisposition::Replayed
                }
            },
        })
    }
}

fn durable_receipt(
    record: &WorkloadSagaRecord,
    request_id: &WorkloadRestartRequestId,
) -> Option<(WorkloadRestartEpoch, bool)> {
    if let Some(active) = record.restart_state().active()
        && active.admission().request_id() == request_id
    {
        return Some((active.admission().restart_epoch(), true));
    }
    record
        .restart_state()
        .last_completed()
        .filter(|completed| completed.request_id() == request_id)
        .map(|completed| (completed.restart_epoch(), false))
}

#[cfg(test)]
#[path = "restart_submission/tests.rs"]
mod tests;
