//! Cross-facade machine-port lifecycle proof for the OCI network process.

use super::support::*;

use std::net::{Ipv4Addr, TcpListener};
use std::sync::Arc;

use nimbus_core::Cidr;
use nimbus_network::{LocalNetworkManager, PortLeasePhase};
use nimbus_process_harness::PortWindow;
use tempfile::TempDir;

use crate::backends::oci::network::OciNetworkProcess;

#[test]
fn oci_network_process_contract_container_backends_share_real_machine_proxy_lifetime_authority() {
    let _serial = OciNetworkProcess::lock_test_process_claim();
    let root = TempDir::new().expect("network process fixture should exist");
    let node_root = root.path().join("node-network");
    let bootstrap =
        LocalNetworkManager::bootstrap(&node_root).expect("fixture should claim node authority");
    let authority = bootstrap.authority();
    let process = OciNetworkProcess::new(
        authority.clone(),
        Cidr::parse("10.80.0.0/16").expect("fixture super-net should validate"),
        24,
    )
    .expect("the OCI process composition should construct");
    drop(bootstrap);

    // Held to the end of the test. The shared machine proxy binds the published
    // port after `plan_start` returns, and the assertions below rebind that exact
    // port to prove the one provider stayed bound and then released it. A foreign
    // process taking it in between would read as a provider that never let go.
    //
    // The window is partitioned: offset 0 is the machine forwarder's own port,
    // and the rest is the published-port pool both facades register a listener
    // over. The pool needs more than one slot because two facades share it, and
    // the test names its first port so the rebind assertions can address the
    // exact provider socket.
    let port_window = PortWindow::claim();
    let forwarder = sample_forwarder(port_window.port(0));
    let published_pool = port_window.ports(1, port_window.usable() - 1);
    let published_port = *published_pool.start();
    let workload_root = root.path().join("container-machine-workload");
    let first_backend = ContainerSandboxBackend::with_network_process(
        machine_backend_config(
            &workload_root,
            &node_root,
            forwarder.clone(),
            published_pool.clone(),
        ),
        Arc::clone(&process),
    )
    .expect("first container should authenticate the process composition");
    let second_backend = ContainerSandboxBackend::with_network_process(
        machine_backend_config(&workload_root, &node_root, forwarder, published_pool),
        Arc::clone(&process),
    )
    .expect("second container should authenticate the process composition");
    let first_registry = first_backend.machine_port_proxies_handle_for_test();
    let second_registry = second_backend.machine_port_proxies_handle_for_test();
    let spec = sample_spec_for_tenant("process-machine-proxy", "shared-machine-proxy")
        .with_port_binding(SandboxPortBinding::tcp("http", published_port, 8080));
    let manifest = first_backend
        .plan_start(&spec)
        .expect("first facade should reserve the machine listener")
        .manifest;
    let key = (manifest.spec.tenant_id.clone(), manifest.handle.id.clone());

    first_backend
        .ensure_machine_port_proxies_running(&manifest.handle.id, &[Ipv4Addr::LOCALHOST], &manifest)
        .expect("first facade should start the real machine proxy lifecycle");
    assert!(
        second_registry
            .lock()
            .expect("the second facade should share the healthy registry")
            .contains_key(&key),
        "the second injected facade must observe the running provider lifecycle"
    );
    second_backend
        .ensure_machine_port_proxies_running(&manifest.handle.id, &[Ipv4Addr::LOCALHOST], &manifest)
        .expect("an exact duplicate facade request should reuse the one running provider");

    let duplicate_start = second_backend
        .ensure_machine_port_proxies_running(
            &manifest.handle.id,
            &[Ipv4Addr::new(127, 0, 0, 2)],
            &manifest,
        )
        .expect_err("a substituted route must conflict through the shared facade state");
    assert!(
        duplicate_start
            .to_string()
            .contains("exact listener generation"),
        "duplicate-start diagnostics must preserve the shared generation conflict: \
         {duplicate_start}"
    );
    let conflicting_bindings = [SandboxPortBinding::tcp("http", published_port, 8081)];
    let duplicate_cleanup = second_backend
        .withdraw_and_stop_machine_port_proxies(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &conflicting_bindings,
            &manifest.port_leases,
        )
        .expect_err("a substituted cleanup generation must fail through shared state");
    assert!(
        duplicate_cleanup
            .to_string()
            .contains("does not match the expected listener generation"),
        "duplicate-cleanup diagnostics must preserve the shared generation conflict: \
         {duplicate_cleanup}"
    );
    let collision = TcpListener::bind((Ipv4Addr::UNSPECIFIED, published_port))
        .expect_err("the one shared provider must remain bound after both conflicts");
    assert_eq!(collision.kind(), std::io::ErrorKind::AddrInUse);

    second_backend
        .withdraw_and_stop_machine_port_proxies(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("the second facade should complete exact shared cleanup");
    assert!(
        first_registry
            .lock()
            .expect("the first facade should share cleanup state")
            .is_empty(),
        "the first facade must observe cleanup completed through the second"
    );
    first_backend
        .withdraw_and_stop_machine_port_proxies(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("duplicate exact cleanup should observe the shared terminal state");
    drop(
        TcpListener::bind((Ipv4Addr::UNSPECIFIED, published_port))
            .expect("exact shared cleanup must release the real provider socket"),
    );
    assert_eq!(
        authority
            .port_leases()
            .inspect(manifest.port_leases[0].lease_id())
            .expect("cleaned machine lease should inspect")
            .expect("cleaned machine lease should remain durable")
            .phase(),
        PortLeasePhase::Released,
        "cleanup through either facade must converge the one durable lease generation"
    );

    let poisoner = std::thread::spawn(move || {
        let _guard = second_registry
            .lock()
            .expect("poison fixture should acquire the shared registry");
        panic!("intentional shared machine-proxy registry poison");
    });
    assert!(poisoner.join().is_err(), "poison fixture must panic");
    assert!(
        first_registry.lock().is_err(),
        "every injected facade must fail closed on the same poisoned registry"
    );
}

fn machine_backend_config(
    workload_root: &std::path::Path,
    network_root: &std::path::Path,
    forwarder: crate::backends::oci::network::OciMachinePortForwarderConfig,
    published_pool: std::ops::RangeInclusive<u16>,
) -> ContainerSandboxBackendConfig {
    let mut config = ContainerSandboxBackendConfig::under_root(workload_root)
        .with_network_state_root(network_root);
    config.node_network_supernet = "10.80.0.0/16".to_owned();
    config.node_tenant_subnet_prefix = 24;
    // The caller holds the claim on every port in this pool, so the fixture
    // publishes into it directly instead of searching for a free one.
    config.published_port_range = published_pool;
    config.machine_port_forwarder = Some(forwarder);
    config
}
