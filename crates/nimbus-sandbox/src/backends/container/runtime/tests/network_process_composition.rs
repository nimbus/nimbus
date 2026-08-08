//! Cross-facade machine-port lifecycle proof for the OCI network process.

use super::support::*;

use std::net::{Ipv4Addr, TcpListener};
use std::sync::Arc;

use nimbus_core::Cidr;
use nimbus_network::{LocalNetworkManager, PortLeasePhase};
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

    let published_port = unused_loopback_port_in(32_500..=32_999);
    let forwarder = sample_forwarder(unused_loopback_port());
    let workload_root = root.path().join("container-machine-workload");
    let first_backend = ContainerSandboxBackend::with_network_process(
        machine_backend_config(&workload_root, &node_root, forwarder.clone()),
        Arc::clone(&process),
    )
    .expect("first container should authenticate the process composition");
    let second_backend = ContainerSandboxBackend::with_network_process(
        machine_backend_config(&workload_root, &node_root, forwarder),
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
) -> ContainerSandboxBackendConfig {
    let mut config = ContainerSandboxBackendConfig::under_root(workload_root)
        .with_network_state_root(network_root);
    config.node_network_supernet = "10.80.0.0/16".to_owned();
    config.node_tenant_subnet_prefix = 24;
    config.published_port_range = 32_000..=32_999;
    config.machine_port_forwarder = Some(forwarder);
    config
}

fn unused_loopback_port() -> u16 {
    TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .expect("ephemeral listener should bind")
        .local_addr()
        .expect("ephemeral listener should have an address")
        .port()
}

fn unused_loopback_port_in(range: std::ops::RangeInclusive<u16>) -> u16 {
    range
        .into_iter()
        .find(|port| TcpListener::bind((Ipv4Addr::LOCALHOST, *port)).is_ok())
        .expect("fixture published-port range should have an unused listener")
}
