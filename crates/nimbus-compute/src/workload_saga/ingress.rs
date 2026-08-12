//! Durable submission of complete workload intent without effect dispatch.

use nimbus_workloads::{
    DesiredWorkloadState, WorkloadSagaCommit, WorkloadSagaError, WorkloadSagaIntent,
    WorkloadSagaIntentUpdate, WorkloadSagaKey, WorkloadSagaRecord, WorkloadSagaStoreError,
};
use thiserror::Error;

use super::{
    WorkloadDesireAdmissionError, WorkloadDesireAdmissionRequest, WorkloadSagaCoordinator,
    WorkloadSagaDecision,
};

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum WorkloadSagaIngressError {
    #[error("workload saga persistence failed: {0}")]
    Saga(#[from] WorkloadSagaStoreError),
    #[error("workload desire admission failed: {0}")]
    Admission(#[from] WorkloadDesireAdmissionError),
}

impl From<WorkloadSagaError> for WorkloadSagaIngressError {
    fn from(error: WorkloadSagaError) -> Self {
        Self::Saga(WorkloadSagaStoreError::InvalidTransition(error))
    }
}

impl PartialEq<WorkloadSagaStoreError> for WorkloadSagaIngressError {
    fn eq(&self, other: &WorkloadSagaStoreError) -> bool {
        matches!(self, Self::Saga(error) if error == other)
    }
}

/// How the ingress confirmed the returned durable record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkloadSagaIngressDisposition {
    /// A compare-and-swap attempt confirmed the exact transition.
    ///
    /// Another writer may have installed the same record. This value does not
    /// grant exactly-once command authority.
    Applied,
    /// The exact submitted record was already current when confirmation ended.
    ///
    /// The ingress can discover this either during its initial load, without
    /// a CAS, or from a store that reports an attempted CAS as unchanged.
    ConfirmedReplay,
}

/// Exact durable record and pure next decision confirmed by one submission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmedWorkloadSagaIntent {
    record: WorkloadSagaRecord,
    decision: WorkloadSagaDecision,
    disposition: WorkloadSagaIngressDisposition,
}

impl ConfirmedWorkloadSagaIntent {
    fn new(
        record: WorkloadSagaRecord,
        disposition: WorkloadSagaIngressDisposition,
    ) -> Result<Self, WorkloadSagaStoreError> {
        let decision = WorkloadSagaDecision::for_record(&record)?;
        Ok(Self {
            record,
            decision,
            disposition,
        })
    }

    pub fn record(&self) -> &WorkloadSagaRecord {
        &self.record
    }

    pub fn decision(&self) -> &WorkloadSagaDecision {
        &self.decision
    }

    pub fn disposition(&self) -> WorkloadSagaIngressDisposition {
        self.disposition
    }

    pub fn into_parts(
        self,
    ) -> (
        WorkloadSagaRecord,
        WorkloadSagaDecision,
        WorkloadSagaIngressDisposition,
    ) {
        (self.record, self.decision, self.disposition)
    }
}

impl WorkloadSagaCoordinator {
    /// Confirms complete desired intent before exposing its pure next decision.
    pub async fn submit_intent(
        &self,
        key: WorkloadSagaKey,
        intent: WorkloadSagaIntent,
    ) -> Result<ConfirmedWorkloadSagaIntent, WorkloadSagaIngressError> {
        let loaded = self.load(&key).await?;
        if loaded.as_ref().is_some_and(|record| record.key() != &key) {
            return Err(WorkloadSagaStoreError::Corrupt.into());
        }

        let admission = (intent.desired_state() == DesiredWorkloadState::Running).then(|| {
            WorkloadDesireAdmissionRequest::new(
                key.clone(),
                intent.source().execution_provider_id().clone(),
                intent.generation(),
                intent.desired_digest(),
                intent.source().source_digest(),
            )
        });
        let next = match loaded.as_ref() {
            None => WorkloadSagaRecord::new(key, intent)?,
            Some(current) => match current.apply_intent(intent)? {
                WorkloadSagaIntentUpdate::Unchanged => {
                    return ConfirmedWorkloadSagaIntent::new(
                        current.clone(),
                        WorkloadSagaIngressDisposition::ConfirmedReplay,
                    )
                    .map_err(Into::into);
                }
                WorkloadSagaIntentUpdate::Transition(next) => *next,
            },
        };

        let _permit = match (&self.desire_admission_guard, admission.as_ref()) {
            (Some(guard), Some(admission)) => Some(guard.acquire(admission).await?),
            _ => None,
        };
        let disposition = match self.commit_loaded(loaded.as_ref(), next.clone()).await? {
            WorkloadSagaCommit::Applied => WorkloadSagaIngressDisposition::Applied,
            WorkloadSagaCommit::Unchanged => WorkloadSagaIngressDisposition::ConfirmedReplay,
        };
        ConfirmedWorkloadSagaIntent::new(next, disposition).map_err(Into::into)
    }
}

#[cfg(test)]
#[path = "ingress/tests.rs"]
mod tests;
