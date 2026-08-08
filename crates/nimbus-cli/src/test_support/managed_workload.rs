use std::path::Path;
use std::sync::Arc;

use nimbus::{Engine, SandboxBackend, SandboxBackendKind, SandboxId};
use nimbus_compute::workload_saga::{
    ConfirmedWorkloadProvisionCommand, IngressPublicationCapability,
    IngressPublicationInspectionCapability, NetworkAttachmentCapability,
    NetworkReservationCapability, WorkloadActivationCapability,
    WorkloadActivationPrerequisiteCapability, WorkloadPreparationCapability,
    WorkloadProvisionCapabilityFuture, WorkloadReadinessCapability,
};
use nimbus_compute::{
    WorkloadExecutionObservationCapability, WorkloadIngressObservationCapability,
};
use nimbus_network::{
    LocalNetworkManager, NetworkAddressFamily, NetworkAttachmentProviderRegistration,
    NetworkCapabilityBundle, NetworkCapabilityRegistry, NetworkCapabilitySelection,
    NetworkControlPlaneLocality, NetworkLifecycleCapabilitySet, NetworkLifecycleFeature,
    NetworkSovereigntyCapabilities, NetworkSovereigntyRequirements,
};
use nimbus_sandbox::{SandboxFuture, SandboxInspection};
use nimbus_server::{
    ServerWorkloadComposition, ServerWorkloadProviders, nimbus_owned_workload_ingress_registration,
};
use nimbus_services::{EmptyServiceDefinitionCatalog, ServiceManager};
use nimbus_workloads::{NodeIdentity, WorkloadExecutionProviderId};

struct EffectForbiddenSandboxBackend;

impl SandboxBackend for EffectForbiddenSandboxBackend {
    fn kind(&self) -> SandboxBackendKind {
        SandboxBackendKind::Krun
    }

    fn inspect(&self, _id: &SandboxId) -> SandboxFuture<Option<SandboxInspection>> {
        panic!("managed server fixture must not inspect a sandbox")
    }

    fn stop(&self, _id: &SandboxId) -> SandboxFuture<()> {
        panic!("managed server fixture must not stop a sandbox")
    }
}

struct EffectForbiddenAttachmentProvider;

macro_rules! effect_forbidden_capability {
    ($provider:ty, $capability:ident) => {
        impl $capability for $provider {
            fn execute<'a>(
                &'a self,
                _command: &'a ConfirmedWorkloadProvisionCommand,
            ) -> WorkloadProvisionCapabilityFuture<'a> {
                panic!("managed server fixture must not execute workload effects")
            }

            fn inspect<'a>(
                &'a self,
                _command: &'a ConfirmedWorkloadProvisionCommand,
            ) -> WorkloadProvisionCapabilityFuture<'a> {
                panic!("managed server fixture must not inspect workload effects")
            }
        }
    };
}

effect_forbidden_capability!(
    EffectForbiddenAttachmentProvider,
    NetworkReservationCapability
);
effect_forbidden_capability!(
    EffectForbiddenAttachmentProvider,
    NetworkAttachmentCapability
);

struct EffectForbiddenExecutionProvider;

effect_forbidden_capability!(
    EffectForbiddenExecutionProvider,
    WorkloadPreparationCapability
);
effect_forbidden_capability!(
    EffectForbiddenExecutionProvider,
    WorkloadActivationCapability
);

impl WorkloadActivationPrerequisiteCapability for EffectForbiddenExecutionProvider {
    fn inspect<'a>(
        &'a self,
        _command: &'a ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionCapabilityFuture<'a> {
        panic!("managed server fixture must not inspect activation prerequisites")
    }
}

impl WorkloadReadinessCapability for EffectForbiddenExecutionProvider {
    fn inspect<'a>(
        &'a self,
        _command: &'a ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionCapabilityFuture<'a> {
        panic!("managed server fixture must not inspect workload readiness")
    }
}

impl WorkloadExecutionObservationCapability for EffectForbiddenExecutionProvider {
    fn observe<'a>(
        &'a self,
        _request: &'a nimbus_compute::WorkloadExecutionObservationRequest,
    ) -> nimbus_compute::WorkloadExecutionObservationFuture<'a> {
        panic!("managed server fixture must not observe workload execution")
    }
}

struct EffectForbiddenIngressProvider;

effect_forbidden_capability!(EffectForbiddenIngressProvider, IngressPublicationCapability);

impl IngressPublicationInspectionCapability for EffectForbiddenIngressProvider {
    fn inspect<'a>(
        &'a self,
        _command: &'a ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionCapabilityFuture<'a> {
        panic!("managed server fixture must not inspect ingress publication")
    }
}

impl WorkloadIngressObservationCapability for EffectForbiddenIngressProvider {
    fn observe<'a>(
        &'a self,
        _request: &'a nimbus_compute::WorkloadIngressObservationRequest,
    ) -> nimbus_compute::WorkloadIngressObservationFuture<'a> {
        panic!("managed server fixture must not observe ingress publication")
    }
}

/// Build a complete, effect-forbidden managed realm for transport-only tests.
pub(crate) fn managed_server_composition(
    engine: Arc<Engine>,
    network_root: &Path,
) -> ServerWorkloadComposition {
    let requirements = nimbus_sandbox::sandbox_network_plan_requirements(SandboxBackendKind::Krun);
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
    let registry =
        NetworkCapabilityRegistry::new([NetworkCapabilityBundle::new(attachment, ingress)])
            .expect("managed fixture provider reports should validate");
    let selection = NetworkCapabilitySelection::new(
        attachment_provider_id.clone(),
        ingress_provider_id.clone(),
    );
    let manager = LocalNetworkManager::bootstrap(network_root)
        .expect("managed fixture should claim one network realm")
        .freeze(registry);
    let services = Arc::new(ServiceManager::new(
        Arc::new(EmptyServiceDefinitionCatalog),
        Arc::new(EffectForbiddenSandboxBackend),
    ));
    ServerWorkloadComposition::new(
        engine,
        manager,
        services,
        NodeIdentity::new("cli-managed-server-fixture").expect("fixture node should validate"),
        selection,
        NetworkSovereigntyRequirements::new(NetworkControlPlaneLocality::LocalOnly, [], true),
        ServerWorkloadProviders::new(
            attachment_provider_id,
            Arc::new(EffectForbiddenAttachmentProvider),
            WorkloadExecutionProviderId::for_registration_key("cli-managed-server-fixture"),
            Arc::new(EffectForbiddenExecutionProvider),
            ingress_provider_id,
            Arc::new(EffectForbiddenIngressProvider),
        ),
    )
    .expect("complete effect-forbidden managed fixture should compose")
}
