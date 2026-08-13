//! Complete server-owned composition for managed workload lifecycle.
//!
//! Protocol serving is a separate explicit profile. Managed serving can be
//! constructed only from this complete bundle, which freezes one Engine,
//! network-manager report snapshot, services owner, provider selection,
//! sovereignty contract, and complete set of narrow provider capabilities.
//! Construction is validation-only: it performs no provider, socket, lease,
//! journal, or desired-state effect.

use std::sync::Arc;

use nimbus_compute::config::control_plane::ControlPlaneConfig;
use nimbus_compute::config::deployment::DeploymentConfig;
use nimbus_compute::config::node_services::NodeServicesConfig;
use nimbus_compute::config::runtime::RuntimeGovernorConfig;
use nimbus_compute::state::{
    ComputeError, ComputeState, ComputeStateConfig, ComputeWorkloadComposition,
};
use nimbus_compute::workload_saga::{
    ExactWorkloadTeardownCapabilityRealm, IngressProvisionCapabilities,
    IngressPublicationCapability, IngressPublicationInspectionCapability,
    NetworkAttachmentCapability, NetworkAttachmentProvisionCapabilities,
    NetworkReservationCapability, NetworkRestartAttachmentCapability, RestartPublicationCapability,
    RestartPublicationObservationCapability, RestartPublicationWithdrawalCapability,
    WorkloadActivationCapability, WorkloadActivationPrerequisiteCapability,
    WorkloadDesireAdmissionGuard, WorkloadExecutionProvisionCapabilities,
    WorkloadExecutionQuiescenceCapability, WorkloadPreparationCapability,
    WorkloadProvisionCapabilityRegistry, WorkloadProvisionCapabilityRegistryError,
    WorkloadReadinessCapability, WorkloadRestartActivationCapability,
    WorkloadRestartActivationPrerequisiteCapability, WorkloadRestartCapabilities,
    WorkloadRestartCapabilityRegistry, WorkloadRestartCapabilityRegistryError,
    WorkloadRestartPreparationCapability, WorkloadRestartReadinessCapability,
    WorkloadTeardownCapabilityRegistry, WorkloadTeardownCapabilityRegistryError,
};
use nimbus_compute::{
    ComputeResourceProvisioner, ComputeResourceRetirer, ServiceManagerWorkloadProjectionSink,
    ServiceManagerWorkloadProvisionSourceAuthority,
};
use nimbus_compute::{
    WorkloadExecutionObservationCapability, WorkloadIngressObservationCapability,
    WorkloadProjectionSink,
};
use nimbus_engine::Engine;
use nimbus_network::{
    LocalNetworkManager, NetworkCapabilitySelection, NetworkCapabilitySelectionError,
    NetworkProviderId, NetworkSovereigntyRequirements,
};
use nimbus_services::ServiceManager;
use nimbus_workloads::{NodeIdentity, WorkloadExecutionProviderId, WorkloadSagaStore};
use thiserror::Error;

use crate::workload_saga_store::EngineWorkloadSagaStore;

/// Three concrete providers that have earned every narrow provision role
/// required by a managed workload composition.
///
/// IDs are explicit and validated against the frozen network selection.
/// Restart roles are registered separately, only after these same providers
/// earn the complete restart capability set.
pub struct ServerWorkloadProviders<Attachment, Execution, Ingress> {
    attachment_provider_id: NetworkProviderId,
    attachment: Arc<Attachment>,
    execution_provider_id: WorkloadExecutionProviderId,
    execution: Arc<Execution>,
    ingress_provider_id: NetworkProviderId,
    ingress: Arc<Ingress>,
    restart_capabilities: Option<WorkloadRestartCapabilities>,
    teardown_capabilities: Option<WorkloadTeardownCapabilityRegistry>,
    desire_admission_guard: Option<Arc<dyn WorkloadDesireAdmissionGuard>>,
}

impl<Attachment, Execution, Ingress> ServerWorkloadProviders<Attachment, Execution, Ingress> {
    pub fn new(
        attachment_provider_id: NetworkProviderId,
        attachment: Arc<Attachment>,
        execution_provider_id: WorkloadExecutionProviderId,
        execution: Arc<Execution>,
        ingress_provider_id: NetworkProviderId,
        ingress: Arc<Ingress>,
    ) -> Self {
        Self {
            attachment_provider_id,
            attachment,
            execution_provider_id,
            execution,
            ingress_provider_id,
            ingress,
            restart_capabilities: None,
            teardown_capabilities: None,
            desire_admission_guard: None,
        }
    }

    /// Register the complete same-realm restart capability set.
    ///
    /// A provider realm that has not implemented restart leaves this unset.
    /// Compute then fails restart dispatch closed with a missing exact
    /// provider selection instead of advertising an unsupported effect path.
    pub fn with_restart_capabilities(mut self) -> Self
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
        self.restart_capabilities = Some(WorkloadRestartCapabilities::new(
            self.execution_provider_id.clone(),
            Some(NetworkCapabilitySelection::new(
                self.attachment_provider_id.clone(),
                self.ingress_provider_id.clone(),
            )),
            Arc::clone(&self.attachment),
            Arc::clone(&self.execution),
            Arc::clone(&self.ingress),
        ));
        self
    }

    /// Register the complete exact teardown capability set for this realm.
    /// Provision and restart support never imply teardown support.
    pub fn with_teardown_capabilities(
        mut self,
        teardown_capabilities: WorkloadTeardownCapabilityRegistry,
    ) -> Self {
        self.teardown_capabilities = Some(teardown_capabilities);
        self
    }

    /// Fence running desire and restart CAS operations with provider-owned
    /// physical-machine stop authority for this exact provider realm.
    pub fn with_desire_admission_guard(
        mut self,
        guard: Arc<dyn WorkloadDesireAdmissionGuard>,
    ) -> Self {
        self.desire_admission_guard = Some(guard);
        self
    }
}

/// Validation failure before managed lifecycle authority exists.
#[derive(Debug, Error)]
pub enum ServerWorkloadCompositionError {
    #[error(
        "attachment capability provider `{registered}` does not match selected provider `{selected}`"
    )]
    AttachmentProviderMismatch {
        selected: NetworkProviderId,
        registered: NetworkProviderId,
    },
    #[error(
        "ingress capability provider `{registered}` does not match selected provider `{selected}`"
    )]
    IngressProviderMismatch {
        selected: NetworkProviderId,
        registered: NetworkProviderId,
    },
    #[error("network manager reports do not contain exact selection {selection}")]
    MissingExactSelection {
        selection: NetworkCapabilitySelection,
    },
    #[error("selected provider reports do not satisfy the fixed sovereignty requirements: {0}")]
    UnsatisfiedSovereignty(#[source] NetworkCapabilitySelectionError),
    #[error("workload capability registry rejected the complete provider set: {0}")]
    CapabilityRegistry(#[from] WorkloadProvisionCapabilityRegistryError),
    #[error("workload restart capability registry rejected the complete provider set: {0}")]
    RestartCapabilityRegistry(#[from] WorkloadRestartCapabilityRegistryError),
    #[error("managed workload composition requires one exact teardown capability realm")]
    MissingExactTeardownCapabilityRealm,
    #[error("workload teardown capability registry rejected the complete provider set: {0}")]
    TeardownCapabilityRegistry(#[from] WorkloadTeardownCapabilityRegistryError),
}

/// Complete server-owned input for one managed workload lifecycle realm.
pub struct ServerWorkloadComposition {
    engine: Arc<Engine>,
    network_manager: Arc<LocalNetworkManager>,
    service_manager: Arc<ServiceManager>,
    local_node: NodeIdentity,
    capability_selection: NetworkCapabilitySelection,
    execution_provider_id: WorkloadExecutionProviderId,
    sovereignty: NetworkSovereigntyRequirements,
    provision_capabilities: WorkloadProvisionCapabilityRegistry,
    restart_capabilities: WorkloadRestartCapabilityRegistry,
    teardown_capabilities: ExactWorkloadTeardownCapabilityRealm,
    desire_admission_guard: Option<Arc<dyn WorkloadDesireAdmissionGuard>>,
}

/// Transport-free lifetime carrier for foreground managed workload owners.
///
/// This runtime exposes only the native resource facades. It owns the same
/// compute composition used by server `AppState`, so
/// standalone callers do not need to construct HTTP state and cannot create a
/// second workload store, coordinator, or provisioner.
pub struct ServerForegroundWorkloadRuntime {
    _compute: ComputeState,
    resource_provisioner: ComputeResourceProvisioner,
}

impl ServerForegroundWorkloadRuntime {
    fn from_managed(composition: ManagedComputeComposition) -> Self {
        let node_services = NodeServicesConfig::default()
            .with_service_manager(Arc::clone(&composition.service_manager));
        let compute = ComputeState::from_config(ComputeStateConfig {
            engine: composition.engine,
            workload_composition: composition.workload,
            deployment: DeploymentConfig::default(),
            control_plane: ControlPlaneConfig::router_options_default(),
            node_services,
            runtime: RuntimeGovernorConfig::default(),
        });
        let resource_provisioner = compute.resource_provisioner().expect(
            "foreground runtime is built only from a complete managed workload composition",
        );
        Self {
            _compute: compute,
            resource_provisioner,
        }
    }

    /// The canonical native resource facade for this managed lifecycle realm.
    pub const fn resource_provisioner(&self) -> &ComputeResourceProvisioner {
        &self.resource_provisioner
    }

    /// Resolve the canonical native retirement facade from this compute realm.
    ///
    /// A foreground profile without exact teardown composition fails closed
    /// without exposing the store, coordinator, registry, or runtime.
    pub fn resource_retirer(&self) -> Result<ComputeResourceRetirer, ComputeError> {
        self._compute.resource_retirer()
    }

    /// Read-only services source and observed-projection owner for this realm.
    pub fn service_manager(&self) -> Arc<ServiceManager> {
        self._compute
            .service_manager()
            .expect("foreground runtime always retains its complete services owner")
    }
}

struct ManagedComputeComposition {
    engine: Arc<Engine>,
    service_manager: Arc<ServiceManager>,
    workload: ComputeWorkloadComposition,
}

impl ServerWorkloadComposition {
    pub fn new<Attachment, Execution, Ingress>(
        engine: Arc<Engine>,
        network_manager: Arc<LocalNetworkManager>,
        service_manager: Arc<ServiceManager>,
        local_node: NodeIdentity,
        capability_selection: NetworkCapabilitySelection,
        sovereignty: NetworkSovereigntyRequirements,
        providers: ServerWorkloadProviders<Attachment, Execution, Ingress>,
    ) -> Result<Self, ServerWorkloadCompositionError>
    where
        Attachment: NetworkReservationCapability + NetworkAttachmentCapability + 'static,
        Execution: WorkloadPreparationCapability
            + WorkloadActivationPrerequisiteCapability
            + WorkloadActivationCapability
            + WorkloadReadinessCapability
            + WorkloadExecutionObservationCapability
            + 'static,
        Ingress: IngressPublicationCapability
            + IngressPublicationInspectionCapability
            + WorkloadIngressObservationCapability
            + 'static,
    {
        if capability_selection.attachment_provider_id() != &providers.attachment_provider_id {
            return Err(ServerWorkloadCompositionError::AttachmentProviderMismatch {
                selected: capability_selection.attachment_provider_id().clone(),
                registered: providers.attachment_provider_id,
            });
        }
        if capability_selection.ingress_provider_id() != &providers.ingress_provider_id {
            return Err(ServerWorkloadCompositionError::IngressProviderMismatch {
                selected: capability_selection.ingress_provider_id().clone(),
                registered: providers.ingress_provider_id,
            });
        }
        let provider_reports = network_manager.capability_registry();
        if let Err(error) =
            provider_reports.select_exact_sovereignty(&capability_selection, &sovereignty)
        {
            return match error {
                NetworkCapabilitySelectionError::UnregisteredComposition { .. } => {
                    Err(ServerWorkloadCompositionError::MissingExactSelection {
                        selection: capability_selection,
                    })
                }
                error => Err(ServerWorkloadCompositionError::UnsatisfiedSovereignty(
                    error,
                )),
            };
        }
        let execution_provider_id = providers.execution_provider_id.clone();
        let restart_capabilities =
            WorkloadRestartCapabilityRegistry::new(providers.restart_capabilities)?;
        let teardown_capabilities = ExactWorkloadTeardownCapabilityRealm::new(
            providers
                .teardown_capabilities
                .ok_or(ServerWorkloadCompositionError::MissingExactTeardownCapabilityRealm)?,
            &capability_selection,
            &providers.execution_provider_id,
        )?;
        let provision_capabilities = WorkloadProvisionCapabilityRegistry::new(
            [NetworkAttachmentProvisionCapabilities::new(
                providers.attachment_provider_id,
                providers.attachment,
            )],
            [WorkloadExecutionProvisionCapabilities::new(
                providers.execution_provider_id,
                providers.execution,
            )],
            [IngressProvisionCapabilities::new(
                providers.ingress_provider_id,
                providers.ingress,
            )],
        )?;
        Ok(Self {
            engine,
            network_manager,
            service_manager,
            local_node,
            capability_selection,
            execution_provider_id,
            sovereignty,
            provision_capabilities,
            restart_capabilities,
            teardown_capabilities,
            desire_admission_guard: providers.desire_admission_guard,
        })
    }

    pub(crate) fn engine(&self) -> Arc<Engine> {
        Arc::clone(&self.engine)
    }

    pub(crate) fn network_manager(&self) -> Arc<LocalNetworkManager> {
        Arc::clone(&self.network_manager)
    }

    pub(crate) fn service_manager(&self) -> Arc<ServiceManager> {
        Arc::clone(&self.service_manager)
    }

    /// Consume this validated composition into a transport-free foreground
    /// runtime without constructing HTTP application state.
    pub fn into_foreground_runtime(
        self,
        saga_store: Arc<dyn WorkloadSagaStore>,
    ) -> ServerForegroundWorkloadRuntime {
        ServerForegroundWorkloadRuntime::from_managed(self.into_managed_compute(saga_store))
    }

    fn into_managed_compute(
        self,
        saga_store: Arc<dyn WorkloadSagaStore>,
    ) -> ManagedComputeComposition {
        let source_authority = Arc::new(ServiceManagerWorkloadProvisionSourceAuthority::new(
            Arc::clone(&self.service_manager),
        ));
        let projection_sink: Arc<dyn WorkloadProjectionSink> = Arc::new(
            ServiceManagerWorkloadProjectionSink::new(Arc::clone(&self.service_manager)),
        );
        ManagedComputeComposition {
            engine: self.engine,
            service_manager: self.service_manager,
            workload: ComputeWorkloadComposition::Managed {
                network_manager: self.network_manager,
                local_node: self.local_node,
                capability_selection: Box::new(self.capability_selection),
                execution_provider_id: self.execution_provider_id,
                sovereignty: self.sovereignty,
                saga_store,
                source_authority,
                provision_capabilities: Box::new(self.provision_capabilities),
                restart_capabilities: Box::new(self.restart_capabilities),
                teardown_capabilities: Some(Box::new(self.teardown_capabilities)),
                desire_admission_guard: self.desire_admission_guard,
                projection_sink,
            },
        }
    }
}

/// Explicit server profile carried intact from public options into AppState.
pub(crate) enum ServerWorkloadProfile {
    ProtocolOnly { engine: Arc<Engine> },
    Managed(Box<ServerWorkloadComposition>),
}

impl ServerWorkloadProfile {
    pub(crate) fn protocol_only(engine: Arc<Engine>) -> Self {
        Self::ProtocolOnly { engine }
    }

    pub(crate) fn managed(composition: ServerWorkloadComposition) -> Self {
        Self::Managed(Box::new(composition))
    }

    pub(crate) fn engine(&self) -> Arc<Engine> {
        match self {
            Self::ProtocolOnly { engine } => Arc::clone(engine),
            Self::Managed(composition) => composition.engine(),
        }
    }

    pub(crate) const fn is_managed(&self) -> bool {
        matches!(self, Self::Managed(_))
    }

    pub(crate) fn authenticate_node_services(&self, node_services: &NodeServicesConfig) {
        if let Self::Managed(composition) = self {
            let configured = node_services.service_manager().expect(
                "managed server workload composition requires its exact ServiceManager in node \
                 services",
            );
            assert!(
                Arc::ptr_eq(&configured, &composition.service_manager),
                "managed server workload composition is crossed with a different \
                 ServiceManager in node services"
            );
        }
    }

    pub(crate) fn into_compute(self) -> (Arc<Engine>, ComputeWorkloadComposition) {
        match self {
            Self::ProtocolOnly { engine } => (engine, ComputeWorkloadComposition::ProtocolOnly),
            Self::Managed(composition) => {
                let saga_store: Arc<dyn WorkloadSagaStore> = Arc::new(
                    EngineWorkloadSagaStore::new(Arc::clone(&composition.engine)),
                );
                let managed = (*composition).into_managed_compute(saga_store);
                (managed.engine, managed.workload)
            }
        }
    }
}

#[cfg(test)]
#[path = "workload_composition/tests.rs"]
mod tests;
