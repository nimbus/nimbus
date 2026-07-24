use super::support::*;

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

use tempfile::TempDir;

use crate::backends::oci::command::CommandSpec;
use nimbus_egress::{EgressPolicy, EgressProtocol, EgressRule};

#[test]
fn detect_runtime_status_marks_stale_pidfiles_as_failed() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let backend =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(temp_dir.path()));
    let mut manifest = backend
        .plan_start_with_id(&sample_spec(), &sandbox_id(), None, None)
        .expect("plan should lower")
        .manifest;
    manifest.conmon_launch.state_command = CommandSpec::new("/bin/sh").args(["-c", "exit 1"]);
    std::fs::write(&manifest.conmon_layout.pidfile, "999999\n").expect("pidfile should write");

    assert_eq!(
        backend
            .detect_runtime_status(&manifest)
            .expect("status should resolve"),
        SandboxStatus::Failed
    );
}

/// NNC0.6 fail-before baseline for NNCF6. Reaching the netns-created boundary
/// does not prove that Netavark status, the egress pin, forwarding, or the PEP
/// exists. Runtime readiness must eventually consume complete attachment
/// evidence rather than infer safety from workload liveness alone.
#[test]
#[ignore = "NNC0.6 expected red until NNC5.2 makes complete attachment evidence part of readiness"]
fn nnc0_6_container_is_not_ready_at_partial_attachment_boundary() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let backend =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(temp_dir.path()));
    let manifest = backend
        .plan_start_with_id(&sample_spec(), &sandbox_id(), None, None)
        .expect("plan should lower")
        .manifest;
    std::fs::create_dir_all(
        manifest
            .network_layout
            .netns_path
            .parent()
            .expect("netns path should have a parent"),
    )
    .expect("netns parent should create");
    std::fs::write(&manifest.network_layout.netns_path, b"netns")
        .expect("netns-created boundary should persist");
    assert!(
        !manifest.network_layout.status_path.exists(),
        "precondition: Netavark status must still be absent at this partial boundary"
    );

    let status = running_status(&manifest);

    assert_ne!(
        status,
        SandboxStatus::Ready,
        "NNCF6: workload liveness without complete same-generation attachment evidence \
         must not publish container readiness"
    );
}

#[test]
fn restart_decision_keeps_failed_container_starting_until_backoff_elapses() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let backend =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(temp_dir.path()));
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec().with_restart_policy(SandboxRestartPolicy::OnFailure { max_restarts: 1 }),
            &sandbox_id(),
            None,
            None,
        )
        .expect("plan should lower")
        .manifest;
    std::fs::write(&manifest.conmon_layout.exit_status_file, "42\n")
        .expect("exit status should write");
    manifest.next_restart_at_millis = Some(1_500);

    let decision =
        mark_restart_decision_after_exit(&mut manifest, 1_000).expect("restart should evaluate");

    assert_eq!(decision, ContainerRestartDecision::WaitingForBackoff);
    assert_eq!(manifest.last_exit_code, Some(42));
    assert_eq!(manifest.restart_count, 0);
    assert_eq!(manifest.next_restart_at_millis, Some(1_500));
    assert_eq!(manifest.status, SandboxStatus::Starting);
    assert_eq!(manifest.handle.status, SandboxStatus::Starting);
}

#[test]
fn restart_decision_counts_due_failed_container_restart() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let backend =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(temp_dir.path()));
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec().with_restart_policy(SandboxRestartPolicy::OnFailure { max_restarts: 2 }),
            &sandbox_id(),
            None,
            None,
        )
        .expect("plan should lower")
        .manifest;
    std::fs::write(&manifest.conmon_layout.exit_status_file, "42\n")
        .expect("exit status should write");
    manifest.next_restart_at_millis = Some(0);

    let decision =
        mark_restart_decision_after_exit(&mut manifest, 1_000).expect("restart should evaluate");

    assert_eq!(decision, ContainerRestartDecision::RestartNow);
    assert_eq!(manifest.last_exit_code, Some(42));
    assert_eq!(manifest.restart_count, 1);
    assert_eq!(manifest.next_restart_at_millis, None);
    assert_eq!(manifest.status, SandboxStatus::Starting);
    assert_eq!(manifest.handle.status, SandboxStatus::Starting);
}

/// NNC0.6a fail-before baseline for NNCF20. Inspection owns a stale manifest
/// copy, reaches the provider-launch entry through restart policy, and parks.
/// The coordinator then durably withdraws the workload before releasing that
/// launch. No readiness outcome can satisfy this side-effect assertion.
#[test]
#[ignore = "NNC0.6a expected red until NNC5.6/NNC6.4a make inspect side-effect-free and fence restart"]
fn nnc0_6a_container_inspect_must_not_restart_after_withdrawal() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let restart_probe = RestartLaunchTestProbe::new(Duration::from_secs(1));
    let backend =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(temp_dir.path()))
            .with_restart_launch_test_probe(restart_probe.clone());
    let sandbox_id = SandboxId::new("nnc0-6a-container");
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec().with_restart_policy(SandboxRestartPolicy::OnFailure { max_restarts: 1 }),
            &sandbox_id,
            None,
            None,
        )
        .expect("execute manifest should plan")
        .manifest;
    manifest.conmon_launch.delete_command = CommandSpec::new("/usr/bin/true");
    manifest.next_restart_at_millis = Some(0);
    std::fs::write(&manifest.conmon_layout.exit_status_file, "42\n")
        .expect("failed exit should persist");
    backend
        .write_manifest(&manifest)
        .expect("restart-eligible manifest should persist");

    let inspect_backend = backend.clone();
    let inspect_id = sandbox_id.clone();
    let inspect_thread = thread::spawn(move || inspect_backend.inspect_sync(&inspect_id));
    if !restart_probe.wait_until_entered() {
        let inspect_result = inspect_thread
            .join()
            .expect("inspect thread should join after a missing barrier");
        panic!(
            "inspect must reach the provider-launch barrier through restart policy; \
             inspect completed instead with {inspect_result:?}"
        );
    }

    let mut withdrawn = manifest;
    withdrawn.shutdown_requested = true;
    withdrawn.next_restart_at_millis = None;
    withdrawn.status = SandboxStatus::Stopped;
    withdrawn.handle.status = SandboxStatus::Stopped;
    withdrawn.handle.published_endpoints.clear();
    backend
        .write_manifest(&withdrawn)
        .expect("coordinator withdrawal should persist before launch release");

    restart_probe.release();
    let inspected = inspect_thread
        .join()
        .expect("inspect thread should join")
        .expect("current inspect restart should complete through the test provider")
        .expect("manifest should remain inspectable");
    assert_eq!(
        inspected.status,
        SandboxStatus::Starting,
        "precondition: stale inspection currently reactivates the withdrawn manifest"
    );

    assert_eq!(
        restart_probe.effect_count(),
        0,
        "NNCF20: inspect must be side-effect-free; a withdrawal/fence persisted before \
         release must veto the stale container restart provider effect"
    );
}

#[test]
fn release_execution_artifacts_ignores_machine_forwarder_unexpose_failures() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
    let port = listener
        .local_addr()
        .expect("listener address should resolve")
        .port();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("connection should arrive");
        let mut buffer = [0_u8; 1024];
        let _ = stream.read(&mut buffer);
        stream
            .write_all(
                b"HTTP/1.0 500 Internal Server Error\r\nContent-Length: 16\r\n\r\nproxy not found",
            )
            .expect("response should write");
    });

    let temp_dir = TempDir::new().expect("tempdir should build");
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.machine_port_forwarder = Some(sample_forwarder(port));
    let backend = ContainerSandboxBackend::new(config);
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("db", 5432, 5432)),
            &sandbox_id(),
            None,
            None,
        )
        .expect("plan should lower")
        .manifest;

    backend
        .release_execution_artifacts(&mut manifest)
        .expect("cleanup should ignore unexpose failures");
    server.join().expect("server thread should join");
}

#[test]
fn release_execution_artifacts_stops_running_egress_proxy() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let proxy_port = unused_loopback_port();
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.node_network_supernet = "127.0.0.0/24".to_owned();
    config.published_port_range = proxy_port..=proxy_port;
    let backend = ContainerSandboxBackend::new(config);
    let mut manifest = backend
        .plan_start_with_id(&sample_spec(), &sandbox_id(), None, None)
        .expect("plan should lower")
        .manifest;

    backend
        .ensure_egress_proxy_running(&manifest)
        .expect("egress proxy should start on loopback test subnet");
    assert!(
        backend
            .egress_proxies
            .contains(&manifest.handle.id)
            .expect("registry lock should hold"),
        "egress proxy should be registered after ensure"
    );

    backend
        .release_execution_artifacts(&mut manifest)
        .expect("cleanup should stop proxy and tolerate absent runtime artifacts");

    assert!(
        !backend
            .egress_proxies
            .contains(&manifest.handle.id)
            .expect("registry lock should hold"),
        "cleanup should drop the live egress proxy handle"
    );
}

#[test]
fn reload_egress_policy_updates_running_container_proxy() {
    let first = TestHttpServer::start("HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nfirst");
    let second = TestHttpServer::start("HTTP/1.1 200 OK\r\nContent-Length: 6\r\n\r\nsecond");
    let temp_dir = TempDir::new().expect("tempdir should build");
    let proxy_port = unused_loopback_port();
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.node_network_supernet = "127.0.0.0/24".to_owned();
    config.published_port_range = proxy_port..=proxy_port;
    let backend = ContainerSandboxBackend::new(config);
    let manifest = backend
        .plan_start_with_id(
            &sample_spec().with_egress_policy(allow_loopback_http_policy(first.addr.port())),
            &sandbox_id(),
            None,
            None,
        )
        .expect("plan should lower")
        .manifest;
    backend
        .write_manifest(&manifest)
        .expect("manifest should persist before reload");
    backend
        .ensure_egress_proxy_running(&manifest)
        .expect("egress proxy should start on loopback test subnet");
    let proxy_addr = manifest
        .egress_proxy
        .as_ref()
        .expect("proxy assignment should exist")
        .bind_addr()
        .expect("proxy bind address should parse");

    let allowed_first = proxy_request(
        proxy_addr,
        format!(
            "GET http://127.0.0.1:{}/ok HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
            first.addr.port()
        ),
    );
    assert!(
        allowed_first.starts_with("HTTP/1.1 200 OK") && allowed_first.contains("first"),
        "initial policy should allow first upstream, got: {allowed_first}"
    );

    backend
        .reload_egress_policy(
            &manifest.handle.id,
            allow_loopback_http_policy(second.addr.port()),
        )
        .expect("egress policy reload should update live proxy");
    let denied_old = proxy_request(
        proxy_addr,
        format!(
            "GET http://127.0.0.1:{}/ok HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
            first.addr.port()
        ),
    );
    let allowed_new = proxy_request(
        proxy_addr,
        format!(
            "GET http://127.0.0.1:{}/ok HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
            second.addr.port()
        ),
    );

    assert!(
        denied_old.starts_with("HTTP/1.1 403 Forbidden"),
        "old upstream should be denied after reload, got: {denied_old}"
    );
    assert!(
        allowed_new.starts_with("HTTP/1.1 200 OK") && allowed_new.contains("second"),
        "new upstream should be allowed after reload, got: {allowed_new}"
    );
    let reloaded_manifest = backend
        .read_manifest(&manifest.handle.id)
        .expect("manifest read should succeed")
        .expect("manifest should remain");
    assert_eq!(
        reloaded_manifest.spec.egress.rules()[0].port,
        second.addr.port()
    );
}

fn allow_loopback_http_policy(port: u16) -> EgressPolicy {
    EgressPolicy::new([
        EgressRule::new("loopback-test", EgressProtocol::Http, "127.0.0.1", port)
            .allow_internal_ips(true),
    ])
}

fn unused_loopback_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("ephemeral loopback listener should bind")
        .local_addr()
        .expect("ephemeral listener should expose address")
        .port()
}

fn proxy_request(proxy_addr: SocketAddr, request: String) -> String {
    let mut stream = TcpStream::connect(proxy_addr).expect("client should connect to proxy");
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("read timeout should set");
    stream
        .write_all(request.as_bytes())
        .expect("client should write request");
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .expect("client should read response");
    response
}

struct TestHttpServer {
    addr: SocketAddr,
}

impl TestHttpServer {
    fn start(response: &'static str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("upstream should bind");
        let addr = listener
            .local_addr()
            .expect("upstream address should resolve");
        thread::spawn(move || {
            while let Ok((mut stream, _)) = listener.accept() {
                let mut request = [0_u8; 1024];
                let _ = stream.read(&mut request);
                let _ = stream.write_all(response.as_bytes());
            }
        });
        Self { addr }
    }
}

/// MTN5 DNS-off posture: the container backend disables the in-subnet
/// aardvark-dns resolver (`enable_dns=false`), matching the krun backend. Under
/// the H1 pin gateway:53 is unreachable, so the resolver is dead weight and a
/// cross-tenant DNS-leak surface; names resolve host-side through the egress PEP.
#[test]
fn container_network_config_disables_bridge_dns_resolver() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let backend =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(temp_dir.path()));

    let tenant = nimbus_core::TenantId::new("dns-tenant").expect("tenant should parse");
    assert!(
        !backend
            .network_config(&tenant)
            .expect("network config should resolve")
            .enable_dns,
        "the container backend must disable the bridge DNS resolver (enable_dns=false)"
    );
}
