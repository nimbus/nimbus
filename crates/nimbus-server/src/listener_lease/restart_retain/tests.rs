use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;

use nimbus_network::{
    ListenerId, LocalPortLeaseAuthority, NetworkLeaseEpoch, NetworkPlanId, NetworkProviderHandle,
    NetworkProviderId, NetworkReservationClaim, NetworkResourceGeneration, NetworkResourceId,
    PortBindRealm, PortBindTarget, PortBindingSpec, PortExposure, PortLeaseAccounting,
    PortLeaseFence, PortLeaseId, PortLeaseLifetime, PortLeasePhase, PortLeaseRequest, PortProtocol,
    PortPublicationIntent, PortRequestMode,
};

use super::*;
use crate::listener_lease::ServerListenerLeaseAuthority;

struct WorkerProof {
    listener_joined: Arc<AtomicBool>,
    connection_joined: Arc<AtomicBool>,
}

fn workload_request(name: &str, plan_id: &NetworkPlanId, published: bool) -> PortLeaseRequest {
    let tenant = nimbus_core::TenantId::new("tenant-restart")
        .expect("fixture tenant identity should validate");
    let listener = ListenerId::for_tenant_workload_listener(&tenant, "workload-a", name);
    PortLeaseRequest::new(
        PortLeaseId::for_listener(&listener),
        NetworkResourceId::from(listener),
        Some(tenant),
        PortLeaseFence::new(NetworkResourceGeneration::new(9), NetworkLeaseEpoch::new(3)),
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

fn workload_plan() -> (Vec<PortLeaseRequest>, Vec<PortLeaseRequest>) {
    let tenant = nimbus_core::TenantId::new("tenant-restart")
        .expect("fixture tenant identity should validate");
    let plan_id = NetworkPlanId::for_tenant_workload_plan(&tenant, "workload-a-generation-9");
    let published = vec![
        workload_request("http", &plan_id, true),
        workload_request("metrics", &plan_id, true),
    ];
    let mut plan_members = published.clone();
    plan_members.push(workload_request("pep", &plan_id, false));
    (published, plan_members)
}

fn reservation_claim() -> NetworkReservationClaim {
    NetworkReservationClaim::new(
        NetworkProviderHandle::new(
            NetworkProviderId::for_registration_key("nimbus-sandbox.restart-test"),
            "restart-listener-reservation",
        )
        .expect("fixture reservation handle should validate"),
    )
}

fn active_listener(
    authority: &ServerListenerLeaseAuthority,
    plan_members: &[PortLeaseRequest],
    request: &PortLeaseRequest,
    reservation: &NetworkReservationClaim,
    fail_after_join: bool,
) -> (
    RestartStoppingServerListener,
    SocketAddr,
    PortLeaseLifetime,
    WorkerProof,
) {
    let prepared = authority
        .prepare_workload_ingress(Some(plan_members), request.clone(), reservation)
        .expect("server should claim the workload listener");
    let listener = std::net::TcpListener::bind(
        prepared
            .bind_addr()
            .expect("claimed bind address should resolve"),
    )
    .expect("workload listener should bind");
    let adopted = prepared
        .adopt_std(listener)
        .expect("workload listener should activate");
    let address = adopted.local_addr().expect("bound address should resolve");
    let (listener, lease, _) = adopted.into_std_parts();
    let lifetime = lease
        .observation_evidence()
        .expect("active listener evidence should be coherent")
        .lifetime();

    let listener_joined = Arc::new(AtomicBool::new(false));
    let connection_joined = Arc::new(AtomicBool::new(false));
    let listener_joined_by_worker = Arc::clone(&listener_joined);
    let connection_joined_by_worker = Arc::clone(&connection_joined);
    let (listener_stop_tx, listener_stop_rx) = mpsc::channel();
    let (connection_stop_tx, connection_stop_rx) = mpsc::channel();
    let connection_worker = thread::spawn(move || {
        connection_stop_rx
            .recv()
            .expect("listener worker should stop its connection worker");
        connection_joined_by_worker.store(true, Ordering::Release);
    });
    let listener_worker = thread::spawn(move || {
        listener_stop_rx
            .recv()
            .expect("restart owner should stop the listener worker");
        drop(listener);
        connection_stop_tx
            .send(())
            .expect("listener worker should signal its connection worker");
        connection_worker
            .join()
            .expect("connection worker should join without panic");
        listener_joined_by_worker.store(true, Ordering::Release);
    });

    let stopping = RestartStoppingServerListener::new(lease, move || {
        listener_stop_tx
            .send(())
            .map_err(|_| io::Error::other("listener stop signal was not delivered"))?;
        listener_worker
            .join()
            .map_err(|_| io::Error::other("listener worker panicked"))?;
        if fail_after_join {
            return Err(io::Error::other(
                "fixture reports an ambiguous provider stop receipt",
            ));
        }
        Ok(())
    });
    (
        stopping,
        address,
        lifetime,
        WorkerProof {
            listener_joined,
            connection_joined,
        },
    )
}

#[test]
fn restart_batch_joins_all_workers_and_rebinds_exact_ports_at_higher_lifetimes() {
    let state_root = tempfile::tempdir().expect("state root should be created");
    let authority = ServerListenerLeaseAuthority::reconstruct_direct(state_root.path())
        .expect("listener authority should reconstruct");
    let port_authority =
        LocalPortLeaseAuthority::open(state_root.path()).expect("port authority should open");
    let reservation = reservation_claim();
    let (requests, plan_members) = workload_plan();
    port_authority
        .reserve_batch_for_coordinator(plan_members.clone(), &reservation)
        .expect("launch coordinator should reserve the complete workload plan");
    let mut stopping = Vec::new();
    let mut addresses = Vec::new();
    let mut old_lifetimes = Vec::new();
    let mut proofs = Vec::new();
    for request in &requests {
        let (listener, address, lifetime, proof) =
            active_listener(&authority, &plan_members, request, &reservation, false);
        stopping.push(listener);
        addresses.push(address);
        old_lifetimes.push(lifetime);
        proofs.push(proof);
    }

    let retained = stop_and_retain_server_listeners_for_restart(&plan_members, stopping)
        .expect("a fully stopped exact listener batch should retain");
    assert_eq!(retained.records().len(), requests.len());
    assert!(retained.records().iter().all(|record| {
        record.phase() == PortLeasePhase::Reserved
            && record.confirmed_stopped_binding().is_some()
            && record.binding().is_none()
            && record.active_lifetime().is_none()
    }));
    for proof in &proofs {
        assert!(proof.listener_joined.load(Ordering::Acquire));
        assert!(proof.connection_joined.load(Ordering::Acquire));
    }

    for address in &addresses {
        drop(
            std::net::TcpListener::bind(address)
                .expect("success requires every old listener to be unreachable"),
        );
    }

    for ((request, address), old_lifetime) in requests
        .iter()
        .zip(addresses.iter())
        .zip(old_lifetimes.iter())
    {
        let prepared = authority
            .prepare_workload_ingress(Some(&plan_members), request.clone(), &reservation)
            .expect("retained listener should claim its next lifetime");
        assert_eq!(
            prepared
                .bind_addr()
                .expect("retained bind address should resolve"),
            *address,
            "restart must rebind the exact retained numeric port"
        );
        let rebound = std::net::TcpListener::bind(address)
            .expect("confirmed stop should permit an exact provider rebind");
        let active = prepared
            .adopt_std(rebound)
            .expect("rebound listener should activate");
        let record = port_authority
            .inspect(request.lease_id())
            .expect("rebound listener should inspect")
            .expect("rebound listener should remain durable");
        assert!(
            record
                .active_lifetime()
                .expect("rebound listener should own a lifetime")
                .generation()
                > old_lifetime.generation()
        );
        drop(active);
    }
}

#[test]
fn partial_stop_failure_is_ambiguous_and_never_releases_or_retains_the_batch() {
    let state_root = tempfile::tempdir().expect("state root should be created");
    let authority = ServerListenerLeaseAuthority::reconstruct_direct(state_root.path())
        .expect("listener authority should reconstruct");
    let port_authority =
        LocalPortLeaseAuthority::open(state_root.path()).expect("port authority should open");
    let reservation = reservation_claim();
    let (requests, plan_members) = workload_plan();
    port_authority
        .reserve_batch_for_coordinator(plan_members.clone(), &reservation)
        .expect("launch coordinator should reserve the complete workload plan");
    let first = active_listener(&authority, &plan_members, &requests[0], &reservation, false);
    let second = active_listener(&authority, &plan_members, &requests[1], &reservation, true);
    let addresses = [first.1, second.1];
    let proofs = [first.3, second.3];

    let error =
        stop_and_retain_server_listeners_for_restart(&plan_members, vec![first.0, second.0])
            .expect_err("one ambiguous stop receipt must reject the complete batch");
    assert_eq!(
        error.kind(),
        RestartListenerRetainFailureKind::StopOrJoinAmbiguous
    );
    assert!(
        proofs.iter().all(|proof| {
            proof.listener_joined.load(Ordering::Acquire)
                && proof.connection_joined.load(Ordering::Acquire)
        }),
        "the batch owner must attempt and join every sibling before returning failure"
    );
    for (request, address) in requests.iter().zip(addresses) {
        let record = port_authority
            .inspect(request.lease_id())
            .expect("failed batch should inspect")
            .expect("failed batch should remain durable");
        assert_eq!(record.phase(), PortLeasePhase::Active);
        assert!(record.confirmed_stopped_binding().is_none());
        drop(
            std::net::TcpListener::bind(address)
                .expect("the old listener effect should still be closed"),
        );
    }
}

#[test]
fn crossed_lifetime_stops_effects_but_cannot_claim_durable_retention() {
    let state_root = tempfile::tempdir().expect("state root should be created");
    let authority = ServerListenerLeaseAuthority::reconstruct_direct(state_root.path())
        .expect("listener authority should reconstruct");
    let port_authority =
        LocalPortLeaseAuthority::open(state_root.path()).expect("port authority should open");
    let reservation = reservation_claim();
    let (requests, plan_members) = workload_plan();
    port_authority
        .reserve_batch_for_coordinator(plan_members.clone(), &reservation)
        .expect("launch coordinator should reserve the complete workload plan");
    let first = active_listener(&authority, &plan_members, &requests[0], &reservation, false);
    let second = active_listener(&authority, &plan_members, &requests[1], &reservation, false);
    let addresses = [first.1, second.1];
    let mut listeners = vec![first.0, second.0];
    let (first_listener, second_listener) = listeners.split_at_mut(1);
    std::mem::swap(
        &mut first_listener[0]
            .lease
            .as_mut()
            .expect("first lease should remain owned")
            .lifetime,
        &mut second_listener[0]
            .lease
            .as_mut()
            .expect("second lease should remain owned")
            .lifetime,
    );

    let error = stop_and_retain_server_listeners_for_restart(&plan_members, listeners)
        .expect_err("crossed lifetime authority must reject the complete batch");
    assert_eq!(
        error.kind(),
        RestartListenerRetainFailureKind::DurableRetentionAmbiguous
    );
    for (request, address) in requests.iter().zip(addresses) {
        let record = port_authority
            .inspect(request.lease_id())
            .expect("crossed batch should inspect")
            .expect("crossed batch should remain durable");
        assert_eq!(record.phase(), PortLeasePhase::Active);
        assert!(record.confirmed_stopped_binding().is_none());
        drop(
            std::net::TcpListener::bind(address)
                .expect("crossed authority still requires the old socket effects to stop"),
        );
    }
}

#[test]
fn terminal_listener_close_still_releases_instead_of_retaining() {
    let state_root = tempfile::tempdir().expect("state root should be created");
    let authority = ServerListenerLeaseAuthority::reconstruct_direct(state_root.path())
        .expect("listener authority should reconstruct");
    let port_authority =
        LocalPortLeaseAuthority::open(state_root.path()).expect("port authority should open");
    let reservation = reservation_claim();
    let tenant = nimbus_core::TenantId::new("tenant-restart")
        .expect("fixture tenant identity should validate");
    let plan_id = NetworkPlanId::for_tenant_workload_plan(&tenant, "terminal-generation-9");
    let request = workload_request("terminal", &plan_id, true);
    let plan_members = vec![request.clone()];
    port_authority
        .reserve_batch_for_coordinator(plan_members.clone(), &reservation)
        .expect("terminal fixture should reserve");
    let prepared = authority
        .prepare_workload_ingress(Some(&plan_members), request.clone(), &reservation)
        .expect("terminal fixture should claim");
    let listener = std::net::TcpListener::bind(
        prepared
            .bind_addr()
            .expect("terminal bind address should resolve"),
    )
    .expect("terminal listener should bind");
    let active = prepared
        .adopt_std(listener)
        .expect("terminal listener should activate");

    active
        .close_and_settle()
        .expect("terminal close should preserve release behavior");
    let record = port_authority
        .inspect(request.lease_id())
        .expect("terminal listener should inspect")
        .expect("terminal record should remain as history");
    assert_eq!(record.phase(), PortLeasePhase::Released);
    assert!(record.confirmed_stopped_binding().is_none());
}
