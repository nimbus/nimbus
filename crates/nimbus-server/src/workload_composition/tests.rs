use std::collections::BTreeMap;
use std::fs;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use nimbus_compute::config::control_plane::ControlPlaneConfig;
use nimbus_compute::config::deployment::DeploymentConfig;
use nimbus_compute::config::node_services::NodeServicesConfig;
use nimbus_compute::config::runtime::RuntimeGovernorConfig;
use nimbus_compute::workload_network_plan::{
    AdmittedWorkloadNetworkSource, WorkloadNetworkEndpointSemanticsInput,
    WorkloadNetworkPlanCompiler,
};
use nimbus_compute::workload_saga::{
    ConfirmedWorkloadProvisionCommand, ConfirmedWorkloadRestartCommand,
    ConfirmedWorkloadTeardownCommand, FinalIngressWithdrawalCapability,
    IngressTeardownCapabilities, NetworkAttachmentTeardownCapabilities,
    NetworkDetachmentCapability, NetworkReleaseCapability, WorkloadExecutionDrainCapability,
    WorkloadExecutionStopCapability, WorkloadExecutionTeardownCapabilities,
    WorkloadProvisionCapabilityFuture, WorkloadRestartCapabilityFuture,
    WorkloadTeardownCapabilityFuture,
};
use nimbus_core::{TenantId, WorkloadId};
use nimbus_network::{
    EndpointProtocol, NetworkAddressFamily, NetworkAttachmentCapabilitySet,
    NetworkAttachmentProviderRegistration, NetworkCapabilityBundle, NetworkCapabilityRegistry,
    NetworkCapabilityRequirements, NetworkControlPlaneLocality, NetworkEndpointCapabilitySet,
    NetworkForwardingCapabilitySet, NetworkIngressCapabilitySet, NetworkLifecycleCapabilitySet,
    NetworkLifecycleFeature, NetworkLifecycleRequirements, NetworkManagementMode,
    NetworkResourceGeneration, NetworkSovereigntyCapabilities, NetworkTlsBehavior,
};
use nimbus_process_harness::{
    ProcessRoleSpec, SubprocessCrashCutHarness, run_crash_cut_child, run_crash_recovery_child,
};
use nimbus_sandbox::{
    SandboxBackendKind, SandboxOwnerSpec, SandboxPortBinding, SandboxProcessSpec, SandboxRootSpec,
    SandboxSpec, sandbox_network_plan_requirements,
};
use nimbus_services::{EmptyServiceDefinitionCatalog, ServiceManager};
use nimbus_tenant::{
    TenantIsolationContext, TenantIsolationPolicyInput, TenantNetworkPolicyDecision,
    TenantServiceGrantPolicyDecision, WorkloadAttributes, WorkloadLocation,
};
use nimbus_workloads::{
    CompiledWorkloadNetworkPlan, DesiredWorkloadKind, DesiredWorkloadState, NodeIdentity,
    ProposedWorkloadTeardownTransition, TenantRetirementExpected, TenantRetirementPhase,
    TenantRetirementRecord, TenantRetirementSource, TenantRetirementStore, TenantWorkloadSpec,
    WorkloadActivationIntent, WorkloadAdmissionEvidence, WorkloadGeneration, WorkloadNetworkIntent,
    WorkloadNetworkPlanContent, WorkloadNetworkPlanIdentity, WorkloadProvisionSourceEvidence,
    WorkloadProvisionSourceGeneration, WorkloadProvisionSourceIdentity,
    WorkloadProvisionSourceResourceVersion, WorkloadPublicationIntent, WorkloadSagaExpected,
    WorkloadSagaIntent, WorkloadSagaIntentUpdate, WorkloadSagaKey, WorkloadSagaPhase,
    WorkloadSagaRecord, WorkloadSagaStore, WorkloadTeardownDecision,
};

use super::*;
use crate::config::transport::TransportConfig;
use crate::network_capabilities::{
    nimbus_owned_local_ingress_provider_id, nimbus_owned_workload_ingress_registration,
};
use crate::state::{AppState, AppStateConfig};

const TENANT_RECOVERY_CHILD_TEST: &str =
    "workload_composition::tests::tenant_retirement_fresh_process_child";
const TENANT_RECOVERY_MODE_ENV: &str = "NIMBUS_NNC61E2_TENANT_MODE";
const TENANT_RECOVERY_CUT_ENV: &str = "NIMBUS_NNC61E2_TENANT_CUT";
const TENANT_RECOVERY_WRITE_MODE: &str = "write";
const TENANT_RECOVERY_READ_MODE: &str = "recover";
const TENANT_REPLACEMENT_WRITE_MODE: &str = "write-replacement";
const TENANT_REPLACEMENT_READ_MODE: &str = "recover-replacement";
const TENANT_REPLACEMENT_BOUNDARY: &str = "replacement-incarnation";
const TENANT_REPLACEMENT_OBSERVATION: &str = "replacement-rejected-without-fence";
const TENANT_RECOVERY_PID_PREFIX: &str = "NIMBUS_NNC61E2_TENANT_PID";
const TENANT_RECOVERY_TIMEOUT: Duration = Duration::from_secs(20);
const TENANT_RECOVERY_ID: &str = "tenant-retirement-process";
const TENANT_RECOVERY_SOURCE_ID: &str = "tenant-retirement-child";
const TENANT_RECOVERY_SOURCE_PROFILE: &str = "recovery-profile";
const TENANT_RECOVERY_SOURCE_VERSION: &str = "recovery-source-v1";
const TENANT_RECOVERY_NODE: &str = "tenant-retirement-process-node";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TenantRecoveryCut {
    IntentCommitted,
    ChildrenRecorded,
    SourcesFinalized,
    EngineDeleted,
    Recorded,
}

impl TenantRecoveryCut {
    const ALL: [Self; 5] = [
        Self::IntentCommitted,
        Self::ChildrenRecorded,
        Self::SourcesFinalized,
        Self::EngineDeleted,
        Self::Recorded,
    ];

    const fn name(self) -> &'static str {
        match self {
            Self::IntentCommitted => "intent-committed",
            Self::ChildrenRecorded => "children-recorded",
            Self::SourcesFinalized => "sources-finalized",
            Self::EngineDeleted => "engine-deleted",
            Self::Recorded => "recorded",
        }
    }

    const fn phase(self) -> TenantRetirementPhase {
        match self {
            Self::IntentCommitted => TenantRetirementPhase::IntentCommitted,
            Self::ChildrenRecorded => TenantRetirementPhase::ChildrenRecorded,
            Self::SourcesFinalized => TenantRetirementPhase::SourcesFinalized,
            Self::EngineDeleted => TenantRetirementPhase::EngineDeleted,
            Self::Recorded => TenantRetirementPhase::Recorded,
        }
    }

    fn parse(value: &str) -> Result<Self, String> {
        Self::ALL
            .into_iter()
            .find(|cut| cut.name() == value)
            .ok_or_else(|| format!("unknown tenant-retirement process cut {value:?}"))
    }

    fn observation(self) -> String {
        format!("tenant-retirement-recovered:{}", self.name())
    }
}

struct EffectForbiddenAttachmentProvider;

macro_rules! effect_restart_capability {
    ($provider:ty, $trait_name:ident) => {
        impl $trait_name for $provider {
            fn execute(
                &self,
                _command: &ConfirmedWorkloadRestartCommand,
            ) -> WorkloadRestartCapabilityFuture<'_> {
                panic!("composition must not execute a restart provider effect")
            }

            fn inspect(
                &self,
                _command: &ConfirmedWorkloadRestartCommand,
            ) -> WorkloadRestartCapabilityFuture<'_> {
                panic!("composition must not inspect restart provider state")
            }
        }
    };
}

macro_rules! inspect_restart_capability {
    ($provider:ty, $trait_name:ident) => {
        impl $trait_name for $provider {
            fn inspect(
                &self,
                _command: &ConfirmedWorkloadRestartCommand,
            ) -> WorkloadRestartCapabilityFuture<'_> {
                panic!("composition must not inspect restart provider state")
            }
        }
    };
}

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

macro_rules! effect_teardown_capability {
    ($provider:ty, $trait_name:ident) => {
        impl $trait_name for $provider {
            fn execute<'a>(
                &'a self,
                _command: &'a ConfirmedWorkloadTeardownCommand,
            ) -> WorkloadTeardownCapabilityFuture<'a> {
                panic!("composition must not execute a teardown provider effect")
            }

            fn inspect<'a>(
                &'a self,
                _command: &'a ConfirmedWorkloadTeardownCommand,
            ) -> WorkloadTeardownCapabilityFuture<'a> {
                panic!("composition must not inspect teardown provider state")
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
effect_restart_capability!(
    EffectForbiddenAttachmentProvider,
    NetworkRestartAttachmentCapability
);
effect_teardown_capability!(
    EffectForbiddenAttachmentProvider,
    NetworkDetachmentCapability
);
effect_teardown_capability!(EffectForbiddenAttachmentProvider, NetworkReleaseCapability);

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

effect_restart_capability!(
    EffectForbiddenExecutionProvider,
    WorkloadExecutionQuiescenceCapability
);
effect_restart_capability!(
    EffectForbiddenExecutionProvider,
    WorkloadRestartPreparationCapability
);
inspect_restart_capability!(
    EffectForbiddenExecutionProvider,
    WorkloadRestartActivationPrerequisiteCapability
);
effect_restart_capability!(
    EffectForbiddenExecutionProvider,
    WorkloadRestartActivationCapability
);
inspect_restart_capability!(
    EffectForbiddenExecutionProvider,
    WorkloadRestartReadinessCapability
);
effect_teardown_capability!(
    EffectForbiddenExecutionProvider,
    WorkloadExecutionDrainCapability
);
effect_teardown_capability!(
    EffectForbiddenExecutionProvider,
    WorkloadExecutionStopCapability
);

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

effect_restart_capability!(
    EffectForbiddenIngressProvider,
    RestartPublicationWithdrawalCapability
);
effect_restart_capability!(EffectForbiddenIngressProvider, RestartPublicationCapability);
inspect_restart_capability!(
    EffectForbiddenIngressProvider,
    RestartPublicationObservationCapability
);
effect_teardown_capability!(
    EffectForbiddenIngressProvider,
    FinalIngressWithdrawalCapability
);

#[derive(Default)]
struct InjectedSagaStore {
    loads: AtomicUsize,
    recovery_reads: AtomicUsize,
    restart_reads: AtomicUsize,
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
        request: nimbus_workloads::WorkloadSagaPageRequest,
    ) -> nimbus_workloads::WorkloadSagaFuture<'a, nimbus_workloads::WorkloadSagaPage> {
        Box::pin(async move {
            self.recovery_reads.fetch_add(1, Ordering::AcqRel);
            nimbus_workloads::WorkloadSagaPage::new(&request, Vec::new(), false)
        })
    }

    fn list_restart_candidates<'a>(
        &'a self,
        request: nimbus_workloads::WorkloadRestartCandidatePageRequest,
    ) -> nimbus_workloads::WorkloadSagaFuture<'a, nimbus_workloads::WorkloadRestartCandidatePage>
    {
        Box::pin(async move {
            self.restart_reads.fetch_add(1, Ordering::AcqRel);
            nimbus_workloads::WorkloadRestartCandidatePage::new(&request, Vec::new(), false)
        })
    }

    fn list_for_tenant<'a>(
        &'a self,
        _tenant_id: &'a TenantId,
        _request: nimbus_workloads::WorkloadSagaTenantPageRequest,
    ) -> nimbus_workloads::WorkloadSagaFuture<'a, nimbus_workloads::WorkloadSagaTenantPage> {
        Box::pin(async move { panic!("composition must not list tenant workload sagas") })
    }
}

impl TenantRetirementStore for InjectedSagaStore {
    fn load_retirement<'a>(
        &'a self,
        _tenant_id: &'a TenantId,
    ) -> nimbus_workloads::TenantRetirementFuture<
        'a,
        Option<nimbus_workloads::TenantRetirementRecord>,
    > {
        Box::pin(async { Ok(None) })
    }

    fn compare_and_swap_retirement<'a>(
        &'a self,
        _expected: nimbus_workloads::TenantRetirementExpected,
        _next: nimbus_workloads::TenantRetirementRecord,
    ) -> nimbus_workloads::TenantRetirementFuture<'a, nimbus_workloads::TenantRetirementCommit>
    {
        Box::pin(async { panic!("composition must not commit tenant retirement state") })
    }

    fn delete_retirement<'a>(
        &'a self,
        _expected: nimbus_workloads::TenantRetirementRecord,
    ) -> nimbus_workloads::TenantRetirementFuture<'a, nimbus_workloads::TenantRetirementCommit>
    {
        Box::pin(async { panic!("composition must not delete tenant retirement state") })
    }

    fn list_active_retirements<'a>(
        &'a self,
        request: nimbus_workloads::TenantRetirementPageRequest,
    ) -> nimbus_workloads::TenantRetirementFuture<'a, nimbus_workloads::TenantRetirementPage> {
        Box::pin(async move {
            nimbus_workloads::TenantRetirementPage::active(&request, Vec::new(), false)
        })
    }

    fn list_retirements<'a>(
        &'a self,
        request: nimbus_workloads::TenantRetirementPageRequest,
    ) -> nimbus_workloads::TenantRetirementFuture<'a, nimbus_workloads::TenantRetirementPage> {
        Box::pin(async move {
            nimbus_workloads::TenantRetirementPage::retained(&request, Vec::new(), false)
        })
    }

    fn load_workload_mutation_epoch<'a>(
        &'a self,
        _tenant_id: &'a TenantId,
    ) -> nimbus_workloads::TenantRetirementFuture<'a, nimbus_workloads::TenantWorkloadMutationEpoch>
    {
        Box::pin(async { Ok(nimbus_workloads::TenantWorkloadMutationEpoch::new(0)) })
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
        SandboxBackendKind::Krun,
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
    .with_restart_capabilities()
}

fn teardown_capabilities(
    attachment_provider_id: NetworkProviderId,
    ingress_provider_id: NetworkProviderId,
    execution_provider_id: WorkloadExecutionProviderId,
) -> WorkloadTeardownCapabilityRegistry {
    let attachment = Arc::new(EffectForbiddenAttachmentProvider);
    let execution = Arc::new(EffectForbiddenExecutionProvider);
    let ingress = Arc::new(EffectForbiddenIngressProvider);
    WorkloadTeardownCapabilityRegistry::new(
        [NetworkAttachmentTeardownCapabilities::new(
            attachment_provider_id,
            attachment.clone(),
            attachment,
        )],
        [WorkloadExecutionTeardownCapabilities::new(
            execution_provider_id,
            execution.clone(),
            execution,
        )],
        [IngressTeardownCapabilities::new(
            ingress_provider_id,
            ingress,
        )],
    )
    .expect("one exact effect-forbidden teardown realm should validate")
}

fn providers_with_teardown(
    attachment_provider_id: NetworkProviderId,
    ingress_provider_id: NetworkProviderId,
    execution_provider_id: WorkloadExecutionProviderId,
) -> TestProviders {
    providers(
        attachment_provider_id.clone(),
        ingress_provider_id.clone(),
        execution_provider_id.clone(),
    )
    .with_teardown_capabilities(teardown_capabilities(
        attachment_provider_id,
        ingress_provider_id,
        execution_provider_id,
    ))
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
        providers_with_teardown(
            fixture.attachment_provider_id.clone(),
            fixture.ingress_provider_id.clone(),
            fixture.execution_provider_id.clone(),
        ),
    )
    .expect("complete exact fixture should compose")
}

fn valid_teardown_composition(fixture: &Fixture) -> ServerWorkloadComposition {
    valid_composition(fixture)
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

    let runtime = valid_composition(&fixture)
        .into_foreground_runtime(injected_store.clone())
        .await
        .expect("foreground startup recovery should complete");

    assert!(Arc::ptr_eq(&runtime._compute.engine, &fixture.engine));
    assert_eq!(injected_store.loads.load(Ordering::Acquire), 0);
    assert_eq!(injected_store.recovery_reads.load(Ordering::Acquire), 1);
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
fn managed_composition_without_exact_teardown_realm_rejects_before_authority_or_effects() {
    let fixture = fixture();
    let before_engine = snapshot_regular_files(fixture.engine_root.path());
    let before_network = snapshot_regular_files(fixture.network_root.path());
    let error = match ServerWorkloadComposition::new(
        Arc::clone(&fixture.engine),
        Arc::clone(&fixture.manager),
        Arc::clone(&fixture.service_manager),
        NodeIdentity::new("missing-teardown-node").expect("fixture node should validate"),
        fixture.selection.clone(),
        sovereignty(),
        providers(
            fixture.attachment_provider_id.clone(),
            fixture.ingress_provider_id.clone(),
            fixture.execution_provider_id.clone(),
        ),
    ) {
        Ok(_) => panic!("a provision-only managed realm must fail before construction"),
        Err(error) => error,
    };

    assert!(matches!(
        error,
        ServerWorkloadCompositionError::MissingExactTeardownCapabilityRealm
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

#[tokio::test]
async fn foreground_retirement_facade_resolves_exact_realm_without_effects() {
    let fixture = fixture();
    let before_engine = snapshot_regular_files(fixture.engine_root.path());
    let before_network = snapshot_regular_files(fixture.network_root.path());
    let injected_store = Arc::new(InjectedSagaStore::default());
    let runtime = valid_teardown_composition(&fixture)
        .into_foreground_runtime(injected_store.clone())
        .await
        .expect("foreground startup recovery should complete");

    let _first = runtime
        .resource_retirer()
        .expect("the exact foreground realm should expose retirement authority");
    let _second = runtime
        .resource_retirer()
        .expect("repeated resolution should reuse the same compute-owned authorities");

    assert_eq!(injected_store.loads.load(Ordering::Acquire), 0);
    assert_eq!(injected_store.recovery_reads.load(Ordering::Acquire), 1);
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
fn crossed_teardown_execution_rejects_before_runtime_or_effects() {
    let fixture = fixture();
    let before_engine = snapshot_regular_files(fixture.engine_root.path());
    let before_network = snapshot_regular_files(fixture.network_root.path());
    let crossed_execution =
        WorkloadExecutionProviderId::for_registration_key("crossed-server-teardown-execution");
    let providers = providers(
        fixture.attachment_provider_id.clone(),
        fixture.ingress_provider_id.clone(),
        fixture.execution_provider_id.clone(),
    )
    .with_teardown_capabilities(teardown_capabilities(
        fixture.attachment_provider_id.clone(),
        fixture.ingress_provider_id.clone(),
        crossed_execution,
    ));

    let error = match ServerWorkloadComposition::new(
        Arc::clone(&fixture.engine),
        Arc::clone(&fixture.manager),
        Arc::clone(&fixture.service_manager),
        NodeIdentity::new("server-composition-node").expect("fixture node should validate"),
        fixture.selection.clone(),
        sovereignty(),
        providers,
    ) {
        Ok(_) => panic!("a crossed teardown execution realm must fail before runtime construction"),
        Err(error) => error,
    };

    assert!(matches!(
        error,
        ServerWorkloadCompositionError::TeardownCapabilityRegistry(
            WorkloadTeardownCapabilityRegistryError::IncompleteExactRealm { .. }
        )
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

#[tokio::test]
async fn foreground_runtime_owns_exact_manager_services_and_provider_arc_lifetimes() {
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
        )
        .with_restart_capabilities()
        .with_teardown_capabilities(teardown_capabilities(
            fixture.attachment_provider_id.clone(),
            fixture.ingress_provider_id.clone(),
            fixture.execution_provider_id.clone(),
        )),
    )
    .expect("complete tracked fixture should compose");

    let runtime = composition
        .into_foreground_runtime(saga_store)
        .await
        .expect("foreground startup recovery should complete");
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
        SandboxBackendKind::Krun,
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

#[test]
fn managed_server_readiness_converges_retirement_before_returning_transport_state() {
    std::thread::Builder::new()
        .name("server-readiness-retirement".to_owned())
        .stack_size(32 * 1024 * 1024)
        .spawn(|| {
            tenant_recovery_runtime()
                .expect("server readiness runtime should build")
                .block_on(async {
                    let fixture = fixture();
                    let tenant_id = TenantId::new("server-readiness-retirement")
                        .expect("server readiness tenant should validate");
                    fixture
                        .engine
                        .create_tenant_async(tenant_id.clone())
                        .await
                        .expect("server readiness tenant should exist");
                    let identity = fixture
                        .engine
                        .enter_tenant_runtime_async(tenant_id.clone())
                        .await
                        .expect("server readiness identity should load");
                    let record = TenantRetirementRecord::new(
                        tenant_id.clone(),
                        identity.tenant_incarnation(),
                        Vec::new(),
                    )
                    .expect("server readiness retirement should validate");
                    drop(identity);
                    let store = EngineWorkloadSagaStore::new(Arc::clone(&fixture.engine));
                    store
                        .compare_and_swap_retirement(TenantRetirementExpected::Missing, record)
                        .await
                        .expect("server readiness retirement should persist");

                    let _prepared = crate::RouterOptions::managed(valid_composition(&fixture))
                        .into_build_config()
                        .prepare_for_serving()
                        .await
                        .expect("server readiness should converge durable retirement");

                    assert!(
                        store
                            .load_retirement(&tenant_id)
                            .await
                            .expect("server readiness retirement should reload")
                            .is_none(),
                        "transport state cannot return before terminal retirement cleanup"
                    );
                    assert!(
                        !fixture
                            .engine
                            .list_tenants_async()
                            .await
                            .expect("server readiness tenants should list")
                            .contains(&tenant_id),
                        "transport state cannot return while the retired tenant remains"
                    );
                });
        })
        .expect("server readiness thread should start")
        .join()
        .expect("server readiness thread should finish");
}

#[test]
fn fresh_process_recovers_every_tenant_retirement_phase_from_durable_roots_only() {
    for cut in TenantRecoveryCut::ALL {
        let root = tempfile::tempdir().expect("tenant recovery root should build");
        let result = SubprocessCrashCutHarness::new(TENANT_RECOVERY_TIMEOUT)
            .run(
                root.path(),
                cut.name(),
                &cut.observation(),
                tenant_recovery_child(
                    &format!("{}-writer", cut.name()),
                    TENANT_RECOVERY_WRITE_MODE,
                    cut,
                ),
                tenant_recovery_child(
                    &format!("{}-recovery", cut.name()),
                    TENANT_RECOVERY_READ_MODE,
                    cut,
                ),
            )
            .unwrap_or_else(|error| {
                panic!("{} tenant-retirement recovery failed: {error}", cut.name())
            });

        assert_eq!(result.boundary(), cut.name());
        assert_eq!(result.observation(), cut.observation());
        assert_eq!(
            result.crash_diagnostic().cleanup(),
            "killed-at-boundary-and-reaped"
        );
        assert_eq!(result.crash_diagnostic().successful(), Some(false));
        assert_eq!(result.recovery_diagnostic().successful(), Some(true));
        assert_ne!(
            tenant_recovery_pid(result.crash_diagnostic().stderr(), "writer"),
            tenant_recovery_pid(result.recovery_diagnostic().stderr(), "recovery"),
            "{} must recover in a distinct process",
            cut.name()
        );
    }
}

#[test]
fn fresh_process_rejects_retired_incarnation_without_fencing_replacement() {
    let root = tempfile::tempdir().expect("tenant replacement root should build");
    let cut = TenantRecoveryCut::SourcesFinalized;
    let result = SubprocessCrashCutHarness::new(TENANT_RECOVERY_TIMEOUT)
        .run(
            root.path(),
            TENANT_REPLACEMENT_BOUNDARY,
            TENANT_REPLACEMENT_OBSERVATION,
            tenant_recovery_child("replacement-writer", TENANT_REPLACEMENT_WRITE_MODE, cut),
            tenant_recovery_child("replacement-recovery", TENANT_REPLACEMENT_READ_MODE, cut),
        )
        .expect("fresh recovery should reject the old retirement identity safely");

    assert_eq!(result.boundary(), TENANT_REPLACEMENT_BOUNDARY);
    assert_eq!(result.observation(), TENANT_REPLACEMENT_OBSERVATION);
    assert_eq!(
        result.crash_diagnostic().cleanup(),
        "killed-at-boundary-and-reaped"
    );
    assert_eq!(result.crash_diagnostic().successful(), Some(false));
    assert_eq!(result.recovery_diagnostic().successful(), Some(true));
    assert_ne!(
        tenant_recovery_pid(result.crash_diagnostic().stderr(), "writer"),
        tenant_recovery_pid(result.recovery_diagnostic().stderr(), "recovery")
    );
}

#[test]
#[ignore = "spawned only by the tenant-retirement fresh-process parent"]
fn tenant_retirement_fresh_process_child() {
    let mode = std::env::var(TENANT_RECOVERY_MODE_ENV)
        .expect("tenant-retirement child mode should be set");
    let cut = TenantRecoveryCut::parse(
        &std::env::var(TENANT_RECOVERY_CUT_ENV).expect("tenant-retirement child cut should be set"),
    )
    .expect("tenant-retirement child cut should validate");
    std::thread::Builder::new()
        .name("tenant-retirement-fresh-process".to_owned())
        .stack_size(32 * 1024 * 1024)
        .spawn(move || run_tenant_retirement_child(&mode, cut))
        .expect("tenant-retirement child thread should start")
        .join()
        .expect("tenant-retirement child thread should finish");
}

fn run_tenant_retirement_child(mode: &str, cut: TenantRecoveryCut) {
    match mode {
        TENANT_RECOVERY_WRITE_MODE => run_crash_cut_child(|context| {
            eprintln!("{TENANT_RECOVERY_PID_PREFIX} writer {}", std::process::id());
            tenant_recovery_runtime()?
                .block_on(write_tenant_retirement_cut(context.state_root(), cut))?;
            context.reach_boundary(cut.name())
        })
        .unwrap_or_else(|error| panic!("tenant-retirement writer failed: {error}")),
        TENANT_RECOVERY_READ_MODE => run_crash_recovery_child(|context| {
            eprintln!(
                "{TENANT_RECOVERY_PID_PREFIX} recovery {}",
                std::process::id()
            );
            tenant_recovery_runtime()?
                .block_on(recover_tenant_retirement_cut(context.state_root(), cut))
        })
        .unwrap_or_else(|error| panic!("tenant-retirement recovery failed: {error}")),
        TENANT_REPLACEMENT_WRITE_MODE => run_crash_cut_child(|context| {
            eprintln!("{TENANT_RECOVERY_PID_PREFIX} writer {}", std::process::id());
            tenant_recovery_runtime()?
                .block_on(write_tenant_replacement_cut(context.state_root()))?;
            context.reach_boundary(TENANT_REPLACEMENT_BOUNDARY)
        })
        .unwrap_or_else(|error| panic!("tenant-replacement writer failed: {error}")),
        TENANT_REPLACEMENT_READ_MODE => run_crash_recovery_child(|context| {
            eprintln!(
                "{TENANT_RECOVERY_PID_PREFIX} recovery {}",
                std::process::id()
            );
            tenant_recovery_runtime()?
                .block_on(recover_tenant_replacement_cut(context.state_root()))
        })
        .unwrap_or_else(|error| panic!("tenant-replacement recovery failed: {error}")),
        unknown => panic!("unknown tenant-retirement child mode {unknown:?}"),
    }
}

fn tenant_recovery_child(role: &str, mode: &str, cut: TenantRecoveryCut) -> ProcessRoleSpec {
    ProcessRoleSpec::new(
        role,
        std::env::current_exe().expect("current test executable should resolve"),
    )
    .arg("--exact")
    .arg(TENANT_RECOVERY_CHILD_TEST)
    .arg("--ignored")
    .arg("--nocapture")
    .env(TENANT_RECOVERY_MODE_ENV, mode)
    .env(TENANT_RECOVERY_CUT_ENV, cut.name())
}

fn tenant_recovery_runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|error| format!("tenant-retirement process runtime failed: {error}"))
}

async fn write_tenant_retirement_cut(root: &Path, cut: TenantRecoveryCut) -> Result<(), String> {
    let engine = Arc::new(
        Engine::new(root.join("engine"))
            .map_err(|error| format!("tenant-retirement writer Engine failed: {error}"))?,
    );
    let tenant_id = TenantId::new(TENANT_RECOVERY_ID)
        .map_err(|error| format!("tenant-retirement tenant ID failed: {error}"))?;
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .map_err(|error| format!("tenant-retirement writer tenant create failed: {error}"))?;
    let identity = engine
        .enter_tenant_runtime_async(tenant_id.clone())
        .await
        .map_err(|error| format!("tenant-retirement writer identity failed: {error}"))?;
    let store = EngineWorkloadSagaStore::new(Arc::clone(&engine));
    let (source, child_history, recorded_child, promoted_child) =
        tenant_recovery_child_records(&tenant_id)?;
    let mut previous_child = None;
    for child in &child_history {
        let expected = previous_child.as_ref().map_or(
            WorkloadSagaExpected::Missing,
            |previous: &WorkloadSagaRecord| WorkloadSagaExpected::Revision(previous.revision()),
        );
        store
            .compare_and_swap(expected, child.clone())
            .await
            .map_err(|error| format!("tenant-retirement writer child failed: {error}"))?;
        previous_child = Some(child.clone());
    }
    let released_child = child_history
        .last()
        .ok_or_else(|| "tenant-retirement writer child history is empty".to_owned())?;
    let mut record = TenantRetirementRecord::new(
        tenant_id.clone(),
        identity.tenant_incarnation(),
        vec![source],
    )
    .map_err(|error| format!("tenant-retirement writer record failed: {error}"))?;
    drop(identity);
    store
        .compare_and_swap_retirement(TenantRetirementExpected::Missing, record.clone())
        .await
        .map_err(|error| format!("tenant-retirement writer intent failed: {error}"))?;

    for phase in [
        TenantRetirementPhase::ChildrenRecorded,
        TenantRetirementPhase::SourcesFinalized,
        TenantRetirementPhase::EngineDeleted,
        TenantRetirementPhase::Recorded,
    ] {
        if record.phase() == cut.phase() {
            break;
        }
        if phase == TenantRetirementPhase::ChildrenRecorded {
            store
                .compare_and_swap(
                    WorkloadSagaExpected::Revision(released_child.revision()),
                    recorded_child.clone(),
                )
                .await
                .map_err(|error| {
                    format!("tenant-retirement writer terminal child failed: {error}")
                })?;
            store
                .compare_and_swap(
                    WorkloadSagaExpected::Revision(recorded_child.revision()),
                    promoted_child.clone(),
                )
                .await
                .map_err(|error| {
                    format!("tenant-retirement writer promoted child failed: {error}")
                })?;
        }
        if phase == TenantRetirementPhase::EngineDeleted {
            engine
                .delete_tenant_async(tenant_id.clone())
                .await
                .map_err(|error| format!("tenant-retirement writer delete failed: {error}"))?;
        }
        let next = record
            .advance(phase)
            .map_err(|error| format!("tenant-retirement writer advance failed: {error}"))?;
        store
            .compare_and_swap_retirement(
                TenantRetirementExpected::Revision(record.revision()),
                next.clone(),
            )
            .await
            .map_err(|error| format!("tenant-retirement writer phase failed: {error}"))?;
        record = next;
    }
    if record.phase() != cut.phase() {
        return Err(format!(
            "tenant-retirement writer stopped at {:?}, expected {:?}",
            record.phase(),
            cut.phase()
        ));
    }
    Ok(())
}

fn tenant_recovery_child_records(
    tenant_id: &TenantId,
) -> Result<
    (
        TenantRetirementSource,
        Vec<WorkloadSagaRecord>,
        WorkloadSagaRecord,
        WorkloadSagaRecord,
    ),
    String,
> {
    let decision = TenantIsolationContext::system(tenant_id.clone(), "tenant-retirement-recovery")
        .with_deployment_generation(1)
        .with_workload_location(WorkloadLocation::new().with_node_id(TENANT_RECOVERY_NODE))
        .admit_decision(TenantIsolationPolicyInput::new(
            WorkloadAttributes::sandbox(TENANT_RECOVERY_SOURCE_PROFILE)
                .with_sandbox_id(TENANT_RECOVERY_SOURCE_ID)
                .with_sandbox_backend(SandboxBackendKind::Krun),
        ))
        .map_err(|error| format!("tenant-retirement child admission failed: {error}"))?;
    let workload = TenantWorkloadSpec::from_decision(&decision)
        .map_err(|error| format!("tenant-retirement child projection failed: {error}"))?;
    let sandbox_spec = SandboxSpec::new(
        tenant_id.clone(),
        SandboxOwnerSpec::standalone_named(TENANT_RECOVERY_SOURCE_PROFILE),
        SandboxBackendKind::Krun,
        SandboxRootSpec::rootfs("/tenant-retirement/recovery-rootfs"),
        SandboxProcessSpec::new(["/bin/true"]),
    );
    let executable = nimbus_compute::workload_executable::encode_sandbox_spec(&sandbox_spec)
        .map_err(|error| format!("tenant-retirement child executable failed: {error}"))?;
    let source_identity = WorkloadProvisionSourceIdentity::standalone_sandbox(
        TENANT_RECOVERY_SOURCE_ID,
        TENANT_RECOVERY_SOURCE_PROFILE,
    )
    .map_err(|error| format!("tenant-retirement child source identity failed: {error}"))?;
    let source_generation = WorkloadProvisionSourceGeneration::new(1);
    let resource_version =
        WorkloadProvisionSourceResourceVersion::new(TENANT_RECOVERY_SOURCE_VERSION)
            .map_err(|error| format!("tenant-retirement child source version failed: {error}"))?;
    let attachment_provider_id = sandbox_network_plan_requirements(SandboxBackendKind::Krun)
        .required_attachment_provider_id()
        .clone();
    let execution_provider_id =
        WorkloadExecutionProviderId::for_registration_key("tenant-retirement-process-execution");
    let source = WorkloadProvisionSourceEvidence::standalone_sandbox(
        source_identity.clone(),
        source_generation,
        resource_version.clone(),
        executable.content_digest(),
        attachment_provider_id,
        execution_provider_id,
    )
    .map_err(|error| format!("tenant-retirement child source evidence failed: {error}"))?;
    let admission = WorkloadAdmissionEvidence::new(
        workload.decision_id().clone(),
        workload.workload_uid().clone(),
        NodeIdentity::new(TENANT_RECOVERY_NODE)
            .map_err(|error| format!("tenant-retirement child node failed: {error}"))?,
    );
    let running = WorkloadSagaIntent::new_without_automatic_restart(
        DesiredWorkloadKind::Sandbox,
        DesiredWorkloadState::Running,
        WorkloadGeneration::new(1),
        executable.clone(),
        source.clone(),
        tenant_recovery_network_intent(tenant_id, 1)?,
        WorkloadActivationIntent::PrepareOnly,
        WorkloadPublicationIntent::Withheld,
        admission.clone(),
    )
    .map_err(|error| format!("tenant-retirement running intent failed: {error}"))?;
    let stopped = WorkloadSagaIntent::new_without_automatic_restart(
        DesiredWorkloadKind::Sandbox,
        DesiredWorkloadState::Stopped,
        WorkloadGeneration::new(2),
        executable,
        source,
        tenant_recovery_network_intent(tenant_id, 2)?,
        WorkloadActivationIntent::PrepareOnly,
        WorkloadPublicationIntent::Withheld,
        admission,
    )
    .map_err(|error| format!("tenant-retirement stopped intent failed: {error}"))?;
    let key = WorkloadSagaKey::new(
        tenant_id.clone(),
        WorkloadId::new(TENANT_RECOVERY_SOURCE_ID)
            .map_err(|error| format!("tenant-retirement child key failed: {error}"))?,
    );
    let active = WorkloadSagaRecord::new(key, running)
        .map_err(|error| format!("tenant-retirement child record failed: {error}"))?;
    let mut history = vec![active.clone()];
    let WorkloadSagaIntentUpdate::Transition(released) = active
        .apply_intent(stopped)
        .map_err(|error| format!("tenant-retirement child withdrawal failed: {error}"))?
    else {
        return Err("tenant-retirement stopped intent did not commit withdrawal".to_owned());
    };
    let mut released = *released;
    history.push(released.clone());
    while released.phase() != WorkloadSagaPhase::NetworkReleased {
        let WorkloadTeardownDecision::PersistCandidate(
            ProposedWorkloadTeardownTransition::ResourceFree { step, .. },
        ) = released
            .decide_teardown()
            .map_err(|error| format!("tenant-retirement child teardown failed: {error}"))?
        else {
            return Err(format!(
                "tenant-retirement child phase {:?} was not resource free",
                released.phase()
            ));
        };
        released = released
            .record_resource_free_teardown_step(step)
            .map_err(|error| format!("tenant-retirement child step failed: {error}"))?;
        history.push(released.clone());
    }
    let recorded = released
        .record_terminal_teardown()
        .map_err(|error| format!("tenant-retirement child terminal failed: {error}"))?;
    let promoted = recorded
        .promote_successor()
        .map_err(|error| format!("tenant-retirement child promotion failed: {error}"))?;
    Ok((
        TenantRetirementSource::new(source_identity, source_generation, resource_version, true),
        history,
        recorded,
        promoted,
    ))
}

fn tenant_recovery_network_intent(
    tenant_id: &TenantId,
    generation: u64,
) -> Result<WorkloadNetworkIntent, String> {
    let identity = WorkloadNetworkPlanIdentity::new(
        tenant_id.clone(),
        "tenant-retirement-child-incarnation",
        NetworkResourceGeneration::new(generation),
    )
    .map_err(|error| format!("tenant-retirement child network identity failed: {error}"))?;
    let requirements = NetworkCapabilityRequirements::new(
        NetworkAttachmentCapabilitySet::new(NetworkManagementMode::NimbusHostManaged, [], []),
        NetworkEndpointCapabilitySet::new([], [], [], [], []),
        NetworkIngressCapabilitySet::new([]),
        NetworkForwardingCapabilitySet::new([]),
        NetworkLifecycleRequirements::new(
            NetworkLifecycleCapabilitySet::new([]),
            NetworkLifecycleCapabilitySet::new([]),
        ),
        sovereignty(),
    );
    let content = WorkloadNetworkPlanContent::new(
        identity,
        requirements,
        None,
        None,
        None,
        [],
        [],
        [],
        WorkloadActivationIntent::PrepareOnly,
        WorkloadPublicationIntent::Withheld,
    )
    .map_err(|error| format!("tenant-retirement child network content failed: {error}"))?;
    CompiledWorkloadNetworkPlan::from_content(content)
        .map(WorkloadNetworkIntent::new)
        .map_err(|error| format!("tenant-retirement child network plan failed: {error}"))
}

async fn recover_tenant_retirement_cut(
    root: &Path,
    cut: TenantRecoveryCut,
) -> Result<String, String> {
    let engine = Arc::new(
        Engine::new(root.join("engine"))
            .map_err(|error| format!("tenant-retirement recovery Engine failed: {error}"))?,
    );
    let requirements = sandbox_network_plan_requirements(SandboxBackendKind::Krun);
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
            .map_err(|error| format!("tenant-retirement provider reports failed: {error}"))?;
    let selection = NetworkCapabilitySelection::new(
        attachment_provider_id.clone(),
        ingress_provider_id.clone(),
    );
    let network_manager = LocalNetworkManager::bootstrap(root.join("network"))
        .map_err(|error| format!("tenant-retirement network bootstrap failed: {error}"))?
        .freeze(registry);
    let service_manager = Arc::new(ServiceManager::new(
        Arc::new(EmptyServiceDefinitionCatalog),
        SandboxBackendKind::Krun,
    ));
    let execution_provider_id =
        WorkloadExecutionProviderId::for_registration_key("tenant-retirement-process-execution");
    let composition = ServerWorkloadComposition::new(
        Arc::clone(&engine),
        Arc::clone(&network_manager),
        service_manager,
        NodeIdentity::new(TENANT_RECOVERY_NODE)
            .map_err(|error| format!("tenant-retirement node ID failed: {error}"))?,
        selection,
        sovereignty(),
        providers_with_teardown(
            attachment_provider_id,
            ingress_provider_id,
            execution_provider_id,
        ),
    )
    .map_err(|error| format!("tenant-retirement composition failed: {error}"))?;
    let store = Arc::new(EngineWorkloadSagaStore::new(Arc::clone(&engine)));
    let runtime = composition
        .into_foreground_runtime(Arc::clone(&store))
        .await
        .map_err(|error| format!("tenant-retirement startup recovery failed: {error}"))?;
    let report = runtime
        ._compute
        .prepare_workload_lifecycle()
        .await
        .map_err(|error| format!("tenant-retirement readiness replay failed: {error}"))?;
    if report.tenant_retirements() != 1 {
        return Err(format!(
            "tenant-retirement recovery reported {} records, expected 1",
            report.tenant_retirements()
        ));
    }
    let tenant_id = TenantId::new(TENANT_RECOVERY_ID)
        .map_err(|error| format!("tenant-retirement tenant ID failed: {error}"))?;
    let child_key = WorkloadSagaKey::new(
        tenant_id.clone(),
        WorkloadId::new(TENANT_RECOVERY_SOURCE_ID)
            .map_err(|error| format!("tenant-retirement child key failed: {error}"))?,
    );
    let child = store
        .load(&child_key)
        .await
        .map_err(|error| format!("tenant-retirement terminal child load failed: {error}"))?
        .ok_or_else(|| "tenant-retirement terminal child is missing".to_owned())?;
    let expected_identity = WorkloadProvisionSourceIdentity::standalone_sandbox(
        TENANT_RECOVERY_SOURCE_ID,
        TENANT_RECOVERY_SOURCE_PROFILE,
    )
    .map_err(|error| format!("tenant-retirement expected source failed: {error}"))?;
    if child.phase() != WorkloadSagaPhase::Recorded
        || child.active_intent().desired_state() != DesiredWorkloadState::Stopped
        || child.successor_intent().is_some()
        || child.active_intent().source().source_identity() != &expected_identity
        || child.active_intent().source().source_generation()
            != WorkloadProvisionSourceGeneration::new(1)
        || child.active_intent().source().resource_version().as_str()
            != TENANT_RECOVERY_SOURCE_VERSION
    {
        return Err("tenant-retirement terminal child lost exact durable source truth".to_owned());
    }
    if store
        .load_retirement(&tenant_id)
        .await
        .map_err(|error| format!("tenant-retirement terminal load failed: {error}"))?
        .is_some()
    {
        return Err("tenant-retirement terminal record was not deleted".to_owned());
    }
    if engine
        .list_tenants_async()
        .await
        .map_err(|error| format!("tenant-retirement tenant listing failed: {error}"))?
        .contains(&tenant_id)
    {
        return Err("tenant-retirement target Engine tenant still exists".to_owned());
    }
    Ok(cut.observation())
}

async fn write_tenant_replacement_cut(root: &Path) -> Result<(), String> {
    write_tenant_retirement_cut(root, TenantRecoveryCut::SourcesFinalized).await?;
    let engine = Arc::new(
        Engine::new(root.join("engine"))
            .map_err(|error| format!("tenant-replacement writer Engine failed: {error}"))?,
    );
    let tenant_id = TenantId::new(TENANT_RECOVERY_ID)
        .map_err(|error| format!("tenant-replacement tenant ID failed: {error}"))?;
    engine
        .delete_tenant_async(tenant_id.clone())
        .await
        .map_err(|error| format!("tenant-replacement old incarnation delete failed: {error}"))?;
    engine
        .create_tenant_async(tenant_id)
        .await
        .map_err(|error| format!("tenant-replacement create failed: {error}"))
}

async fn recover_tenant_replacement_cut(root: &Path) -> Result<String, String> {
    let engine = Arc::new(
        Engine::new(root.join("engine"))
            .map_err(|error| format!("tenant-replacement recovery Engine failed: {error}"))?,
    );
    let tenant_id = TenantId::new(TENANT_RECOVERY_ID)
        .map_err(|error| format!("tenant-replacement tenant ID failed: {error}"))?;
    let store = Arc::new(EngineWorkloadSagaStore::new(Arc::clone(&engine)));
    let retained = store
        .load_retirement(&tenant_id)
        .await
        .map_err(|error| format!("tenant-replacement retirement load failed: {error}"))?
        .ok_or_else(|| "tenant-replacement retirement record is missing".to_owned())?;
    let live_before = engine
        .enter_tenant_runtime_async(tenant_id.clone())
        .await
        .map_err(|error| format!("tenant-replacement live identity failed: {error}"))?;
    if live_before.tenant_incarnation() == retained.tenant_incarnation() {
        return Err("tenant-replacement writer did not create a new incarnation".to_owned());
    }
    drop(live_before);

    let requirements = sandbox_network_plan_requirements(SandboxBackendKind::Krun);
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
            .map_err(|error| format!("tenant-replacement provider reports failed: {error}"))?;
    let selection = NetworkCapabilitySelection::new(
        attachment_provider_id.clone(),
        ingress_provider_id.clone(),
    );
    let network_manager = LocalNetworkManager::bootstrap(root.join("network"))
        .map_err(|error| format!("tenant-replacement network bootstrap failed: {error}"))?
        .freeze(registry);
    let composition = ServerWorkloadComposition::new(
        Arc::clone(&engine),
        network_manager,
        Arc::new(ServiceManager::new(
            Arc::new(EmptyServiceDefinitionCatalog),
            SandboxBackendKind::Krun,
        )),
        NodeIdentity::new("tenant-replacement-process-node")
            .map_err(|error| format!("tenant-replacement node ID failed: {error}"))?,
        selection,
        sovereignty(),
        providers_with_teardown(
            attachment_provider_id,
            ingress_provider_id,
            WorkloadExecutionProviderId::for_registration_key(
                "tenant-replacement-process-execution",
            ),
        ),
    )
    .map_err(|error| format!("tenant-replacement composition failed: {error}"))?;
    let error = match composition
        .into_foreground_runtime(Arc::clone(&store))
        .await
    {
        Ok(_) => return Err("old retirement identity passed managed readiness".to_owned()),
        Err(error) => error,
    };
    if !error
        .to_string()
        .contains("does not match the expected deletion incarnation")
    {
        return Err(format!(
            "tenant-replacement recovery returned the wrong failure: {error}"
        ));
    }
    let live_after = engine
        .enter_tenant_runtime_async(tenant_id.clone())
        .await
        .map_err(|error| format!("tenant-replacement was incorrectly fenced: {error}"))?;
    if live_after.tenant_incarnation() == retained.tenant_incarnation() {
        return Err("tenant-replacement recovery crossed back to the old incarnation".to_owned());
    }
    drop(live_after);
    let observed = store
        .load_retirement(&tenant_id)
        .await
        .map_err(|error| format!("tenant-replacement terminal load failed: {error}"))?;
    if observed.as_ref() != Some(&retained) {
        return Err("tenant-replacement recovery mutated old retirement truth".to_owned());
    }
    Ok(TENANT_REPLACEMENT_OBSERVATION.to_owned())
}

fn tenant_recovery_pid(stderr: &str, role: &str) -> u32 {
    stderr
        .lines()
        .find_map(|line| {
            line.strip_prefix(&format!("{TENANT_RECOVERY_PID_PREFIX} {role} "))
                .and_then(|value| value.parse().ok())
        })
        .unwrap_or_else(|| panic!("missing {role} process ID in stderr:\n{stderr}"))
}
