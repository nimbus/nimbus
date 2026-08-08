use std::collections::BTreeMap;
use std::fs;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use nimbus_compute::config::control_plane::ControlPlaneConfig;
use nimbus_compute::config::deployment::DeploymentConfig;
use nimbus_compute::config::node_services::NodeServicesConfig;
use nimbus_compute::config::runtime::RuntimeGovernorConfig;
use nimbus_compute::workload_network_plan::{
    AdmittedWorkloadNetworkSource, WorkloadNetworkEndpointSemanticsInput,
    WorkloadNetworkPlanCompiler,
};
use nimbus_compute::workload_saga::{
    ConfirmedWorkloadProvisionCommand, WorkloadProvisionCapabilityFuture,
};
use nimbus_core::{TenantId, WorkloadId};
use nimbus_network::{
    EndpointProtocol, NetworkAddressFamily, NetworkAttachmentProviderRegistration,
    NetworkCapabilityBundle, NetworkCapabilityRegistry, NetworkControlPlaneLocality,
    NetworkLifecycleCapabilitySet, NetworkLifecycleFeature, NetworkSovereigntyCapabilities,
    NetworkTlsBehavior,
};
use nimbus_sandbox::{
    SandboxBackend, SandboxBackendKind, SandboxFuture, SandboxId, SandboxInspection,
    SandboxOwnerSpec, SandboxPortBinding, SandboxProcessSpec, SandboxRootSpec, SandboxSpec,
    sandbox_network_plan_requirements,
};
use nimbus_services::{EmptyServiceDefinitionCatalog, ServiceManager};
use nimbus_tenant::{
    TenantIsolationContext, TenantIsolationPolicyInput, TenantNetworkPolicyDecision,
    TenantServiceGrantPolicyDecision, WorkloadAttributes, WorkloadLocation,
};
use nimbus_workloads::{WorkloadActivationIntent, WorkloadPublicationIntent};

use super::*;
use crate::config::transport::TransportConfig;
use crate::network_capabilities::{
    nimbus_owned_local_ingress_provider_id, nimbus_owned_workload_ingress_registration,
};
use crate::state::{AppState, AppStateConfig};

struct EffectForbiddenSandboxBackend;

impl SandboxBackend for EffectForbiddenSandboxBackend {
    fn kind(&self) -> SandboxBackendKind {
        SandboxBackendKind::Krun
    }

    fn inspect(&self, _id: &SandboxId) -> SandboxFuture<Option<SandboxInspection>> {
        panic!("composition must not inspect a sandbox")
    }

    fn stop(&self, _id: &SandboxId) -> SandboxFuture<()> {
        panic!("composition must not stop a sandbox")
    }
}

struct EffectForbiddenAttachmentProvider;

macro_rules! effect_capability {
    ($provider:ty, $trait_name:ident) => {
        impl $trait_name for $provider {
            fn execute<'a>(
                &'a self,
                _command: &'a ConfirmedWorkloadProvisionCommand,
            ) -> WorkloadProvisionCapabilityFuture<'a> {
                panic!("composition must not execute a provider effect")
            }

            fn inspect<'a>(
                &'a self,
                _command: &'a ConfirmedWorkloadProvisionCommand,
            ) -> WorkloadProvisionCapabilityFuture<'a> {
                panic!("composition must not inspect provider state")
            }
        }
    };
}

effect_capability!(
    EffectForbiddenAttachmentProvider,
    NetworkReservationCapability
);
effect_capability!(
    EffectForbiddenAttachmentProvider,
    NetworkAttachmentCapability
);

struct EffectForbiddenExecutionProvider;

effect_capability!(
    EffectForbiddenExecutionProvider,
    WorkloadPreparationCapability
);
effect_capability!(
    EffectForbiddenExecutionProvider,
    WorkloadActivationCapability
);

impl WorkloadActivationPrerequisiteCapability for EffectForbiddenExecutionProvider {
    fn inspect<'a>(
        &'a self,
        _command: &'a ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionCapabilityFuture<'a> {
        panic!("composition must not inspect activation prerequisites")
    }
}

impl WorkloadReadinessCapability for EffectForbiddenExecutionProvider {
    fn inspect<'a>(
        &'a self,
        _command: &'a ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionCapabilityFuture<'a> {
        panic!("composition must not inspect readiness")
    }
}

impl WorkloadExecutionObservationCapability for EffectForbiddenExecutionProvider {
    fn observe<'a>(
        &'a self,
        _request: &'a nimbus_compute::WorkloadExecutionObservationRequest,
    ) -> nimbus_compute::WorkloadExecutionObservationFuture<'a> {
        panic!("composition must not observe execution")
    }
}

struct EffectForbiddenIngressProvider;

effect_capability!(EffectForbiddenIngressProvider, IngressPublicationCapability);

impl IngressPublicationInspectionCapability for EffectForbiddenIngressProvider {
    fn inspect<'a>(
        &'a self,
        _command: &'a ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionCapabilityFuture<'a> {
        panic!("composition must not inspect ingress publication")
    }
}

impl WorkloadIngressObservationCapability for EffectForbiddenIngressProvider {
    fn observe<'a>(
        &'a self,
        _request: &'a nimbus_compute::WorkloadIngressObservationRequest,
    ) -> nimbus_compute::WorkloadIngressObservationFuture<'a> {
        panic!("composition must not observe ingress")
    }
}

#[derive(Default)]
struct InjectedSagaStore {
    loads: AtomicUsize,
}

impl WorkloadSagaStore for InjectedSagaStore {
    fn load<'a>(
        &'a self,
        _key: &'a nimbus_workloads::WorkloadSagaKey,
    ) -> nimbus_workloads::WorkloadSagaFuture<'a, Option<nimbus_workloads::WorkloadSagaRecord>>
    {
        Box::pin(async move {
            self.loads.fetch_add(1, Ordering::AcqRel);
            Ok(None)
        })
    }

    fn compare_and_swap<'a>(
        &'a self,
        _expected: nimbus_workloads::WorkloadSagaExpected,
        _next: nimbus_workloads::WorkloadSagaRecord,
    ) -> nimbus_workloads::WorkloadSagaFuture<'a, nimbus_workloads::WorkloadSagaCommit> {
        Box::pin(async move { panic!("composition must not commit workload saga state") })
    }

    fn list_recoverable<'a>(
        &'a self,
        _request: nimbus_workloads::WorkloadSagaPageRequest,
    ) -> nimbus_workloads::WorkloadSagaFuture<'a, nimbus_workloads::WorkloadSagaPage> {
        Box::pin(async move { panic!("composition must not list recoverable workload sagas") })
    }

    fn list_restart_candidates<'a>(
        &'a self,
        _request: nimbus_workloads::WorkloadRestartCandidatePageRequest,
    ) -> nimbus_workloads::WorkloadSagaFuture<'a, nimbus_workloads::WorkloadRestartCandidatePage>
    {
        Box::pin(async move { panic!("composition must not list restart candidates") })
    }

    fn list_for_tenant<'a>(
        &'a self,
        _tenant_id: &'a TenantId,
        _request: nimbus_workloads::WorkloadSagaTenantPageRequest,
    ) -> nimbus_workloads::WorkloadSagaFuture<'a, nimbus_workloads::WorkloadSagaTenantPage> {
        Box::pin(async move { panic!("composition must not list tenant workload sagas") })
    }
}

type TestProviders = ServerWorkloadProviders<
    EffectForbiddenAttachmentProvider,
    EffectForbiddenExecutionProvider,
    EffectForbiddenIngressProvider,
>;

struct Fixture {
    engine: Arc<Engine>,
    crossed_engine: Arc<Engine>,
    manager: Arc<LocalNetworkManager>,
    service_manager: Arc<ServiceManager>,
    selection: NetworkCapabilitySelection,
    attachment_provider_id: NetworkProviderId,
    ingress_provider_id: NetworkProviderId,
    execution_provider_id: WorkloadExecutionProviderId,
    engine_root: tempfile::TempDir,
    crossed_engine_root: tempfile::TempDir,
    network_root: tempfile::TempDir,
    _network_authority_guard: std::rc::Rc<std::sync::MutexGuard<'static, ()>>,
}

fn network_authority_test_lock() -> &'static std::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
}

fn fixture() -> Fixture {
    fixture_with_attachment_sovereignty(NetworkSovereigntyCapabilities::new(
        NetworkControlPlaneLocality::LocalOnly,
        [],
        true,
    ))
}

fn fixture_with_attachment_sovereignty(
    attachment_sovereignty: NetworkSovereigntyCapabilities,
) -> Fixture {
    let network_authority_guard = network_authority_test_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let engine_root = tempfile::tempdir().expect("engine root should build");
    let crossed_engine_root = tempfile::tempdir().expect("crossed engine root should build");
    let network_root = tempfile::tempdir().expect("network root should build");
    let engine = Arc::new(Engine::new(engine_root.path()).expect("engine should initialize"));
    let crossed_engine = Arc::new(
        Engine::new(crossed_engine_root.path()).expect("crossed engine should initialize"),
    );
    let requirements = sandbox_network_plan_requirements(SandboxBackendKind::Krun);
    let attachment_provider_id = requirements.required_attachment_provider_id().clone();
    let ingress = nimbus_owned_workload_ingress_registration();
    let ingress_provider_id = ingress.provider_id().clone();
    let lifecycle = NetworkLifecycleCapabilitySet::new([
        NetworkLifecycleFeature::DurableInspect,
        NetworkLifecycleFeature::Reconcile,
        NetworkLifecycleFeature::Delete,
    ]);
    let attachment = NetworkAttachmentProviderRegistration::new(
        attachment_provider_id.clone(),
        requirements.capability_requirements().attachment().clone(),
        [NetworkAddressFamily::Ipv4],
        lifecycle,
        attachment_sovereignty,
    );
    let registry =
        NetworkCapabilityRegistry::new([NetworkCapabilityBundle::new(attachment, ingress)])
            .expect("fixture capability bundle should validate");
    let selection = NetworkCapabilitySelection::new(
        attachment_provider_id.clone(),
        ingress_provider_id.clone(),
    );
    let bootstrap = LocalNetworkManager::bootstrap(network_root.path())
        .expect("fixture should claim network authority");
    let manager = bootstrap.freeze(registry);
    let service_manager = Arc::new(ServiceManager::new(
        Arc::new(EmptyServiceDefinitionCatalog),
        Arc::new(EffectForbiddenSandboxBackend),
    ));
    Fixture {
        engine,
        crossed_engine,
        manager,
        service_manager,
        selection,
        attachment_provider_id,
        ingress_provider_id,
        execution_provider_id: WorkloadExecutionProviderId::for_registration_key(
            "server-composition-test-execution",
        ),
        engine_root,
        crossed_engine_root,
        network_root,
        _network_authority_guard: std::rc::Rc::new(network_authority_guard),
    }
}

fn providers(
    attachment_provider_id: NetworkProviderId,
    ingress_provider_id: NetworkProviderId,
    execution_provider_id: WorkloadExecutionProviderId,
) -> TestProviders {
    ServerWorkloadProviders::new(
        attachment_provider_id,
        Arc::new(EffectForbiddenAttachmentProvider),
        execution_provider_id,
        Arc::new(EffectForbiddenExecutionProvider),
        ingress_provider_id,
        Arc::new(EffectForbiddenIngressProvider),
    )
}

fn sovereignty() -> NetworkSovereigntyRequirements {
    NetworkSovereigntyRequirements::new(NetworkControlPlaneLocality::LocalOnly, [], true)
}

fn valid_composition(fixture: &Fixture) -> ServerWorkloadComposition {
    ServerWorkloadComposition::new(
        Arc::clone(&fixture.engine),
        Arc::clone(&fixture.manager),
        Arc::clone(&fixture.service_manager),
        NodeIdentity::new("server-composition-node").expect("fixture node should validate"),
        fixture.selection.clone(),
        sovereignty(),
        providers(
            fixture.attachment_provider_id.clone(),
            fixture.ingress_provider_id.clone(),
            fixture.execution_provider_id.clone(),
        ),
    )
    .expect("complete exact fixture should compose")
}

fn snapshot_regular_files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, path: &Path, snapshot: &mut BTreeMap<PathBuf, Vec<u8>>) {
        let mut entries = fs::read_dir(path)
            .expect("fixture directory should list")
            .collect::<Result<Vec<_>, _>>()
            .expect("fixture entries should resolve");
        entries.sort_by_key(fs::DirEntry::file_name);
        for entry in entries {
            let path = entry.path();
            let kind = entry.file_type().expect("fixture file type should resolve");
            if kind.is_dir() {
                visit(root, &path, snapshot);
            } else if kind.is_file() {
                snapshot.insert(
                    path.strip_prefix(root)
                        .expect("fixture path should remain below root")
                        .to_path_buf(),
                    fs::read(path).expect("fixture state file should read"),
                );
            }
        }
    }

    let mut snapshot = BTreeMap::new();
    visit(root, root, &mut snapshot);
    snapshot
}

#[tokio::test]
async fn complete_composition_builds_one_exact_managed_authority_without_eager_effects() {
    let fixture = fixture();
    let before_engine = snapshot_regular_files(fixture.engine_root.path());
    let before_network = snapshot_regular_files(fixture.network_root.path());
    let composition = valid_composition(&fixture);
    assert!(Arc::ptr_eq(&composition.engine(), &fixture.engine));
    assert!(Arc::ptr_eq(
        &composition.network_manager(),
        &fixture.manager
    ));
    assert!(Arc::ptr_eq(
        &composition.service_manager(),
        &fixture.service_manager
    ));

    let state = AppState::from_config(AppStateConfig {
        workload: ServerWorkloadProfile::managed(composition),
        deployment: DeploymentConfig::default(),
        control_plane: ControlPlaneConfig::router_options_default(),
        node_services: NodeServicesConfig::default()
            .with_service_manager(Arc::clone(&fixture.service_manager)),
        transport: TransportConfig::default(),
        runtime: RuntimeGovernorConfig::default(),
    });
    assert!(Arc::ptr_eq(&state.engine, &fixture.engine));
    assert!(Arc::ptr_eq(
        &state
            .network_manager()
            .expect("managed state should retain exact manager"),
        &fixture.manager
    ));
    assert!(Arc::ptr_eq(
        &state
            .service_manager()
            .expect("managed state should retain services owner"),
        &fixture.service_manager
    ));
    let provisioner = state
        .workload_provisioner()
        .expect("complete state should expose one provisioner");
    assert_eq!(provisioner.capability_selection(), &fixture.selection);
    assert_eq!(
        provisioner
            .provider_reports()
            .selections()
            .collect::<Vec<_>>(),
        fixture
            .manager
            .capability_registry()
            .selections()
            .collect::<Vec<_>>()
    );
    assert_eq!(
        provisioner.local_node(),
        &NodeIdentity::new("server-composition-node").expect("fixture node should validate")
    );
    assert_eq!(provisioner.sovereignty(), &sovereignty());
    let coordinator = state
        .workload_saga_coordinator()
        .expect("managed state should retain one saga coordinator");
    assert!(Arc::ptr_eq(
        &coordinator,
        &state
            .workload_saga_coordinator()
            .expect("coordinator lookup should be stable")
    ));
    assert_eq!(
        snapshot_regular_files(fixture.engine_root.path()),
        before_engine
    );
    assert_eq!(
        snapshot_regular_files(fixture.network_root.path()),
        before_network
    );

    let key = nimbus_workloads::WorkloadSagaKey::new(
        TenantId::new("composition-tenant").expect("tenant should validate"),
        WorkloadId::new("composition-workload").expect("workload should validate"),
    );
    assert_eq!(coordinator.load(&key).await, Ok(None));
    assert!(
        !fixture
            .engine
            .list_tenants_async()
            .await
            .expect("owner engine tenants should list")
            .is_empty(),
        "the lazily used saga store must write through the exact owned Engine"
    );
    assert!(
        fixture
            .crossed_engine
            .list_tenants_async()
            .await
            .expect("crossed engine tenants should list")
            .is_empty(),
        "a different Engine cannot be paired after composition"
    );
}

#[tokio::test]
async fn foreground_runtime_retains_exact_injected_authority_without_eager_effects() {
    let fixture = fixture();
    let before_engine = snapshot_regular_files(fixture.engine_root.path());
    let before_network = snapshot_regular_files(fixture.network_root.path());
    let injected_store = Arc::new(InjectedSagaStore::default());

    let runtime = valid_composition(&fixture).into_foreground_runtime(injected_store.clone());

    assert!(Arc::ptr_eq(&runtime._compute.engine, &fixture.engine));
    assert_eq!(injected_store.loads.load(Ordering::Acquire), 0);
    assert!(Arc::ptr_eq(
        &runtime
            ._compute
            .network_manager()
            .expect("foreground runtime should retain the exact manager"),
        &fixture.manager,
    ));
    assert!(Arc::ptr_eq(
        &runtime
            ._compute
            .service_manager()
            .expect("foreground runtime should retain the exact services owner"),
        &fixture.service_manager,
    ));
    let provisioner = runtime
        ._compute
        .workload_provisioner()
        .expect("foreground runtime should retain one workload provisioner");
    assert!(Arc::ptr_eq(
        &provisioner,
        &runtime
            ._compute
            .workload_provisioner()
            .expect("foreground provisioner lookup should remain stable"),
    ));
    assert_eq!(provisioner.capability_selection(), &fixture.selection);
    assert_eq!(
        provisioner
            .provider_reports()
            .selections()
            .collect::<Vec<_>>(),
        fixture
            .manager
            .capability_registry()
            .selections()
            .collect::<Vec<_>>(),
    );
    let coordinator = runtime
        ._compute
        .workload_saga_coordinator()
        .expect("foreground runtime should retain one saga coordinator");
    assert!(Arc::ptr_eq(
        &coordinator,
        &runtime
            ._compute
            .workload_saga_coordinator()
            .expect("foreground coordinator lookup should remain stable"),
    ));
    let key = nimbus_workloads::WorkloadSagaKey::new(
        TenantId::new("foreground-composition-tenant").expect("tenant should validate"),
        WorkloadId::new("foreground-composition-workload").expect("workload should validate"),
    );
    assert_eq!(coordinator.load(&key).await, Ok(None));
    assert_eq!(
        injected_store.loads.load(Ordering::Acquire),
        1,
        "the foreground coordinator must use exactly the injected canonical store",
    );
    assert!(std::ptr::eq(
        runtime.resource_provisioner(),
        runtime.resource_provisioner(),
    ));
    assert_eq!(
        snapshot_regular_files(fixture.engine_root.path()),
        before_engine,
    );
    assert_eq!(
        snapshot_regular_files(fixture.network_root.path()),
        before_network,
    );
}

#[test]
fn foreground_runtime_owns_exact_manager_services_and_provider_arc_lifetimes() {
    let fixture = fixture();
    let network_authority_guard = std::rc::Rc::clone(&fixture._network_authority_guard);
    let engine = Arc::downgrade(&fixture.engine);
    let manager = Arc::downgrade(&fixture.manager);
    let service_manager = Arc::downgrade(&fixture.service_manager);
    let attachment = Arc::new(EffectForbiddenAttachmentProvider);
    let attachment_weak = Arc::downgrade(&attachment);
    let execution = Arc::new(EffectForbiddenExecutionProvider);
    let execution_weak = Arc::downgrade(&execution);
    let ingress = Arc::new(EffectForbiddenIngressProvider);
    let ingress_weak = Arc::downgrade(&ingress);
    let saga_store = Arc::new(InjectedSagaStore::default());
    let saga_store_weak = Arc::downgrade(&saga_store);
    let composition = ServerWorkloadComposition::new(
        Arc::clone(&fixture.engine),
        Arc::clone(&fixture.manager),
        Arc::clone(&fixture.service_manager),
        NodeIdentity::new("server-composition-node").expect("fixture node should validate"),
        fixture.selection.clone(),
        sovereignty(),
        ServerWorkloadProviders::new(
            fixture.attachment_provider_id.clone(),
            attachment,
            fixture.execution_provider_id.clone(),
            execution,
            fixture.ingress_provider_id.clone(),
            ingress,
        ),
    )
    .expect("complete tracked fixture should compose");

    let runtime = composition.into_foreground_runtime(saga_store);
    drop(fixture);

    assert!(engine.upgrade().is_some());
    assert!(manager.upgrade().is_some());
    assert!(service_manager.upgrade().is_some());
    assert!(attachment_weak.upgrade().is_some());
    assert!(execution_weak.upgrade().is_some());
    assert!(ingress_weak.upgrade().is_some());
    assert!(saga_store_weak.upgrade().is_some());

    drop(runtime);

    assert!(engine.upgrade().is_none());
    assert!(manager.upgrade().is_none());
    assert!(service_manager.upgrade().is_none());
    assert!(attachment_weak.upgrade().is_none());
    assert!(execution_weak.upgrade().is_none());
    assert!(ingress_weak.upgrade().is_none());
    assert!(saga_store_weak.upgrade().is_none());
    drop(network_authority_guard);
}

#[test]
fn invalid_selection_or_crossed_provider_ids_reject_without_mutation() {
    let fixture = fixture();
    let before_engine = snapshot_regular_files(fixture.engine_root.path());
    let before_crossed_engine = snapshot_regular_files(fixture.crossed_engine_root.path());
    let before_network = snapshot_regular_files(fixture.network_root.path());
    let unknown_attachment = NetworkProviderId::for_registration_key("missing-attachment");
    let unknown_ingress = NetworkProviderId::for_registration_key("missing-ingress");

    let missing_selection = ServerWorkloadComposition::new(
        Arc::clone(&fixture.crossed_engine),
        Arc::clone(&fixture.manager),
        Arc::clone(&fixture.service_manager),
        NodeIdentity::new("rejected-node").expect("fixture node should validate"),
        NetworkCapabilitySelection::new(unknown_attachment.clone(), unknown_ingress.clone()),
        sovereignty(),
        providers(
            unknown_attachment,
            unknown_ingress,
            fixture.execution_provider_id.clone(),
        ),
    );
    let wrong_attachment = NetworkProviderId::for_registration_key("crossed-attachment");
    let attachment_mismatch = ServerWorkloadComposition::new(
        Arc::clone(&fixture.crossed_engine),
        Arc::clone(&fixture.manager),
        Arc::clone(&fixture.service_manager),
        NodeIdentity::new("rejected-node").expect("fixture node should validate"),
        fixture.selection.clone(),
        sovereignty(),
        providers(
            wrong_attachment,
            fixture.ingress_provider_id.clone(),
            fixture.execution_provider_id.clone(),
        ),
    );
    let ingress_mismatch = ServerWorkloadComposition::new(
        Arc::clone(&fixture.crossed_engine),
        Arc::clone(&fixture.manager),
        Arc::clone(&fixture.service_manager),
        NodeIdentity::new("rejected-node").expect("fixture node should validate"),
        fixture.selection.clone(),
        sovereignty(),
        providers(
            fixture.attachment_provider_id.clone(),
            NetworkProviderId::for_registration_key("crossed-ingress"),
            fixture.execution_provider_id.clone(),
        ),
    );
    enum ExpectedRejection {
        MissingSelection,
        AttachmentMismatch,
        IngressMismatch,
    }
    for (label, result, expected) in [
        (
            "missing selected reports",
            missing_selection,
            ExpectedRejection::MissingSelection,
        ),
        (
            "crossed attachment capability ID",
            attachment_mismatch,
            ExpectedRejection::AttachmentMismatch,
        ),
        (
            "crossed ingress capability ID",
            ingress_mismatch,
            ExpectedRejection::IngressMismatch,
        ),
    ] {
        let error = match result {
            Err(error) => error,
            Ok(_) => panic!("{label} must reject before composition"),
        };
        assert!(
            matches!(
                (expected, error),
                (
                    ExpectedRejection::MissingSelection,
                    ServerWorkloadCompositionError::MissingExactSelection { .. }
                ) | (
                    ExpectedRejection::AttachmentMismatch,
                    ServerWorkloadCompositionError::AttachmentProviderMismatch { .. }
                ) | (
                    ExpectedRejection::IngressMismatch,
                    ServerWorkloadCompositionError::IngressProviderMismatch { .. }
                )
            ),
            "{label} returned the wrong rejection"
        );
    }

    assert_eq!(
        snapshot_regular_files(fixture.engine_root.path()),
        before_engine
    );
    assert_eq!(
        snapshot_regular_files(fixture.crossed_engine_root.path()),
        before_crossed_engine
    );
    assert_eq!(
        snapshot_regular_files(fixture.network_root.path()),
        before_network
    );
}

#[test]
fn composition_rejects_selected_reports_that_violate_fixed_sovereignty() {
    let fixture = fixture_with_attachment_sovereignty(NetworkSovereigntyCapabilities::new(
        NetworkControlPlaneLocality::ThirdParty,
        [],
        false,
    ));
    let before_engine = snapshot_regular_files(fixture.engine_root.path());
    let before_network = snapshot_regular_files(fixture.network_root.path());

    let result = ServerWorkloadComposition::new(
        Arc::clone(&fixture.engine),
        Arc::clone(&fixture.manager),
        Arc::clone(&fixture.service_manager),
        NodeIdentity::new("rejected-sovereignty-node").expect("fixture node should validate"),
        fixture.selection.clone(),
        sovereignty(),
        providers(
            fixture.attachment_provider_id.clone(),
            fixture.ingress_provider_id.clone(),
            fixture.execution_provider_id.clone(),
        ),
    );
    assert!(matches!(
        result,
        Err(ServerWorkloadCompositionError::UnsatisfiedSovereignty(_))
    ));
    assert_eq!(
        snapshot_regular_files(fixture.engine_root.path()),
        before_engine
    );
    assert_eq!(
        snapshot_regular_files(fixture.network_root.path()),
        before_network
    );
}

#[test]
fn app_state_rejects_crossed_service_manager_before_authority_or_provider_effects() {
    let fixture = fixture();
    let crossed_service_manager = Arc::new(ServiceManager::new(
        Arc::new(EmptyServiceDefinitionCatalog),
        Arc::new(EffectForbiddenSandboxBackend),
    ));
    let before_engine = snapshot_regular_files(fixture.engine_root.path());
    let before_network = snapshot_regular_files(fixture.network_root.path());
    let refusal = catch_unwind(AssertUnwindSafe(|| {
        AppState::from_config(AppStateConfig {
            workload: ServerWorkloadProfile::managed(valid_composition(&fixture)),
            deployment: DeploymentConfig::default(),
            control_plane: ControlPlaneConfig::router_options_default(),
            node_services: NodeServicesConfig::default()
                .with_service_manager(crossed_service_manager),
            transport: TransportConfig::default(),
            runtime: RuntimeGovernorConfig::default(),
        });
    }));

    assert!(
        refusal.is_err(),
        "AppState must reject crossed source/projection and runtime services owners"
    );
    assert_eq!(
        snapshot_regular_files(fixture.engine_root.path()),
        before_engine
    );
    assert_eq!(
        snapshot_regular_files(fixture.network_root.path()),
        before_network
    );
}

#[test]
fn protocol_only_profile_owns_no_workload_authority() {
    let fixture = fixture();
    let state = AppState::from_config(AppStateConfig {
        workload: ServerWorkloadProfile::protocol_only(Arc::clone(&fixture.engine)),
        deployment: DeploymentConfig::default(),
        control_plane: ControlPlaneConfig::router_options_default(),
        node_services: NodeServicesConfig::default(),
        transport: TransportConfig::default(),
        runtime: RuntimeGovernorConfig::default(),
    });

    assert!(state.network_manager().is_none());
    assert!(state.workload_saga_coordinator().is_none());
    assert!(state.workload_provisioner().is_none());
}

#[test]
fn provider_ids_used_by_report_selection_and_capabilities_are_exact() {
    let fixture = fixture();
    assert_eq!(
        fixture.selection.ingress_provider_id(),
        &nimbus_owned_local_ingress_provider_id()
    );
    let composition = valid_composition(&fixture);
    assert!(Arc::ptr_eq(
        &composition.network_manager(),
        &fixture.manager
    ));
    // The generic constructor above compiles only because attachment,
    // execution, and ingress providers each implement every effect and
    // observation role required by their narrow registration.
}

#[test]
fn honest_server_bundle_satisfies_the_exact_compiled_http_workload_plan() {
    let fixture = fixture();
    let tenant_id = TenantId::new("composition-tenant").expect("fixture tenant should validate");
    let decision = TenantIsolationContext::system(tenant_id.clone(), "server-composition-test")
        .with_deployment_generation(7)
        .with_workload_location(WorkloadLocation::new().with_node_id("composition-node"))
        .admit_decision(
            TenantIsolationPolicyInput::new(
                WorkloadAttributes::sandbox("python")
                    .with_sandbox_id("composition-sandbox")
                    .with_sandbox_backend(SandboxBackendKind::Krun),
            )
            .with_services(TenantServiceGrantPolicyDecision::new(["upstream"]))
            .with_network(TenantNetworkPolicyDecision::new([])),
        )
        .expect("fixture workload should admit");
    let sandbox_spec = SandboxSpec::new(
        tenant_id,
        SandboxOwnerSpec::standalone_named("python"),
        SandboxBackendKind::Krun,
        SandboxRootSpec::rootfs("/fixture/rootfs"),
        SandboxProcessSpec::new(["/bin/true"]),
    )
    .with_port_bindings([SandboxPortBinding::new(
        "http",
        EndpointProtocol::Http,
        0,
        8080,
    )]);

    WorkloadNetworkPlanCompiler
        .compile(
            &decision,
            AdmittedWorkloadNetworkSource::Sandbox {
                stable_resource_id: "composition-sandbox",
                profile: "python",
                generation: 7,
                sandbox_spec: &sandbox_spec,
            },
            Some(&fixture.selection),
            fixture.manager.capability_registry(),
            sovereignty(),
            &[WorkloadNetworkEndpointSemanticsInput::new(
                "http",
                nimbus_workloads::WorkloadNetworkForwardingBehavior::PortForwarded,
                NetworkTlsBehavior::Disabled,
            )],
            WorkloadActivationIntent::ActivateWhenAttached,
            WorkloadPublicationIntent::PublishWhenReady,
        )
        .expect(
            "the exact honest attachment/ingress bundle must satisfy one real compiled HTTP plan",
        );
}

#[test]
fn managed_router_and_serve_options_consume_only_complete_effect_free_bundles() {
    let fixture = fixture();
    let before_engine = snapshot_regular_files(fixture.engine_root.path());
    let before_network = snapshot_regular_files(fixture.network_root.path());

    {
        let _router_options = crate::RouterOptions::managed(valid_composition(&fixture));
        let _serve_options = crate::ServeOptions::managed(valid_composition(&fixture));
    }

    assert_eq!(
        snapshot_regular_files(fixture.engine_root.path()),
        before_engine
    );
    assert_eq!(
        snapshot_regular_files(fixture.network_root.path()),
        before_network
    );
}
