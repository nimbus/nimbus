//! Freshness-gated routing of exact confirmed restart commands.
//!
//! The dispatcher authenticates current source and provider-report evidence
//! before new effect authority. Recovery inspection remains available after
//! source drift so an already-issued effect cannot be stranded.

use std::sync::Arc;

use nimbus_network::{
    NetworkCapabilityRegistry, NetworkCapabilitySelectionError, NetworkCapabilitySourceDigest,
};
use nimbus_workloads::{WorkloadProvisionSourceDigest, WorkloadSagaRecord, WorkloadSagaStoreError};
use thiserror::Error;

use super::restart_provider::{
    WorkloadRestartCapabilityRegistry, WorkloadRestartCapabilityRegistryError,
};
use super::{
    ConfirmedWorkloadRestartTransition, ProposedWorkloadRestartTransition,
    WorkloadProvisionSourceAuthority, WorkloadProvisionSourceAuthorityError,
    WorkloadRestartCommandMode, WorkloadRestartCommandResult, WorkloadSagaCoordinator,
};

/// Freshness, exact-routing, or confirmation failure before provider effects.
#[derive(Debug, Error)]
pub(super) enum WorkloadRestartDispatchError {
    #[error("current workload source lookup failed: {0}")]
    Source(#[from] WorkloadProvisionSourceAuthorityError),
    #[error("current workload source digest {current} does not match admitted digest {admitted}")]
    CurrentSourceMismatch {
        admitted: WorkloadProvisionSourceDigest,
        current: WorkloadProvisionSourceDigest,
    },
    #[error("current network provider reports do not satisfy the admitted selection: {0}")]
    ProviderSelection(#[from] NetworkCapabilitySelectionError),
    #[error("current network provider digest {current} does not match admitted digest {admitted}")]
    CurrentProviderReportMismatch {
        admitted: NetworkCapabilitySourceDigest,
        current: NetworkCapabilitySourceDigest,
    },
    #[error("restart capability routing failed: {0}")]
    Capability(#[from] WorkloadRestartCapabilityRegistryError),
    #[error("restart provider observation is crossed with its confirmed command")]
    CrossedProviderObservation,
    #[error("workload restart confirmation failed: {0}")]
    Saga(#[from] WorkloadSagaStoreError),
}

/// Compute-owned freshness gate and exact small-capability router.
pub(super) struct WorkloadRestartDispatcher {
    source_authority: Arc<dyn WorkloadProvisionSourceAuthority>,
    provider_reports: NetworkCapabilityRegistry,
    capabilities: Arc<WorkloadRestartCapabilityRegistry>,
}

impl WorkloadRestartDispatcher {
    pub(super) fn new(
        source_authority: Arc<dyn WorkloadProvisionSourceAuthority>,
        provider_reports: NetworkCapabilityRegistry,
        capabilities: Arc<WorkloadRestartCapabilityRegistry>,
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
    ) -> Result<(), WorkloadRestartDispatchError> {
        let admitted = record.active_intent().source();
        let current = self
            .source_authority
            .current_source(record.key(), admitted.source_identity())
            .await?;
        if current != *admitted {
            return Err(WorkloadRestartDispatchError::CurrentSourceMismatch {
                admitted: admitted.source_digest(),
                current: current.source_digest(),
            });
        }
        Ok(())
    }

    fn validate_current_provider_report(
        &self,
        record: &WorkloadSagaRecord,
    ) -> Result<(), WorkloadRestartDispatchError> {
        let content = record.active_intent().network().compiled_plan().content();
        let Some(admitted) = content.capability_selection_evidence() else {
            return Ok(());
        };
        let current = self
            .provider_reports
            .select_exact(admitted.selection(), content.capability_requirements())?
            .selection_evidence();
        if current.source_digest() != admitted.source_digest() {
            return Err(
                WorkloadRestartDispatchError::CurrentProviderReportMismatch {
                    admitted: admitted.source_digest(),
                    current: current.source_digest(),
                },
            );
        }
        Ok(())
    }

    /// Authenticate current source evidence before the exact candidate CAS.
    pub(super) async fn confirm_transition(
        &self,
        coordinator: &WorkloadSagaCoordinator,
        loaded: &WorkloadSagaRecord,
        proposed: &ProposedWorkloadRestartTransition,
    ) -> Result<ConfirmedWorkloadRestartTransition, WorkloadRestartDispatchError> {
        self.validate_current_source(proposed.candidate()).await?;
        self.validate_current_provider_report(proposed.candidate())?;
        coordinator
            .claim_restart_command(loaded, proposed)
            .await
            .map_err(Into::into)
    }

    /// Reauthenticate new effect authority and route one exact capability.
    pub(super) async fn dispatch_confirmed(
        &self,
        confirmed: &ConfirmedWorkloadRestartTransition,
    ) -> Result<Option<WorkloadRestartCommandResult>, WorkloadRestartDispatchError> {
        let Some(command) = confirmed.command() else {
            return Ok(None);
        };
        let record = confirmed.confirmed_record().ok_or({
            WorkloadSagaStoreError::InvalidTransition(
                nimbus_workloads::WorkloadSagaError::InvalidTransition(
                    "restart command requires exact confirmed durable state",
                ),
            )
        })?;
        if command.mode() == WorkloadRestartCommandMode::Execute {
            self.validate_current_source(record).await?;
            self.validate_current_provider_report(record)?;
        }
        let capabilities = self.capabilities.resolve_restart_capabilities(command)?;
        let observation = capabilities.invoke(command).await;
        if !observation.matches_command(command) {
            return Err(WorkloadRestartDispatchError::CrossedProviderObservation);
        }
        Ok(Some(WorkloadRestartCommandResult::for_command(
            command,
            observation.into_outcome(),
        )))
    }
}

#[cfg(test)]
#[path = "restart_dispatcher/tests.rs"]
mod tests;
