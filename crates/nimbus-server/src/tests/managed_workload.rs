use std::net::Ipv4Addr;
use std::num::NonZeroU16;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use nimbus_compute::workload_saga::provision_provider::{
    ProviderProvisionEffectObservation, ProviderProvisionPhaseAdapter,
};
use nimbus_compute::workload_saga::{
    ConfirmedWorkloadProvisionCommand, IngressPublicationCapability,
    IngressPublicationInspectionCapability, NetworkAttachmentCapability,
    NetworkReservationCapability, WorkloadActivationCapability,
    WorkloadActivationPrerequisiteCapability, WorkloadPreparationCapability,
    WorkloadProvisionCapabilityFuture, WorkloadReadinessCapability, sandbox_execution_provider_id,
};
use nimbus_compute::{
    WorkloadExecutionObservationCapability, WorkloadExecutionObservationFuture,
    WorkloadExecutionObservationRequest, WorkloadIngressBindingWitness,
    WorkloadIngressObservationCapability, WorkloadIngressObservationFuture,
    WorkloadIngressObservationRequest, WorkloadObservedIngressEndpoint,
    WorkloadProviderObservation,
};
use nimbus_engine::Engine;
use nimbus_network::{
    LocalNetworkManager, NetworkAddressFamily, NetworkAttachmentProviderRegistration,
    NetworkCapabilityBundle, NetworkCapabilityRegistry, NetworkCapabilitySelection,
    NetworkControlPlaneLocality, NetworkLifecycleCapabilitySet, NetworkLifecycleFeature,
    NetworkSovereigntyCapabilities, NetworkSovereigntyRequirements, PortBindRealm, PortBindTarget,
    PortBindingProvenance, PortBoundEndpoint, PortLeaseLifetime, PortProtocol,
};
use nimbus_sandbox::{
    ProviderProvisionAttemptJournal, SandboxBackend, SandboxBackendKind, SandboxError,
    SandboxFuture, SandboxHandle, SandboxId, SandboxInspection, SandboxSpec,
    sandbox_network_plan_requirements,
};
use nimbus_services::{EmptyServiceDefinitionCatalog, ServiceManager};
use nimbus_workloads::{NodeIdentity, WorkloadNetworkPortRequestMode};

use crate::network_capabilities::nimbus_owned_workload_ingress_registration;
use crate::router::{RouterBuildConfig, RouterOptions};
use crate::workload_composition::{ServerWorkloadComposition, ServerWorkloadProviders};

static TEST_REALM_ID: AtomicU64 = AtomicU64::new(1);

/// Test-only activation substitute for the deleted coarse sandbox start API.
///
/// Implementations receive the exact execution identity selected by compute;
/// they cannot choose a second workload identity or bypass the saga.
pub(super) trait TestSandboxActivation: SandboxBackend {
    fn activate_for_test(
        &self,
        spec: SandboxSpec,
        execution_id: SandboxId,
    ) -> Result<SandboxHandle, SandboxError>;

    /// Return the exact activation retained by this test provider.
    ///
    /// The managed provision adapter, execution observation, and legacy
    /// retirement adapter all consult this one backend-owned fake state so a
    /// test cannot report presence through one seam and absence through
    /// another.
    fn activated_handle_for_test(&self, execution_id: &SandboxId) -> Option<SandboxHandle>;
}

struct EffectForbiddenSandboxBackend;

impl SandboxBackend for EffectForbiddenSandboxBackend {
    fn kind(&self) -> SandboxBackendKind {
        SandboxBackendKind::Krun
    }

    fn inspect(&self, _id: &SandboxId) -> SandboxFuture<Option<SandboxInspection>> {
        panic!("transport-only managed fixture must not inspect a sandbox")
    }

    fn stop(&self, _id: &SandboxId) -> SandboxFuture<()> {
        panic!("transport-only managed fixture must not stop a sandbox")
    }
}

impl TestSandboxActivation for EffectForbiddenSandboxBackend {
    fn activate_for_test(
        &self,
        _spec: SandboxSpec,
        _execution_id: SandboxId,
    ) -> Result<SandboxHandle, SandboxError> {
        panic!("transport-only managed fixture must not activate a sandbox")
    }

    fn activated_handle_for_test(&self, _execution_id: &SandboxId) -> Option<SandboxHandle> {
        panic!("transport-only managed fixture must not observe sandbox activation")
    }
}

struct ManagedTestWorkloadProvider<Backend> {
    backend: Arc<Backend>,
    phases: ProviderProvisionPhaseAdapter,
}

impl<Backend> ManagedTestWorkloadProvider<Backend>
where
    Backend: TestSandboxActivation,
{
    fn new(backend: Arc<Backend>, phases: ProviderProvisionPhaseAdapter) -> Self {
        Self { backend, phases }
    }

    fn execute_success(
        &self,
        command: &ConfirmedWorkloadProvisionCommand,
    ) -> nimbus_workloads::WorkloadProvisionInspectionResult {
        self.phases
            .execute(command, || ProviderProvisionEffectObservation::Succeeded {
                evidence: format!("managed-test:{:?}", command.step()).into_bytes(),
            })
    }

    fn inspect_success(
        &self,
        command: &ConfirmedWorkloadProvisionCommand,
    ) -> nimbus_workloads::WorkloadProvisionInspectionResult {
        self.phases
            .inspect(command, || ProviderProvisionEffectObservation::Succeeded {
                evidence: format!("managed-test:{:?}", command.step()).into_bytes(),
            })
    }

    fn activate(
        &self,
        command: &ConfirmedWorkloadProvisionCommand,
    ) -> ProviderProvisionEffectObservation {
        let spec =
            match nimbus_compute::workload_executable::decode_sandbox_spec(command.executable()) {
                Ok(spec) => spec,
                Err(error) => {
                    return ProviderProvisionEffectObservation::DefiniteFailure {
                        code: "managed_test_executable_invalid".to_owned(),
                        evidence: error.to_string().into_bytes(),
                    };
                }
            };
        let sandbox_id = SandboxId::new(command.execution().execution_id().as_str());
        match self.backend.activate_for_test(spec, sandbox_id.clone()) {
            Ok(handle)
                if handle.id == sandbox_id
                    && self.backend.activated_handle_for_test(&sandbox_id).as_ref()
                        == Some(&handle) =>
            {
                ProviderProvisionEffectObservation::Succeeded {
                    evidence: b"managed-test:activated".to_vec(),
                }
            }
            Ok(handle) => ProviderProvisionEffectObservation::DefiniteFailure {
                code: "managed_test_activation_identity_or_retention_mismatch".to_owned(),
                evidence: handle.id.as_str().as_bytes().to_vec(),
            },
            Err(error) => ProviderProvisionEffectObservation::DefiniteFailure {
                code: "managed_test_activation_failed".to_owned(),
                evidence: error.to_string().into_bytes(),
            },
        }
    }

    fn inspect_activation(
        &self,
        command: &ConfirmedWorkloadProvisionCommand,
    ) -> ProviderProvisionEffectObservation {
        let sandbox_id = SandboxId::new(command.execution().execution_id().as_str());
        if self
            .backend
            .activated_handle_for_test(&sandbox_id)
            .is_some()
        {
            ProviderProvisionEffectObservation::Succeeded {
                evidence: b"managed-test:activation-present".to_vec(),
            }
        } else {
            ProviderProvisionEffectObservation::Absent {
                evidence: b"managed-test:activation-absent".to_vec(),
            }
        }
    }
}

macro_rules! effect_capability {
    ($trait_name:ident) => {
        impl<Backend> $trait_name for ManagedTestWorkloadProvider<Backend>
        where
            Backend: TestSandboxActivation,
        {
            fn execute<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadProvisionCommand,
            ) -> WorkloadProvisionCapabilityFuture<'a> {
                Box::pin(async move { self.execute_success(command) })
            }

            fn inspect<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadProvisionCommand,
            ) -> WorkloadProvisionCapabilityFuture<'a> {
                Box::pin(async move { self.inspect_success(command) })
            }
        }
    };
}

effect_capability!(NetworkReservationCapability);
effect_capability!(WorkloadPreparationCapability);
effect_capability!(NetworkAttachmentCapability);

impl<Backend> WorkloadActivationPrerequisiteCapability for ManagedTestWorkloadProvider<Backend>
where
    Backend: TestSandboxActivation,
{
    fn inspect<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionCapabilityFuture<'a> {
        Box::pin(async move { self.inspect_success(command) })
    }
}

impl<Backend> WorkloadActivationCapability for ManagedTestWorkloadProvider<Backend>
where
    Backend: TestSandboxActivation,
{
    fn execute<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionCapabilityFuture<'a> {
        Box::pin(async move { self.phases.execute(command, || self.activate(command)) })
    }

    fn inspect<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionCapabilityFuture<'a> {
        Box::pin(async move {
            self.phases
                .inspect(command, || self.inspect_activation(command))
        })
    }
}

impl<Backend> WorkloadReadinessCapability for ManagedTestWorkloadProvider<Backend>
where
    Backend: TestSandboxActivation,
{
    fn inspect<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionCapabilityFuture<'a> {
        Box::pin(async move {
            self.phases
                .inspect(command, || self.inspect_activation(command))
        })
    }
}

effect_capability!(IngressPublicationCapability);

impl<Backend> IngressPublicationInspectionCapability for ManagedTestWorkloadProvider<Backend>
where
    Backend: TestSandboxActivation,
{
    fn inspect<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionCapabilityFuture<'a> {
        Box::pin(async move { self.inspect_success(command) })
    }
}

impl<Backend> WorkloadExecutionObservationCapability for ManagedTestWorkloadProvider<Backend>
where
    Backend: TestSandboxActivation,
{
    fn observe<'a>(
        &'a self,
        request: &'a WorkloadExecutionObservationRequest,
    ) -> WorkloadExecutionObservationFuture<'a> {
        Box::pin(async move {
            let sandbox_id = SandboxId::new(request.execution().execution_id().as_str());
            self.backend
                .activated_handle_for_test(&sandbox_id)
                .map(SandboxInspection::provider_reported)
                .map(WorkloadProviderObservation::Present)
                .unwrap_or(WorkloadProviderObservation::Absent)
        })
    }
}

impl<Backend> WorkloadIngressObservationCapability for ManagedTestWorkloadProvider<Backend>
where
    Backend: TestSandboxActivation,
{
    fn observe<'a>(
        &'a self,
        request: &'a WorkloadIngressObservationRequest,
    ) -> WorkloadIngressObservationFuture<'a> {
        Box::pin(async move {
            let sandbox_id = SandboxId::new(request.execution().execution_id().as_str());
            let handle = self.backend.activated_handle_for_test(&sandbox_id);
            let Some(handle) = handle else {
                return WorkloadProviderObservation::Absent;
            };
            let plan = request.compiled_plan();
            let mut observed = Vec::with_capacity(plan.content().listeners().len());
            for listener in plan.content().listeners() {
                let Some(endpoint) = handle
                    .published_endpoints
                    .iter()
                    .find(|endpoint| endpoint.name == listener.name())
                else {
                    return WorkloadProviderObservation::Absent;
                };
                let Some(port) = NonZeroU16::new(endpoint.address.port()) else {
                    return WorkloadProviderObservation::Ambiguous;
                };
                let lifetime = process_lifetime(plan.content().identity().generation().as_u64());
                let bound_endpoint = match PortBoundEndpoint::new(
                    PortProtocol::Tcp,
                    PortBindRealm::Host,
                    PortBindTarget::ipv4_specific(Ipv4Addr::LOCALHOST),
                    port,
                ) {
                    Ok(bound_endpoint) => bound_endpoint,
                    Err(_) => return WorkloadProviderObservation::Ambiguous,
                };
                observed.push(WorkloadObservedIngressEndpoint::new(
                    listener.endpoint_id().clone(),
                    endpoint.address,
                    WorkloadIngressBindingWitness::new(
                        plan.plan().plan_id().clone(),
                        plan.plan().digest(),
                        plan.content().identity().generation(),
                        listener.listener_id().clone(),
                        listener.port_lease_id().clone(),
                        lifetime,
                        lifetime,
                        bound_endpoint,
                        match listener.port_request() {
                            WorkloadNetworkPortRequestMode::Exact { .. } => {
                                PortBindingProvenance::NimbusOwned
                            }
                            WorkloadNetworkPortRequestMode::ProviderAssigned => {
                                PortBindingProvenance::ProviderAssigned
                            }
                        },
                    ),
                ));
            }
            WorkloadProviderObservation::Present(observed)
        })
    }
}

fn process_lifetime(generation: u64) -> PortLeaseLifetime {
    serde_json::from_value(serde_json::json!({
        "generation": generation,
        "effect_scope": "process_bound",
    }))
    .expect("managed test lifetime should validate")
}

pub(super) fn managed_router_config<Backend>(
    engine: Arc<Engine>,
    service_manager: Arc<ServiceManager>,
    backend: Arc<Backend>,
) -> RouterBuildConfig
where
    Backend: TestSandboxActivation,
{
    let realm = TEST_REALM_ID.fetch_add(1, Ordering::Relaxed);
    let backend_kind = backend.kind();
    let network_root = engine
        .data_dir()
        .join("managed-workload-tests")
        .join(realm.to_string());
    let requirements = sandbox_network_plan_requirements(backend_kind);
    let attachment_provider_id = requirements.required_attachment_provider_id().clone();
    let ingress = nimbus_owned_workload_ingress_registration();
    let ingress_provider_id = ingress.provider_id().clone();
    let attachment = NetworkAttachmentProviderRegistration::new(
        attachment_provider_id.clone(),
        requirements.capability_requirements().attachment().clone(),
        [NetworkAddressFamily::Ipv4],
        NetworkLifecycleCapabilitySet::new([
            NetworkLifecycleFeature::DurableInspect,
            NetworkLifecycleFeature::Reconcile,
            NetworkLifecycleFeature::Delete,
        ]),
        NetworkSovereigntyCapabilities::new(NetworkControlPlaneLocality::LocalOnly, [], true),
    );
    let reports =
        NetworkCapabilityRegistry::new([NetworkCapabilityBundle::new(attachment, ingress)])
            .expect("managed test provider reports should validate");
    let selection = NetworkCapabilitySelection::new(
        attachment_provider_id.clone(),
        ingress_provider_id.clone(),
    );
    let bootstrap = LocalNetworkManager::bootstrap(&network_root)
        .expect("managed test network authority should bootstrap");
    let network_manager = bootstrap.freeze(reports);
    let journal = ProviderProvisionAttemptJournal::open(
        network_root.join("provider"),
        "server-managed-test-provider",
    )
    .expect("managed test provider journal should open");
    let provider = Arc::new(ManagedTestWorkloadProvider::new(
        backend,
        ProviderProvisionPhaseAdapter::new(journal),
    ));
    let execution_provider_id = sandbox_execution_provider_id(backend_kind);
    let providers = ServerWorkloadProviders::new(
        attachment_provider_id,
        provider.clone(),
        execution_provider_id,
        provider.clone(),
        ingress_provider_id,
        provider,
    );
    let composition = ServerWorkloadComposition::new(
        engine,
        network_manager,
        service_manager,
        NodeIdentity::new(format!("server-managed-test-{realm}"))
            .expect("managed test node identity should validate"),
        selection,
        NetworkSovereigntyRequirements::new(
            NetworkControlPlaneLocality::LocalOnly,
            std::collections::BTreeSet::new(),
            true,
        ),
        providers,
    )
    .expect("managed test workload composition should validate");
    RouterOptions::managed(composition).into_build_config()
}

/// Complete managed composition for tests that exercise only sibling routes.
pub(super) fn effect_forbidden_managed_router_config(engine: Arc<Engine>) -> RouterBuildConfig {
    let backend = Arc::new(EffectForbiddenSandboxBackend);
    let services = Arc::new(ServiceManager::new(
        Arc::new(EmptyServiceDefinitionCatalog),
        backend.clone(),
    ));
    managed_router_config(engine, services, backend)
}
