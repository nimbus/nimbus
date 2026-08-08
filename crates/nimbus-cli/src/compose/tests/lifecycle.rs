use super::*;
use crate::compose::lifecycle::{
    ComposeForegroundOwner, ComposeProvisionFuture, ComposeServiceProvision,
    provision_compose_service,
};
use nimbus_compute::workload_saga::{
    ConfirmedWorkloadProvisionCommand, IngressPublicationCapability,
    IngressPublicationInspectionCapability, NetworkAttachmentCapability,
    NetworkReservationCapability, WorkloadActivationCapability,
    WorkloadActivationPrerequisiteCapability, WorkloadPreparationCapability,
    WorkloadProvisionCapabilityFuture, WorkloadReadinessCapability,
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
use nimbus_workloads::{NodeIdentity, WorkloadExecutionProviderId};

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

foreground_effect_capability!(ForegroundAttachmentProvider, NetworkReservationCapability);
foreground_effect_capability!(ForegroundAttachmentProvider, NetworkAttachmentCapability);

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
        let observation = ServiceDefinitionObservation {
            tenant_id: tenant.clone(),
            name: "db".to_owned(),
            observed_generation: definition.generation,
            handle: SandboxHandle::new(
                tenant,
                SandboxId::new(format!("db-{backend:?}")),
                "db",
                backend,
                SandboxStatus::Ready,
                Vec::new(),
            ),
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
async fn compose_local_and_forwarded_use_compute_dispatch() {
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

#[tokio::test]
#[serial_test::serial]
async fn foreground_compose_owner_retains_listener_rejects_second_realm_and_settles_before_return()
{
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
        Arc::new(StubBackend::default()),
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
    let composition = ServerWorkloadComposition::new(
        Arc::clone(&engine),
        Arc::clone(&manager),
        Arc::clone(&services),
        NodeIdentity::new("compose-foreground-node").expect("node identity should validate"),
        selection,
        NetworkSovereigntyRequirements::new(NetworkControlPlaneLocality::LocalOnly, [], true),
        ServerWorkloadProviders::new(
            attachment_provider_id,
            Arc::new(ForegroundAttachmentProvider),
            WorkloadExecutionProviderId::for_registration_key("compose-foreground-execution"),
            Arc::new(ForegroundExecutionProvider),
            ingress_provider_id,
            Arc::clone(&ingress),
        ),
    )
    .expect("complete foreground composition should validate");
    let owner = ComposeForegroundOwner::open_composition_for_test(Arc::clone(&engine), composition);
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

#[test]
fn resolve_service_down_targets_deduplicates_manifest_history_per_service_identity() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let compose_path = write_compose_fixture(temp_dir.path());
    let control_data_dir = temp_dir.path().join("control");
    let context = load_compose_project_context(&compose_path, &control_data_dir)
        .expect("compose project context should load");
    let krun_config = context
        .control_plane
        .reconstruct_direct_krun_backend_config();
    let tenant = context.control_plane.local_tenant_id.clone();

    write_manifest(
        &krun_config.workload_state_root,
        "db-01aaa",
        tenant.as_str(),
        "db",
        SandboxStatus::Stopped,
    );
    write_manifest(
        &krun_config.workload_state_root,
        "db-01bbb",
        tenant.as_str(),
        "db",
        SandboxStatus::Ready,
    );
    write_manifest(
        &krun_config.workload_state_root,
        "cache-01aaa",
        tenant.as_str(),
        "cache",
        SandboxStatus::Stopped,
    );

    let state_view = KrunSandboxStateView::from_config(&krun_config);
    let targets = resolve_service_down_targets(
        &state_view,
        &tenant,
        None,
        &context.control_plane.project_name,
    )
    .expect("targets should resolve");

    assert_eq!(targets.len(), 2);
    assert_eq!(
        targets
            .iter()
            .map(|target| {
                (
                    target.service_name.as_str(),
                    target.sandbox_id.as_str(),
                    target.status,
                )
            })
            .collect::<Vec<_>>(),
        vec![
            ("cache", "cache-01aaa", SandboxStatus::Stopped),
            ("db", "db-01bbb", SandboxStatus::Ready),
        ]
    );
}

#[tokio::test]
async fn stop_service_target_stops_active_handles_and_reports_already_stopped_terminal_ones() {
    let tenant = TenantId::new("svc-demo").expect("tenant should parse");
    let active_id = SandboxId::new("db-01aaa");
    let stopping_id = SandboxId::new("db-01bbb");
    let stopped_id = SandboxId::new("db-01ccc");
    let stopped_handle = stub_handle(&tenant, &stopped_id, "db", SandboxStatus::Stopped);
    let backend = StubBackend::with_handles([
        stub_handle(&tenant, &active_id, "db", SandboxStatus::Ready),
        stub_handle(&tenant, &stopping_id, "db", SandboxStatus::Stopping),
        stopped_handle.clone(),
    ]);
    backend.report_inspection(
        nimbus_sandbox::SandboxInspection::provider_reported(stopped_handle.clone())
            .with_provider_projection(
                stopped_handle,
                nimbus_sandbox::SandboxExecutionObservation::Exited { exit_code: 0 },
                nimbus_sandbox::SandboxRestartAssessment::Ineligible {
                    reason: nimbus_sandbox::SandboxRestartIneligibility::ShutdownRequested,
                },
                nimbus_sandbox::SandboxCleanupObservation::Finalized,
            ),
    );

    let stopped = stop_service_target(
        &backend,
        &tenant,
        ServiceLifecycleTarget {
            sandbox_id: active_id.clone(),
            service_name: "db".to_owned(),
            status: SandboxStatus::Ready,
        },
    )
    .await
    .expect("active handle should stop");
    assert_eq!(stopped.action, ServiceLifecycleAction::Stopped);
    assert_eq!(stopped.status, SandboxStatus::Stopped);

    let retained_stopping = stop_service_target(
        &backend,
        &tenant,
        ServiceLifecycleTarget {
            sandbox_id: stopping_id.clone(),
            service_name: "db".to_owned(),
            status: SandboxStatus::Stopping,
        },
    )
    .await
    .expect("Stopping with retained cleanup should run explicit teardown");
    assert_eq!(retained_stopping.action, ServiceLifecycleAction::Stopped);
    assert_eq!(retained_stopping.status, SandboxStatus::Stopped);

    let replayed = stop_service_target(
        &backend,
        &tenant,
        ServiceLifecycleTarget {
            sandbox_id: stopping_id.clone(),
            service_name: "db".to_owned(),
            status: SandboxStatus::Stopping,
        },
    )
    .await
    .expect("stop replay after backend absence should no-op");
    assert_eq!(replayed.action, ServiceLifecycleAction::AlreadyStopped);

    let already_stopped = stop_service_target(
        &backend,
        &tenant,
        ServiceLifecycleTarget {
            sandbox_id: stopped_id.clone(),
            service_name: "db".to_owned(),
            status: SandboxStatus::Stopped,
        },
    )
    .await
    .expect("stopped handle should no-op");
    assert_eq!(
        already_stopped.action,
        ServiceLifecycleAction::AlreadyStopped
    );

    let stopped_ids = backend
        .stopped_ids
        .lock()
        .expect("stopped ids lock should hold");
    assert_eq!(
        stopped_ids.as_slice(),
        &[
            active_id.as_str().to_owned(),
            stopping_id.as_str().to_owned()
        ]
    );
}

#[tokio::test]
async fn stop_service_target_rejects_crossed_identity_before_lifecycle_effects() {
    let tenant = TenantId::new("svc-demo").expect("tenant should parse");
    let sandbox_id = SandboxId::new("db-identity");

    for case in ["sandbox-id", "tenant", "service-name", "backend"] {
        let expected = stub_handle(&tenant, &sandbox_id, "db", SandboxStatus::Stopping);
        let backend = StubBackend::with_handles([expected.clone()]);
        let mut crossed = expected.clone();
        match case {
            "sandbox-id" => crossed.id = SandboxId::new("crossed-compose-sandbox"),
            "tenant" => {
                crossed.tenant_id =
                    TenantId::new("crossed-tenant").expect("crossed tenant should parse");
            }
            "service-name" => crossed.name = "crossed-service".to_owned(),
            "backend" => crossed.backend = SandboxBackendKind::Container,
            _ => unreachable!("the identity table is exhaustive"),
        }
        backend.report_inspection_for(
            &sandbox_id,
            nimbus_sandbox::SandboxInspection::provider_reported(crossed),
        );

        let error = stop_service_target(
            &backend,
            &tenant,
            ServiceLifecycleTarget {
                sandbox_id: sandbox_id.clone(),
                service_name: "db".to_owned(),
                status: SandboxStatus::Stopping,
            },
        )
        .await
        .expect_err("crossed compose inspection identity must fail closed");

        assert!(
            error.to_string().contains("crossed inspection identity"),
            "{case}: rejection must name the backend contract failure: {error}"
        );
        assert!(
            backend
                .stopped_ids
                .lock()
                .expect("stopped ids lock should hold")
                .is_empty(),
            "{case}: rejected evidence must not reach backend stop"
        );
        assert_eq!(
            backend
                .handles
                .lock()
                .expect("handles lock should hold")
                .get(sandbox_id.as_str()),
            Some(&expected),
            "{case}: rejected evidence must not mutate the tracked handle"
        );
    }
}

#[tokio::test]
async fn compose_down_localizes_only_explicit_retirement_calls() {
    let tenant = TenantId::new("svc-demo").expect("tenant should parse");
    let sandbox_id = SandboxId::new("db-01aaa");
    let backend = StubBackend::with_handles([stub_handle(
        &tenant,
        &sandbox_id,
        "db",
        SandboxStatus::Ready,
    )]);
    let stopped = stop_service_target(
        &backend,
        &tenant,
        ServiceLifecycleTarget {
            sandbox_id,
            service_name: "db".to_owned(),
            status: SandboxStatus::Ready,
        },
    )
    .await
    .expect("explicit Compose down should retire the selected service");
    assert_eq!(stopped.action, ServiceLifecycleAction::Stopped);
}
