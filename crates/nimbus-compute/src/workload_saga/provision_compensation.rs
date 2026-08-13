//! Durable handoff from one exact failed provision attempt to teardown.
//!
//! Provider adapters classify provision outcomes. This compute-owned concept
//! only turns an already durable definite failure into the portable
//! `FailedProvision` cause and then reuses the canonical teardown runtime.

use std::sync::Arc;

use nimbus_workloads::{WorkloadSagaKey, WorkloadSagaRecord, WorkloadSagaStoreError};
use thiserror::Error;

use super::{
    WorkloadSagaCoordinator, WorkloadTeardownCancellationToken, WorkloadTeardownRun,
    WorkloadTeardownRuntime, WorkloadTeardownSubmissionError,
};

/// Failure before exact failed-provision compensation can return durable truth.
#[derive(Debug, Error)]
pub enum WorkloadProvisionCompensationError {
    #[error("failed-provision compensation saga transition failed: {0}")]
    Saga(#[from] WorkloadSagaStoreError),
    #[error("failed-provision compensation teardown failed: {0}")]
    Teardown(#[from] WorkloadTeardownSubmissionError),
}

/// Concrete compute composition of the durable cause CAS and teardown runtime.
pub(crate) struct WorkloadProvisionCompensator {
    coordinator: Arc<WorkloadSagaCoordinator>,
    teardown_runtime: Arc<WorkloadTeardownRuntime>,
}

impl WorkloadProvisionCompensator {
    pub(crate) fn new(
        coordinator: Arc<WorkloadSagaCoordinator>,
        teardown_runtime: Arc<WorkloadTeardownRuntime>,
    ) -> Self {
        Self {
            coordinator,
            teardown_runtime,
        }
    }

    /// Commit the exact retained failure cause before any teardown effect.
    pub(crate) async fn compensate_definite_provision_failure(
        &self,
        failed: &WorkloadSagaRecord,
    ) -> Result<WorkloadTeardownRun, WorkloadProvisionCompensationError> {
        let withdrawal = self
            .coordinator
            .commit_failed_provision_compensation(failed)
            .await?;
        let cancellation = WorkloadTeardownCancellationToken::default();
        self.teardown_runtime
            .submit(withdrawal.key().clone(), &cancellation)
            .await
            .map_err(Into::into)
    }

    /// Resume only the already committed failed-provision teardown state.
    pub(crate) async fn resume(
        &self,
        key: WorkloadSagaKey,
    ) -> Result<WorkloadTeardownRun, WorkloadProvisionCompensationError> {
        let cancellation = WorkloadTeardownCancellationToken::default();
        self.teardown_runtime
            .submit(key, &cancellation)
            .await
            .map_err(Into::into)
    }
}

/// Exercise the concrete compensation owner against a process-reopened store.
///
/// This test-only entry point exists for the server-owned Engine adapter's
/// crash-cut proof. Production constructs the same concrete owner inside
/// [`crate::workload_provisioner::WorkloadProvisioner`].
#[cfg(any(test, feature = "test-hooks"))]
pub async fn compensate_definite_provision_failure_once_for_test(
    coordinator: Arc<WorkloadSagaCoordinator>,
    teardown_runtime: Arc<WorkloadTeardownRuntime>,
    failed: &WorkloadSagaRecord,
) -> Result<WorkloadTeardownRun, WorkloadProvisionCompensationError> {
    WorkloadProvisionCompensator::new(coordinator, teardown_runtime)
        .compensate_definite_provision_failure(failed)
        .await
}

#[cfg(test)]
#[path = "provision_compensation/tests.rs"]
mod tests;
