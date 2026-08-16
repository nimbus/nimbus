use std::net::{Ipv4Addr, TcpListener};

use nimbus_network::{
    ListenerId, LocalPortLeaseAuthority, NetworkAttachmentId, NetworkLeaseEpoch, NetworkPlanId,
    NetworkProviderHandle, NetworkProviderId, NetworkReservationClaim, NetworkResourceGeneration,
    NetworkResourceId, PortBindRealm, PortBindTarget, PortBindingSpec, PortExposure,
    PortLeaseAccounting, PortLeaseFence, PortLeaseId, PortLeasePhase, PortLeaseRequest,
    PortProtocol, PortPublicationIntent, PortRequestMode,
};

use super::*;

fn planned_request(
    tenant_id: &nimbus_core::TenantId,
    plan_id: &NetworkPlanId,
    name: &str,
    published: bool,
) -> PortLeaseRequest {
    let listener_id = ListenerId::for_tenant_workload_listener(tenant_id, "workload-a", name);
    PortLeaseRequest::new(
        PortLeaseId::for_listener(&listener_id),
        NetworkResourceId::from(listener_id),
        Some(tenant_id.clone()),
        PortLeaseFence::new(NetworkResourceGeneration::new(7), NetworkLeaseEpoch::new(1)),
        if published {
            PortLeaseAccounting::TenantPublished
        } else {
            PortLeaseAccounting::HostInternal
        },
        if published {
            PortPublicationIntent::host(Ipv4Addr::LOCALHOST.into())
        } else {
            PortPublicationIntent::Unpublished
        },
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

fn reservation_claim() -> NetworkReservationClaim {
    NetworkReservationClaim::new(
        NetworkProviderHandle::new(
            NetworkProviderId::for_registration_key("nimbus-server.owner-exit-test"),
            "owner-exit-reservation",
        )
        .expect("fixture reservation handle should validate"),
    )
}

#[test]
fn published_batch_drop_retains_complete_plan_for_fresh_owner_rebind() {
    let state_root = tempfile::tempdir().expect("fixture state root should exist");
    let port_leases =
        LocalPortLeaseAuthority::open(state_root.path()).expect("port authority should open");
    let listeners = ServerListenerLeaseAuthority::reconstruct_direct(state_root.path())
        .expect("server listener authority should reconstruct");
    let tenant_id =
        nimbus_core::TenantId::new("tenant-owner-exit").expect("tenant identity should validate");
    let plan_id = NetworkPlanId::for_tenant_workload_plan(&tenant_id, "workload-a");
    let published = planned_request(&tenant_id, &plan_id, "http", true);
    let unrelated = planned_request(&tenant_id, &plan_id, "pep", false);
    let requested_plan = vec![published.clone(), unrelated.clone()];
    let claim = reservation_claim();
    port_leases
        .reserve_batch_for_coordinator(requested_plan, &claim)
        .expect("launch owner should reserve the complete plan");
    let plan_members = listeners
        .authenticate_workload_ingress_plan(
            &plan_id,
            &tenant_id,
            NetworkResourceGeneration::new(7),
            std::slice::from_ref(&published),
            &claim,
        )
        .expect("server should authenticate the complete durable plan");
    let prepared = listeners
        .prepare_workload_ingress(Some(&plan_members), published.clone(), &claim)
        .expect("server should claim the published member");
    let listener = TcpListener::bind(
        prepared
            .bind_addr()
            .expect("published bind address should resolve"),
    )
    .expect("published listener should bind");
    let adopted = prepared
        .adopt_std(listener)
        .expect("server should adopt the published listener");
    let route = RunningIngressRoute::start(
        ExpectedRoute {
            listener_id: ListenerId::for_tenant_workload_listener(&tenant_id, "workload-a", "http"),
            request: published.clone(),
            upstream: (Ipv4Addr::LOCALHOST, 9).into(),
        },
        adopted,
        DEFAULT_MAX_ACTIVE_CONNECTIONS,
    )
    .expect("published route should start");
    let published_address = route.bound_addr;
    let batch = RunningIngressBatch {
        execution_id: "execution-owner-exit".to_owned(),
        tenant_id,
        plan_id,
        generation: NetworkResourceGeneration::new(7),
        attachment_id: NetworkAttachmentId::for_workload_attachment(
            "tenant-owner-exit/workload-a",
            "private",
        ),
        plan_members: plan_members.clone(),
        routes: vec![route],
        publication: PublishedIngressAuthority::direct_fixture(),
        final_phase: FinalIngressPhase::Published,
    };

    drop(batch);

    let retained = port_leases
        .inspect(published.lease_id())
        .expect("published lease should inspect")
        .expect("published lease history should remain durable");
    assert_eq!(retained.phase(), PortLeasePhase::Reserved);
    assert!(retained.active_lifetime().is_none());
    assert_eq!(
        retained
            .confirmed_stopped_binding()
            .expect("owner exit should retain confirmed-stop binding")
            .actual_port()
            .get(),
        published_address.port()
    );
    let sibling = port_leases
        .inspect(unrelated.lease_id())
        .expect("unrelated member should inspect")
        .expect("unrelated member should remain durable");
    assert_eq!(sibling.phase(), PortLeasePhase::Reserved);
    assert_eq!(sibling.reservation_claim(), Some(&claim));
    assert!(sibling.binding().is_none());

    let prepared = listeners
        .prepare_workload_ingress(Some(&plan_members), published, &claim)
        .expect("fresh owner should claim the retained exact-port rebind");
    assert_eq!(
        prepared
            .bind_addr()
            .expect("retained bind address should resolve"),
        published_address
    );
    let rebound = TcpListener::bind(published_address)
        .expect("confirmed owner exit should make the retained port available");
    drop(
        prepared
            .adopt_std(rebound)
            .expect("fresh owner should adopt the retained listener"),
    );
}
