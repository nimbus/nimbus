use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use nimbus_compute::workload_saga::{
    IngressProvisionCapabilities, WorkloadProvisionCapabilityRegistry,
};
use nimbus_network::{
    ListenerId, LocalNetworkAuthority, LocalNetworkManager, LocalPortLeaseAuthority,
    NetworkAttachmentCapabilitySet, NetworkAttachmentId, NetworkCapabilityRegistry,
    NetworkCapabilityRequirements, NetworkControlPlaneLocality, NetworkEndpointCapabilitySet,
    NetworkForwardingCapabilitySet, NetworkIngressCapabilitySet, NetworkLeaseEpoch,
    NetworkLifecycleCapabilitySet, NetworkLifecycleRequirements, NetworkManagementMode,
    NetworkPlan, NetworkPlanContentDigest, NetworkPlanDigest, NetworkPlanId, NetworkProviderHandle,
    NetworkProviderId, NetworkReservationClaim, NetworkResourceGeneration, NetworkResourceId,
    NetworkSovereigntyRequirements, PortBindRealm, PortBindTarget, PortBindingProvenance,
    PortBindingSpec, PortExposure, PortLeaseAccounting, PortLeaseFence, PortLeaseId,
    PortLeaseLifetime, PortLeaseRequest, PortProtocol, PortPublicationIntent, PortRequestMode,
    PublishedEndpointId,
};

use super::*;

static PROCESS_NETWORK_AUTHORITY_TEST_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn process_network_authority_test_guard() -> std::sync::MutexGuard<'static, ()> {
    PROCESS_NETWORK_AUTHORITY_TEST_GUARD
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

struct AbsentContainerIngressSource;

impl LocalSandboxIngressTargetSource for AbsentContainerIngressSource {
    fn backend_kind(&self) -> SandboxBackendKind {
        SandboxBackendKind::Container
    }

    fn inspect_targets(
        &self,
        _sandbox_id: &nimbus_sandbox::SandboxId,
        _execution_attempt_id: &nimbus_sandbox::SandboxExecutionAttemptId,
        _network_plan: &SandboxProvisionNetworkPlan,
    ) -> Result<SandboxProvisionIngressTargetObservation, SandboxError> {
        Ok(SandboxProvisionIngressTargetObservation::Absent {
            evidence: b"fixture private attachment absent".to_vec(),
        })
    }
}

fn reservation_claim(label: &str) -> NetworkReservationClaim {
    NetworkReservationClaim::new(
        NetworkProviderHandle::new(
            NetworkProviderId::for_registration_key("nimbus-sandbox.test-attachment"),
            label,
        )
        .expect("fixture provider handle should validate"),
    )
}

fn workload_request(label: &str) -> PortLeaseRequest {
    let listener = ListenerId::for_workload_listener("tenant-a/workload-a", label);
    PortLeaseRequest::new(
        PortLeaseId::for_listener(&listener),
        NetworkResourceId::from(listener),
        Some(nimbus_core::TenantId::new("tenant-a").expect("fixture tenant should parse")),
        PortLeaseFence::new(NetworkResourceGeneration::new(7), NetworkLeaseEpoch::new(1)),
        PortLeaseAccounting::TenantPublished,
        PortPublicationIntent::host(Ipv4Addr::LOCALHOST.into()),
        PortBindingSpec::new(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            PortBindTarget::ipv4_specific(Ipv4Addr::LOCALHOST),
            PortExposure::Loopback,
            PortRequestMode::ProviderAssigned,
        ),
    )
}

#[test]
fn real_server_ingress_adapter_substitutes_for_publication_inspection_and_observation() {
    let _process_authority_guard = process_network_authority_test_guard();
    let root = tempfile::tempdir().expect("fixture root should exist");
    let bootstrap = LocalNetworkManager::bootstrap(root.path())
        .expect("fixture network authority should bootstrap");
    let manager = Arc::new(bootstrap.freeze(
        NetworkCapabilityRegistry::new([]).expect("empty report registry should validate"),
    ));
    let adapter = Arc::new(
        ServerIngressPublicationAdapter::new(
            Arc::new(AbsentContainerIngressSource),
            manager.authority(),
        )
        .expect("server ingress journal should open"),
    );

    WorkloadProvisionCapabilityRegistry::new(
        [],
        [],
        [IngressProvisionCapabilities::new(
            nimbus_owned_local_ingress_provider_id(),
            adapter,
        )],
    )
    .expect("the real server adapter should earn all three narrow ingress capabilities");
}

struct LiveObservationFixture {
    adapter: Arc<ServerIngressPublicationAdapter>,
    network_authority: LocalNetworkAuthority,
    query: LiveIngressObservationQuery,
    expected_lifetimes: BTreeMap<PublishedEndpointId, PortLeaseLifetime>,
    state_root: tempfile::TempDir,
    _process_authority_guard: std::sync::MutexGuard<'static, ()>,
}

fn live_workload_request(
    tenant_id: &nimbus_core::TenantId,
    plan_id: &NetworkPlanId,
    listener_name: &str,
) -> PortLeaseRequest {
    let listener = ListenerId::for_tenant_workload_listener(tenant_id, "workload-a", listener_name);
    PortLeaseRequest::new(
        PortLeaseId::for_listener(&listener),
        NetworkResourceId::from(listener),
        Some(tenant_id.clone()),
        PortLeaseFence::new(NetworkResourceGeneration::new(7), NetworkLeaseEpoch::new(1)),
        PortLeaseAccounting::TenantPublished,
        PortPublicationIntent::host(Ipv4Addr::LOCALHOST.into()),
        PortBindingSpec::new(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            PortBindTarget::ipv4_specific(Ipv4Addr::LOCALHOST),
            PortExposure::Loopback,
            PortRequestMode::ProviderAssigned,
        ),
    )
    .with_plan_id(plan_id.clone())
}

fn live_observation_fixture(listener_names: &[&str]) -> LiveObservationFixture {
    let process_authority_guard = process_network_authority_test_guard();
    let state_root = tempfile::tempdir().expect("fixture root should exist");
    let bootstrap = LocalNetworkManager::bootstrap(state_root.path())
        .expect("fixture network authority should bootstrap");
    let manager = bootstrap
        .freeze(NetworkCapabilityRegistry::new([]).expect("empty report registry should validate"));
    let network_authority = manager.authority();
    let adapter = Arc::new(
        ServerIngressPublicationAdapter::new(
            Arc::new(AbsentContainerIngressSource),
            network_authority.clone(),
        )
        .expect("server ingress journal should open"),
    );
    let tenant_id = nimbus_core::TenantId::new("tenant-a").expect("fixture tenant should parse");
    let plan_id = NetworkPlanId::for_tenant_workload_plan(&tenant_id, "workload-a");
    let plan_digest = NetworkPlanDigest::from_bytes([0x4a; 32]);
    let generation = NetworkResourceGeneration::new(7);
    let claim = reservation_claim("server-ingress-observation");
    let mut routes = Vec::new();
    let mut listeners = BTreeMap::new();
    let mut expected_lifetimes = BTreeMap::new();

    // Deliberately reverse construction order. Observation must canonicalize
    // by stable endpoint identity rather than preserving provider order.
    let planned = listener_names
        .iter()
        .rev()
        .map(|listener_name| {
            let listener_id =
                ListenerId::for_tenant_workload_listener(&tenant_id, "workload-a", listener_name);
            let endpoint_id =
                PublishedEndpointId::for_workload_endpoint("tenant-a/workload-a", listener_name);
            let request = live_workload_request(&tenant_id, &plan_id, listener_name);
            (listener_id, endpoint_id, request)
        })
        .collect::<Vec<_>>();
    let requested_plan = planned
        .iter()
        .map(|(_, _, request)| request.clone())
        .collect::<Vec<_>>();
    network_authority
        .port_leases()
        .reserve_batch_for_coordinator(requested_plan.clone(), &claim)
        .expect("launch owner should atomically reserve the complete listener plan");
    let plan_members = adapter
        .listeners
        .authenticate_workload_ingress_plan(
            &plan_id,
            &tenant_id,
            generation,
            &requested_plan,
            &claim,
        )
        .expect("server publisher should authenticate complete durable plan membership");

    for (listener_id, endpoint_id, request) in planned {
        let prepared = adapter
            .listeners
            .prepare_workload_ingress(Some(&plan_members), request.clone(), &claim)
            .expect("server publisher should claim the launch reservation");
        let listener = TcpListener::bind(
            prepared
                .bind_addr()
                .expect("authorized bind address should resolve"),
        )
        .expect("fixture publication listener should bind");
        let adopted = prepared
            .adopt_std(listener)
            .expect("fixture listener should activate the exact lease");
        let route = RunningIngressRoute::start(
            ExpectedRoute {
                listener_id: listener_id.clone(),
                request: request.clone(),
                upstream: (Ipv4Addr::LOCALHOST, 9).into(),
            },
            adopted,
            DEFAULT_MAX_ACTIVE_CONNECTIONS,
        )
        .expect("fixture ingress worker should start");
        let evidence = route
            .lease
            .as_ref()
            .and_then(ActiveServerListenerLease::observation_evidence)
            .expect("live fixture lease should expose stripped observation evidence");
        expected_lifetimes.insert(endpoint_id.clone(), evidence.lifetime());
        listeners.insert(
            listener_id.clone(),
            LiveIngressListenerExpectation {
                endpoint_id,
                listener_id,
                port_lease_id: request.lease_id().clone(),
                desired_host_address: Ipv4Addr::LOCALHOST.into(),
            },
        );
        routes.push(route);
    }

    let saga_id = "fixture-workload-saga".to_owned();
    adapter
        .running
        .lock()
        .expect("fixture registry lock should remain healthy")
        .insert(
            PublicationKey {
                saga_id: saga_id.clone(),
                attempt_id: "fixture-publication-attempt".to_owned(),
                execution_id: "execution-workload-a".to_owned(),
                generation: generation.as_u64(),
                network_plan_digest: plan_digest.to_string(),
            },
            RunningIngressBatch {
                execution_id: "execution-workload-a".to_owned(),
                tenant_id: tenant_id.clone(),
                plan_id: plan_id.clone(),
                generation,
                attachment_id: NetworkAttachmentId::for_workload_attachment(
                    "tenant-a/workload-a",
                    "private",
                ),
                plan_members,
                routes,
            },
        );

    LiveObservationFixture {
        adapter,
        network_authority,
        query: LiveIngressObservationQuery {
            saga_id,
            execution_id: "execution-workload-a".to_owned(),
            attempt_id: "fixture-publication-attempt".to_owned(),
            tenant_id,
            plan_id,
            plan_digest,
            generation,
            listeners,
        },
        expected_lifetimes,
        state_root,
        _process_authority_guard: process_authority_guard,
    }
}

fn snapshot_regular_files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, path: &Path, snapshot: &mut BTreeMap<PathBuf, Vec<u8>>) {
        let mut entries = fs::read_dir(path)
            .expect("fixture state directory should list")
            .collect::<Result<Vec<_>, _>>()
            .expect("fixture state entries should resolve");
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

fn restart_publication_for(
    key: &PublicationKey,
    batch: &RunningIngressBatch,
) -> ValidatedRestartPublication {
    let requirements = NetworkCapabilityRequirements::new(
        NetworkAttachmentCapabilitySet::new(NetworkManagementMode::NimbusHostManaged, [], []),
        NetworkEndpointCapabilitySet::new([], [], [], [], []),
        NetworkIngressCapabilitySet::new([]),
        NetworkForwardingCapabilitySet::new([]),
        NetworkLifecycleRequirements::new(
            NetworkLifecycleCapabilitySet::new([]),
            NetworkLifecycleCapabilitySet::new([]),
        ),
        NetworkSovereigntyRequirements::new(NetworkControlPlaneLocality::LocalOnly, [], true),
    );
    let network_plan = NetworkPlan::new(
        batch.plan_id.clone(),
        batch.generation,
        NetworkPlanContentDigest::sha256(b"restart-listener-retention-fixture"),
        requirements,
    );
    let listeners = batch.routes.iter().enumerate().map(|(ordinal, route)| {
        nimbus_sandbox::SandboxProvisionListener::new(
            route.expected.listener_id.clone(),
            nimbus_sandbox::SandboxPortBinding::tcp(format!("listener-{ordinal}"), 0, 9),
            route.expected.request.clone(),
        )
    });
    let network_plan = SandboxProvisionNetworkPlan::new(
        network_plan,
        batch.tenant_id.clone(),
        batch.generation,
        batch.attachment_id.clone(),
        listeners,
        [],
    )
    .expect("restart fixture network plan should validate");
    let source_attempt = nimbus_sandbox::SandboxExecutionAttemptId::new(key.attempt_id.clone())
        .expect("source attempt should validate");
    let target_attempt = nimbus_sandbox::SandboxExecutionAttemptId::new("restart-target")
        .expect("target attempt should validate");
    let attempt_fence =
        nimbus_sandbox::SandboxRestartAttemptFence::new(source_attempt, target_attempt, 1)
            .expect("restart attempt fence should validate");
    ValidatedRestartPublication {
        source_key: key.clone(),
        target_key: PublicationKey {
            saga_id: key.saga_id.clone(),
            attempt_id: attempt_fence.attempt_id().as_str().to_owned(),
            execution_id: key.execution_id.clone(),
            generation: key.generation,
            network_plan_digest: key.network_plan_digest.clone(),
        },
        sandbox_id: nimbus_sandbox::SandboxId::new(key.execution_id.clone()),
        attempt_fence,
        network_plan,
    }
}

fn expect_present(
    observation: WorkloadProviderObservation<Vec<WorkloadObservedIngressEndpoint>>,
) -> Vec<WorkloadObservedIngressEndpoint> {
    match observation {
        WorkloadProviderObservation::Present(endpoints) => endpoints,
        other => panic!("expected present ingress evidence, got {other:?}"),
    }
}

#[test]
fn live_observation_returns_canonical_provider_assigned_witnesses_without_mutation() {
    let fixture = live_observation_fixture(&["http", "admin"]);
    let before_files = snapshot_regular_files(fixture.state_root.path());
    let before_leases = fixture
        .network_authority
        .port_leases()
        .list()
        .expect("fixture leases should list");

    let first = fixture.adapter.observe_live_publication(&fixture.query);
    let replay = fixture.adapter.observe_live_publication(&fixture.query);
    assert_eq!(
        replay, first,
        "replay must return byte-identical value state"
    );
    let endpoints = expect_present(first.clone());
    assert_eq!(endpoints.len(), 2);
    assert!(
        endpoints
            .windows(2)
            .all(|pair| pair[0].endpoint_id() < pair[1].endpoint_id()),
        "provider order must be canonicalized by stable endpoint identity"
    );
    for endpoint in &endpoints {
        let expected = fixture
            .query
            .listeners
            .values()
            .find(|expected| expected.endpoint_id == *endpoint.endpoint_id())
            .expect("every observed endpoint should be an authenticated member");
        let binding = endpoint.binding();
        assert_eq!(binding.plan_id(), &fixture.query.plan_id);
        assert_eq!(binding.plan_digest(), fixture.query.plan_digest);
        assert_eq!(binding.generation(), fixture.query.generation);
        assert_eq!(binding.listener_id(), &expected.listener_id);
        assert_eq!(binding.port_lease_id(), &expected.port_lease_id);
        assert_eq!(binding.lifetime(), binding.binding_lifetime());
        assert_eq!(
            fixture.expected_lifetimes.get(endpoint.endpoint_id()),
            Some(&binding.lifetime())
        );
        assert_eq!(
            binding.provenance(),
            PortBindingProvenance::ProviderAssigned
        );
        assert_eq!(
            binding.bound_endpoint().port().get(),
            endpoint.published_address().port()
        );
        assert_eq!(
            binding.bound_endpoint().target().specific_address(),
            Some(endpoint.published_address().ip())
        );
    }

    let concurrent = std::thread::scope(|scope| {
        (0..8)
            .map(|_| scope.spawn(|| fixture.adapter.observe_live_publication(&fixture.query)))
            .collect::<Vec<_>>()
            .into_iter()
            .map(|worker| worker.join().expect("observation worker should not panic"))
            .collect::<Vec<_>>()
    });
    assert!(concurrent.iter().all(|observation| observation == &first));
    assert_eq!(
        fixture
            .network_authority
            .port_leases()
            .list()
            .expect("fixture leases should remain readable"),
        before_leases,
        "observation must not mutate durable lease authority"
    );
    assert_eq!(
        snapshot_regular_files(fixture.state_root.path()),
        before_files,
        "replay and concurrent observation must leave journal and lease bytes unchanged"
    );
}

#[test]
fn restart_without_live_listener_ownership_remains_in_progress_and_effect_free() {
    let fixture = live_observation_fixture(&["http"]);
    let restarted = ServerIngressPublicationAdapter::new(
        Arc::new(AbsentContainerIngressSource),
        fixture.network_authority.clone(),
    )
    .expect("restart fixture should reopen the existing phase journal");
    let before = snapshot_regular_files(fixture.state_root.path());

    assert_eq!(
        restarted.observe_live_publication(&fixture.query),
        WorkloadProviderObservation::InProgress
    );
    assert_eq!(snapshot_regular_files(fixture.state_root.path()), before);
}

#[test]
fn restart_withdrawal_joins_listener_and_rebinds_the_same_retained_port() {
    let fixture = live_observation_fixture(&["http"]);
    let (key, batch) = fixture
        .adapter
        .running
        .lock()
        .expect("fixture registry lock should remain healthy")
        .pop_first()
        .expect("fixture should retain one exact publication");
    let request = batch.routes[0].expected.request.clone();
    let expected = batch.routes[0].expected.clone();
    let original_port = batch.routes[0].bound_addr.port();
    let plan_members = batch.plan_members.clone();

    let evidence = batch
        .stop_and_retain_for_restart()
        .expect("restart withdrawal should stop, join, and retain the complete batch");
    assert!(String::from_utf8_lossy(&evidence).contains("restart_retained="));
    let retained = fixture
        .network_authority
        .port_leases()
        .inspect(request.lease_id())
        .expect("retained lease should inspect")
        .expect("retained lease should remain durable");
    assert_eq!(retained.phase(), nimbus_network::PortLeasePhase::Reserved);
    assert_eq!(
        retained
            .confirmed_stopped_binding()
            .expect("retained lease should carry confirmed-stop evidence")
            .actual_port()
            .get(),
        original_port
    );

    let prepared = fixture
        .adapter
        .listeners
        .prepare_workload_ingress(
            Some(&plan_members),
            request.clone(),
            &reservation_claim("server-ingress-observation"),
        )
        .expect("target attempt should claim the retained exact-port rebind");
    let bind_addr = prepared
        .bind_addr()
        .expect("retained rebind address should resolve");
    assert_eq!(bind_addr.port(), original_port);
    let listener = TcpListener::bind(bind_addr).expect("retained exact port should rebind");
    let adopted = prepared
        .adopt_std(listener)
        .expect("target listener should adopt the retained lease");
    let route = RunningIngressRoute::start(expected, adopted, DEFAULT_MAX_ACTIVE_CONNECTIONS)
        .expect("target attempt ingress route should start");
    assert_eq!(route.bound_addr.port(), original_port);
    assert!(route.is_healthy());
    drop(route);
    assert!(
        !fixture
            .adapter
            .running
            .lock()
            .expect("fixture registry lock should remain healthy")
            .contains_key(&key),
        "the source attempt must stay withdrawn"
    );
}

#[test]
fn restart_withdrawal_inspection_requires_durable_retention_and_never_recovers() {
    let fixture = live_observation_fixture(&["http"]);
    let (key, mut batch) = fixture
        .adapter
        .running
        .lock()
        .expect("fixture registry lock should remain healthy")
        .pop_first()
        .expect("fixture should retain one exact publication");
    let validated = restart_publication_for(&key, &batch);
    for route in &mut batch.routes {
        drop(
            route
                .take_for_restart()
                .expect("fixture route should retain its listener effect"),
        );
    }
    drop(batch);

    let before = snapshot_regular_files(fixture.state_root.path());
    let inspected = fixture.adapter.inspect_restart_withdrawal(&validated);
    assert!(
        matches!(inspected, ProviderRestartEffectObservation::Absent { .. }),
        "active durable state without a live owner is not retained withdrawal evidence"
    );
    assert_eq!(snapshot_regular_files(fixture.state_root.path()), before);

    let recovered = fixture.adapter.withdraw_restart_publication(&validated);
    assert!(
        matches!(
            recovered,
            ProviderRestartEffectObservation::Succeeded { .. }
        ),
        "execute-time recovery must durably retain the dead listener"
    );
    let after_recovery = snapshot_regular_files(fixture.state_root.path());
    assert!(
        matches!(
            fixture.adapter.inspect_restart_withdrawal(&validated),
            ProviderRestartEffectObservation::Succeeded { .. }
        ),
        "exact retained records must be sufficient withdrawal evidence"
    );
    assert_eq!(
        snapshot_regular_files(fixture.state_root.path()),
        after_recovery
    );
}

#[test]
fn crossed_or_unhealthy_live_ingress_evidence_fails_closed() {
    let fixture = live_observation_fixture(&["http"]);
    let mut cases = Vec::new();

    let mut wrong_plan = fixture.query.clone();
    wrong_plan.plan_id =
        NetworkPlanId::for_tenant_workload_plan(&fixture.query.tenant_id, "replacement-workload");
    cases.push(wrong_plan);
    let mut wrong_digest = fixture.query.clone();
    wrong_digest.plan_digest = NetworkPlanDigest::from_bytes([0x9b; 32]);
    cases.push(wrong_digest);
    let mut wrong_execution = fixture.query.clone();
    wrong_execution.execution_id = "execution-crossed".to_owned();
    cases.push(wrong_execution);
    let mut wrong_generation = fixture.query.clone();
    wrong_generation.generation = NetworkResourceGeneration::new(8);
    cases.push(wrong_generation);
    let mut wrong_tenant = fixture.query.clone();
    wrong_tenant.tenant_id =
        nimbus_core::TenantId::new("tenant-b").expect("crossed tenant should parse");
    cases.push(wrong_tenant);
    let mut wrong_lease = fixture.query.clone();
    let expectation = wrong_lease
        .listeners
        .values_mut()
        .next()
        .expect("fixture should carry one listener");
    expectation.port_lease_id =
        PortLeaseId::for_listener(&ListenerId::for_tenant_workload_listener(
            &fixture.query.tenant_id,
            "workload-a",
            "crossed",
        ));
    cases.push(wrong_lease);
    let mut wrong_listener = fixture.query.clone();
    wrong_listener
        .listeners
        .values_mut()
        .next()
        .expect("fixture should carry one listener")
        .listener_id =
        ListenerId::for_tenant_workload_listener(&fixture.query.tenant_id, "workload-a", "crossed");
    cases.push(wrong_listener);
    let mut wrong_host = fixture.query.clone();
    wrong_host
        .listeners
        .values_mut()
        .next()
        .expect("fixture should carry one listener")
        .desired_host_address = Ipv4Addr::new(127, 0, 0, 2).into();
    cases.push(wrong_host);

    for crossed in cases {
        assert_eq!(
            fixture.adapter.observe_live_publication(&crossed),
            WorkloadProviderObservation::Ambiguous,
            "crossed stable identity, fence, or desired bind must fail closed"
        );
    }

    {
        let running = fixture
            .adapter
            .running
            .lock()
            .expect("fixture registry lock should remain healthy");
        let route = &running
            .values()
            .next()
            .expect("fixture batch should remain live")
            .routes[0];
        route.failed.store(true, Ordering::Release);
    }
    assert_eq!(
        fixture.adapter.observe_live_publication(&fixture.query),
        WorkloadProviderObservation::Ambiguous
    );
}

#[test]
fn live_listener_batch_never_regresses_to_absent_when_its_source_temporarily_disappears() {
    let fixture = live_observation_fixture(&["http"]);
    let key = fixture
        .adapter
        .running
        .lock()
        .expect("fixture registry lock should remain healthy")
        .keys()
        .next()
        .expect("fixture should retain one exact publication")
        .clone();

    let inspected = fixture.adapter.inspect_with_source(
        &key,
        Err(ProviderProvisionEffectObservation::Absent {
            evidence: b"source temporarily absent".to_vec(),
        }),
        true,
    );
    assert!(
        matches!(
            inspected,
            ProviderProvisionEffectObservation::Succeeded { .. }
        ),
        "a healthy owned listener is durable positive evidence and cannot authorize a duplicate retry"
    );
}

#[test]
fn live_listener_batch_preserves_ambiguous_source_on_publication_replay() {
    let fixture = live_observation_fixture(&["http"]);
    let running = fixture
        .adapter
        .running
        .lock()
        .expect("fixture registry lock should remain healthy");
    let (key, batch) = running
        .iter()
        .next()
        .expect("fixture should retain one exact publication");
    let observation = classify_existing_publication(
        batch,
        &key.execution_id,
        Err(ProviderProvisionEffectObservation::Ambiguous {
            evidence: b"private-route source temporarily unavailable".to_vec(),
        }),
    );
    assert!(
        matches!(
            observation,
            ProviderProvisionEffectObservation::Ambiguous { ref evidence }
                if evidence == b"private-route source temporarily unavailable"
        ),
        "a transient source read cannot convert a live exact listener into terminal failure"
    );
}

#[test]
fn transparent_tcp_route_forwards_bytes_and_releases_its_exact_lease() {
    let root = tempfile::tempdir().expect("fixture root should exist");
    let request = workload_request("http");
    let claim = reservation_claim("server-ingress-forwarding");
    let port_authority =
        LocalPortLeaseAuthority::open(root.path()).expect("port authority should open");
    port_authority
        .reserve_for_coordinator(request.clone(), &claim)
        .expect("launch owner should reserve the exact request");

    let upstream =
        TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("fixture upstream listener should bind");
    let upstream_addr = upstream
        .local_addr()
        .expect("fixture upstream address should resolve");
    let upstream_worker = std::thread::spawn(move || {
        let (mut stream, _) = upstream.accept().expect("proxy should reach upstream");
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("upstream timeout should configure");
        let mut request = [0_u8; 4];
        stream
            .read_exact(&mut request)
            .expect("upstream should receive the exact request");
        assert_eq!(&request, b"ping");
        stream
            .write_all(b"pong")
            .expect("upstream should return the exact response");
    });

    let authority = ServerListenerLeaseAuthority::reconstruct_direct(root.path())
        .expect("listener authority should reconstruct");
    let prepared = authority
        .prepare_workload_ingress(None, request.clone(), &claim)
        .expect("publication should claim the launch reservation");
    let listener = TcpListener::bind(
        prepared
            .bind_addr()
            .expect("authorized bind address should resolve"),
    )
    .expect("publication listener should bind");
    let adopted = prepared
        .adopt_std(listener)
        .expect("publication listener should activate the exact lease");
    let route = RunningIngressRoute::start(
        ExpectedRoute {
            listener_id: ListenerId::for_workload_listener("tenant-a/workload-a", "http"),
            request: request.clone(),
            upstream: upstream_addr,
        },
        adopted,
        DEFAULT_MAX_ACTIVE_CONNECTIONS,
    )
    .expect("transparent ingress route should start");
    assert!(route.is_healthy());
    let published_addr = route.bound_addr;

    let mut client = TcpStream::connect(published_addr)
        .expect("client should connect through the published route");
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("client timeout should configure");
    client
        .write_all(b"ping")
        .expect("client request should reach the route");
    client
        .shutdown(Shutdown::Write)
        .expect("client write half should close");
    let mut response = [0_u8; 4];
    client
        .read_exact(&mut response)
        .expect("client should receive the upstream response");
    assert_eq!(&response, b"pong");
    upstream_worker
        .join()
        .expect("upstream worker should finish cleanly");

    drop(route);
    let settled = port_authority
        .inspect(request.lease_id())
        .expect("settled lease should inspect")
        .expect("released lease evidence should remain observable");
    assert_eq!(settled.phase(), nimbus_network::PortLeasePhase::Released);
    assert!(settled.reservation_claim().is_none());
    assert!(settled.bind_claim().is_none());
    let binding = settled
        .binding()
        .expect("released authority should retain observed binding history");
    assert_eq!(binding.actual_port().get(), published_addr.port());
    assert_eq!(
        binding.provenance(),
        nimbus_network::PortBindingProvenance::ProviderAssigned
    );
}

#[test]
fn ingress_route_bounds_tracks_and_joins_connection_workers_before_lease_settlement() {
    let root = tempfile::tempdir().expect("fixture root should exist");
    let request = workload_request("bounded");
    let claim = reservation_claim("server-ingress-bounded-workers");
    let port_authority =
        LocalPortLeaseAuthority::open(root.path()).expect("port authority should open");
    port_authority
        .reserve_for_coordinator(request.clone(), &claim)
        .expect("launch owner should reserve the exact request");
    let upstream =
        TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("fixture upstream should bind");
    let upstream_addr = upstream
        .local_addr()
        .expect("upstream address should resolve");
    let (accepted_tx, accepted_rx) = std::sync::mpsc::channel();
    let upstream_worker = std::thread::spawn(move || {
        let (mut stream, _) = upstream
            .accept()
            .expect("first proxy should reach upstream");
        accepted_tx
            .send(())
            .expect("accept signal receiver should remain open");
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("upstream timeout should configure");
        let mut byte = [0_u8; 1];
        let _ = stream.read(&mut byte);
    });

    let authority = ServerListenerLeaseAuthority::reconstruct_direct(root.path())
        .expect("listener authority should reconstruct");
    let prepared = authority
        .prepare_workload_ingress(None, request.clone(), &claim)
        .expect("publication should claim the launch reservation");
    let listener = TcpListener::bind(prepared.bind_addr().expect("bind address should resolve"))
        .expect("publication listener should bind");
    let adopted = prepared
        .adopt_std(listener)
        .expect("publication listener should activate the exact lease");
    let route = RunningIngressRoute::start(
        ExpectedRoute {
            listener_id: ListenerId::for_workload_listener("tenant-a/workload-a", "bounded"),
            request: request.clone(),
            upstream: upstream_addr,
        },
        adopted,
        1,
    )
    .expect("bounded ingress route should start");

    let first = TcpStream::connect(route.bound_addr).expect("first client should connect");
    accepted_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("first connection should reach its upstream");
    wait_for_counter(&route.active_connections, 1, "active connection");
    let second = TcpStream::connect(route.bound_addr).expect("second client should connect");
    wait_for_counter(&route.rejected_connections, 1, "rejected connection");
    assert_eq!(route.peak_connections.load(Ordering::Acquire), 1);
    let active_connections = Arc::clone(&route.active_connections);

    drop(second);
    drop(first);
    drop(route);
    assert_eq!(
        active_connections.load(Ordering::Acquire),
        0,
        "listener settlement must wait for every transitively owned connection worker"
    );
    upstream_worker
        .join()
        .expect("upstream worker should finish after route shutdown");
    assert_eq!(
        port_authority
            .inspect(request.lease_id())
            .expect("settled lease should inspect")
            .expect("settled lease evidence should remain")
            .phase(),
        nimbus_network::PortLeasePhase::Released,
        "route drop must join every tracked connection before settling the lease"
    );
}

fn wait_for_counter(counter: &AtomicUsize, minimum: usize, label: &str) {
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while counter.load(Ordering::Acquire) < minimum {
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for {label}; observed {}",
            counter.load(Ordering::Acquire)
        );
        std::thread::yield_now();
    }
}
