//! Freshness-gated invocation of exact workload teardown capabilities.

use std::sync::Arc;

use nimbus_network::{
    NetworkCapabilityRegistry, NetworkCapabilitySelectionError, NetworkCapabilitySelectionEvidence,
};
use nimbus_workloads::{
    DesiredWorkloadState, WorkloadProvisionSourceEvidence, WorkloadSagaRecord,
    WorkloadSagaStoreError, WorkloadTeardownCommandMode,
};
use thiserror::Error;

use super::teardown_command::{ConfirmedWorkloadTeardownTransition, WorkloadTeardownCommandResult};
use super::teardown_registry::{
    WorkloadTeardownCapabilityRegistry, WorkloadTeardownCapabilityRegistryError,
};
use super::{WorkloadProvisionSourceAuthority, WorkloadProvisionSourceAuthorityError};

/// Freshness, exact-routing, or callback-correlation failure.
#[derive(Debug, Error)]
pub enum WorkloadTeardownDispatchError {
    #[error("current workload source lookup failed: {0}")]
    Source(#[from] WorkloadProvisionSourceAuthorityError),
    #[error("current workload source does not match the admitted teardown source")]
    CurrentSourceMismatch {
        admitted: Box<WorkloadProvisionSourceEvidence>,
        current: Box<WorkloadProvisionSourceEvidence>,
    },
    #[error("process-frozen network provider reports do not satisfy the admitted selection: {0}")]
    ProviderSelection(#[from] NetworkCapabilitySelectionError),
    #[error(
        "process-frozen network provider evidence does not match the admitted teardown evidence"
    )]
    CurrentProviderReportMismatch {
        admitted: Box<NetworkCapabilitySelectionEvidence>,
        current: Box<NetworkCapabilitySelectionEvidence>,
    },
    #[error("teardown capability routing failed: {0}")]
    Capability(#[from] WorkloadTeardownCapabilityRegistryError),
    #[error("teardown provider observation is crossed with its confirmed command")]
    CrossedProviderObservation,
    #[error("workload teardown confirmation failed: {0}")]
    Saga(#[from] WorkloadSagaStoreError),
}

/// Compute-owned freshness gate and exact five-capability router.
pub struct WorkloadTeardownDispatcher {
    source_authority: Arc<dyn WorkloadProvisionSourceAuthority>,
    provider_reports: NetworkCapabilityRegistry,
    capabilities: Arc<WorkloadTeardownCapabilityRegistry>,
}

impl WorkloadTeardownDispatcher {
    pub fn new(
        source_authority: Arc<dyn WorkloadProvisionSourceAuthority>,
        provider_reports: NetworkCapabilityRegistry,
        capabilities: Arc<WorkloadTeardownCapabilityRegistry>,
    ) -> Self {
        Self {
            source_authority,
            provider_reports,
            capabilities,
        }
    }

    async fn validate_current_source(
        &self,
        record: &WorkloadSagaRecord,
    ) -> Result<(), WorkloadTeardownDispatchError> {
        let admitted = record.active_intent().source();
        let current = match self
            .source_authority
            .current_source(record.key(), admitted.source_identity())
            .await
        {
            Ok(current) => current,
            Err(WorkloadProvisionSourceAuthorityError::NotFound)
                if durable_stopped_successor_authenticates_missing_source(record) =>
            {
                return Ok(());
            }
            Err(error) => return Err(error.into()),
        };
        if current != *admitted {
            return Err(WorkloadTeardownDispatchError::CurrentSourceMismatch {
                admitted: Box::new(admitted.clone()),
                current: Box::new(current),
            });
        }
        Ok(())
    }

    fn validate_process_provider_reports(
        &self,
        record: &WorkloadSagaRecord,
    ) -> Result<(), WorkloadTeardownDispatchError> {
        let content = record.active_intent().network().compiled_plan().content();
        let Some(admitted) = content.capability_selection_evidence() else {
            return Ok(());
        };
        let current = self
            .provider_reports
            .select_exact(admitted.selection(), content.capability_requirements())?
            .selection_evidence();
        if current != *admitted {
            return Err(
                WorkloadTeardownDispatchError::CurrentProviderReportMismatch {
                    admitted: Box::new(admitted.clone()),
                    current: Box::new(current),
                },
            );
        }
        Ok(())
    }

    /// Reauthenticate only new effect authority, then invoke one exact role.
    /// Inspection remains available after source or report drift so recovery
    /// cannot strand an already-issued provider effect.
    pub(super) async fn dispatch_confirmed(
        &self,
        confirmed: &ConfirmedWorkloadTeardownTransition,
    ) -> Result<Option<WorkloadTeardownCommandResult>, WorkloadTeardownDispatchError> {
        let Some(command) = confirmed.command() else {
            return Ok(None);
        };
        let record =
            confirmed
                .confirmed_record()
                .ok_or(WorkloadSagaStoreError::InvalidTransition(
                    nimbus_workloads::WorkloadSagaError::InvalidTransition(
                        "teardown command requires exact confirmed durable state",
                    ),
                ))?;
        if command.mode() == WorkloadTeardownCommandMode::Execute {
            self.validate_current_source(record).await?;
            self.validate_process_provider_reports(record)?;
        }
        let capability = self.capabilities.select_exact(command)?;
        let observation = capability.invoke(command).await;
        if !observation.matches_command(command) {
            return Err(WorkloadTeardownDispatchError::CrossedProviderObservation);
        }
        Ok(Some(WorkloadTeardownCommandResult::for_command(
            record,
            command,
            observation.into_outcome(),
        )?))
    }
}

fn durable_stopped_successor_authenticates_missing_source(record: &WorkloadSagaRecord) -> bool {
    record
        .successor_intent()
        .is_some_and(|successor| successor.desired_state() == DesiredWorkloadState::Stopped)
}

#[cfg(test)]
#[path = "teardown_dispatch/tests.rs"]
mod tests;
