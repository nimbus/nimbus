//! Immutable exact registry for small workload teardown capabilities.

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use nimbus_network::{NetworkCapabilitySelectionEvidence, NetworkPlanDigest, NetworkProviderId};
use nimbus_workloads::{
    NodeIdentity, WorkloadDesiredDigest, WorkloadExecutionProviderId, WorkloadExecutionReference,
    WorkloadGeneration, WorkloadProvisionSourceDigest, WorkloadProvisionSourceEvidence,
    WorkloadSagaId, WorkloadSagaKey, WorkloadSagaRevision, WorkloadSagaTransitionId,
    WorkloadTeardownAttemptId, WorkloadTeardownCommandId, WorkloadTeardownCommandMode,
    WorkloadTeardownDispatchEpoch, WorkloadTeardownProviderTarget, WorkloadTeardownStep,
    WorkloadTeardownSubjects,
};
use thiserror::Error;

use super::teardown_command::{ConfirmedWorkloadTeardownCommand, WorkloadTeardownProviderOutcome};

/// One asynchronous teardown-capability invocation.
///
/// Every adapter must synchronize `inspect` with the exact provider-owned
/// effect journal used by `execute`. `NotCompleted` is valid only when no
/// matching effect is complete or in flight and no older matching operation
/// can later commit. An operation that can still finish must report progress
/// or ambiguity. Effects that can outlive Nimbus require durable provider
/// evidence; process memory alone is not sufficient.
pub type WorkloadTeardownCapabilityFuture<'a> =
    Pin<Box<dyn Future<Output = WorkloadTeardownProviderObservation> + Send + 'a>>;

/// Raw provider observation with every fence required to reject stale or
/// crossed callbacks before durable result persistence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadTeardownProviderObservation {
    command_id: WorkloadTeardownCommandId,
    key: WorkloadSagaKey,
    saga_id: WorkloadSagaId,
    confirmed_revision: WorkloadSagaRevision,
    confirmed_transition_id: WorkloadSagaTransitionId,
    generation: WorkloadGeneration,
    desired_digest: WorkloadDesiredDigest,
    required_node: NodeIdentity,
    source: WorkloadProvisionSourceEvidence,
    source_digest: WorkloadProvisionSourceDigest,
    network_plan_digest: NetworkPlanDigest,
    selection_evidence: Option<NetworkCapabilitySelectionEvidence>,
    execution_locator: WorkloadExecutionReference,
    attempt_id: WorkloadTeardownAttemptId,
    dispatch_epoch: WorkloadTeardownDispatchEpoch,
    provider_target: WorkloadTeardownProviderTarget,
    subjects: WorkloadTeardownSubjects,
    step: WorkloadTeardownStep,
    mode: WorkloadTeardownCommandMode,
    outcome: WorkloadTeardownProviderOutcome,
}

impl WorkloadTeardownProviderObservation {
    /// Build a callback fence from the exact command supplied to an adapter.
    pub fn for_command(
        command: &ConfirmedWorkloadTeardownCommand,
        outcome: WorkloadTeardownProviderOutcome,
    ) -> Self {
        Self {
            command_id: command.command_id(),
            key: command.key().clone(),
            saga_id: command.saga_id().clone(),
            confirmed_revision: command.confirmed_revision(),
            confirmed_transition_id: command.confirmed_transition_id().clone(),
            generation: command.generation(),
            desired_digest: command.desired_digest(),
            required_node: command.required_node().clone(),
            source: command.source().clone(),
            source_digest: command.source_digest(),
            network_plan_digest: command.network_plan_digest(),
            selection_evidence: command.selection_evidence().cloned(),
            execution_locator: command.execution_locator().clone(),
            attempt_id: command.attempt_id().clone(),
            dispatch_epoch: command.dispatch_epoch(),
            provider_target: command.provider_target().clone(),
            subjects: command.subjects().clone(),
            step: command.step(),
            mode: command.mode(),
            outcome,
        }
    }

    pub(super) fn matches_command(&self, command: &ConfirmedWorkloadTeardownCommand) -> bool {
        self.command_id == command.command_id()
            && self.key == *command.key()
            && self.saga_id == *command.saga_id()
            && self.confirmed_revision == command.confirmed_revision()
            && self.confirmed_transition_id == *command.confirmed_transition_id()
            && self.generation == command.generation()
            && self.desired_digest == command.desired_digest()
            && self.required_node == *command.required_node()
            && self.source == *command.source()
            && self.source_digest == command.source_digest()
            && self.network_plan_digest == command.network_plan_digest()
            && self.selection_evidence.as_ref() == command.selection_evidence()
            && self.execution_locator == *command.execution_locator()
            && self.attempt_id == *command.attempt_id()
            && self.dispatch_epoch == command.dispatch_epoch()
            && self.provider_target == *command.provider_target()
            && self.subjects == *command.subjects()
            && self.step == command.step()
            && self.mode == command.mode()
    }

    pub(super) fn into_outcome(self) -> WorkloadTeardownProviderOutcome {
        self.outcome
    }

    #[cfg(test)]
    pub(crate) fn cross_confirmed_revision_for_test(&mut self) {
        self.confirmed_revision = self
            .confirmed_revision
            .checked_next()
            .expect("fixture confirmed revision has room to advance");
    }

    #[cfg(test)]
    pub(crate) fn cross_execution_locator_for_test(&mut self, locator: WorkloadExecutionReference) {
        self.execution_locator = locator;
    }
}

/// Withdraw final ingress for one exact published workload endpoint set.
pub trait FinalIngressWithdrawalCapability: Send + Sync {
    fn execute<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadTeardownCommand,
    ) -> WorkloadTeardownCapabilityFuture<'a>;

    fn inspect<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadTeardownCommand,
    ) -> WorkloadTeardownCapabilityFuture<'a>;
}

/// Drain one exact workload execution without stopping it.
pub trait WorkloadExecutionDrainCapability: Send + Sync {
    fn execute<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadTeardownCommand,
    ) -> WorkloadTeardownCapabilityFuture<'a>;

    fn inspect<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadTeardownCommand,
    ) -> WorkloadTeardownCapabilityFuture<'a>;
}

/// Stop one exact drained workload execution.
pub trait WorkloadExecutionStopCapability: Send + Sync {
    fn execute<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadTeardownCommand,
    ) -> WorkloadTeardownCapabilityFuture<'a>;

    fn inspect<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadTeardownCommand,
    ) -> WorkloadTeardownCapabilityFuture<'a>;
}

/// Detach one exact network attachment while its lease remains fenced.
pub trait NetworkDetachmentCapability: Send + Sync {
    fn execute<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadTeardownCommand,
    ) -> WorkloadTeardownCapabilityFuture<'a>;

    fn inspect<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadTeardownCommand,
    ) -> WorkloadTeardownCapabilityFuture<'a>;
}

/// Release one exact detached network attachment and its durable resources.
pub trait NetworkReleaseCapability: Send + Sync {
    fn execute<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadTeardownCommand,
    ) -> WorkloadTeardownCapabilityFuture<'a>;

    fn inspect<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadTeardownCommand,
    ) -> WorkloadTeardownCapabilityFuture<'a>;
}

/// Exact ingress-role capability registration.
pub struct IngressTeardownCapabilities {
    provider_id: NetworkProviderId,
    withdrawal: Arc<dyn FinalIngressWithdrawalCapability>,
}

impl IngressTeardownCapabilities {
    pub fn new(
        provider_id: NetworkProviderId,
        withdrawal: Arc<dyn FinalIngressWithdrawalCapability>,
    ) -> Self {
        Self {
            provider_id,
            withdrawal,
        }
    }
}

/// Exact execution-role capability registration.
pub struct WorkloadExecutionTeardownCapabilities {
    provider_id: WorkloadExecutionProviderId,
    drain: Arc<dyn WorkloadExecutionDrainCapability>,
    stop: Arc<dyn WorkloadExecutionStopCapability>,
}

impl WorkloadExecutionTeardownCapabilities {
    pub fn new(
        provider_id: WorkloadExecutionProviderId,
        drain: Arc<dyn WorkloadExecutionDrainCapability>,
        stop: Arc<dyn WorkloadExecutionStopCapability>,
    ) -> Self {
        Self {
            provider_id,
            drain,
            stop,
        }
    }
}

/// Exact attachment-role capability registration.
pub struct NetworkAttachmentTeardownCapabilities {
    provider_id: NetworkProviderId,
    detach: Arc<dyn NetworkDetachmentCapability>,
    release: Arc<dyn NetworkReleaseCapability>,
}

impl NetworkAttachmentTeardownCapabilities {
    pub fn new(
        provider_id: NetworkProviderId,
        detach: Arc<dyn NetworkDetachmentCapability>,
        release: Arc<dyn NetworkReleaseCapability>,
    ) -> Self {
        Self {
            provider_id,
            detach,
            release,
        }
    }
}

/// Exact construction or selection failure. No variant permits fallback.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum WorkloadTeardownCapabilityRegistryError {
    #[error("duplicate ingress teardown provider {provider_id}")]
    DuplicateIngressProvider { provider_id: NetworkProviderId },
    #[error("duplicate execution teardown provider {provider_id}")]
    DuplicateExecutionProvider {
        provider_id: WorkloadExecutionProviderId,
    },
    #[error("duplicate attachment teardown provider {provider_id}")]
    DuplicateAttachmentProvider { provider_id: NetworkProviderId },
    #[error("network provider {provider_id} is registered for both ingress and attachment")]
    NetworkRoleConflict { provider_id: NetworkProviderId },
    #[error("teardown step {step:?} is crossed with provider target {provider_target:?}")]
    ProviderTargetMismatch {
        step: WorkloadTeardownStep,
        provider_target: WorkloadTeardownProviderTarget,
    },
    #[error("no exact capability for teardown step {step:?} and target {provider_target:?}")]
    MissingExactCapability {
        step: WorkloadTeardownStep,
        provider_target: WorkloadTeardownProviderTarget,
    },
}

/// Immutable exact registry for the five teardown lifecycle concepts.
#[derive(Clone)]
pub struct WorkloadTeardownCapabilityRegistry {
    ingress_withdrawal: BTreeMap<NetworkProviderId, Arc<dyn FinalIngressWithdrawalCapability>>,
    execution_drain:
        BTreeMap<WorkloadExecutionProviderId, Arc<dyn WorkloadExecutionDrainCapability>>,
    execution_stop: BTreeMap<WorkloadExecutionProviderId, Arc<dyn WorkloadExecutionStopCapability>>,
    network_detach: BTreeMap<NetworkProviderId, Arc<dyn NetworkDetachmentCapability>>,
    network_release: BTreeMap<NetworkProviderId, Arc<dyn NetworkReleaseCapability>>,
}

impl WorkloadTeardownCapabilityRegistry {
    pub fn new(
        attachments: impl IntoIterator<Item = NetworkAttachmentTeardownCapabilities>,
        executions: impl IntoIterator<Item = WorkloadExecutionTeardownCapabilities>,
        ingresses: impl IntoIterator<Item = IngressTeardownCapabilities>,
    ) -> Result<Self, WorkloadTeardownCapabilityRegistryError> {
        let mut registry = Self {
            ingress_withdrawal: BTreeMap::new(),
            execution_drain: BTreeMap::new(),
            execution_stop: BTreeMap::new(),
            network_detach: BTreeMap::new(),
            network_release: BTreeMap::new(),
        };
        for registration in attachments {
            let provider_id = registration.provider_id;
            if registry.network_detach.contains_key(&provider_id) {
                return Err(
                    WorkloadTeardownCapabilityRegistryError::DuplicateAttachmentProvider {
                        provider_id,
                    },
                );
            }
            registry
                .network_detach
                .insert(provider_id.clone(), registration.detach);
            registry
                .network_release
                .insert(provider_id, registration.release);
        }
        for registration in executions {
            let provider_id = registration.provider_id;
            if registry.execution_drain.contains_key(&provider_id) {
                return Err(
                    WorkloadTeardownCapabilityRegistryError::DuplicateExecutionProvider {
                        provider_id,
                    },
                );
            }
            registry
                .execution_drain
                .insert(provider_id.clone(), registration.drain);
            registry
                .execution_stop
                .insert(provider_id, registration.stop);
        }
        for registration in ingresses {
            let provider_id = registration.provider_id;
            if registry.ingress_withdrawal.contains_key(&provider_id) {
                return Err(
                    WorkloadTeardownCapabilityRegistryError::DuplicateIngressProvider {
                        provider_id,
                    },
                );
            }
            if registry.network_detach.contains_key(&provider_id) {
                return Err(
                    WorkloadTeardownCapabilityRegistryError::NetworkRoleConflict { provider_id },
                );
            }
            registry
                .ingress_withdrawal
                .insert(provider_id, registration.withdrawal);
        }
        Ok(registry)
    }

    pub(super) fn select_exact(
        &self,
        command: &ConfirmedWorkloadTeardownCommand,
    ) -> Result<ExactWorkloadTeardownCapability, WorkloadTeardownCapabilityRegistryError> {
        self.select_for(command.step(), command.provider_target())
    }

    fn select_for(
        &self,
        step: WorkloadTeardownStep,
        target: &WorkloadTeardownProviderTarget,
    ) -> Result<ExactWorkloadTeardownCapability, WorkloadTeardownCapabilityRegistryError> {
        let selected = match (step, target) {
            (
                WorkloadTeardownStep::WithdrawPublication,
                WorkloadTeardownProviderTarget::Ingress { provider_id, .. },
            ) => self
                .ingress_withdrawal
                .get(provider_id)
                .cloned()
                .map(ExactWorkloadTeardownCapability::IngressWithdrawal),
            (
                WorkloadTeardownStep::DrainExecution,
                WorkloadTeardownProviderTarget::Execution { provider_id, .. },
            ) => self
                .execution_drain
                .get(provider_id)
                .cloned()
                .map(ExactWorkloadTeardownCapability::ExecutionDrain),
            (
                WorkloadTeardownStep::StopExecution,
                WorkloadTeardownProviderTarget::Execution { provider_id, .. },
            ) => self
                .execution_stop
                .get(provider_id)
                .cloned()
                .map(ExactWorkloadTeardownCapability::ExecutionStop),
            (
                WorkloadTeardownStep::DetachNetwork,
                WorkloadTeardownProviderTarget::Attachment { provider_id, .. },
            ) => self
                .network_detach
                .get(provider_id)
                .cloned()
                .map(ExactWorkloadTeardownCapability::NetworkDetach),
            (
                WorkloadTeardownStep::ReleaseNetwork,
                WorkloadTeardownProviderTarget::Attachment { provider_id, .. },
            ) => self
                .network_release
                .get(provider_id)
                .cloned()
                .map(ExactWorkloadTeardownCapability::NetworkRelease),
            _ => {
                return Err(
                    WorkloadTeardownCapabilityRegistryError::ProviderTargetMismatch {
                        step,
                        provider_target: target.clone(),
                    },
                );
            }
        };
        selected.ok_or_else(
            || WorkloadTeardownCapabilityRegistryError::MissingExactCapability {
                step,
                provider_target: target.clone(),
            },
        )
    }
}

pub(super) enum ExactWorkloadTeardownCapability {
    IngressWithdrawal(Arc<dyn FinalIngressWithdrawalCapability>),
    ExecutionDrain(Arc<dyn WorkloadExecutionDrainCapability>),
    ExecutionStop(Arc<dyn WorkloadExecutionStopCapability>),
    NetworkDetach(Arc<dyn NetworkDetachmentCapability>),
    NetworkRelease(Arc<dyn NetworkReleaseCapability>),
}

impl ExactWorkloadTeardownCapability {
    pub(super) async fn invoke(
        self,
        command: &ConfirmedWorkloadTeardownCommand,
    ) -> WorkloadTeardownProviderObservation {
        match (self, command.mode()) {
            (Self::IngressWithdrawal(capability), WorkloadTeardownCommandMode::Execute) => {
                capability.execute(command).await
            }
            (Self::IngressWithdrawal(capability), WorkloadTeardownCommandMode::Inspect) => {
                capability.inspect(command).await
            }
            (Self::ExecutionDrain(capability), WorkloadTeardownCommandMode::Execute) => {
                capability.execute(command).await
            }
            (Self::ExecutionDrain(capability), WorkloadTeardownCommandMode::Inspect) => {
                capability.inspect(command).await
            }
            (Self::ExecutionStop(capability), WorkloadTeardownCommandMode::Execute) => {
                capability.execute(command).await
            }
            (Self::ExecutionStop(capability), WorkloadTeardownCommandMode::Inspect) => {
                capability.inspect(command).await
            }
            (Self::NetworkDetach(capability), WorkloadTeardownCommandMode::Execute) => {
                capability.execute(command).await
            }
            (Self::NetworkDetach(capability), WorkloadTeardownCommandMode::Inspect) => {
                capability.inspect(command).await
            }
            (Self::NetworkRelease(capability), WorkloadTeardownCommandMode::Execute) => {
                capability.execute(command).await
            }
            (Self::NetworkRelease(capability), WorkloadTeardownCommandMode::Inspect) => {
                capability.inspect(command).await
            }
        }
    }
}

#[cfg(test)]
#[path = "teardown_registry/tests.rs"]
mod tests;
