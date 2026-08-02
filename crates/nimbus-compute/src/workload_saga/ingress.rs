//! Durable submission of complete workload intent without effect dispatch.

use nimbus_workloads::{
    WorkloadSagaCommit, WorkloadSagaIntent, WorkloadSagaIntentUpdate, WorkloadSagaKey,
    WorkloadSagaRecord, WorkloadSagaStoreError,
};

use super::{WorkloadSagaCoordinator, WorkloadSagaDecision};

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
    ) -> Result<ConfirmedWorkloadSagaIntent, WorkloadSagaStoreError> {
        let loaded = self.load(&key).await?;
        if loaded.as_ref().is_some_and(|record| record.key() != &key) {
            return Err(WorkloadSagaStoreError::Corrupt);
        }

        let next = match loaded.as_ref() {
            None => WorkloadSagaRecord::new(key, intent)?,
            Some(current) => match current.apply_intent(intent)? {
                WorkloadSagaIntentUpdate::Unchanged => {
                    return ConfirmedWorkloadSagaIntent::new(
                        current.clone(),
                        WorkloadSagaIngressDisposition::ConfirmedReplay,
                    );
                }
                WorkloadSagaIntentUpdate::Transition(next) => *next,
            },
        };

        let disposition = match self.commit_loaded(loaded.as_ref(), next.clone()).await? {
            WorkloadSagaCommit::Applied => WorkloadSagaIngressDisposition::Applied,
            WorkloadSagaCommit::Unchanged => WorkloadSagaIngressDisposition::ConfirmedReplay,
        };
        ConfirmedWorkloadSagaIntent::new(next, disposition)
    }
}

#[cfg(test)]
#[path = "ingress/tests.rs"]
mod tests;
