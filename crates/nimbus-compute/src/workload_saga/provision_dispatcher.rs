//! Freshness-gated routing of confirmed provision commands.
//!
//! The dispatcher owns no provider effect. It authenticates current source
//! and capability evidence, selects one exact registered capability, and
//! routes execute or inspection authority without fallback.

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use nimbus_network::{
    NetworkCapabilityRegistry, NetworkCapabilityRole, NetworkCapabilitySelectionError,
    NetworkCapabilitySourceDigest, NetworkProviderId,
};
use nimbus_workloads::{
    WorkloadExecutionProviderId, WorkloadProvisionInspectionResult,
    WorkloadProvisionProviderTarget, WorkloadProvisionSourceDigest,
    WorkloadProvisionSourceEvidence, WorkloadProvisionSourceIdentity, WorkloadProvisionStep,
    WorkloadSagaKey, WorkloadSagaRecord, WorkloadSagaStoreError,
};
use thiserror::Error;

use crate::workload_projection::{
    WorkloadExecutionObservationCapability, WorkloadExecutionObservationRequest,
    WorkloadIngressObservationCapability, WorkloadIngressObservationRequest,
    WorkloadObservedIngressEndpoint, WorkloadProviderObservation,
};

use super::{
    ConfirmedWorkloadProvisionCommand, ConfirmedWorkloadProvisionTransition,
    ProposedWorkloadProvisionTransition, WorkloadProvisionCommandMode,
    WorkloadProvisionCommandResult, WorkloadSagaCoordinator,
};

/// One asynchronous source-owner read.
pub type WorkloadProvisionSourceFuture<'a> = Pin<
    Box<
        dyn Future<
                Output = Result<
                    WorkloadProvisionSourceEvidence,
                    WorkloadProvisionSourceAuthorityError,
                >,
            > + Send
            + 'a,
    >,
>;

/// One asynchronous provider capability invocation.
pub type WorkloadProvisionCapabilityFuture<'a> =
    Pin<Box<dyn Future<Output = WorkloadProvisionInspectionResult> + Send + 'a>>;

/// Closed source-owner lookup failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum WorkloadProvisionSourceAuthorityError {
    #[error("the workload provision source does not exist")]
    NotFound,
    #[error("the workload provision source is unavailable")]
    Unavailable,
    #[error("the workload provision source is corrupt")]
    Corrupt,
}

/// Current immutable source evidence for a stable source identity.
pub trait WorkloadProvisionSourceAuthority: Send + Sync {
    fn current_source<'a>(
        &'a self,
        key: &'a WorkloadSagaKey,
        identity: &'a WorkloadProvisionSourceIdentity,
    ) -> WorkloadProvisionSourceFuture<'a>;
}

/// Reserve attachment-owned network resources.
pub trait NetworkReservationCapability: Send + Sync {
    fn execute<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionCapabilityFuture<'a>;

    fn inspect<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionCapabilityFuture<'a>;
}

/// Prepare workload-owned artifacts without activation.
pub trait WorkloadPreparationCapability: Send + Sync {
    fn execute<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionCapabilityFuture<'a>;

    fn inspect<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionCapabilityFuture<'a>;
}

/// Attach a prepared workload without making ingress routable.
pub trait NetworkAttachmentCapability: Send + Sync {
    fn execute<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionCapabilityFuture<'a>;

    fn inspect<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionCapabilityFuture<'a>;
}

/// Inspect the exact prerequisites for activation.
pub trait WorkloadActivationPrerequisiteCapability: Send + Sync {
    fn inspect<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionCapabilityFuture<'a>;
}

/// Activate one exact prepared workload execution.
pub trait WorkloadActivationCapability: Send + Sync {
    fn execute<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionCapabilityFuture<'a>;

    fn inspect<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionCapabilityFuture<'a>;
}

/// Inspect workload readiness without publishing ingress.
pub trait WorkloadReadinessCapability: Send + Sync {
    fn inspect<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionCapabilityFuture<'a>;
}

/// Publish ingress only after exact workload readiness.
pub trait IngressPublicationCapability: Send + Sync {
    fn execute<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionCapabilityFuture<'a>;

    fn inspect<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionCapabilityFuture<'a>;
}

/// Inspect publication after its durable publish transition.
pub trait IngressPublicationInspectionCapability: Send + Sync {
    fn inspect<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionCapabilityFuture<'a>;
}

/// Attachment-role capabilities earned by one concrete provider adapter.
pub struct NetworkAttachmentProvisionCapabilities {
    provider_id: NetworkProviderId,
    reservation: Arc<dyn NetworkReservationCapability>,
    attachment: Arc<dyn NetworkAttachmentCapability>,
}

impl NetworkAttachmentProvisionCapabilities {
    pub fn new<Provider>(provider_id: NetworkProviderId, provider: Arc<Provider>) -> Self
    where
        Provider: NetworkReservationCapability + NetworkAttachmentCapability + 'static,
    {
        let reservation: Arc<dyn NetworkReservationCapability> = provider.clone();
        let attachment: Arc<dyn NetworkAttachmentCapability> = provider;
        Self {
            provider_id,
            reservation,
            attachment,
        }
    }
}

/// Execution-role capabilities earned by one concrete provider adapter.
pub struct WorkloadExecutionProvisionCapabilities {
    provider_id: WorkloadExecutionProviderId,
    preparation: Arc<dyn WorkloadPreparationCapability>,
    activation_prerequisite: Arc<dyn WorkloadActivationPrerequisiteCapability>,
    activation: Arc<dyn WorkloadActivationCapability>,
    readiness: Arc<dyn WorkloadReadinessCapability>,
    projection_observation: Arc<dyn WorkloadExecutionObservationCapability>,
}

impl WorkloadExecutionProvisionCapabilities {
    pub fn new<Provider>(provider_id: WorkloadExecutionProviderId, provider: Arc<Provider>) -> Self
    where
        Provider: WorkloadPreparationCapability
            + WorkloadActivationPrerequisiteCapability
            + WorkloadActivationCapability
            + WorkloadReadinessCapability
            + WorkloadExecutionObservationCapability
            + 'static,
    {
        let preparation: Arc<dyn WorkloadPreparationCapability> = provider.clone();
        let activation_prerequisite: Arc<dyn WorkloadActivationPrerequisiteCapability> =
            provider.clone();
        let activation: Arc<dyn WorkloadActivationCapability> = provider.clone();
        let readiness: Arc<dyn WorkloadReadinessCapability> = provider.clone();
        let projection_observation: Arc<dyn WorkloadExecutionObservationCapability> = provider;
        Self {
            provider_id,
            preparation,
            activation_prerequisite,
            activation,
            readiness,
            projection_observation,
        }
    }
}

/// Ingress-role capabilities earned by one concrete provider adapter.
pub struct IngressProvisionCapabilities {
    provider_id: NetworkProviderId,
    publication: Arc<dyn IngressPublicationCapability>,
    publication_observation: Arc<dyn IngressPublicationInspectionCapability>,
    endpoint_observation: Arc<dyn WorkloadIngressObservationCapability>,
}

impl IngressProvisionCapabilities {
    pub fn new<Provider>(provider_id: NetworkProviderId, provider: Arc<Provider>) -> Self
    where
        Provider: IngressPublicationCapability
            + IngressPublicationInspectionCapability
            + WorkloadIngressObservationCapability
            + 'static,
    {
        let publication: Arc<dyn IngressPublicationCapability> = provider.clone();
        let publication_observation: Arc<dyn IngressPublicationInspectionCapability> =
            provider.clone();
        let endpoint_observation: Arc<dyn WorkloadIngressObservationCapability> = provider;
        Self {
            provider_id,
            publication,
            publication_observation,
            endpoint_observation,
        }
    }
}

/// Exact capability-registry construction failure.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum WorkloadProvisionCapabilityRegistryError {
    #[error("duplicate attachment provision provider `{provider_id}`")]
    DuplicateAttachment { provider_id: NetworkProviderId },
    #[error("duplicate execution provision provider `{provider_id}`")]
    DuplicateExecution {
        provider_id: WorkloadExecutionProviderId,
    },
    #[error("duplicate ingress provision provider `{provider_id}`")]
    DuplicateIngress { provider_id: NetworkProviderId },
    #[error("network provider `{provider_id}` is registered for attachment and ingress effects")]
    NetworkRoleConflict { provider_id: NetworkProviderId },
}

/// Exact read-only projection-capability lookup failure.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum WorkloadProjectionCapabilityError {
    #[error("no execution observation capability is registered for `{provider_id}`")]
    MissingExecutionObservation {
        provider_id: WorkloadExecutionProviderId,
    },
    #[error("no ingress observation capability is registered for `{provider_id}`")]
    MissingIngressObservation { provider_id: NetworkProviderId },
}

/// Immutable exact routing table for small provision capabilities.
#[derive(Clone)]
pub struct WorkloadProvisionCapabilityRegistry {
    reservations: BTreeMap<NetworkProviderId, Arc<dyn NetworkReservationCapability>>,
    attachments: BTreeMap<NetworkProviderId, Arc<dyn NetworkAttachmentCapability>>,
    preparations: BTreeMap<WorkloadExecutionProviderId, Arc<dyn WorkloadPreparationCapability>>,
    activation_prerequisites:
        BTreeMap<WorkloadExecutionProviderId, Arc<dyn WorkloadActivationPrerequisiteCapability>>,
    activations: BTreeMap<WorkloadExecutionProviderId, Arc<dyn WorkloadActivationCapability>>,
    readiness: BTreeMap<WorkloadExecutionProviderId, Arc<dyn WorkloadReadinessCapability>>,
    execution_observations:
        BTreeMap<WorkloadExecutionProviderId, Arc<dyn WorkloadExecutionObservationCapability>>,
    publications: BTreeMap<NetworkProviderId, Arc<dyn IngressPublicationCapability>>,
    publication_observations:
        BTreeMap<NetworkProviderId, Arc<dyn IngressPublicationInspectionCapability>>,
    ingress_observations:
        BTreeMap<NetworkProviderId, Arc<dyn WorkloadIngressObservationCapability>>,
}

impl WorkloadProvisionCapabilityRegistry {
    pub fn new(
        attachments: impl IntoIterator<Item = NetworkAttachmentProvisionCapabilities>,
        executions: impl IntoIterator<Item = WorkloadExecutionProvisionCapabilities>,
        ingresses: impl IntoIterator<Item = IngressProvisionCapabilities>,
    ) -> Result<Self, WorkloadProvisionCapabilityRegistryError> {
        let mut registry = Self {
            reservations: BTreeMap::new(),
            attachments: BTreeMap::new(),
            preparations: BTreeMap::new(),
            activation_prerequisites: BTreeMap::new(),
            activations: BTreeMap::new(),
            readiness: BTreeMap::new(),
            execution_observations: BTreeMap::new(),
            publications: BTreeMap::new(),
            publication_observations: BTreeMap::new(),
            ingress_observations: BTreeMap::new(),
        };
        for registration in attachments {
            if registry
                .reservations
                .insert(registration.provider_id.clone(), registration.reservation)
                .is_some()
            {
                return Err(
                    WorkloadProvisionCapabilityRegistryError::DuplicateAttachment {
                        provider_id: registration.provider_id,
                    },
                );
            }
            registry
                .attachments
                .insert(registration.provider_id, registration.attachment);
        }
        for registration in executions {
            if registry
                .preparations
                .insert(registration.provider_id.clone(), registration.preparation)
                .is_some()
            {
                return Err(
                    WorkloadProvisionCapabilityRegistryError::DuplicateExecution {
                        provider_id: registration.provider_id,
                    },
                );
            }
            registry.activation_prerequisites.insert(
                registration.provider_id.clone(),
                registration.activation_prerequisite,
            );
            registry
                .activations
                .insert(registration.provider_id.clone(), registration.activation);
            registry
                .readiness
                .insert(registration.provider_id.clone(), registration.readiness);
            registry.execution_observations.insert(
                registration.provider_id,
                registration.projection_observation,
            );
        }
        for registration in ingresses {
            if registry
                .reservations
                .contains_key(&registration.provider_id)
            {
                return Err(
                    WorkloadProvisionCapabilityRegistryError::NetworkRoleConflict {
                        provider_id: registration.provider_id,
                    },
                );
            }
            if registry
                .publications
                .insert(registration.provider_id.clone(), registration.publication)
                .is_some()
            {
                return Err(WorkloadProvisionCapabilityRegistryError::DuplicateIngress {
                    provider_id: registration.provider_id,
                });
            }
            registry.publication_observations.insert(
                registration.provider_id.clone(),
                registration.publication_observation,
            );
            registry
                .ingress_observations
                .insert(registration.provider_id, registration.endpoint_observation);
        }
        Ok(registry)
    }

    /// Read one exact execution provider without fallback or effect authority.
    pub async fn observe_execution(
        &self,
        provider_id: &WorkloadExecutionProviderId,
        request: &WorkloadExecutionObservationRequest,
    ) -> Result<
        WorkloadProviderObservation<nimbus_sandbox::SandboxInspection>,
        WorkloadProjectionCapabilityError,
    > {
        let capability = self
            .execution_observations
            .get(provider_id)
            .ok_or_else(
                || WorkloadProjectionCapabilityError::MissingExecutionObservation {
                    provider_id: provider_id.clone(),
                },
            )?;
        Ok(capability.observe(request).await)
    }

    /// Read one exact ingress provider without fallback or effect authority.
    pub async fn observe_ingress(
        &self,
        provider_id: &NetworkProviderId,
        request: &WorkloadIngressObservationRequest,
    ) -> Result<
        WorkloadProviderObservation<Vec<WorkloadObservedIngressEndpoint>>,
        WorkloadProjectionCapabilityError,
    > {
        let capability = self.ingress_observations.get(provider_id).ok_or_else(|| {
            WorkloadProjectionCapabilityError::MissingIngressObservation {
                provider_id: provider_id.clone(),
            }
        })?;
        Ok(capability.observe(request).await)
    }

    fn select_exact_provider(
        &self,
        command: &ConfirmedWorkloadProvisionCommand,
    ) -> Result<ExactProvisionCapability<'_>, WorkloadProvisionDispatchError> {
        match (command.step(), command.provider_target()) {
            (
                WorkloadProvisionStep::ReserveNetwork,
                WorkloadProvisionProviderTarget::Network {
                    role: NetworkCapabilityRole::Attachment,
                    provider_id,
                    ..
                },
            ) => self
                .reservations
                .get(provider_id)
                .map(|capability| ExactProvisionCapability::Reservation(capability.as_ref())),
            (
                WorkloadProvisionStep::PrepareWorkload,
                WorkloadProvisionProviderTarget::Execution { provider_id, .. },
            ) => self
                .preparations
                .get(provider_id)
                .map(|capability| ExactProvisionCapability::Preparation(capability.as_ref())),
            (
                WorkloadProvisionStep::AttachNetwork,
                WorkloadProvisionProviderTarget::Network {
                    role: NetworkCapabilityRole::Attachment,
                    provider_id,
                    ..
                },
            ) => self
                .attachments
                .get(provider_id)
                .map(|capability| ExactProvisionCapability::Attachment(capability.as_ref())),
            (
                WorkloadProvisionStep::InspectActivationPrerequisites,
                WorkloadProvisionProviderTarget::Execution { provider_id, .. },
            ) => self
                .activation_prerequisites
                .get(provider_id)
                .map(|capability| {
                    ExactProvisionCapability::ActivationPrerequisite(capability.as_ref())
                }),
            (
                WorkloadProvisionStep::ActivateWorkload,
                WorkloadProvisionProviderTarget::Execution { provider_id, .. },
            ) => self
                .activations
                .get(provider_id)
                .map(|capability| ExactProvisionCapability::Activation(capability.as_ref())),
            (
                WorkloadProvisionStep::InspectWorkloadReadiness,
                WorkloadProvisionProviderTarget::Execution { provider_id, .. },
            ) => self
                .readiness
                .get(provider_id)
                .map(|capability| ExactProvisionCapability::Readiness(capability.as_ref())),
            (
                WorkloadProvisionStep::Publish,
                WorkloadProvisionProviderTarget::Network {
                    role: NetworkCapabilityRole::Ingress,
                    provider_id,
                    ..
                },
            ) => self
                .publications
                .get(provider_id)
                .map(|capability| ExactProvisionCapability::Publication(capability.as_ref())),
            (
                WorkloadProvisionStep::ObservePublication,
                WorkloadProvisionProviderTarget::Network {
                    role: NetworkCapabilityRole::Ingress,
                    provider_id,
                    ..
                },
            ) => self
                .publication_observations
                .get(provider_id)
                .map(|capability| {
                    ExactProvisionCapability::PublicationObservation(capability.as_ref())
                }),
            _ => {
                return Err(WorkloadProvisionDispatchError::ProviderTargetMismatch {
                    step: command.step(),
                    provider_target: command.provider_target().clone(),
                });
            }
        }
        .ok_or_else(|| WorkloadProvisionDispatchError::MissingCapability {
            step: command.step(),
            provider_target: command.provider_target().clone(),
        })
    }
}

enum ExactProvisionCapability<'a> {
    Reservation(&'a dyn NetworkReservationCapability),
    Preparation(&'a dyn WorkloadPreparationCapability),
    Attachment(&'a dyn NetworkAttachmentCapability),
    ActivationPrerequisite(&'a dyn WorkloadActivationPrerequisiteCapability),
    Activation(&'a dyn WorkloadActivationCapability),
    Readiness(&'a dyn WorkloadReadinessCapability),
    Publication(&'a dyn IngressPublicationCapability),
    PublicationObservation(&'a dyn IngressPublicationInspectionCapability),
}

impl ExactProvisionCapability<'_> {
    async fn invoke(
        self,
        command: &ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionInspectionResult {
        match (self, command.mode()) {
            (Self::Reservation(capability), WorkloadProvisionCommandMode::Execute) => {
                capability.execute(command).await
            }
            (Self::Reservation(capability), WorkloadProvisionCommandMode::Inspect) => {
                capability.inspect(command).await
            }
            (Self::Preparation(capability), WorkloadProvisionCommandMode::Execute) => {
                capability.execute(command).await
            }
            (Self::Preparation(capability), WorkloadProvisionCommandMode::Inspect) => {
                capability.inspect(command).await
            }
            (Self::Attachment(capability), WorkloadProvisionCommandMode::Execute) => {
                capability.execute(command).await
            }
            (Self::Attachment(capability), WorkloadProvisionCommandMode::Inspect) => {
                capability.inspect(command).await
            }
            (Self::ActivationPrerequisite(capability), _) => capability.inspect(command).await,
            (Self::Activation(capability), WorkloadProvisionCommandMode::Execute) => {
                capability.execute(command).await
            }
            (Self::Activation(capability), WorkloadProvisionCommandMode::Inspect) => {
                capability.inspect(command).await
            }
            (Self::Readiness(capability), _) => capability.inspect(command).await,
            (Self::Publication(capability), WorkloadProvisionCommandMode::Execute) => {
                capability.execute(command).await
            }
            (Self::Publication(capability), WorkloadProvisionCommandMode::Inspect) => {
                capability.inspect(command).await
            }
            (Self::PublicationObservation(capability), _) => capability.inspect(command).await,
        }
    }
}

/// Freshness, exact-routing, or confirmation failure before provider effects.
#[derive(Debug, Error)]
pub enum WorkloadProvisionDispatchError {
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
    #[error("provision step {step:?} is crossed with provider target {provider_target:?}")]
    ProviderTargetMismatch {
        step: WorkloadProvisionStep,
        provider_target: WorkloadProvisionProviderTarget,
    },
    #[error(
        "no exact capability is registered for provision step {step:?} and target {provider_target:?}"
    )]
    MissingCapability {
        step: WorkloadProvisionStep,
        provider_target: WorkloadProvisionProviderTarget,
    },
    #[error("workload saga confirmation failed: {0}")]
    Saga(#[from] WorkloadSagaStoreError),
}

/// Compute-owned freshness gate and exact small-capability router.
pub struct WorkloadProvisionDispatcher {
    source_authority: Arc<dyn WorkloadProvisionSourceAuthority>,
    provider_reports: NetworkCapabilityRegistry,
    provision_capabilities: Arc<WorkloadProvisionCapabilityRegistry>,
}

impl WorkloadProvisionDispatcher {
    pub fn new(
        source_authority: Arc<dyn WorkloadProvisionSourceAuthority>,
        provider_reports: NetworkCapabilityRegistry,
        provision_capabilities: Arc<WorkloadProvisionCapabilityRegistry>,
    ) -> Self {
        Self {
            source_authority,
            provider_reports,
            provision_capabilities,
        }
    }

    async fn validate_current_source(
        &self,
        record: &WorkloadSagaRecord,
    ) -> Result<(), WorkloadProvisionDispatchError> {
        let admitted = record.active_intent().source();
        let current = self
            .source_authority
            .current_source(record.key(), admitted.source_identity())
            .await?;
        if current != *admitted {
            return Err(WorkloadProvisionDispatchError::CurrentSourceMismatch {
                admitted: admitted.source_digest(),
                current: current.source_digest(),
            });
        }
        Ok(())
    }

    fn validate_current_provider_report(
        &self,
        record: &WorkloadSagaRecord,
    ) -> Result<(), WorkloadProvisionDispatchError> {
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
                WorkloadProvisionDispatchError::CurrentProviderReportMismatch {
                    admitted: admitted.source_digest(),
                    current: current.source_digest(),
                },
            );
        }
        Ok(())
    }

    /// Authenticate freshness before the exact attempt candidate CAS.
    pub async fn confirm_transition(
        &self,
        coordinator: &WorkloadSagaCoordinator,
        loaded: &WorkloadSagaRecord,
        proposed: &ProposedWorkloadProvisionTransition,
    ) -> Result<ConfirmedWorkloadProvisionTransition, WorkloadProvisionDispatchError> {
        self.validate_current_source(proposed.candidate()).await?;
        self.validate_current_provider_report(proposed.candidate())?;
        coordinator
            .confirm_provision_transition(loaded, proposed)
            .await
            .map_err(Into::into)
    }

    /// Reauthenticate freshness and route one exact confirmed command.
    pub async fn dispatch_confirmed(
        &self,
        confirmed: &ConfirmedWorkloadProvisionTransition,
    ) -> Result<Option<WorkloadProvisionCommandResult>, WorkloadProvisionDispatchError> {
        let Some(command) = confirmed.command() else {
            return Ok(None);
        };
        let record = confirmed.confirmed_record().ok_or({
            WorkloadSagaStoreError::InvalidTransition(
                nimbus_workloads::WorkloadSagaError::InvalidTransition(
                    "provider command requires exact confirmed durable state",
                ),
            )
        })?;
        // Freshness gates new effect authority. Inspection must remain
        // available after an already-authorized effect even when its source or
        // provider report has since changed; otherwise exact result recovery
        // can be stranded forever.
        if command.mode() == WorkloadProvisionCommandMode::Execute {
            self.validate_current_source(record).await?;
            self.validate_current_provider_report(record)?;
        }
        let capability = self.provision_capabilities.select_exact_provider(command)?;
        let outcome = capability.invoke(command).await;
        WorkloadProvisionCommandResult::for_command(command, outcome)
            .map(Some)
            .map_err(Into::into)
    }

    /// Load durable recovery truth and route inspection only.
    pub async fn inspect_recovery(
        &self,
        coordinator: &WorkloadSagaCoordinator,
        key: &WorkloadSagaKey,
    ) -> Result<WorkloadProvisionCommandResult, WorkloadProvisionDispatchError> {
        let confirmed = coordinator.inspect_confirmed_provision(key).await?;
        self.dispatch_confirmed(&confirmed).await?.ok_or_else(|| {
            WorkloadSagaStoreError::InvalidTransition(
                nimbus_workloads::WorkloadSagaError::InvalidTransition(
                    "durable recovery did not produce an inspection command",
                ),
            )
            .into()
        })
    }
}

#[cfg(test)]
#[path = "provision_dispatcher/tests.rs"]
mod tests;
