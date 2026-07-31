//! NNC5.4a fail-before and convergence proofs for machine publication batches.

use super::*;

use std::collections::{BTreeMap, BTreeSet};
use std::net::{Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

use crate::backends::capabilities::{
    SandboxAttachmentRegistrationKind, host_managed_attachment_provider_id,
};
use crate::backends::oci::network::{AttachmentBackendKind, oci_attachment_plan};

const TEST_IO_TIMEOUT: Duration = Duration::from_secs(2);
type WithdrawBatchResult = (
    BTreeMap<String, usize>,
    Vec<String>,
    BTreeSet<(String, String, String)>,
);

#[path = "machine_port_batch_recovery/fresh_process.rs"]
mod fresh_process;

struct ExposeLossServer {
    address: SocketAddr,
    server: thread::JoinHandle<(BTreeMap<String, usize>, Vec<String>)>,
}

struct WithdrawBatchServer {
    address: SocketAddr,
    server: thread::JoinHandle<WithdrawBatchResult>,
}

impl ExposeLossServer {
    fn spawn(listener: TcpListener) -> Self {
        let address = listener
            .local_addr()
            .expect("expose-loss server address should resolve");
        let server = thread::spawn(move || {
            let mut routes = BTreeSet::<(String, String, String)>::new();
            let mut mutations = BTreeMap::<String, usize>::new();
            let mut requests = Vec::new();
            let mut lose_mutation_response = true;
            let mut lose_inspection = true;
            loop {
                let (mut stream, _) = listener.accept().expect("request should arrive");
                let request = read_complete_request(&mut stream);
                if request.starts_with("POST /__nimbus_nnc5_4a_complete ") {
                    write_response(&mut stream, b"[]");
                    return (mutations, requests);
                }
                if request.starts_with("POST /services/forwarder/expose ") {
                    let (local, remote, protocol) = parse_route(&request);
                    *mutations.entry(local.clone()).or_default() += 1;
                    routes.replace((local, remote, protocol));
                    requests.push(request);
                    if lose_mutation_response {
                        lose_mutation_response = false;
                        stream
                            .shutdown(Shutdown::Write)
                            .expect("lost mutation response should close");
                    } else {
                        write_response(&mut stream, &[]);
                    }
                    continue;
                }
                assert!(
                    request.starts_with("GET /services/forwarder/all "),
                    "unexpected forwarder request: {request}"
                );
                requests.push(request);
                if lose_inspection && !routes.is_empty() {
                    lose_inspection = false;
                    stream
                        .shutdown(Shutdown::Write)
                        .expect("lost inspection response should close");
                    continue;
                }
                let body = serde_json::to_vec(
                    &routes
                        .iter()
                        .map(|(local, remote, protocol)| {
                            serde_json::json!({
                                "local": local,
                                "remote": remote,
                                "protocol": protocol,
                            })
                        })
                        .collect::<Vec<_>>(),
                )
                .expect("route list should encode");
                write_response(&mut stream, &body);
            }
        });
        Self { address, server }
    }

    fn finish(self) -> (BTreeMap<String, usize>, Vec<String>) {
        let mut stream =
            TcpStream::connect(self.address).expect("completion request should connect");
        stream
            .write_all(b"POST /__nimbus_nnc5_4a_complete HTTP/1.0\r\nContent-Length: 0\r\n\r\n")
            .expect("completion request should write");
        stream
            .shutdown(Shutdown::Write)
            .expect("completion request should finish");
        self.server.join().expect("expose-loss server should join")
    }
}

impl WithdrawBatchServer {
    fn spawn(
        listener: TcpListener,
        routes: BTreeSet<(String, String, String)>,
        fail_local_once: Option<String>,
    ) -> Self {
        let address = listener
            .local_addr()
            .expect("withdraw-batch server address should resolve");
        let server = thread::spawn(move || {
            let mut routes = routes;
            let mut mutations = BTreeMap::<String, usize>::new();
            let mut requests = Vec::new();
            let mut failed = false;
            loop {
                let (mut stream, _) = listener.accept().expect("request should arrive");
                let request = read_complete_request(&mut stream);
                if request.starts_with("POST /__nimbus_nnc5_4a_complete ") {
                    write_response(&mut stream, b"[]");
                    return (mutations, requests, routes);
                }
                if request.starts_with("GET /services/forwarder/all ") {
                    requests.push(request);
                    let body = serde_json::to_vec(
                        &routes
                            .iter()
                            .map(|(local, remote, protocol)| {
                                serde_json::json!({
                                    "local": local,
                                    "remote": remote,
                                    "protocol": protocol,
                                })
                            })
                            .collect::<Vec<_>>(),
                    )
                    .expect("route list should encode");
                    write_response(&mut stream, &body);
                    continue;
                }
                assert!(
                    request.starts_with("POST /services/forwarder/unexpose "),
                    "unexpected forwarder request: {request}"
                );
                let (local, protocol) = parse_withdraw_route(&request);
                *mutations.entry(local.clone()).or_default() += 1;
                requests.push(request);
                if !failed && fail_local_once.as_ref() == Some(&local) {
                    failed = true;
                    write_status_response(&mut stream, 500, b"scripted failure");
                    continue;
                }
                routes.retain(|(candidate, _, candidate_protocol)| {
                    candidate != &local || candidate_protocol != &protocol
                });
                write_response(&mut stream, &[]);
            }
        });
        Self { address, server }
    }

    fn finish(self) -> WithdrawBatchResult {
        let mut stream =
            TcpStream::connect(self.address).expect("completion request should connect");
        stream
            .write_all(b"POST /__nimbus_nnc5_4a_complete HTTP/1.0\r\nContent-Length: 0\r\n\r\n")
            .expect("completion request should write");
        stream
            .shutdown(Shutdown::Write)
            .expect("completion request should finish");
        self.server
            .join()
            .expect("withdraw-batch server should join")
    }
}

fn read_complete_request(stream: &mut TcpStream) -> String {
    stream
        .set_read_timeout(Some(TEST_IO_TIMEOUT))
        .expect("request timeout should configure");
    let mut request = Vec::new();
    let mut expected_len = None;
    let mut chunk = [0_u8; 1024];
    loop {
        let read = stream.read(&mut chunk).expect("request should read");
        assert_ne!(read, 0, "request must include its complete body");
        request.extend_from_slice(&chunk[..read]);
        if expected_len.is_none()
            && let Some(header_end) = request.windows(4).position(|window| window == b"\r\n\r\n")
        {
            let headers =
                std::str::from_utf8(&request[..header_end]).expect("headers should be UTF-8");
            let content_len = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then_some(value.trim())
                })
                .map(|value| value.parse::<usize>().expect("content length should parse"))
                .unwrap_or_default();
            expected_len = Some(header_end + 4 + content_len);
        }
        if expected_len.is_some_and(|expected| request.len() >= expected) {
            return String::from_utf8(request).expect("request should be UTF-8");
        }
    }
}

fn parse_route(request: &str) -> (String, String, String) {
    let (_, body) = request
        .split_once("\r\n\r\n")
        .expect("route request should contain a body");
    let value: serde_json::Value =
        serde_json::from_str(body).expect("route request body should be JSON");
    (
        value["local"]
            .as_str()
            .expect("route local should exist")
            .to_owned(),
        value["remote"]
            .as_str()
            .expect("route remote should exist")
            .to_owned(),
        value["protocol"]
            .as_str()
            .expect("route protocol should exist")
            .to_owned(),
    )
}

fn parse_withdraw_route(request: &str) -> (String, String) {
    let (_, body) = request
        .split_once("\r\n\r\n")
        .expect("route request should contain a body");
    let value: serde_json::Value =
        serde_json::from_str(body).expect("route request body should be JSON");
    (
        value["local"]
            .as_str()
            .expect("route local should exist")
            .to_owned(),
        value["protocol"]
            .as_str()
            .expect("route protocol should exist")
            .to_owned(),
    )
}

fn write_response(stream: &mut TcpStream, body: &[u8]) {
    write_status_response(stream, 200, body);
}

fn write_status_response(stream: &mut TcpStream, status: u16, body: &[u8]) {
    stream
        .set_write_timeout(Some(TEST_IO_TIMEOUT))
        .expect("response timeout should configure");
    write!(
        stream,
        "HTTP/1.0 {status} TEST\r\nContent-Type: application/json\r\nContent-Length: \
         {}\r\n\r\n",
        body.len()
    )
    .expect("response headers should write");
    stream.write_all(body).expect("response body should write");
}

fn seed_publication_attachment(
    backend: &ContainerSandboxBackend,
    manifest: &ContainerSandboxManifest,
) {
    let network_config = manifest
        .network_config
        .as_ref()
        .expect("machine publication fixture should carry placed network authority");
    let attachment_id = default_network_attachment_id(&manifest.handle.id);
    let reservation = backend
        .segment_allocator
        .inspect_attachment_reservation(
            &manifest.spec.tenant_id,
            &attachment_id,
            &network_config.reservation_claim,
        )
        .expect("machine publication reservation should inspect");
    let association = reservation
        .association()
        .expect("machine publication reservation should carry its exact association")
        .clone();
    let plan = oci_attachment_plan(
        &manifest.spec.tenant_id,
        &manifest.handle.id,
        AttachmentBackendKind::Container,
    );
    backend
        .attachment_authority
        .as_ref()
        .expect("machine publication attachment authority should initialize")
        .reserve(
            &manifest.spec.tenant_id,
            host_managed_attachment_provider_id(SandboxAttachmentRegistrationKind::Container),
            &plan,
            attachment_id,
            association,
        )
        .expect("machine publication attachment authority should reserve");
}

fn publication_record(manifest: &ContainerSandboxManifest) -> serde_json::Value {
    let path = manifest
        .conmon_layout
        .container_state_dir
        .join(".nimbus-machine-port-evidence.json");
    let bytes = std::fs::read(&path).unwrap_or_else(|error| {
        panic!(
            "durable machine publication record {} should read: {error}",
            path.display()
        )
    });
    serde_json::from_slice::<serde_json::Value>(&bytes)
        .expect("durable machine publication record should parse")["record"]
        .clone()
}

#[test]
fn nnc5_4a_lifecycle_retry_preserves_authority_and_does_not_duplicate_expose() {
    let temp = tempfile::tempdir().expect("temporary root should exist");
    let first_port = unused_loopback_port();
    let mut second_port = unused_loopback_port();
    while second_port == first_port {
        second_port = unused_loopback_port();
    }
    let listener =
        TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("machine forwarder fixture should bind");
    let forwarder_port = listener
        .local_addr()
        .expect("forwarder address should resolve")
        .port();
    let mut config = ContainerSandboxBackendConfig::under_root(temp.path());
    config.machine_port_forwarder = Some(sample_forwarder(forwarder_port));
    let backend = ContainerSandboxBackend::new(config);
    let manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_bindings([
                SandboxPortBinding::tcp("first", first_port, 8_080),
                SandboxPortBinding::tcp("second", second_port, 8_081),
            ]),
            &SandboxId::new("machine-batch-expose-loss"),
            None,
            None,
        )
        .expect("machine publication fixture should reserve")
        .manifest;
    seed_publication_attachment(&backend, &manifest);
    let server = ExposeLossServer::spawn(listener);
    let publish = || backend.converge_exposed_machine_port_publication(&manifest);

    let first = backend
        .ensure_machine_port_proxies_running_with_publication(
            &manifest.handle.id,
            &[Ipv4Addr::LOCALHOST],
            &manifest,
            MachinePortPreparationReleaseAuthority::FreshLaunch(
                manifest
                    .launch_reservation_claim
                    .as_ref()
                    .expect("fixture should retain the exact launch claim"),
            ),
            publish,
        )
        .expect_err("lost response plus lost inspection must keep publication ambiguous");
    assert!(
        first.to_string().contains("ambiguous"),
        "the primary ambiguity should survive: {first}"
    );

    let authority =
        nimbus_network::LocalPortLeaseAuthority::open(&backend.config.network_state_root)
            .expect("port authority should reopen");
    let after_first = manifest
        .port_leases
        .iter()
        .map(|lease| {
            authority
                .inspect(lease.lease_id())
                .expect("lease should inspect")
                .expect("lease should remain durable")
        })
        .collect::<Vec<_>>();
    let partial = publication_record(&manifest);
    assert_eq!(
        partial["phase"], "exposing",
        "ambiguity must retain the in-flight batch"
    );
    assert_eq!(
        partial["slots"][0]["state"], "effect_may_exist",
        "the mutating slot must be journaled before its ambiguous provider effect"
    );

    backend
        .ensure_machine_port_proxies_running_with_publication(
            &manifest.handle.id,
            &[Ipv4Addr::LOCALHOST],
            &manifest,
            MachinePortPreparationReleaseAuthority::FreshLaunch(
                manifest
                    .launch_reservation_claim
                    .as_ref()
                    .expect("fixture should retain the exact launch claim"),
            ),
            publish,
        )
        .expect("retry should converge after exact current observation");
    let after_retry = manifest
        .port_leases
        .iter()
        .map(|lease| {
            authority
                .inspect(lease.lease_id())
                .expect("lease should inspect")
                .expect("lease should remain durable")
        })
        .collect::<Vec<_>>();
    assert_eq!(
        after_retry, after_first,
        "provider retry must preserve the exact port authority generation"
    );

    let (mutations, requests) = server.finish();
    for binding in &manifest.spec.port_bindings {
        let local = format!("{}:{}", binding.host_address, binding.host_port);
        assert_eq!(
            mutations.get(&local),
            Some(&1),
            "NNC5.4a requires inspect-before-retry for each already-visible route; requests: \
             {requests:?}"
        );
    }
}

#[test]
fn nnc5_4a_fresh_owner_withdrawal_preserves_authority_and_does_not_duplicate_absence() {
    let temp = tempfile::tempdir().expect("temporary root should exist");
    let first_port = unused_loopback_port();
    let mut second_port = unused_loopback_port();
    while second_port == first_port {
        second_port = unused_loopback_port();
    }
    let listener =
        TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("machine forwarder fixture should bind");
    let forwarder_port = listener
        .local_addr()
        .expect("forwarder address should resolve")
        .port();
    let mut config = ContainerSandboxBackendConfig::under_root(temp.path());
    config.machine_port_forwarder = Some(sample_forwarder(forwarder_port));
    let owner = ContainerSandboxBackend::new(config.clone());
    let manifest = owner
        .plan_start_with_id(
            &sample_spec().with_port_bindings([
                SandboxPortBinding::tcp("first", first_port, 8_080),
                SandboxPortBinding::tcp("second", second_port, 8_081),
            ]),
            &SandboxId::new("machine-batch-withdraw-owner-loss"),
            None,
            None,
        )
        .expect("machine withdrawal fixture should reserve")
        .manifest;
    seed_publication_attachment(&owner, &manifest);
    owner
        .ensure_machine_port_proxies_running(&manifest.handle.id, &[Ipv4Addr::LOCALHOST], &manifest)
        .expect("machine withdrawal fixture should own exact local lifetimes");
    let forwarder = manifest
        .runner_config
        .machine_port_forwarder
        .as_ref()
        .expect("machine withdrawal fixture should retain provider authority")
        .clone();
    let initial_routes = manifest
        .spec
        .port_bindings
        .iter()
        .map(|binding| {
            (
                format!("{}:{}", binding.host_address, binding.host_port),
                format!(":{}", binding.host_port),
                "tcp".to_owned(),
            )
        })
        .collect::<BTreeSet<_>>();
    let failed_local = format!(
        "{}:{}",
        manifest.spec.port_bindings[1].host_address, manifest.spec.port_bindings[1].host_port
    );
    let first_server =
        WithdrawBatchServer::spawn(listener, initial_routes, Some(failed_local.clone()));
    let cleanup = owner
        .begin_machine_port_proxy_release(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("first owner should begin exact release")
        .expect("live machine publication should require cleanup");
    let first = owner
        .unexpose_machine_port_proxy_publications(&cleanup, &forwarder)
        .expect_err("one failed route must retain partial withdrawal");
    assert!(
        first.to_string().contains(&failed_local),
        "the partial result should identify only the still-visible route: {first}"
    );
    let (first_mutations, first_requests, remaining_routes) = first_server.finish();
    assert_eq!(
        first_mutations.values().sum::<usize>(),
        2,
        "the first owner must attempt every binding once: {first_requests:?}"
    );
    assert_eq!(
        remaining_routes.len(),
        1,
        "the first owner must leave only the failed route visible"
    );
    let partial = publication_record(&manifest);
    assert_eq!(
        partial["phase"], "withdrawing",
        "partial withdrawal must retain the in-flight batch"
    );
    assert_eq!(
        partial["slots"][0]["state"], "observed_absent",
        "the first exact absence must survive owner loss"
    );
    assert_eq!(
        partial["slots"][1]["state"], "effect_may_exist",
        "the failed binding must remain an inspect-before-retry slot"
    );

    drop(cleanup);
    drop(owner);

    let recovery = ContainerSandboxBackend::new(config);
    let recovered = recovery
        .begin_machine_port_proxy_release(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("fresh owner should recover exact dead-owner authority")
        .expect("partial provider publication must remain cleanup-pending");
    let authority_path = nimbus_network::LocalNetworkStateStore::authority_path_for(
        &recovery.config.network_state_root,
    );
    let authority_before = std::fs::read(&authority_path)
        .expect("combined port, attachment, and segment authority should remain durable");
    let port_authority =
        nimbus_network::LocalPortLeaseAuthority::open(&recovery.config.network_state_root)
            .expect("port authority should reopen");
    let port_records_before = manifest
        .port_leases
        .iter()
        .map(|lease| {
            port_authority
                .inspect(lease.lease_id())
                .expect("lease should inspect")
                .expect("lease should remain durable")
        })
        .collect::<Vec<_>>();
    assert!(
        port_records_before
            .iter()
            .all(|record| record.phase() == nimbus_network::PortLeasePhase::CleanupPending),
        "fresh-owner recovery must fence every listener before provider inspection"
    );

    let retry_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, forwarder_port))
        .expect("fresh-owner forwarder should bind");
    let retry_server = WithdrawBatchServer::spawn(retry_listener, remaining_routes, None);
    recovery
        .unexpose_machine_port_proxy_publications(&recovered, &forwarder)
        .expect("fresh owner should converge after exact current observation");
    let authority_after = std::fs::read(&authority_path)
        .expect("combined authority should remain readable after provider effects");
    let port_records_after = manifest
        .port_leases
        .iter()
        .map(|lease| {
            port_authority
                .inspect(lease.lease_id())
                .expect("lease should inspect")
                .expect("lease should remain durable")
        })
        .collect::<Vec<_>>();
    assert_eq!(
        authority_after, authority_before,
        "provider withdrawal must not mutate port, attachment, or segment authority"
    );
    assert_eq!(
        port_records_after, port_records_before,
        "provider withdrawal must preserve every exact cleanup fence"
    );

    let (retry_mutations, retry_requests, final_routes) = retry_server.finish();
    assert!(
        final_routes.is_empty(),
        "fresh-owner convergence must observe every route absent"
    );
    let first_local = format!(
        "{}:{}",
        manifest.spec.port_bindings[0].host_address, manifest.spec.port_bindings[0].host_port
    );
    assert_eq!(
        first_mutations
            .get(&first_local)
            .copied()
            .unwrap_or_default()
            + retry_mutations
                .get(&first_local)
                .copied()
                .unwrap_or_default(),
        1,
        "NNC5.4a requires fresh-owner inspection before replaying a route already observed absent; \
         first requests: {first_requests:?}; retry requests: {retry_requests:?}"
    );
    assert_eq!(
        first_mutations
            .get(&failed_local)
            .copied()
            .unwrap_or_default()
            + retry_mutations
                .get(&failed_local)
                .copied()
                .unwrap_or_default(),
        2,
        "the failed route must be retried exactly once after exact visibility"
    );
    recovery
        .complete_machine_port_proxy_cleanup(&recovered)
        .expect("complete exact absence may release the fenced listener generation");
}
