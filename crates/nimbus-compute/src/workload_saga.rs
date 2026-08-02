//! Compute-owned coordinator for portable workload-saga transitions.

use std::sync::Arc;

use nimbus_workloads::{
    WorkloadSagaCommit, WorkloadSagaExpected, WorkloadSagaKey, WorkloadSagaPage,
    WorkloadSagaPageRequest, WorkloadSagaRecord, WorkloadSagaStore, WorkloadSagaStoreError,
};

mod ingress;
mod recovery;

pub use ingress::{ConfirmedWorkloadSagaIntent, WorkloadSagaIngressDisposition};
pub use recovery::{WorkloadSagaAction, WorkloadSagaDecision, WorkloadSagaDecisionPage};

/// Sole cross-domain writer of portable workload-saga transitions.
pub struct WorkloadSagaCoordinator {
    store: Arc<dyn WorkloadSagaStore>,
}

impl WorkloadSagaCoordinator {
    pub fn new(store: Arc<dyn WorkloadSagaStore>) -> Self {
        Self { store }
    }

    pub async fn load(
        &self,
        key: &WorkloadSagaKey,
    ) -> Result<Option<WorkloadSagaRecord>, WorkloadSagaStoreError> {
        self.store.load(key).await
    }

    async fn commit_loaded(
        &self,
        loaded: Option<&WorkloadSagaRecord>,
        next: WorkloadSagaRecord,
    ) -> Result<WorkloadSagaCommit, WorkloadSagaStoreError> {
        let expected = match loaded {
            Some(current) => {
                current.validate_successor(&next)?;
                WorkloadSagaExpected::Revision(current.revision())
            }
            None => {
                next.validate()?;
                if next.revision().as_u64() != 0 || next.last_transition().source_phase().is_some()
                {
                    return Err(WorkloadSagaStoreError::InvalidTransition(
                        nimbus_workloads::WorkloadSagaError::InvalidTransition(
                            "missing-store creation requires the initial revision",
                        ),
                    ));
                }
                WorkloadSagaExpected::Missing
            }
        };
        match self.store.compare_and_swap(expected, next.clone()).await {
            Err(WorkloadSagaStoreError::Ambiguous) => {
                self.resolve_ambiguous_commit(loaded, expected, &next).await
            }
            result => result,
        }
    }

    pub async fn list_recoverable(
        &self,
        request: WorkloadSagaPageRequest,
    ) -> Result<WorkloadSagaPage, WorkloadSagaStoreError> {
        self.store.list_recoverable(request).await
    }

    async fn resolve_ambiguous_commit(
        &self,
        loaded: Option<&WorkloadSagaRecord>,
        expected: WorkloadSagaExpected,
        next: &WorkloadSagaRecord,
    ) -> Result<WorkloadSagaCommit, WorkloadSagaStoreError> {
        let observed = self.store.load(next.key()).await?;
        if observed
            .as_ref()
            .is_some_and(|record| record.key() != next.key())
        {
            return Err(WorkloadSagaStoreError::Corrupt);
        }
        if observed.as_ref() == Some(next) {
            return Ok(WorkloadSagaCommit::Applied);
        }
        if observed.is_none() || observed.as_ref() == loaded {
            return Err(WorkloadSagaStoreError::Ambiguous);
        }
        Err(WorkloadSagaStoreError::Conflict {
            expected,
            observed: observed.as_ref().map(WorkloadSagaRecord::revision),
        })
    }
}

#[cfg(test)]
#[path = "workload_saga/tests.rs"]
mod tests;
