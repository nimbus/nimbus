//! Exact small-capability routing for confirmed restart commands.
//!
//! The registry binds one durable execution-provider selection to separate
//! lifecycle concepts. It owns no provider effect and has no fallback path.

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use nimbus_network::NetworkCapabilitySelection;
use nimbus_workloads::{
    WorkloadDesiredDigest, WorkloadExecutionAttemptId, WorkloadExecutionProviderId,
    WorkloadGeneration, WorkloadRestartCommandId, WorkloadRestartDispatchEpoch,
    WorkloadRestartEpoch, WorkloadRestartRequestId, WorkloadRestartStep, WorkloadSagaTransitionId,
};
use thiserror::Error;

use super::{
    ConfirmedWorkloadRestartCommand, WorkloadRestartCommandMode, WorkloadRestartCommandOutcome,
};

/// One asynchronous restart capability invocation.
pub type WorkloadRestartCapabilityFuture<'a> =
    Pin<Box<dyn Future<Output = WorkloadRestartProviderObservation> + Send + 'a>>;

/// Raw provider observation with every fence needed to reject stale callbacks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadRestartProviderObservation {
    command_id: WorkloadRestartCommandId,
    transition_id: WorkloadSagaTransitionId,
    generation: WorkloadGeneration,
    desired_digest: WorkloadDesiredDigest,
    request_id: WorkloadRestartRequestId,
    source_attempt_id: WorkloadExecutionAttemptId,
    attempt_id: WorkloadExecutionAttemptId,
    restart_epoch: WorkloadRestartEpoch,
    dispatch_epoch: WorkloadRestartDispatchEpoch,
    provider_selection: WorkloadExecutionProviderId,
    outcome: WorkloadRestartCommandOutcome,
}

/// Complete callback correlation supplied by a concrete provider adapter.
pub(super) struct WorkloadRestartProviderObservationInput {
    pub(super) command_id: WorkloadRestartCommandId,
    pub(super) transition_id: WorkloadSagaTransitionId,
    pub(super) generation: WorkloadGeneration,
    pub(super) desired_digest: WorkloadDesiredDigest,
    pub(super) request_id: WorkloadRestartRequestId,
    pub(super) source_attempt_id: WorkloadExecutionAttemptId,
    pub(super) attempt_id: WorkloadExecutionAttemptId,
    pub(super) restart_epoch: WorkloadRestartEpoch,
    pub(super) dispatch_epoch: WorkloadRestartDispatchEpoch,
    pub(super) provider_selection: WorkloadExecutionProviderId,
    pub(super) outcome: WorkloadRestartCommandOutcome,
}

impl WorkloadRestartProviderObservation {
    pub(super) fn new(input: WorkloadRestartProviderObservationInput) -> Self {
        Self {
            command_id: input.command_id,
            transition_id: input.transition_id,
            generation: input.generation,
            desired_digest: input.desired_digest,
            request_id: input.request_id,
            source_attempt_id: input.source_attempt_id,
            attempt_id: input.attempt_id,
            restart_epoch: input.restart_epoch,
            dispatch_epoch: input.dispatch_epoch,
            provider_selection: input.provider_selection,
            outcome: input.outcome,
        }
    }

    pub(super) fn matches_command(&self, command: &ConfirmedWorkloadRestartCommand) -> bool {
        self.command_id == *command.command_id()
            && self.transition_id == *command.transition_id()
            && self.generation == command.generation()
            && self.desired_digest == command.desired_digest()
            && self.request_id == *command.request_id()
            && self.source_attempt_id == *command.source_attempt_id()
            && self.attempt_id == *command.attempt_id()
            && self.restart_epoch == command.restart_epoch()
            && self.dispatch_epoch == command.dispatch_epoch()
            && self.provider_selection == *command.provider_selection()
    }

    pub(super) fn into_outcome(self) -> WorkloadRestartCommandOutcome {
        self.outcome
    }
}

/// Withdraw publication before execution quiescence.
pub trait RestartPublicationWithdrawalCapability: Send + Sync {
    fn execute(
        &self,
        command: &ConfirmedWorkloadRestartCommand,
    ) -> WorkloadRestartCapabilityFuture<'_>;

    fn inspect(
        &self,
        command: &ConfirmedWorkloadRestartCommand,
    ) -> WorkloadRestartCapabilityFuture<'_>;
}

/// Quiesce one exact execution attempt without releasing retained networking.
pub trait WorkloadExecutionQuiescenceCapability: Send + Sync {
    fn execute(
        &self,
        command: &ConfirmedWorkloadRestartCommand,
    ) -> WorkloadRestartCapabilityFuture<'_>;

    fn inspect(
        &self,
        command: &ConfirmedWorkloadRestartCommand,
    ) -> WorkloadRestartCapabilityFuture<'_>;
}

/// Prepare the next exact execution attempt under retained authority.
pub trait WorkloadRestartPreparationCapability: Send + Sync {
    fn execute(
        &self,
        command: &ConfirmedWorkloadRestartCommand,
    ) -> WorkloadRestartCapabilityFuture<'_>;

    fn inspect(
        &self,
        command: &ConfirmedWorkloadRestartCommand,
    ) -> WorkloadRestartCapabilityFuture<'_>;
}

/// Reattach same-generation networking and its PEP to the new attempt.
pub trait NetworkRestartAttachmentCapability: Send + Sync {
    fn execute(
        &self,
        command: &ConfirmedWorkloadRestartCommand,
    ) -> WorkloadRestartCapabilityFuture<'_>;

    fn inspect(
        &self,
        command: &ConfirmedWorkloadRestartCommand,
    ) -> WorkloadRestartCapabilityFuture<'_>;
}

/// Inspect attachment, PEP, and preparation prerequisites before activation.
pub trait WorkloadRestartActivationPrerequisiteCapability: Send + Sync {
    fn inspect(
        &self,
        command: &ConfirmedWorkloadRestartCommand,
    ) -> WorkloadRestartCapabilityFuture<'_>;
}

/// Activate one exact restarted execution attempt.
pub trait WorkloadRestartActivationCapability: Send + Sync {
    fn execute(
        &self,
        command: &ConfirmedWorkloadRestartCommand,
    ) -> WorkloadRestartCapabilityFuture<'_>;

    fn inspect(
        &self,
        command: &ConfirmedWorkloadRestartCommand,
    ) -> WorkloadRestartCapabilityFuture<'_>;
}

/// Inspect readiness for the exact new execution attempt.
pub trait WorkloadRestartReadinessCapability: Send + Sync {
    fn inspect(
        &self,
        command: &ConfirmedWorkloadRestartCommand,
    ) -> WorkloadRestartCapabilityFuture<'_>;
}

/// Publish ingress for the exact ready execution attempt.
pub trait RestartPublicationCapability: Send + Sync {
    fn execute(
        &self,
        command: &ConfirmedWorkloadRestartCommand,
    ) -> WorkloadRestartCapabilityFuture<'_>;

    fn inspect(
        &self,
        command: &ConfirmedWorkloadRestartCommand,
    ) -> WorkloadRestartCapabilityFuture<'_>;
}

/// Observe the exact new-attempt publication without granting effect authority.
pub trait RestartPublicationObservationCapability: Send + Sync {
    fn inspect(
        &self,
        command: &ConfirmedWorkloadRestartCommand,
    ) -> WorkloadRestartCapabilityFuture<'_>;
}

/// Exact restart-role composition for one admitted provider realm.
pub struct WorkloadRestartCapabilities {
    execution_provider_id: WorkloadExecutionProviderId,
    network_selection: Option<NetworkCapabilitySelection>,
    publication_withdrawal: Arc<dyn RestartPublicationWithdrawalCapability>,
    execution_quiescence: Arc<dyn WorkloadExecutionQuiescenceCapability>,
    preparation: Arc<dyn WorkloadRestartPreparationCapability>,
    attachment: Arc<dyn NetworkRestartAttachmentCapability>,
    activation_prerequisite: Arc<dyn WorkloadRestartActivationPrerequisiteCapability>,
    activation: Arc<dyn WorkloadRestartActivationCapability>,
    readiness: Arc<dyn WorkloadRestartReadinessCapability>,
    publication: Arc<dyn RestartPublicationCapability>,
    publication_observation: Arc<dyn RestartPublicationObservationCapability>,
}

impl WorkloadRestartCapabilities {
    pub fn new<Attachment, Execution, Ingress>(
        execution_provider_id: WorkloadExecutionProviderId,
        network_selection: Option<NetworkCapabilitySelection>,
        attachment: Arc<Attachment>,
        execution: Arc<Execution>,
        ingress: Arc<Ingress>,
    ) -> Self
    where
        Attachment: NetworkRestartAttachmentCapability + 'static,
        Execution: WorkloadExecutionQuiescenceCapability
            + WorkloadRestartPreparationCapability
            + WorkloadRestartActivationPrerequisiteCapability
            + WorkloadRestartActivationCapability
            + WorkloadRestartReadinessCapability
            + 'static,
        Ingress: RestartPublicationWithdrawalCapability
            + RestartPublicationCapability
            + RestartPublicationObservationCapability
            + 'static,
    {
        let publication_withdrawal: Arc<dyn RestartPublicationWithdrawalCapability> =
            ingress.clone();
        let execution_quiescence: Arc<dyn WorkloadExecutionQuiescenceCapability> =
            execution.clone();
        let preparation: Arc<dyn WorkloadRestartPreparationCapability> = execution.clone();
        let attachment: Arc<dyn NetworkRestartAttachmentCapability> = attachment;
        let activation_prerequisite: Arc<dyn WorkloadRestartActivationPrerequisiteCapability> =
            execution.clone();
        let activation: Arc<dyn WorkloadRestartActivationCapability> = execution.clone();
        let readiness: Arc<dyn WorkloadRestartReadinessCapability> = execution;
        let publication: Arc<dyn RestartPublicationCapability> = ingress.clone();
        let publication_observation: Arc<dyn RestartPublicationObservationCapability> = ingress;
        Self {
            execution_provider_id,
            network_selection,
            publication_withdrawal,
            execution_quiescence,
            preparation,
            attachment,
            activation_prerequisite,
            activation,
            readiness,
            publication,
            publication_observation,
        }
    }

    fn matches_command(&self, command: &ConfirmedWorkloadRestartCommand) -> bool {
        let selected_network = command
            .compiled_network_plan()
            .content()
            .capability_selection_evidence()
            .map(|evidence| evidence.selection());
        self.execution_provider_id == *command.provider_selection()
            && self.network_selection.as_ref() == selected_network
    }

    pub(super) async fn invoke(
        &self,
        command: &ConfirmedWorkloadRestartCommand,
    ) -> WorkloadRestartProviderObservation {
        match (command.step(), command.mode()) {
            (WorkloadRestartStep::WithdrawPublication, WorkloadRestartCommandMode::Execute) => {
                self.publication_withdrawal.execute(command).await
            }
            (WorkloadRestartStep::WithdrawPublication, WorkloadRestartCommandMode::Inspect) => {
                self.publication_withdrawal.inspect(command).await
            }
            (WorkloadRestartStep::QuiesceExecution, WorkloadRestartCommandMode::Execute) => {
                self.execution_quiescence.execute(command).await
            }
            (WorkloadRestartStep::QuiesceExecution, WorkloadRestartCommandMode::Inspect) => {
                self.execution_quiescence.inspect(command).await
            }
            (WorkloadRestartStep::PrepareExecution, WorkloadRestartCommandMode::Execute) => {
                self.preparation.execute(command).await
            }
            (WorkloadRestartStep::PrepareExecution, WorkloadRestartCommandMode::Inspect) => {
                self.preparation.inspect(command).await
            }
            (WorkloadRestartStep::AttachNetwork, WorkloadRestartCommandMode::Execute) => {
                self.attachment.execute(command).await
            }
            (WorkloadRestartStep::AttachNetwork, WorkloadRestartCommandMode::Inspect) => {
                self.attachment.inspect(command).await
            }
            (WorkloadRestartStep::InspectActivationPrerequisites, _) => {
                self.activation_prerequisite.inspect(command).await
            }
            (WorkloadRestartStep::ActivateExecution, WorkloadRestartCommandMode::Execute) => {
                self.activation.execute(command).await
            }
            (WorkloadRestartStep::ActivateExecution, WorkloadRestartCommandMode::Inspect) => {
                self.activation.inspect(command).await
            }
            (WorkloadRestartStep::InspectReadiness, _) => self.readiness.inspect(command).await,
            (WorkloadRestartStep::Publish, WorkloadRestartCommandMode::Execute) => {
                self.publication.execute(command).await
            }
            (WorkloadRestartStep::Publish, WorkloadRestartCommandMode::Inspect) => {
                self.publication.inspect(command).await
            }
            (WorkloadRestartStep::ObservePublication, _) => {
                self.publication_observation.inspect(command).await
            }
        }
    }
}

/// Exact registry construction or resolution failure.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum WorkloadRestartCapabilityRegistryError {
    #[error(
        "duplicate restart provider realm execution={execution_provider_id}, network={network_selection:?}"
    )]
    DuplicateProviderSelection {
        execution_provider_id: WorkloadExecutionProviderId,
        network_selection: Option<NetworkCapabilitySelection>,
    },
    #[error(
        "no restart capabilities are registered for exact realm execution={execution_provider_id}, network={network_selection:?}"
    )]
    MissingProviderSelection {
        execution_provider_id: WorkloadExecutionProviderId,
        network_selection: Option<NetworkCapabilitySelection>,
    },
    #[error("restart provider realm is crossed with the confirmed command")]
    CrossedProviderRealm,
}

/// Immutable exact routing table for restart lifecycle capabilities.
pub struct WorkloadRestartCapabilityRegistry {
    providers: BTreeMap<
        (
            WorkloadExecutionProviderId,
            Option<NetworkCapabilitySelection>,
        ),
        WorkloadRestartCapabilities,
    >,
}

impl WorkloadRestartCapabilityRegistry {
    pub fn new(
        registrations: impl IntoIterator<Item = WorkloadRestartCapabilities>,
    ) -> Result<Self, WorkloadRestartCapabilityRegistryError> {
        let mut registry = Self {
            providers: BTreeMap::new(),
        };
        for capabilities in registrations {
            registry.register_restart_capabilities(capabilities)?;
        }
        Ok(registry)
    }

    fn register_restart_capabilities(
        &mut self,
        capabilities: WorkloadRestartCapabilities,
    ) -> Result<(), WorkloadRestartCapabilityRegistryError> {
        let realm = (
            capabilities.execution_provider_id.clone(),
            capabilities.network_selection.clone(),
        );
        if self.providers.insert(realm.clone(), capabilities).is_some() {
            return Err(
                WorkloadRestartCapabilityRegistryError::DuplicateProviderSelection {
                    execution_provider_id: realm.0,
                    network_selection: realm.1,
                },
            );
        }
        Ok(())
    }

    pub(super) fn resolve_restart_capabilities(
        &self,
        command: &ConfirmedWorkloadRestartCommand,
    ) -> Result<&WorkloadRestartCapabilities, WorkloadRestartCapabilityRegistryError> {
        let network_selection = command
            .compiled_network_plan()
            .content()
            .capability_selection_evidence()
            .map(|evidence| evidence.selection().clone());
        let realm = (command.provider_selection().clone(), network_selection);
        let capabilities = self.providers.get(&realm).ok_or_else(|| {
            WorkloadRestartCapabilityRegistryError::MissingProviderSelection {
                execution_provider_id: realm.0.clone(),
                network_selection: realm.1.clone(),
            }
        })?;
        if !capabilities.matches_command(command) {
            return Err(WorkloadRestartCapabilityRegistryError::CrossedProviderRealm);
        }
        Ok(capabilities)
    }
}

#[cfg(test)]
#[path = "restart_provider/tests.rs"]
mod tests;
