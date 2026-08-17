use super::*;
use crate::compose::lifecycle::{
    ComposeForegroundOwner, ComposeProvisionFuture, ComposeServiceProvision,
    provision_compose_service,
};
use nimbus_compute::workload_saga::{
    ConfirmedWorkloadProvisionCommand, ConfirmedWorkloadTeardownCommand,
    FinalIngressWithdrawalCapability, IngressPublicationCapability,
    IngressPublicationInspectionCapability, IngressTeardownCapabilities,
    NetworkAttachmentCapability, NetworkAttachmentTeardownCapabilities,
    NetworkDetachmentCapability, NetworkReleaseCapability, NetworkReservationCapability,
    WorkloadActivationCapability, WorkloadActivationPrerequisiteCapability,
    WorkloadExecutionDrainCapability, WorkloadExecutionStopCapability,
    WorkloadExecutionTeardownCapabilities, WorkloadPreparationCapability,
    WorkloadProvisionCapabilityFuture, WorkloadReadinessCapability,
    WorkloadTeardownCapabilityFuture, WorkloadTeardownCapabilityRegistry,
};
use nimbus_compute::{
    SandboxServiceProvisionSnapshot, WorkloadExecutionObservationCapability,
    WorkloadIngressObservationCapability,
};
use nimbus_network::{
    LocalNetworkManager, NetworkAddressFamily, NetworkAttachmentProviderRegistration,
    NetworkCapabilityBundle, NetworkCapabilityRegistry, NetworkCapabilitySelection,
    NetworkControlPlaneLocality, NetworkLifecycleCapabilitySet, NetworkLifecycleFeature,
    NetworkSovereigntyCapabilities, NetworkSovereigntyRequirements,
};
use nimbus_server::{ServerWorkloadComposition, ServerWorkloadProviders};
use nimbus_services::{
    EmptyServiceDefinitionCatalog, ServiceDefinition, ServiceDefinitionObservation, ServiceManager,
};
use nimbus_tenant::TenantIsolationContext;
use nimbus_workloads::{
    NodeIdentity, TenantWorkloadUid, WorkloadDesiredDigest, WorkloadExecutionAttemptId,
    WorkloadExecutionId, WorkloadExecutionProviderId, WorkloadExecutionReference,
    WorkloadGeneration, WorkloadRestartEpoch,
};

struct ForegroundAttachmentProvider;

macro_rules! foreground_effect_capability {
    ($provider:ty, $capability:ident) => {
        impl $capability for $provider {
            fn execute<'a>(
                &'a self,
                _command: &'a ConfirmedWorkloadProvisionCommand,
            ) -> WorkloadProvisionCapabilityFuture<'a> {
                panic!("foreground ownership construction must not execute provider effects")
            }

            fn inspect<'a>(
                &'a self,
                _command: &'a ConfirmedWorkloadProvisionCommand,
            ) -> WorkloadProvisionCapabilityFuture<'a> {
                panic!("foreground ownership construction must not inspect provider effects")
            }
        }
    };
}

macro_rules! foreground_teardown_capability {
    ($provider:ty, $capability:ident) => {
        impl $capability for $provider {
            fn execute<'a>(
                &'a self,
                _command: &'a ConfirmedWorkloadTeardownCommand,
            ) -> WorkloadTeardownCapabilityFuture<'a> {
                panic!("foreground ownership construction must not execute teardown effects")
            }

            fn inspect<'a>(
                &'a self,
                _command: &'a ConfirmedWorkloadTeardownCommand,
            ) -> WorkloadTeardownCapabilityFuture<'a> {
                panic!("foreground ownership construction must not inspect teardown effects")
            }
        }
    };
}

foreground_effect_capability!(ForegroundAttachmentProvider, NetworkReservationCapability);
foreground_effect_capability!(ForegroundAttachmentProvider, NetworkAttachmentCapability);
foreground_teardown_capability!(ForegroundAttachmentProvider, NetworkDetachmentCapability);
foreground_teardown_capability!(ForegroundAttachmentProvider, NetworkReleaseCapability);

struct ForegroundExecutionProvider;

foreground_effect_capability!(ForegroundExecutionProvider, WorkloadPreparationCapability);
foreground_effect_capability!(ForegroundExecutionProvider, WorkloadActivationCapability);

impl WorkloadActivationPrerequisiteCapability for ForegroundExecutionProvider {
    fn inspect<'a>(
        &'a self,
        _command: &'a ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionCapabilityFuture<'a> {
        panic!("foreground ownership construction must not inspect activation prerequisites")
    }
}

impl WorkloadReadinessCapability for ForegroundExecutionProvider {
    fn inspect<'a>(
        &'a self,
        _command: &'a ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionCapabilityFuture<'a> {
        panic!("foreground ownership construction must not inspect workload readiness")
    }
}

impl WorkloadExecutionObservationCapability for ForegroundExecutionProvider {
    fn observe<'a>(
        &'a self,
        _request: &'a nimbus_compute::WorkloadExecutionObservationRequest,
    ) -> nimbus_compute::WorkloadExecutionObservationFuture<'a> {
        panic!("foreground ownership construction must not observe workload execution")
    }
}

foreground_teardown_capability!(
    ForegroundExecutionProvider,
    WorkloadExecutionDrainCapability
);
foreground_teardown_capability!(ForegroundExecutionProvider, WorkloadExecutionStopCapability);

struct ForegroundIngressProvider {
    _listener: std::net::TcpListener,
}

foreground_effect_capability!(ForegroundIngressProvider, IngressPublicationCapability);

impl IngressPublicationInspectionCapability for ForegroundIngressProvider {
    fn inspect<'a>(
        &'a self,
        _command: &'a ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionCapabilityFuture<'a> {
        panic!("foreground ownership construction must not inspect ingress publication")
    }
}

impl WorkloadIngressObservationCapability for ForegroundIngressProvider {
    fn observe<'a>(
        &'a self,
        _request: &'a nimbus_compute::WorkloadIngressObservationRequest,
    ) -> nimbus_compute::WorkloadIngressObservationFuture<'a> {
        panic!("foreground ownership construction must not observe ingress publication")
    }
}

foreground_teardown_capability!(ForegroundIngressProvider, FinalIngressWithdrawalCapability);

struct RecordingComposeProvision {
    definition: ServiceDefinition,
    provisioned_observation: ServiceDefinitionObservation,
    observed: Mutex<Option<ServiceDefinitionObservation>>,
    calls: Mutex<usize>,
}

impl RecordingComposeProvision {
    fn new(backend: SandboxBackendKind) -> Self {
        let tenant = TenantId::new("svc-demo").expect("tenant should parse");
        let mut spec = sample_spec(&tenant, "db");
        spec.backend = backend;
        let definition =
            ServiceDefinition::static_catalog(tenant.clone(), "db", ServiceBackend::sandbox(spec));
        let generation = WorkloadGeneration::new(definition.generation);
        let desired_digest = WorkloadDesiredDigest::sha256(format!(
            "compose-lifecycle:{}:{backend:?}",
            tenant.as_str()
        ));
        let workload_uid: TenantWorkloadUid = format!(
            "twu_{}",
            match backend {
                SandboxBackendKind::Krun => "a".repeat(64),
                SandboxBackendKind::Container => "b".repeat(64),
            }
        )
        .try_into()
        .expect("fixture workload UID should validate");
        let node_identity =
            NodeIdentity::new("compose-lifecycle-node").expect("fixture node should validate");
        let execution_id =
            WorkloadExecutionId::for_execution(&workload_uid, &node_identity, generation);
        let restart_epoch = WorkloadRestartEpoch::new(0);
        let attempt_id = WorkloadExecutionAttemptId::for_execution(&execution_id, restart_epoch);
        let execution: WorkloadExecutionReference = serde_json::from_value(serde_json::json!({
            "workloadUid": workload_uid,
            "nodeIdentity": node_identity,
            "executionId": execution_id,
            "restartEpoch": restart_epoch,
            "attemptId": attempt_id,
            "generation": generation,
            "desiredDigest": desired_digest,
        }))
        .expect("fixture execution should validate");
        let observation = ServiceDefinitionObservation {
            tenant_id: tenant.clone(),
            name: "db".to_owned(),
            source_generation: definition.generation,
            observed_execution_generation: execution.generation().as_u64(),
            execution,
            handle: SandboxHandle::new(
                tenant,
                SandboxId::new(format!("db-{backend:?}")),
                "db",
                backend,
                SandboxStatus::Ready,
                Vec::new(),
            ),
            published_endpoints: Vec::new(),
            endpoint_identity_fence: BTreeMap::new(),
            observed_at_millis: 1,
        };
        Self {
            definition,
            provisioned_observation: observation,
            observed: Mutex::new(None),
            calls: Mutex::new(0),
        }
    }
}

impl ComposeServiceProvision for RecordingComposeProvision {
    fn definition(&self, tenant_id: &TenantId, service_name: &str) -> Option<ServiceDefinition> {
        (&self.definition.tenant_id == tenant_id && self.definition.name == service_name)
            .then(|| self.definition.clone())
    }

    fn observation(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
    ) -> Option<ServiceDefinitionObservation> {
        (&self.definition.tenant_id == tenant_id && self.definition.name == service_name)
            .then(|| self.observed.lock().expect("observation lock").clone())
            .flatten()
    }

    fn provision<'a>(
        &'a self,
        _context: &'a TenantIsolationContext,
        _service_name: &'a str,
    ) -> ComposeProvisionFuture<'a> {
        *self.calls.lock().expect("call counter lock") += 1;
        let observation = self.provisioned_observation.clone();
        *self.observed.lock().expect("observation lock") = Some(observation.clone());
        let snapshot = SandboxServiceProvisionSnapshot {
            definition: self.definition.clone(),
            observation: Some(observation),
        };
        Box::pin(async move { Ok(snapshot) })
    }
}

#[tokio::test]
async fn compose_local_and_forwarded_provision_use_compute_dispatch() {
    for backend in [SandboxBackendKind::Krun, SandboxBackendKind::Container] {
        let provision = RecordingComposeProvision::new(backend);
        let tenant = provision.definition.tenant_id.clone();
        let context = TenantIsolationContext::system(tenant, "compose-caller-test");

        let started = provision_compose_service(&provision, &context, "db")
            .await
            .expect("both provider realms should dispatch through the compute facade");
        let replayed = provision_compose_service(&provision, &context, "db")
            .await
            .expect("exact observed projection should replay without another dispatch");

        assert_eq!(started.action, ServiceLifecycleAction::Started);
        assert_eq!(replayed.action, ServiceLifecycleAction::AlreadyRunning);
        assert_eq!(replayed.status, SandboxStatus::Ready);
        assert_eq!(
            *provision.calls.lock().expect("call counter lock"),
            1,
            "both provider realms must dispatch exactly once and then adopt exact projection"
        );
    }
}

#[test]
fn compose_local_and_forwarded_restart_use_compute() {
    let local = include_str!("../../network_composition.rs");
    let forwarded_server = include_str!("../../network_composition/forwarded.rs");
    let forwarded_foreground = include_str!("../provision.rs");
    let canonical = include_str!("../../network_composition/forwarded/profile.rs");

    for (owner, source) in [
        ("local server profile", local),
        ("canonical forwarded profile", canonical),
    ] {
        assert_eq!(
            source.matches(".with_restart_capabilities()").count(),
            1,
            "{owner} must register exactly one complete restart capability set with the compute composition"
        );
        assert!(
            !source.contains("submit_service_restart") && !source.contains("stop_service_sandbox"),
            "{owner} must not compose an explicit or stop/start restart path"
        );
    }
    assert_eq!(
        canonical
            .matches("WorkloadTeardownCapabilityRegistry::new")
            .count(),
        1,
        "the canonical forwarded composition must construct one exact teardown registry"
    );
    assert!(canonical.contains(".with_teardown_capabilities(teardown_capabilities)"));
    for (owner, source, function) in [
        (
            "forwarded server profile",
            forwarded_server,
            "fn compose_forwarded_server(",
        ),
        (
            "forwarded foreground Compose",
            forwarded_foreground,
            "fn compose_forwarded_foreground(",
        ),
    ] {
        assert!(
            source.contains(function),
            "{owner} must retain its thin consumer"
        );
        assert!(
            source.contains("prepare_forwarded_workload_profile("),
            "{owner} must return the canonical forwarded composition"
        );
        assert!(!source.contains("WorkloadTeardownCapabilityRegistry::new"));
        assert!(!source.contains(".with_restart_capabilities()"));
    }
}

#[test]
#[serial_test::serial]
fn foreground_compose_owner_retains_listener_rejects_second_realm_and_settles_before_return() {
    std::thread::Builder::new()
        .name("compose-foreground-lifecycle".to_owned())
        .stack_size(32 * 1024 * 1024)
        .spawn(|| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("foreground lifecycle runtime should initialize");
            runtime.block_on(foreground_compose_lifecycle_body());
        })
        .expect("foreground lifecycle test thread should spawn")
        .join()
        .expect("foreground lifecycle test thread should not panic");
}

async fn foreground_compose_lifecycle_body() {
    let engine_root = TempDir::new().expect("Engine root should exist");
    let network_root = TempDir::new().expect("network root should exist");
    let engine = Arc::new(
        nimbus::Engine::new(engine_root.path()).expect("foreground Engine should initialize"),
    );
    let requirements = nimbus_sandbox::sandbox_network_plan_requirements(SandboxBackendKind::Krun);
    let attachment_provider_id = requirements.required_attachment_provider_id().clone();
    let ingress_registration = nimbus_server::nimbus_owned_workload_ingress_registration();
    let ingress_provider_id = ingress_registration.provider_id().clone();
    let attachment_registration = NetworkAttachmentProviderRegistration::new(
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
    let selection = NetworkCapabilitySelection::new(
        attachment_provider_id.clone(),
        ingress_provider_id.clone(),
    );
    let registry = NetworkCapabilityRegistry::new([NetworkCapabilityBundle::new(
        attachment_registration,
        ingress_registration,
    )])
    .expect("foreground provider reports should validate");
    let manager = LocalNetworkManager::bootstrap(network_root.path())
        .expect("foreground process should claim its network realm")
        .freeze(registry);
    let services = Arc::new(ServiceManager::new(
        Arc::new(EmptyServiceDefinitionCatalog),
        nimbus::SandboxBackendKind::Krun,
    ));
    let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .expect("process-bound foreground listener should bind");
    let published_addr = listener
        .local_addr()
        .expect("foreground listener address should resolve");
    let ingress = Arc::new(ForegroundIngressProvider {
        _listener: listener,
    });
    let ingress_lifetime = Arc::downgrade(&ingress);
    let attachment = Arc::new(ForegroundAttachmentProvider);
    let execution = Arc::new(ForegroundExecutionProvider);
    let execution_provider_id =
        WorkloadExecutionProviderId::for_registration_key("compose-foreground-execution");
    let teardown_capabilities = WorkloadTeardownCapabilityRegistry::new(
        [NetworkAttachmentTeardownCapabilities::new(
            attachment_provider_id.clone(),
            attachment.clone(),
            attachment.clone(),
        )],
        [WorkloadExecutionTeardownCapabilities::new(
            execution_provider_id.clone(),
            execution.clone(),
            execution.clone(),
        )],
        [IngressTeardownCapabilities::new(
            ingress_provider_id.clone(),
            ingress.clone(),
        )],
    )
    .expect("foreground teardown capabilities should validate");
    let composition = ServerWorkloadComposition::new(
        Arc::clone(&engine),
        Arc::clone(&manager),
        Arc::clone(&services),
        NodeIdentity::new("compose-foreground-node").expect("node identity should validate"),
        selection,
        NetworkSovereigntyRequirements::new(NetworkControlPlaneLocality::LocalOnly, [], true),
        ServerWorkloadProviders::new(
            attachment_provider_id,
            attachment,
            execution_provider_id,
            execution,
            ingress_provider_id,
            Arc::clone(&ingress),
        )
        .with_teardown_capabilities(teardown_capabilities),
    )
    .expect("complete foreground composition should validate");
    let owner =
        ComposeForegroundOwner::open_composition_for_test(Arc::clone(&engine), composition).await;
    let cancellation = owner.cancellation();
    drop(ingress);
    drop(manager);
    drop(services);

    let live_connection =
        std::net::TcpStream::connect_timeout(&published_addr, std::time::Duration::from_secs(1))
            .expect(
                "the process-bound endpoint must remain connectable while Compose owns the runtime",
            );
    drop(live_connection);
    assert!(
        ingress_lifetime.upgrade().is_some(),
        "the Compose owner must retain the exact ingress provider"
    );
    let duplicate = LocalNetworkManager::bootstrap(network_root.path())
        .expect_err("a second foreground owner must not claim the live process realm");
    assert!(
        matches!(
            duplicate,
            nimbus_network::LocalNetworkManagerError::DuplicateProcessComposition { .. }
        ),
        "the second owner must fail with typed duplicate-authority evidence: {duplicate}"
    );

    owner.shutdown(&engine).await;

    assert!(
        cancellation.is_cancelled(),
        "foreground shutdown must cancel retained provision waiters"
    );
    assert!(
        ingress_lifetime.upgrade().is_none(),
        "the ingress provider must be dropped before foreground shutdown returns"
    );
    std::net::TcpStream::connect_timeout(&published_addr, std::time::Duration::from_millis(250))
        .expect_err("the process-bound endpoint must be withdrawn before shutdown returns");

    let reopened_network = LocalNetworkManager::bootstrap(network_root.path())
        .expect("the settled network realm should be reopenable")
        .freeze(NetworkCapabilityRegistry::new([]).expect("empty registry should validate"));
    drop(reopened_network);
    drop(engine);
    let reopened_engine =
        nimbus::Engine::new(engine_root.path()).expect("the canonical Engine store should reopen");
    reopened_engine.quiesce().await;
}
