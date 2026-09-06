use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr};
use std::num::NonZeroU16;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use nimbus_core::{DocumentId, TableName, TenantId};
use nimbus_engine::Engine;
use nimbus_network::{
    EndpointProtocol, IngressRouteId, ListenerId, LocalPortLeaseAuthority, NetworkAttachmentHandle,
    NetworkAttachmentId, NetworkCondition, NetworkConditionKind, NetworkConditionState,
    NetworkLeaseEpoch, NetworkPlan, NetworkPlanContentDigest, NetworkPlanId, NetworkProviderId,
    NetworkResourceGeneration, NetworkResourceId, NetworkResourcePhase, PortBindRealm,
    PortBindTarget, PortBindingSpec, PortBoundEndpoint, PortExposure, PortLeaseAccounting,
    PortLeaseFence, PortLeaseId, PortLeaseRequest, PortProtocol, PortPublicationIntent,
    PortRequestMode, PublishedEndpoint, PublishedEndpointHandle, PublishedEndpointId,
};
use nimbus_sandbox::{
    SandboxBackendKind, SandboxOwnerSpec, SandboxPortBinding, SandboxProcessSpec,
    SandboxProvisionEndpointIdentity, SandboxProvisionListener, SandboxProvisionNetworkPlan,
    SandboxRootSpec, SandboxSpec,
};
use nimbus_storage::{FaultInjector, FaultPoint};
use nimbus_testing::EngineFixture;
use serde_json::json;

use crate::SystemConnectivityProjectionRuntime;
use crate::keys::{
    connectivity_route_document_id, listener_document_id, port_document_id, service_document_id,
};
use crate::records::{
    SystemConnectivityObservationError, SystemPortListenerObservation,
    SystemPublishedEndpointObservation, SystemServiceConnectivityObservation,
    claim_server_listener_projection_async, record_port_listener_observation_async,
    record_service_connectivity_observation_async,
};
use crate::schema::{SystemTable, system_table_schemas};
use crate::system_tenant_id;

fn table_name(table: SystemTable) -> TableName {
    table.table_name().expect("system table name should parse")
}

fn provider_id(label: &str) -> NetworkProviderId {
    NetworkProviderId::for_registration_key(label)
}

fn conditions() -> Vec<NetworkCondition> {
    vec![
        NetworkCondition::new(NetworkConditionKind::Ready, NetworkConditionState::True),
        NetworkCondition::new(NetworkConditionKind::Published, NetworkConditionState::True),
        NetworkCondition::new(
            NetworkConditionKind::CleanupPending,
            NetworkConditionState::False,
        ),
    ]
}

#[derive(Default)]
struct PermanentProjectionFault {
    active: AtomicBool,
    failures: AtomicUsize,
}

impl FaultInjector for PermanentProjectionFault {
    fn check(&self, point: FaultPoint) -> nimbus_core::Result<()> {
        if self.active.load(Ordering::Acquire) {
            self.failures.fetch_add(1, Ordering::AcqRel);
            return Err(nimbus_core::Error::Internal(format!(
                "injected permanent projection failure at {}",
                point.as_str()
            )));
        }
        Ok(())
    }
}

fn request(
    tenant_id: Option<TenantId>,
    listener_id: &ListenerId,
    generation: NetworkResourceGeneration,
) -> PortLeaseRequest {
    request_with_identity_and_binding(
        tenant_id,
        PortLeaseId::for_listener(listener_id),
        NetworkResourceId::from(listener_id.clone()),
        generation,
        PortProtocol::Tcp,
        PortBindTarget::ipv4_specific(Ipv4Addr::LOCALHOST),
        PortRequestMode::range(
            NonZeroU16::new(18_000).expect("non-zero port"),
            NonZeroU16::new(29_000).expect("non-zero port"),
        )
        .expect("ordered range"),
    )
}

fn request_with_identity_and_binding(
    tenant_id: Option<TenantId>,
    lease_id: PortLeaseId,
    owner_id: NetworkResourceId,
    generation: NetworkResourceGeneration,
    protocol: PortProtocol,
    target: PortBindTarget,
    mode: PortRequestMode,
) -> PortLeaseRequest {
    PortLeaseRequest::new(
        lease_id,
        owner_id,
        tenant_id,
        PortLeaseFence::new(generation, NetworkLeaseEpoch::new(9)),
        PortLeaseAccounting::TenantPublished,
        PortPublicationIntent::host(IpAddr::V4(Ipv4Addr::LOCALHOST)),
        PortBindingSpec::new(
            protocol,
            PortBindRealm::Host,
            target,
            PortExposure::Loopback,
            mode,
        ),
    )
}

fn listener(
    tenant_id: Option<TenantId>,
    listener_id: ListenerId,
    generation: NetworkResourceGeneration,
    port: u16,
) -> SystemPortListenerObservation {
    let request = request(tenant_id, &listener_id, generation);
    let endpoint = PortBoundEndpoint::new(
        PortProtocol::Tcp,
        PortBindRealm::Host,
        PortBindTarget::ipv4_specific(Ipv4Addr::LOCALHOST),
        NonZeroU16::new(port).expect("non-zero port"),
    )
    .expect("bound endpoint should validate");
    SystemPortListenerObservation::new(
        "workload-ingress",
        "http",
        listener_id,
        request,
        endpoint,
        provider_id("system-connectivity-test"),
        NetworkResourcePhase::Ready,
        conditions(),
    )
    .expect("listener observation should validate")
}

struct ServiceProjectionFixture {
    spec: SandboxSpec,
    plan: SandboxProvisionNetworkPlan,
    attachment: NetworkAttachmentHandle,
}

fn service_fixture(
    tenant_id: &TenantId,
    service_name: &str,
    workload_incarnation: &str,
    endpoint_name: &str,
    protocol: EndpointProtocol,
    generation: NetworkResourceGeneration,
    guest_port: u16,
) -> ServiceProjectionFixture {
    service_fixture_with_lease_epoch(
        tenant_id,
        service_name,
        workload_incarnation,
        endpoint_name,
        protocol,
        generation,
        NetworkLeaseEpoch::new(9),
        guest_port,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "the fixture varies each service correlation and listener fence dimension"
)]
fn service_fixture_with_lease_epoch(
    tenant_id: &TenantId,
    service_name: &str,
    workload_incarnation: &str,
    endpoint_name: &str,
    protocol: EndpointProtocol,
    generation: NetworkResourceGeneration,
    lease_epoch: NetworkLeaseEpoch,
    guest_port: u16,
) -> ServiceProjectionFixture {
    let binding = SandboxPortBinding::new(endpoint_name, protocol, 0, guest_port);
    let spec = SandboxSpec::new(
        tenant_id.clone(),
        SandboxOwnerSpec::service(service_name),
        SandboxBackendKind::Krun,
        SandboxRootSpec::rootfs("/rootfs"),
        SandboxProcessSpec::new(["service"]),
    )
    .with_port_binding(binding.clone());
    let network_plan = NetworkPlan::new(
        NetworkPlanId::for_tenant_workload_plan(tenant_id, workload_incarnation),
        generation,
        NetworkPlanContentDigest::sha256(workload_incarnation.as_bytes()),
        nimbus_sandbox::sandbox_network_plan_requirements(SandboxBackendKind::Krun)
            .capability_requirements()
            .clone(),
    );
    let listener_id =
        ListenerId::for_tenant_workload_listener(tenant_id, workload_incarnation, endpoint_name);
    let endpoint_id =
        PublishedEndpointId::for_workload_endpoint(workload_incarnation, endpoint_name);
    let request = PortLeaseRequest::new(
        PortLeaseId::for_listener(&listener_id),
        NetworkResourceId::from(listener_id.clone()),
        Some(tenant_id.clone()),
        PortLeaseFence::new(generation, lease_epoch),
        PortLeaseAccounting::TenantPublished,
        PortPublicationIntent::host(IpAddr::V4(Ipv4Addr::LOCALHOST)),
        PortBindingSpec::new(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            PortBindTarget::ipv4_specific(Ipv4Addr::LOCALHOST),
            PortExposure::Loopback,
            PortRequestMode::ProviderAssigned,
        ),
    )
    .with_plan_id(network_plan.plan_id().clone());
    let attachment_id = NetworkAttachmentId::for_workload_attachment(
        &format!("{}/{workload_incarnation}", tenant_id.as_str()),
        "primary",
    );
    let plan = SandboxProvisionNetworkPlan::new(
        network_plan,
        tenant_id.clone(),
        generation,
        attachment_id.clone(),
        [SandboxProvisionEndpointIdentity::new(
            listener_id.clone(),
            endpoint_id.clone(),
        )],
        [SandboxProvisionListener::new(
            endpoint_id,
            listener_id,
            binding,
            request,
        )],
        [],
    )
    .expect("service projection plan should validate");
    ServiceProjectionFixture {
        spec,
        plan,
        attachment: NetworkAttachmentHandle::new(attachment_id, generation),
    }
}

fn planned_endpoint(
    fixture: &ServiceProjectionFixture,
    port: u16,
) -> SystemPublishedEndpointObservation {
    let planned = &fixture.plan.listeners()[0];
    let bound = PortBoundEndpoint::new(
        PortProtocol::Tcp,
        PortBindRealm::Host,
        PortBindTarget::ipv4_specific(Ipv4Addr::LOCALHOST),
        NonZeroU16::new(port).expect("non-zero port"),
    )
    .expect("bound endpoint should validate");
    let listener = SystemPortListenerObservation::new(
        "workload-ingress",
        match planned.binding().protocol {
            EndpointProtocol::Tcp => "tcp",
            EndpointProtocol::Http => "http",
            EndpointProtocol::Https => "https",
        },
        planned.listener_id().clone(),
        planned.port_lease().clone(),
        bound,
        provider_id("system-connectivity-test"),
        NetworkResourcePhase::Ready,
        conditions(),
    )
    .expect("planned listener observation should validate");
    let endpoint = PublishedEndpointHandle::new(
        planned.endpoint_id().clone(),
        fixture.plan.generation(),
        PublishedEndpoint::new(
            &planned.binding().name,
            planned.binding().protocol,
            format!("127.0.0.1:{port}")
                .parse()
                .expect("endpoint should parse"),
        )
        .with_guest_port(planned.binding().guest_port),
    );
    SystemPublishedEndpointObservation::new(
        IngressRouteId::for_published_endpoint(endpoint.endpoint_id()),
        endpoint,
        listener,
    )
    .expect("planned endpoint observation should validate")
}

#[test]
fn connectivity_routes_are_structurally_distinct_from_http_routes() {
    let schemas = system_table_schemas().expect("schemas should build");
    let http = schemas
        .iter()
        .find(|schema| schema.table == table_name(SystemTable::Routes))
        .expect("HTTP routes schema should exist");
    let connectivity = schemas
        .iter()
        .find(|schema| schema.table == table_name(SystemTable::ConnectivityRoutes))
        .expect("connectivity routes schema should exist");

    assert!(http.fields.iter().any(|field| field.name == "method"));
    assert!(http.fields.iter().any(|field| field.name == "path"));
    assert!(!http.fields.iter().any(|field| field.name == "routeId"));
    assert!(
        connectivity
            .fields
            .iter()
            .any(|field| field.name == "routeId")
    );
    assert_ne!(
        SystemTable::Routes.name(),
        SystemTable::ConnectivityRoutes.name()
    );

    let route_id = IngressRouteId::for_published_endpoint(
        &PublishedEndpointId::for_workload_endpoint("scope", "api"),
    );
    assert!(connectivity_route_document_id(&route_id).starts_with("connectivity-route:"));
}

#[test]
fn connectivity_route_keys_are_injective_and_disjoint_from_http_route_inventory() {
    let connectivity_keys = ["scope/a", "scope~2fa", "Scope/a", "scope a"]
        .into_iter()
        .map(|scope| {
            let endpoint_id = PublishedEndpointId::for_workload_endpoint(scope, "api/v1");
            connectivity_route_document_id(&IngressRouteId::for_published_endpoint(&endpoint_id))
        })
        .collect::<BTreeSet<_>>();
    let http_keys = crate::route_inventory()
        .into_iter()
        .map(|route| route.document_id())
        .collect::<BTreeSet<_>>();

    assert_eq!(connectivity_keys.len(), 4);
    assert!(connectivity_keys.is_disjoint(&http_keys));
    assert!(
        connectivity_keys
            .iter()
            .all(|key| key.starts_with("connectivity-route:"))
    );
    assert!(http_keys.iter().all(|key| key.starts_with("route:")));
}

#[tokio::test]
async fn listener_address_movement_preserves_stable_listener_and_port_documents() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let listener_id = ListenerId::for_workload_listener("system-test", "http");
    let generation = NetworkResourceGeneration::new(7);
    let first = listener(None, listener_id.clone(), generation, 18_080);
    record_port_listener_observation_async(&engine, &first)
        .await
        .expect("first observation should project");
    let moved = listener(None, listener_id.clone(), generation, 28_080);
    record_port_listener_observation_async(&engine, &moved)
        .await
        .expect("moved observation should project");

    let system_tenant = system_tenant_id().expect("system tenant should parse");
    let listener_document = engine
        .get_document_async(
            system_tenant.clone(),
            table_name(SystemTable::Listeners),
            DocumentId::from_key(listener_document_id(&listener_id)).expect("id should parse"),
        )
        .await
        .expect("listener should exist");
    let lease_id = PortLeaseId::for_listener(&listener_id);
    let port_document = engine
        .get_document_async(
            system_tenant,
            table_name(SystemTable::Ports),
            DocumentId::from_key(port_document_id(&lease_id)).expect("id should parse"),
        )
        .await
        .expect("port should exist");

    assert_eq!(listener_document.fields["listenerId"], json!(listener_id));
    assert_eq!(listener_document.fields["portLeaseId"], json!(lease_id));
    assert_eq!(listener_document.fields["generation"], json!("7"));
    assert_eq!(listener_document.fields["leaseEpoch"], json!("9"));
    assert_eq!(
        listener_document.fields["actualAddress"],
        json!("127.0.0.1:28080")
    );
    assert_eq!(listener_document.fields["cleanupState"], json!("clear"));
    assert_eq!(port_document.fields["hostPort"], json!(28_080));
}

#[tokio::test]
async fn server_listener_projection_replaces_only_the_prior_server_incarnation() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let generation = NetworkResourceGeneration::new(7);
    let stale_main_id = ListenerId::for_workload_listener("server:stale", "main:http");
    let stale_s3_id = ListenerId::for_workload_listener("server:stale", "s3");
    let machine_id = ListenerId::for_workload_listener("managed-machine:dev", "ssh");
    let stale = [
        listener(None, stale_main_id.clone(), generation, 18_080),
        listener(None, stale_s3_id.clone(), generation, 19_000),
    ];
    for observation in &stale {
        record_port_listener_observation_async(&engine, observation)
            .await
            .expect("prior observation should project");
    }
    record_port_listener_observation_async(
        &engine,
        &listener(None, machine_id.clone(), generation, 22_022).with_machine_id("dev"),
    )
    .await
    .expect("machine observation should project");

    let current_main_id = ListenerId::for_workload_listener("server:current", "main:http");
    let current_s3_id = ListenerId::for_workload_listener("server:current", "s3");
    let current = [
        listener(None, current_main_id.clone(), generation, 28_080),
        listener(None, current_s3_id.clone(), generation, 29_000),
    ];
    claim_server_listener_projection_async(&engine, "server:current")
        .await
        .expect("current server incarnation should claim the inventory");
    let runtime = SystemConnectivityProjectionRuntime::new(&engine);
    runtime.project_server_listeners("server:current", current.clone());
    wait_for_listener_address(
        &engine,
        &DocumentId::from_key(listener_document_id(&current_s3_id))
            .expect("current listener id should parse"),
        "127.0.0.1:29000",
    )
    .await;
    crate::records::replace_server_port_listener_observations_async(
        &engine,
        "server:current",
        &current,
    )
    .await
    .expect("repeating the complete current inventory should be idempotent");
    crate::records::replace_server_port_listener_observations_async(
        &engine,
        "server:stale",
        &stale,
    )
    .await
    .expect("a superseded server projection should retire without changing current rows");

    let system_tenant = system_tenant_id().expect("system tenant should parse");
    let listeners = engine
        .list_documents_async(system_tenant.clone(), table_name(SystemTable::Listeners))
        .await
        .expect("listener inventory should list");
    let ports = engine
        .list_documents_async(system_tenant.clone(), table_name(SystemTable::Ports))
        .await
        .expect("port inventory should list");
    assert_eq!(
        listeners.len(),
        3,
        "two current server rows plus one machine row"
    );
    assert_eq!(
        ports.len(),
        3,
        "two current server rows plus one machine row"
    );
    for stale_id in [stale_main_id, stale_s3_id] {
        assert!(
            engine
                .get_document_async(
                    system_tenant.clone(),
                    table_name(SystemTable::Listeners),
                    DocumentId::from_key(listener_document_id(&stale_id))
                        .expect("stale listener id should parse"),
                )
                .await
                .is_err(),
            "the earlier server incarnation must leave the current inventory",
        );
    }
    engine
        .get_document_async(
            system_tenant,
            table_name(SystemTable::Listeners),
            DocumentId::from_key(listener_document_id(&machine_id))
                .expect("machine listener id should parse"),
        )
        .await
        .expect("server replacement must preserve machine-owned listeners");
}

#[tokio::test]
async fn stale_listener_projection_cannot_replace_a_newer_fence() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let listener_id = ListenerId::for_workload_listener("projection-independence", "http");
    let newer = listener(
        None,
        listener_id.clone(),
        NetworkResourceGeneration::new(8),
        28_080,
    );
    let stale = listener(
        None,
        listener_id.clone(),
        NetworkResourceGeneration::new(7),
        18_080,
    );
    record_port_listener_observation_async(&engine, &newer)
        .await
        .expect("newer listener should project");
    record_port_listener_observation_async(&engine, &stale)
        .await
        .expect("stale listener should be an idempotent no-op");

    let document = engine
        .get_document_async(
            system_tenant_id().expect("system tenant should parse"),
            table_name(SystemTable::Listeners),
            DocumentId::from_key(listener_document_id(&listener_id)).expect("id should parse"),
        )
        .await
        .expect("listener should exist");
    assert_eq!(document.fields["generation"], json!("8"));
    assert_eq!(document.fields["actualAddress"], json!("127.0.0.1:28080"));
}

#[tokio::test]
async fn crossed_listener_and_lease_identity_fails_before_any_write() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let listener_id = ListenerId::for_workload_listener("system-test", "expected");
    let crossed_id = ListenerId::for_workload_listener("system-test", "crossed");
    let generation = NetworkResourceGeneration::new(7);
    let crossed_request = request(None, &crossed_id, generation);
    let endpoint = PortBoundEndpoint::new(
        PortProtocol::Tcp,
        PortBindRealm::Host,
        PortBindTarget::ipv4_specific(Ipv4Addr::LOCALHOST),
        NonZeroU16::new(18_080).expect("non-zero port"),
    )
    .expect("bound endpoint should validate");

    let error = SystemPortListenerObservation::new(
        "workload-ingress",
        "http",
        listener_id,
        crossed_request,
        endpoint,
        provider_id("system-connectivity-test"),
        NetworkResourcePhase::Ready,
        conditions(),
    )
    .expect_err("crossed owner must fail");
    assert!(error.to_string().contains("listener"));

    assert!(
        engine
            .list_documents_async(
                system_tenant_id().expect("system tenant should parse"),
                table_name(SystemTable::Listeners),
            )
            .await
            .is_err(),
        "constructor rejection must happen before system schema or document writes"
    );
}

#[tokio::test]
async fn crossed_connectivity_dimensions_fail_before_any_projection_write() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_a = TenantId::new("tenant-a").expect("tenant should parse");
    let tenant_b = TenantId::new("tenant-b").expect("tenant should parse");
    let generation = NetworkResourceGeneration::new(7);
    let listener_id =
        ListenerId::for_tenant_workload_listener(&tenant_a, "search-incarnation", "http");
    let crossed_listener_id =
        ListenerId::for_tenant_workload_listener(&tenant_a, "other-incarnation", "http");
    let endpoint = PortBoundEndpoint::new(
        PortProtocol::Tcp,
        PortBindRealm::Host,
        PortBindTarget::ipv4_specific(Ipv4Addr::LOCALHOST),
        NonZeroU16::new(18_080).expect("non-zero port"),
    )
    .expect("bound endpoint should validate");

    let listener_errors = [
        (
            SystemPortListenerObservation::new(
                "workload-ingress",
                "http",
                listener_id.clone(),
                request(Some(tenant_a.clone()), &crossed_listener_id, generation),
                endpoint.clone(),
                provider_id("system-connectivity-test"),
                NetworkResourcePhase::Ready,
                conditions(),
            )
            .expect_err("crossed owner should fail"),
            SystemConnectivityObservationError::ListenerOwnerMismatch,
        ),
        (
            SystemPortListenerObservation::new(
                "workload-ingress",
                "http",
                listener_id.clone(),
                request_with_identity_and_binding(
                    Some(tenant_a.clone()),
                    PortLeaseId::for_listener(&crossed_listener_id),
                    NetworkResourceId::from(listener_id.clone()),
                    generation,
                    PortProtocol::Tcp,
                    PortBindTarget::ipv4_specific(Ipv4Addr::LOCALHOST),
                    PortRequestMode::Exact(NonZeroU16::new(18_080).expect("non-zero port")),
                ),
                endpoint.clone(),
                provider_id("system-connectivity-test"),
                NetworkResourcePhase::Ready,
                conditions(),
            )
            .expect_err("crossed lease id should fail"),
            SystemConnectivityObservationError::ListenerLeaseMismatch,
        ),
        (
            SystemPortListenerObservation::new(
                "workload-ingress",
                "http",
                listener_id.clone(),
                request(Some(tenant_a.clone()), &listener_id, generation),
                PortBoundEndpoint::new(
                    PortProtocol::Tcp,
                    PortBindRealm::Host,
                    PortBindTarget::ipv4_wildcard(),
                    NonZeroU16::new(18_080).expect("non-zero port"),
                )
                .expect("bound endpoint should validate"),
                provider_id("system-connectivity-test"),
                NetworkResourcePhase::Ready,
                conditions(),
            )
            .expect_err("crossed binding should fail"),
            SystemConnectivityObservationError::BindingMismatch,
        ),
    ];
    for (actual, expected) in listener_errors {
        assert_eq!(actual, expected);
    }

    let valid_listener = listener(
        Some(tenant_a.clone()),
        listener_id.clone(),
        generation,
        18_080,
    );
    let endpoint_id = PublishedEndpointId::for_workload_endpoint("tenant-a/search", "http");
    let valid_endpoint = PublishedEndpointHandle::new(
        endpoint_id.clone(),
        generation,
        PublishedEndpoint::new(
            "http",
            EndpointProtocol::Http,
            "127.0.0.1:18080".parse().expect("endpoint should parse"),
        ),
    );
    assert_eq!(
        SystemPublishedEndpointObservation::new(
            IngressRouteId::for_published_endpoint(&PublishedEndpointId::for_workload_endpoint(
                "tenant-a/search",
                "other"
            )),
            valid_endpoint.clone(),
            valid_listener.clone(),
        )
        .expect_err("crossed route should fail"),
        SystemConnectivityObservationError::EndpointRouteMismatch
    );
    assert_eq!(
        SystemPublishedEndpointObservation::new(
            IngressRouteId::for_published_endpoint(&endpoint_id),
            PublishedEndpointHandle::new(
                endpoint_id.clone(),
                NetworkResourceGeneration::new(8),
                valid_endpoint.endpoint().clone(),
            ),
            valid_listener.clone(),
        )
        .expect_err("crossed generation should fail"),
        SystemConnectivityObservationError::EndpointGenerationMismatch
    );
    assert_eq!(
        SystemPublishedEndpointObservation::new(
            IngressRouteId::for_published_endpoint(&endpoint_id),
            PublishedEndpointHandle::new(
                endpoint_id.clone(),
                generation,
                PublishedEndpoint::new(
                    "http",
                    EndpointProtocol::Http,
                    "127.0.0.1:18081".parse().expect("endpoint should parse"),
                ),
            ),
            valid_listener.clone(),
        )
        .expect_err("crossed address should fail"),
        SystemConnectivityObservationError::EndpointAddressMismatch
    );

    let udp_listener_id =
        ListenerId::for_tenant_workload_listener(&tenant_a, "udp-incarnation", "udp");
    let udp_listener = SystemPortListenerObservation::new(
        "workload-ingress",
        "udp",
        udp_listener_id.clone(),
        request_with_identity_and_binding(
            Some(tenant_a.clone()),
            PortLeaseId::for_listener(&udp_listener_id),
            NetworkResourceId::from(udp_listener_id),
            generation,
            PortProtocol::Udp,
            PortBindTarget::ipv4_specific(Ipv4Addr::LOCALHOST),
            PortRequestMode::Exact(NonZeroU16::new(18_080).expect("non-zero port")),
        ),
        PortBoundEndpoint::new(
            PortProtocol::Udp,
            PortBindRealm::Host,
            PortBindTarget::ipv4_specific(Ipv4Addr::LOCALHOST),
            NonZeroU16::new(18_080).expect("non-zero port"),
        )
        .expect("UDP endpoint should validate"),
        provider_id("system-connectivity-test"),
        NetworkResourcePhase::Ready,
        conditions(),
    )
    .expect("UDP listener evidence should validate");
    assert_eq!(
        SystemPublishedEndpointObservation::new(
            IngressRouteId::for_published_endpoint(&endpoint_id),
            valid_endpoint.clone(),
            udp_listener,
        )
        .expect_err("crossed protocol should fail"),
        SystemConnectivityObservationError::EndpointProtocolMismatch
    );

    let search = service_fixture(
        &tenant_a,
        "search",
        "search-incarnation",
        "http",
        EndpointProtocol::Http,
        generation,
        8_080,
    );
    let billing = service_fixture(
        &tenant_a,
        "billing",
        "billing-incarnation",
        "http",
        EndpointProtocol::Http,
        generation,
        8_081,
    );
    let tenant_b_search = service_fixture(
        &tenant_b,
        "search",
        "search-incarnation",
        "http",
        EndpointProtocol::Http,
        generation,
        8_080,
    );
    assert_eq!(
        SystemServiceConnectivityObservation::new(
            &search.spec,
            &search.plan,
            1,
            search.attachment.clone(),
            provider_id("system-attachment-test"),
            NetworkResourcePhase::Ready,
            conditions(),
            [planned_endpoint(&tenant_b_search, 18_082)],
        )
        .expect_err("crossed tenant should fail"),
        SystemConnectivityObservationError::ServiceTenantMismatch
    );
    assert_eq!(
        SystemServiceConnectivityObservation::new(
            &search.spec,
            &search.plan,
            1,
            NetworkAttachmentHandle::new(
                search.plan.attachment_id().clone(),
                NetworkResourceGeneration::new(8),
            ),
            provider_id("system-attachment-test"),
            NetworkResourcePhase::Ready,
            conditions(),
            [planned_endpoint(&search, 18_080)],
        )
        .expect_err("crossed attachment generation should fail"),
        SystemConnectivityObservationError::ServiceAttachmentMismatch
    );
    assert_eq!(
        SystemServiceConnectivityObservation::new(
            &search.spec,
            &search.plan,
            1,
            search.attachment.clone(),
            provider_id("system-attachment-test"),
            NetworkResourcePhase::Ready,
            conditions(),
            [planned_endpoint(&billing, 18_081)],
        )
        .expect_err("another service's valid tuple must fail"),
        SystemConnectivityObservationError::ServicePlanCorrelationMismatch
    );
    let mut crossed_source = search.spec.clone();
    crossed_source.port_bindings[0] = billing.spec.port_bindings[0].clone();
    assert_eq!(
        SystemServiceConnectivityObservation::new(
            &crossed_source,
            &search.plan,
            1,
            search.attachment.clone(),
            provider_id("system-attachment-test"),
            NetworkResourcePhase::Ready,
            conditions(),
            [planned_endpoint(&search, 18_080)],
        )
        .expect_err("source bindings crossed with a valid plan must fail"),
        SystemConnectivityObservationError::ServicePlanCorrelationMismatch
    );

    assert!(
        engine
            .list_documents_async(
                system_tenant_id().expect("system tenant should parse"),
                table_name(SystemTable::Listeners),
            )
            .await
            .is_err(),
        "all crossed evidence must fail before schema or document writes"
    );
}

#[tokio::test]
async fn service_projection_writes_stable_child_identity_and_removes_only_its_stale_rows() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = TenantId::new("tenant-a").expect("tenant should parse");
    let generation = NetworkResourceGeneration::new(11);
    let search_fixture = service_fixture(
        &tenant_id,
        "search",
        "search-incarnation",
        "http",
        EndpointProtocol::Http,
        generation,
        8_080,
    );
    let billing_fixture = service_fixture(
        &tenant_id,
        "billing",
        "billing-incarnation",
        "http",
        EndpointProtocol::Http,
        generation,
        8_081,
    );
    let attachment = search_fixture.attachment.clone();
    let attachment_provider = provider_id("system-attachment-test");
    let search_listener_id = search_fixture.plan.listeners()[0].listener_id().clone();
    let search_endpoint_id = search_fixture.plan.listeners()[0].endpoint_id().clone();
    let search_route_id = IngressRouteId::for_published_endpoint(&search_endpoint_id);
    let search = SystemServiceConnectivityObservation::new(
        &search_fixture.spec,
        &search_fixture.plan,
        4,
        attachment.clone(),
        attachment_provider.clone(),
        NetworkResourcePhase::Ready,
        conditions(),
        [planned_endpoint(&search_fixture, 18_080)],
    )
    .expect("service observation should validate");
    record_service_connectivity_observation_async(&engine, &search)
        .await
        .expect("search should project");

    let moved_search = SystemServiceConnectivityObservation::new(
        &search_fixture.spec,
        &search_fixture.plan,
        4,
        attachment.clone(),
        attachment_provider.clone(),
        NetworkResourcePhase::Ready,
        conditions(),
        [planned_endpoint(&search_fixture, 28_080)],
    )
    .expect("moved service observation should validate");
    record_service_connectivity_observation_async(&engine, &moved_search)
        .await
        .expect("moved search should update its stable projection");

    let system_tenant = system_tenant_id().expect("system tenant should parse");
    let search_service_id = service_document_id(&tenant_id, "search");
    let search_lease_id = PortLeaseId::for_listener(&search_listener_id);
    engine
        .delete_document_async(
            system_tenant.clone(),
            table_name(SystemTable::Ports),
            DocumentId::from_key(port_document_id(&search_lease_id))
                .expect("port document id should parse"),
        )
        .await
        .expect("one projected child should delete");
    record_service_connectivity_observation_async(&engine, &moved_search)
        .await
        .expect("equal-fence replay should repair a missing child");
    let search_documents = [
        (
            SystemTable::Listeners,
            listener_document_id(&search_listener_id),
        ),
        (SystemTable::Ports, port_document_id(&search_lease_id)),
        (
            SystemTable::ConnectivityRoutes,
            connectivity_route_document_id(&search_route_id),
        ),
    ];
    for (table, document_id) in &search_documents {
        let document = engine
            .get_document_async(
                system_tenant.clone(),
                table_name(*table),
                DocumentId::from_key(document_id.clone()).expect("document id should parse"),
            )
            .await
            .expect("moved connectivity document should retain its stable key");
        assert_eq!(document.fields["actualAddress"], json!("127.0.0.1:28080"));
        assert_eq!(document.fields["generation"], json!("11"));
        assert_eq!(
            document.fields["providerId"],
            json!(provider_id("system-connectivity-test"))
        );
        assert_eq!(document.fields["cleanupState"], json!("clear"));
    }
    let search_service = engine
        .get_document_async(
            system_tenant.clone(),
            table_name(SystemTable::Services),
            DocumentId::from_key(search_service_id.clone()).expect("service id should parse"),
        )
        .await
        .expect("search service should exist");
    assert_eq!(
        search_service.fields["attachmentId"],
        json!(attachment.attachment_id())
    );
    assert_eq!(search_service.fields["generation"], json!("11"));
    assert_eq!(
        search_service.fields["attachmentProviderId"],
        json!(attachment_provider)
    );
    assert_eq!(search_service.fields["cleanupState"], json!("clear"));
    assert_eq!(
        search_service.fields["endpoints"][0]["endpointId"],
        json!(search_endpoint_id)
    );

    let billing = SystemServiceConnectivityObservation::new(
        &billing_fixture.spec,
        &billing_fixture.plan,
        2,
        billing_fixture.attachment.clone(),
        attachment_provider,
        NetworkResourcePhase::Ready,
        conditions(),
        [planned_endpoint(&billing_fixture, 18_081)],
    )
    .expect("service observation should validate");
    record_service_connectivity_observation_async(&engine, &billing)
        .await
        .expect("billing should project");

    let updated_search = SystemServiceConnectivityObservation::new(
        &search_fixture.spec,
        &search_fixture.plan,
        5,
        attachment,
        provider_id("system-attachment-test"),
        NetworkResourcePhase::Ready,
        conditions(),
        [],
    )
    .expect("empty endpoint replacement should validate");
    record_service_connectivity_observation_async(&engine, &updated_search)
        .await
        .expect("updated search should project");

    for table in [
        SystemTable::Listeners,
        SystemTable::Ports,
        SystemTable::ConnectivityRoutes,
    ] {
        let documents = engine
            .list_documents_async(system_tenant.clone(), table_name(table))
            .await
            .expect("connectivity children should list");
        assert_eq!(
            documents.len(),
            1,
            "only the billing child should remain in {}",
            table.name()
        );
        assert!(documents.iter().all(|document| {
            document.fields.get("serviceId")
                == Some(&json!(service_document_id(&tenant_id, "billing")))
        }));
    }
    for (table, document_id) in search_documents {
        assert!(
            engine
                .get_document_async(
                    system_tenant.clone(),
                    table_name(table),
                    DocumentId::from_key(document_id).expect("document id should parse"),
                )
                .await
                .is_err(),
            "stale search child should be removed from {}",
            table.name()
        );
    }
}

#[tokio::test]
async fn stale_service_projection_cannot_delete_newer_children() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = TenantId::new("projection-independence").expect("tenant should parse");
    let fixture = service_fixture_with_lease_epoch(
        &tenant_id,
        "search",
        "search-incarnation",
        "http",
        EndpointProtocol::Http,
        NetworkResourceGeneration::new(8),
        NetworkLeaseEpoch::new(10),
        8_080,
    );
    let current = SystemServiceConnectivityObservation::new(
        &fixture.spec,
        &fixture.plan,
        8,
        fixture.attachment.clone(),
        provider_id("system-attachment-test"),
        NetworkResourcePhase::Ready,
        conditions(),
        [planned_endpoint(&fixture, 28_080)],
    )
    .expect("current service observation should validate");
    let runtime = SystemConnectivityProjectionRuntime::new(&engine);
    runtime.project_service_connectivity(current.clone());
    let current_listener = fixture.plan.listeners()[0].listener_id();
    let current_listener_document = DocumentId::from_key(listener_document_id(current_listener))
        .expect("listener id should parse");
    wait_for_listener_address(&engine, &current_listener_document, "127.0.0.1:28080").await;

    let stale = SystemServiceConnectivityObservation::new(
        &fixture.spec,
        &fixture.plan,
        7,
        fixture.attachment.clone(),
        provider_id("system-attachment-test"),
        NetworkResourcePhase::Ready,
        conditions(),
        [],
    )
    .expect("stale service observation should validate");
    record_service_connectivity_observation_async(&engine, &stale)
        .await
        .expect("stale service should be an idempotent no-op");

    let stale_attachment_fixture = service_fixture(
        &tenant_id,
        "search",
        "search-incarnation",
        "http",
        EndpointProtocol::Http,
        NetworkResourceGeneration::new(7),
        8_080,
    );
    let stale_attachment = SystemServiceConnectivityObservation::new(
        &stale_attachment_fixture.spec,
        &stale_attachment_fixture.plan,
        8,
        stale_attachment_fixture.attachment.clone(),
        provider_id("system-attachment-test"),
        NetworkResourcePhase::Ready,
        conditions(),
        [planned_endpoint(&stale_attachment_fixture, 18_080)],
    )
    .expect("stale attachment observation should validate");
    record_service_connectivity_observation_async(&engine, &stale_attachment)
        .await
        .expect("stale attachment should be an idempotent no-op");

    let stale_listener_fixture = service_fixture(
        &tenant_id,
        "search",
        "search-incarnation",
        "http",
        EndpointProtocol::Http,
        NetworkResourceGeneration::new(8),
        8_080,
    );
    let stale_listener = SystemServiceConnectivityObservation::new(
        &stale_listener_fixture.spec,
        &stale_listener_fixture.plan,
        8,
        stale_listener_fixture.attachment.clone(),
        provider_id("system-attachment-test"),
        NetworkResourcePhase::Ready,
        conditions(),
        [planned_endpoint(&stale_listener_fixture, 18_080)],
    )
    .expect("stale listener observation should validate");
    runtime.project_service_connectivity(stale_listener.clone());
    assert_eq!(runtime.retained_entry_count_for_testing(), 1);

    let system_tenant = system_tenant_id().expect("system tenant should parse");
    for table in [
        SystemTable::Listeners,
        SystemTable::Ports,
        SystemTable::ConnectivityRoutes,
    ] {
        let table_name = table_name(table);
        for document in engine
            .list_documents_async(system_tenant.clone(), table_name.clone())
            .await
            .expect("current children should list before deletion")
        {
            engine
                .delete_document_async(system_tenant.clone(), table_name.clone(), document.id)
                .await
                .expect("projection child should delete");
        }
    }
    record_service_connectivity_observation_async(&engine, &stale_listener)
        .await
        .expect("stale listener should remain a no-op after child loss");
    for table in [
        SystemTable::Listeners,
        SystemTable::Ports,
        SystemTable::ConnectivityRoutes,
    ] {
        assert!(
            engine
                .list_documents_async(system_tenant.clone(), table_name(table))
                .await
                .expect("deleted projection children should list")
                .is_empty(),
            "a stale listener fence must not recreate deleted {} rows",
            table.name()
        );
    }
    let service_document = engine
        .get_document_async(
            system_tenant.clone(),
            table_name(SystemTable::Services),
            DocumentId::from_key(service_document_id(&tenant_id, "search"))
                .expect("id should parse"),
        )
        .await
        .expect("current service parent should retain its listener fences");
    assert_eq!(
        service_document.fields["endpoints"][0]["listenerGeneration"],
        json!("8")
    );
    assert_eq!(
        service_document.fields["endpoints"][0]["listenerLeaseEpoch"],
        json!("10")
    );
    record_service_connectivity_observation_async(&engine, &current)
        .await
        .expect("equal current replay should repair deleted children");
    wait_for_listener_address(&engine, &current_listener_document, "127.0.0.1:28080").await;

    let service = engine
        .get_document_async(
            system_tenant.clone(),
            table_name(SystemTable::Services),
            DocumentId::from_key(service_document_id(&tenant_id, "search"))
                .expect("id should parse"),
        )
        .await
        .expect("current service row should remain");
    assert_eq!(service.fields["sourceGeneration"], json!("8"));
    for table in [
        SystemTable::Listeners,
        SystemTable::Ports,
        SystemTable::ConnectivityRoutes,
    ] {
        let rows = engine
            .list_documents_async(system_tenant.clone(), table_name(table))
            .await
            .expect("current child rows should list");
        assert_eq!(rows.len(), 1, "stale service must retain {}", table.name());
        assert_eq!(rows[0].fields["actualAddress"], json!("127.0.0.1:28080"));
        if table != SystemTable::ConnectivityRoutes {
            assert_eq!(rows[0].fields["leaseEpoch"], json!("10"));
        }
    }
}

#[tokio::test]
async fn projection_rebuild_restores_deleted_rows_without_touching_authority() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let runtime = SystemConnectivityProjectionRuntime::new(&engine);
    let source_tenant = TenantId::new("projection-authority").expect("tenant should parse");
    let listener_id = ListenerId::for_workload_listener("projection-rebuild", "http");
    let generation = NetworkResourceGeneration::new(11);
    let authority = LocalPortLeaseAuthority::open(fixture.data_dir().join("network-authority"))
        .expect("network authority should open");
    authority
        .reserve(request(
            Some(source_tenant.clone()),
            &listener_id,
            generation,
        ))
        .expect("source lease should reserve");
    let authority_before =
        std::fs::read(authority.authority_path()).expect("authority bytes should read");

    runtime.project_port_listener(listener(
        Some(source_tenant.clone()),
        listener_id.clone(),
        generation,
        18_080,
    ));
    runtime.project_port_listener(listener(
        Some(source_tenant.clone()),
        listener_id.clone(),
        generation,
        28_080,
    ));
    runtime.project_port_listener(listener(
        Some(source_tenant),
        listener_id.clone(),
        NetworkResourceGeneration::new(10),
        18_081,
    ));
    assert_eq!(runtime.retained_entry_count_for_testing(), 1);

    let tenant_id = system_tenant_id().expect("system tenant should parse");
    let listener_document =
        DocumentId::from_key(listener_document_id(&listener_id)).expect("listener id should parse");
    let lease_id = PortLeaseId::for_listener(&listener_id);
    let port_document =
        DocumentId::from_key(port_document_id(&lease_id)).expect("port id should parse");
    wait_for_listener_address(&engine, &listener_document, "127.0.0.1:28080").await;

    engine
        .delete_document_async(
            tenant_id.clone(),
            table_name(SystemTable::Listeners),
            listener_document.clone(),
        )
        .await
        .expect("listener projection should delete");
    engine
        .delete_document_async(tenant_id, table_name(SystemTable::Ports), port_document)
        .await
        .expect("port projection should delete");

    runtime.rebuild();
    wait_for_listener_address(&engine, &listener_document, "127.0.0.1:28080").await;
    assert_eq!(runtime.retained_entry_count_for_testing(), 1);
    assert_eq!(
        std::fs::read(authority.authority_path()).expect("authority bytes should reread"),
        authority_before,
        "projection rebuild must not mutate source network authority"
    );
}

#[tokio::test]
async fn retained_projection_automatically_repairs_deleted_rows() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let runtime = SystemConnectivityProjectionRuntime::new_with_rebuild_interval_for_testing(
        &engine,
        std::time::Duration::from_millis(50),
    );
    let listener_id = ListenerId::for_workload_listener("projection-auto-rebuild", "http");
    let listener_document =
        DocumentId::from_key(listener_document_id(&listener_id)).expect("listener id should parse");
    runtime.project_port_listener(listener(
        None,
        listener_id,
        NetworkResourceGeneration::new(12),
        28_081,
    ));
    wait_for_listener_address(&engine, &listener_document, "127.0.0.1:28081").await;

    engine
        .delete_document_async(
            system_tenant_id().expect("system tenant should parse"),
            table_name(SystemTable::Listeners),
            listener_document.clone(),
        )
        .await
        .expect("listener projection should delete");

    wait_for_listener_address(&engine, &listener_document, "127.0.0.1:28081").await;
    assert_eq!(runtime.retained_entry_count_for_testing(), 1);
}

#[tokio::test]
async fn projection_retry_coalesces_backs_off_and_cancels_with_engine() {
    let fault = Arc::new(PermanentProjectionFault::default());
    let engine_fault: Arc<dyn FaultInjector> = fault.clone();
    let fixture = EngineFixture::new(move |path| {
        Engine::new_with_simulation(path, Arc::new(nimbus_core::SystemWallClock), engine_fault)
    });
    let engine = fixture.engine();
    let runtime = SystemConnectivityProjectionRuntime::new(&engine);
    let listener_id = ListenerId::for_workload_listener("projection-backoff", "http");
    fault.active.store(true, Ordering::Release);
    for port in 18_080..18_100 {
        runtime.project_port_listener(listener(
            None,
            listener_id.clone(),
            NetworkResourceGeneration::new(11),
            port,
        ));
    }
    assert_eq!(runtime.retained_entry_count_for_testing(), 1);

    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while fault.failures.load(Ordering::Acquire) < 3 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("permanent failure should reach bounded retries");
    let after_three = fault.failures.load(Ordering::Acquire);
    tokio::time::sleep(std::time::Duration::from_millis(175)).await;
    let after_backoff = fault.failures.load(Ordering::Acquire);
    assert!(
        after_backoff.saturating_sub(after_three) <= 2,
        "bounded backoff must prevent a permanent-failure busy loop"
    );
    assert!(runtime.driver_running_for_testing());

    drop(engine);
    drop(fixture);
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while runtime.driver_running_for_testing() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("engine shutdown must cancel and drop the projection driver");
}

async fn wait_for_listener_address(
    engine: &std::sync::Arc<Engine>,
    document_id: &DocumentId,
    expected: &str,
) {
    let tenant_id = system_tenant_id().expect("system tenant should parse");
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if engine
                .get_document_async(
                    tenant_id.clone(),
                    table_name(SystemTable::Listeners),
                    document_id.clone(),
                )
                .await
                .is_ok_and(|document| document.fields["actualAddress"] == json!(expected))
            {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("retained projection should converge");
}

#[tokio::test]
async fn connectivity_projection_does_not_wait_on_source_tenant_delete_fence() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    crate::records::ensure_system_tenant_async(&engine)
        .await
        .expect("system tenant should prepare before the source delete fence");
    let tenant_id = TenantId::new("deleting-connectivity-owner").expect("tenant should parse");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("application tenant should create");
    let deletion = engine
        .begin_tenant_delete_async(tenant_id.clone())
        .await
        .expect("application tenant delete fence should begin");
    let stopped = service_fixture(
        &tenant_id,
        "stopped-service",
        "stopped-service-incarnation",
        "http",
        EndpointProtocol::Http,
        NetworkResourceGeneration::new(1),
        8_080,
    );
    let observation = SystemServiceConnectivityObservation::new(
        &stopped.spec,
        &stopped.plan,
        1,
        stopped.attachment,
        provider_id("system-attachment-test"),
        NetworkResourcePhase::Released,
        [NetworkCondition::new(
            NetworkConditionKind::CleanupPending,
            NetworkConditionState::False,
        )],
        [],
    )
    .expect("released service observation should validate");

    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        record_service_connectivity_observation_async(&engine, &observation),
    )
    .await
    .expect("system projection must not wait on an unrelated tenant load fence")
    .expect("released service observation should project");

    engine
        .finish_tenant_delete_async(deletion)
        .await
        .expect("application tenant delete should finish");
}
