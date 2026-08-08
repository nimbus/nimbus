use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use nimbus_core::{TenantId, WorkloadId};
use nimbus_network::{
    EndpointProtocol, NetworkAddressFamily, NetworkAttachmentProviderRegistration,
    NetworkBindRealmKind, NetworkCapabilityBundle, NetworkCapabilityRegistry,
    NetworkCapabilitySelection, NetworkControlPlaneLocality, NetworkEndpointCapabilitySet,
    NetworkExposure, NetworkForwardingCapabilitySet, NetworkIngressCapabilitySet,
    NetworkIngressProviderRegistration, NetworkLifecycleCapabilitySet, NetworkLifecycleFeature,
    NetworkPortAssignmentMode, NetworkProviderId, NetworkSovereigntyCapabilities,
    NetworkSovereigntyRequirements, NetworkTlsBehavior, PortProtocol,
};
use nimbus_sandbox::{
    SandboxBackend, SandboxBackendKind, SandboxFuture, SandboxHandle, SandboxId, SandboxInspection,
    SandboxOwnerSpec, SandboxPortBinding, SandboxProcessSpec, SandboxRootSpec, SandboxSpec,
};
use nimbus_services::{EmptyServiceDefinitionCatalog, ServiceBackend, ServiceManager};
use nimbus_tenant::{
    TenantIsolationContext, TenantIsolationPolicyInput, WorkloadAttributes, WorkloadLocation,
};
use nimbus_workloads::{
    WorkloadActivationIntent, WorkloadExecutionReference, WorkloadNetworkForwardingBehavior,
    WorkloadProvisionInspectionResult, WorkloadProvisionSourceGeneration,
    WorkloadProvisionSourceResourceVersion, WorkloadPublicationIntent, WorkloadSagaCommit,
    WorkloadSagaExpected, WorkloadSagaFuture, WorkloadSagaKey, WorkloadSagaPage,
    WorkloadSagaPageRequest, WorkloadSagaRecord, WorkloadSagaStore, WorkloadSagaStoreError,
    WorkloadSagaTenantPage, WorkloadSagaTenantPageRequest,
};

use super::*;
use crate::embedded_local_node_identity;
use crate::workload_projection::ServiceManagerWorkloadProjectionSink;
use crate::workload_provision_source::ServiceManagerWorkloadProvisionSourceAuthority;
use crate::workload_saga::{
    ConfirmedWorkloadProvisionCommand, IngressProvisionCapabilities, IngressPublicationCapability,
    IngressPublicationInspectionCapability, NetworkAttachmentCapability,
    NetworkAttachmentProvisionCapabilities, NetworkReservationCapability,
    WorkloadActivationCapability, WorkloadActivationPrerequisiteCapability,
    WorkloadExecutionProvisionCapabilities, WorkloadPreparationCapability,
    WorkloadProvisionCapabilityFuture, WorkloadProvisionCapabilityRegistry,
    WorkloadProvisionSourceAuthority, WorkloadReadinessCapability, WorkloadSagaCoordinator,
};

fn tenant() -> TenantId {
    TenantId::new("tenant-a").expect("fixture tenant should validate")
}

fn spec(bindings: impl IntoIterator<Item = SandboxPortBinding>) -> SandboxSpec {
    SandboxSpec::new(
        tenant(),
        SandboxOwnerSpec::standalone_named("worker"),
        SandboxBackendKind::Krun,
        SandboxRootSpec::rootfs("/fixture/rootfs"),
        SandboxProcessSpec::new(["/bin/true"]),
    )
    .with_port_bindings(bindings)
}

fn decision() -> nimbus_tenant::TenantIsolationDecision {
    TenantIsolationContext::system(tenant(), "resource-provision-test")
        .with_deployment_generation(1)
        .with_workload_location(
            WorkloadLocation::new().with_node_id(embedded_local_node_identity().as_str()),
        )
        .admit_decision(TenantIsolationPolicyInput::new(
            WorkloadAttributes::sandbox("worker")
                .with_sandbox_id("stable-worker")
                .with_sandbox_backend(SandboxBackendKind::Krun),
        ))
        .expect("fixture workload should admit")
}

fn source(spec: SandboxSpec) -> WorkloadProvisionSource {
    WorkloadProvisionSource::StandaloneSandbox {
        stable_resource_id: "stable-worker".to_owned(),
        profile: "worker".to_owned(),
        source_generation: WorkloadProvisionSourceGeneration::new(1),
        resource_version: WorkloadProvisionSourceResourceVersion::new("source-v1")
            .expect("fixture source version should validate"),
        sandbox_spec: spec,
    }
}

#[derive(Default)]
struct NativeSagaStore {
    records: Mutex<BTreeMap<WorkloadSagaKey, WorkloadSagaRecord>>,
}

impl NativeSagaStore {
    fn record_count(&self) -> usize {
        self.records
            .lock()
            .expect("native saga store lock should remain healthy")
            .len()
    }

    fn record(&self, key: &WorkloadSagaKey) -> WorkloadSagaRecord {
        self.records
            .lock()
            .expect("native saga store lock should remain healthy")
            .get(key)
            .cloned()
            .expect("native saga record should exist")
    }
}

impl WorkloadSagaStore for NativeSagaStore {
    fn load<'a>(
        &'a self,
        key: &'a WorkloadSagaKey,
    ) -> WorkloadSagaFuture<'a, Option<WorkloadSagaRecord>> {
        Box::pin(async move {
            Ok(self
                .records
                .lock()
                .expect("native saga store lock should remain healthy")
                .get(key)
                .cloned())
        })
    }

    fn compare_and_swap<'a>(
        &'a self,
        expected: WorkloadSagaExpected,
        next: WorkloadSagaRecord,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaCommit> {
        Box::pin(async move {
            let key = next.key().clone();
            let mut records = self
                .records
                .lock()
                .expect("native saga store lock should remain healthy");
            if records.get(&key) == Some(&next) {
                return Ok(WorkloadSagaCommit::Unchanged);
            }
            let observed = records.get(&key);
            let matches = match (expected, observed) {
                (WorkloadSagaExpected::Missing, None) => true,
                (WorkloadSagaExpected::Revision(expected), Some(record)) => {
                    record.revision() == expected
                }
                _ => false,
            };
            if !matches {
                return Err(WorkloadSagaStoreError::Conflict {
                    expected,
                    observed: observed.map(WorkloadSagaRecord::revision),
                });
            }
            records.insert(key, next);
            Ok(WorkloadSagaCommit::Applied)
        })
    }

    fn list_recoverable<'a>(
        &'a self,
        request: WorkloadSagaPageRequest,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaPage> {
        Box::pin(async move { WorkloadSagaPage::new(&request, Vec::new(), false) })
    }

    fn list_restart_candidates<'a>(
        &'a self,
        request: nimbus_workloads::WorkloadRestartCandidatePageRequest,
    ) -> nimbus_workloads::WorkloadSagaFuture<'a, nimbus_workloads::WorkloadRestartCandidatePage>
    {
        Box::pin(async move {
            nimbus_workloads::WorkloadRestartCandidatePage::new(&request, Vec::new(), false)
        })
    }

    fn list_for_tenant<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        request: WorkloadSagaTenantPageRequest,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaTenantPage> {
        Box::pin(async move { WorkloadSagaTenantPage::new(tenant_id, &request, Vec::new(), false) })
    }
}

#[derive(Default)]
struct NativeProvisionProvider {
    calls: Mutex<Vec<(WorkloadSagaKey, nimbus_workloads::WorkloadProvisionStep)>>,
    execution_observations: AtomicUsize,
}

impl NativeProvisionProvider {
    fn outcome(
        &self,
        command: &ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionInspectionResult {
        self.calls
            .lock()
            .expect("native provision provider lock should remain healthy")
            .push((command.claim().attempt().key().clone(), command.step()));
        WorkloadProvisionInspectionResult::Succeeded {
            attempt_id: command.attempt_id().clone(),
            dispatch_epoch: command.dispatch_epoch(),
            provider_target: command.provider_target().clone(),
            evidence: crate::workload_saga::test_support::success_for(command.claim().attempt()),
        }
    }

    fn calls_for(&self, key: &WorkloadSagaKey) -> usize {
        self.calls
            .lock()
            .expect("native provision provider lock should remain healthy")
            .iter()
            .filter(|(candidate, _)| candidate == key)
            .count()
    }
}

macro_rules! native_effect_capability {
    ($trait_name:ident) => {
        impl $trait_name for NativeProvisionProvider {
            fn execute<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadProvisionCommand,
            ) -> WorkloadProvisionCapabilityFuture<'a> {
                Box::pin(async move { self.outcome(command) })
            }

            fn inspect<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadProvisionCommand,
            ) -> WorkloadProvisionCapabilityFuture<'a> {
                Box::pin(async move { self.outcome(command) })
            }
        }
    };
}

macro_rules! native_inspection_capability {
    ($trait_name:ident) => {
        impl $trait_name for NativeProvisionProvider {
            fn inspect<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadProvisionCommand,
            ) -> WorkloadProvisionCapabilityFuture<'a> {
                Box::pin(async move { self.outcome(command) })
            }
        }
    };
}

native_effect_capability!(NetworkReservationCapability);
native_effect_capability!(WorkloadPreparationCapability);
native_effect_capability!(NetworkAttachmentCapability);
native_inspection_capability!(WorkloadActivationPrerequisiteCapability);
native_effect_capability!(WorkloadActivationCapability);
native_inspection_capability!(WorkloadReadinessCapability);
native_effect_capability!(IngressPublicationCapability);
native_inspection_capability!(IngressPublicationInspectionCapability);

impl crate::workload_projection::WorkloadExecutionObservationCapability
    for NativeProvisionProvider
{
    fn observe<'a>(
        &'a self,
        request: &'a crate::workload_projection::WorkloadExecutionObservationRequest,
    ) -> crate::workload_projection::WorkloadExecutionObservationFuture<'a> {
        Box::pin(async move {
            self.execution_observations.fetch_add(1, Ordering::AcqRel);
            let spec = crate::workload_executable::decode_sandbox_spec(request.executable())
                .expect("native fixture executable should decode");
            crate::workload_projection::WorkloadProviderObservation::Present(
                SandboxInspection::provider_reported(SandboxHandle::new(
                    request.key().tenant_id().clone(),
                    SandboxId::new(request.execution().execution_id().as_str()),
                    spec.display_name(),
                    spec.backend,
                    nimbus_sandbox::SandboxStatus::Ready,
                    Vec::new(),
                )),
            )
        })
    }
}

impl crate::workload_projection::WorkloadIngressObservationCapability for NativeProvisionProvider {
    fn observe<'a>(
        &'a self,
        _request: &'a crate::workload_projection::WorkloadIngressObservationRequest,
    ) -> crate::workload_projection::WorkloadIngressObservationFuture<'a> {
        Box::pin(async { crate::workload_projection::WorkloadProviderObservation::Ambiguous })
    }
}

#[derive(Default)]
struct EffectForbiddenSandboxBackend;

impl SandboxBackend for EffectForbiddenSandboxBackend {
    fn kind(&self) -> SandboxBackendKind {
        SandboxBackendKind::Krun
    }

    fn inspect(&self, _id: &SandboxId) -> SandboxFuture<Option<SandboxInspection>> {
        panic!("native facade reads must not inspect through ServiceManager")
    }

    fn stop(&self, _id: &SandboxId) -> SandboxFuture<()> {
        panic!("native provision test must not retire through ServiceManager")
    }
}

fn native_provider_realm() -> (NetworkCapabilityRegistry, NetworkCapabilitySelection) {
    let requirements = nimbus_sandbox::sandbox_network_plan_requirements(SandboxBackendKind::Krun);
    let ingress_provider = NetworkProviderId::for_registration_key("native-fixture-ingress");
    let lifecycle = NetworkLifecycleCapabilitySet::new([
        NetworkLifecycleFeature::DurableInspect,
        NetworkLifecycleFeature::Reconcile,
        NetworkLifecycleFeature::Delete,
    ]);
    let attachment = NetworkAttachmentProviderRegistration::new(
        requirements.required_attachment_provider_id().clone(),
        requirements.capability_requirements().attachment().clone(),
        [NetworkAddressFamily::Ipv4, NetworkAddressFamily::Ipv6],
        lifecycle.clone(),
        NetworkSovereigntyCapabilities::new(NetworkControlPlaneLocality::LocalOnly, [], true),
    );
    let ingress = NetworkIngressProviderRegistration::new(
        ingress_provider.clone(),
        NetworkEndpointCapabilitySet::new(
            [NetworkAddressFamily::Ipv4, NetworkAddressFamily::Ipv6],
            [NetworkBindRealmKind::Host],
            [NetworkExposure::Loopback, NetworkExposure::Private],
            [PortProtocol::Tcp],
            [
                NetworkPortAssignmentMode::Exact,
                NetworkPortAssignmentMode::ProviderAssigned,
            ],
        ),
        NetworkIngressCapabilitySet::new([]),
        NetworkForwardingCapabilitySet::new([]),
        lifecycle,
        NetworkSovereigntyCapabilities::new(NetworkControlPlaneLocality::LocalOnly, [], true),
    );
    let selection = NetworkCapabilitySelection::new(
        requirements.required_attachment_provider_id().clone(),
        ingress_provider,
    );
    (
        NetworkCapabilityRegistry::new([NetworkCapabilityBundle::new(attachment, ingress)])
            .expect("native fixture provider reports should validate"),
        selection,
    )
}

fn native_provisioner(
    manager: Arc<ServiceManager>,
    store: Arc<NativeSagaStore>,
    provider: Arc<NativeProvisionProvider>,
) -> Arc<WorkloadProvisioner> {
    let (provider_reports, selection) = native_provider_realm();
    let execution_provider = sandbox_execution_provider_id(SandboxBackendKind::Krun);
    let attachment_provider =
        nimbus_sandbox::sandbox_network_plan_requirements(SandboxBackendKind::Krun)
            .required_attachment_provider_id()
            .clone();
    let capabilities = WorkloadProvisionCapabilityRegistry::new(
        [NetworkAttachmentProvisionCapabilities::new(
            attachment_provider,
            provider.clone(),
        )],
        [WorkloadExecutionProvisionCapabilities::new(
            execution_provider,
            provider.clone(),
        )],
        [IngressProvisionCapabilities::new(
            selection.ingress_provider_id().clone(),
            provider,
        )],
    )
    .expect("native fixture capabilities should validate");
    let store: Arc<dyn WorkloadSagaStore> = store;
    let source_authority: Arc<dyn WorkloadProvisionSourceAuthority> = Arc::new(
        ServiceManagerWorkloadProvisionSourceAuthority::new(manager.clone()),
    );
    Arc::new(
        WorkloadProvisioner::new(
            embedded_local_node_identity(),
            provider_reports,
            selection,
            NetworkSovereigntyRequirements::new(
                NetworkControlPlaneLocality::LocalOnly,
                BTreeSet::new(),
                true,
            ),
            Arc::new(WorkloadSagaCoordinator::new(store)),
            source_authority,
            capabilities,
            Arc::new(ServiceManagerWorkloadProjectionSink::new(manager)),
        )
        .expect("native fixture provider realm should be coherent"),
    )
}

#[test]
fn canonical_request_preserves_named_port_forwarding_and_exact_tls_semantics() {
    let spec = spec([
        SandboxPortBinding::new("tcp", EndpointProtocol::Tcp, 14001, 4001),
        SandboxPortBinding::new("http", EndpointProtocol::Http, 14002, 4002),
        SandboxPortBinding::new("https", EndpointProtocol::Https, 14003, 4003),
    ]);

    let request = provision_request(decision(), source(spec.clone()), &spec);

    assert_eq!(
        request.execution_provider_id,
        sandbox_execution_provider_id(SandboxBackendKind::Krun)
    );
    assert_eq!(
        request.activation,
        WorkloadActivationIntent::ActivateWhenAttached
    );
    assert_eq!(
        request.publication,
        WorkloadPublicationIntent::PublishWhenReady
    );
    assert_eq!(request.endpoint_semantics.len(), 3);
    assert_eq!(request.endpoint_semantics[0].listener_name(), "tcp");
    assert_eq!(request.endpoint_semantics[1].listener_name(), "http");
    assert_eq!(request.endpoint_semantics[2].listener_name(), "https");
    assert!(request.endpoint_semantics.iter().all(|endpoint| {
        endpoint.forwarding() == WorkloadNetworkForwardingBehavior::PortForwarded
    }));
    assert_eq!(
        request.endpoint_semantics[0].tls(),
        NetworkTlsBehavior::Disabled
    );
    assert_eq!(
        request.endpoint_semantics[1].tls(),
        NetworkTlsBehavior::Disabled
    );
    assert_eq!(
        request.endpoint_semantics[2].tls(),
        NetworkTlsBehavior::Passthrough
    );
}

#[test]
fn canonical_request_withholds_publication_when_no_bindings_exist() {
    let spec = spec([]);

    let request = provision_request(decision(), source(spec.clone()), &spec);

    assert!(request.endpoint_semantics.is_empty());
    assert_eq!(request.publication, WorkloadPublicationIntent::Withheld);
    assert_eq!(
        request.activation,
        WorkloadActivationIntent::ActivateWhenAttached
    );
}

#[tokio::test]
async fn native_service_and_sandbox_callers_use_compute_dispatch() {
    let tenant_id = tenant();
    let manager = Arc::new(ServiceManager::new(
        Arc::new(EmptyServiceDefinitionCatalog),
        Arc::new(EffectForbiddenSandboxBackend),
    ));
    let service_spec = SandboxSpec::new(
        tenant_id.clone(),
        SandboxOwnerSpec::service("service-worker"),
        SandboxBackendKind::Krun,
        SandboxRootSpec::rootfs("/fixture/service-rootfs"),
        SandboxProcessSpec::new(["/bin/service"]),
    );
    manager
        .create_service_definition(
            &tenant_id,
            "service-worker",
            ServiceBackend::sandbox(service_spec),
            BTreeMap::new(),
        )
        .expect("native sandbox-backed service source should be declared");

    let store = Arc::new(NativeSagaStore::default());
    let provider = Arc::new(NativeProvisionProvider::default());
    let facade = ComputeResourceProvisioner::new(
        manager.clone(),
        native_provisioner(manager, store.clone(), provider.clone()),
    );
    let context = TenantIsolationContext::system(tenant_id.clone(), "native-resource-provision");
    let cancellation = WorkloadProvisionCancellation::default();

    let standalone = facade
        .provision_standalone_sandbox(
            &context,
            "standalone-worker",
            "worker",
            SandboxSpec::new(
                tenant_id.clone(),
                SandboxOwnerSpec::standalone_named("standalone-worker"),
                SandboxBackendKind::Krun,
                SandboxRootSpec::rootfs("/fixture/standalone-rootfs"),
                SandboxProcessSpec::new(["/bin/worker"]),
            ),
            BTreeMap::new(),
            &cancellation,
        )
        .await
        .expect("standalone native caller should complete through compute dispatch");
    let service = facade
        .provision_sandbox_service(&context, "service-worker", &cancellation)
        .await
        .expect("service native caller should complete through compute dispatch");

    let standalone_observation = standalone
        .observation
        .as_ref()
        .expect("standalone dispatch should project exact provider evidence");
    assert_eq!(
        standalone_observation.observed_generation,
        standalone.source.generation
    );
    assert_eq!(
        standalone_observation.handle.id.as_str(),
        WorkloadExecutionReference::for_intent(
            store
                .record(&WorkloadSagaKey::new(
                    tenant_id.clone(),
                    WorkloadId::new("standalone-worker").expect("standalone ID should validate"),
                ))
                .active_intent(),
        )
        .execution_id()
        .as_str()
    );
    let service_observation = service
        .observation
        .as_ref()
        .expect("service dispatch should project exact provider evidence");
    assert_eq!(
        service_observation.observed_generation,
        service.definition.generation
    );

    let standalone_key = WorkloadSagaKey::new(
        tenant_id.clone(),
        WorkloadId::new("standalone-worker").expect("standalone ID should validate"),
    );
    let service_key = WorkloadSagaKey::new(
        tenant_id,
        WorkloadId::new("service-worker").expect("service ID should validate"),
    );
    assert_eq!(store.record_count(), 2);
    assert_eq!(provider.calls_for(&standalone_key), 6);
    assert_eq!(provider.calls_for(&service_key), 6);
    assert_eq!(provider.execution_observations.load(Ordering::Acquire), 2);

    let replayed_standalone = facade
        .provision_standalone_sandbox(
            &context,
            "standalone-worker",
            "worker",
            standalone.source.spec.clone(),
            BTreeMap::new(),
            &WorkloadProvisionCancellation::default(),
        )
        .await
        .expect("exact standalone replay should reuse compute dispatch truth");
    let replayed_service = facade
        .provision_sandbox_service(
            &context,
            "service-worker",
            &WorkloadProvisionCancellation::default(),
        )
        .await
        .expect("exact service replay should reuse compute dispatch truth");
    assert_eq!(replayed_standalone, standalone);
    assert_eq!(replayed_service, service);
    assert_eq!(store.record_count(), 2);
    assert_eq!(provider.calls_for(&standalone_key), 6);
    assert_eq!(provider.calls_for(&service_key), 6);
    assert_eq!(
        provider.execution_observations.load(Ordering::Acquire),
        4,
        "exact replay may refresh read-only observed state but must not repeat provider effects"
    );
}
