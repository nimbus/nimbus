//! Machine-port proxy activation, identity, and exact-plan reuse.

use super::*;

#[test]
fn machine_proxy_rejects_caller_manifest_identity_mismatch_before_effect() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let port = unused_loopback_port();
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.machine_port_forwarder = Some(sample_forwarder(unused_loopback_port()));
    let backend = ContainerSandboxBackend::new(config);
    let manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("http", port, 8080)),
            &SandboxId::new("machine-manifest-owner"),
            None,
            None,
        )
        .expect("plan should reserve the machine listener")
        .manifest;
    let published = Arc::new(AtomicBool::new(false));
    let published_by_call = Arc::clone(&published);

    let error = backend
        .ensure_machine_port_proxies_running_with_publication(
            &SandboxId::new("machine-substituted-caller"),
            &[Ipv4Addr::LOCALHOST],
            &manifest,
            MachinePortPreparationReleaseAuthority::FreshLaunch(
                manifest
                    .launch_reservation_claim
                    .as_ref()
                    .expect("planned launch should retain coordinator claim"),
            ),
            move || {
                published_by_call.store(true, Ordering::SeqCst);
                Ok(())
            },
        )
        .expect_err("a substituted caller identity must fail before provider publication");

    assert!(
        error
            .to_string()
            .contains("does not match manifest sandbox"),
        "the rejection must identify the caller/manifest mismatch: {error}"
    );
    assert!(
        !published.load(Ordering::SeqCst),
        "identity validation must precede provider publication"
    );
    assert!(
        backend
            .machine_port_proxies
            .lock()
            .expect("machine proxy registry should lock")
            .is_empty(),
        "identity rejection must not register a provider effect"
    );
    let port_probe = TcpListener::bind((Ipv4Addr::UNSPECIFIED, port))
        .expect("identity rejection must not bind the requested host port");
    drop(port_probe);
    let record = nimbus_network::LocalPortLeaseAuthority::open(&backend.config.network_state_root)
        .expect("port authority should open")
        .inspect(manifest.port_leases[0].lease_id())
        .expect("lease should inspect")
        .expect("reservation should remain durable");
    assert_eq!(
        record.phase(),
        nimbus_network::PortLeasePhase::Reserved,
        "identity rejection must precede durable provider adoption"
    );
    backend
        .port_lease_coordinator()
        .release_never_bound_requests(
            &manifest.port_leases,
            manifest
                .launch_reservation_claim
                .as_ref()
                .expect("planned launch should retain coordinator claim"),
        )
        .expect("test reservation should release after absence is proven");
}

#[test]
fn machine_proxy_activation_failure_drops_listeners_and_abandons_exact_claims() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let port = unused_loopback_port();
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.machine_port_forwarder = Some(sample_forwarder(unused_loopback_port()));
    let backend = ContainerSandboxBackend::new(config);
    let manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("http", port, 8080)),
            &SandboxId::new("machine-activation-failure"),
            None,
            None,
        )
        .expect("plan should reserve the machine listener")
        .manifest;

    let error = backend
        .ensure_machine_port_proxies_running_with_activation_failure(
            &manifest.handle.id,
            &[Ipv4Addr::LOCALHOST],
            &manifest,
            || {
                Err(SandboxError::OperationFailed {
                    message: "injected machine activation failure".to_owned(),
                })
            },
        )
        .expect_err("injected durable activation failure must fail startup");
    assert!(
        error
            .to_string()
            .contains("injected machine activation failure"),
        "the provider failure must remain primary: {error}"
    );
    assert!(
        backend
            .machine_port_proxies
            .lock()
            .expect("machine proxy registry should lock")
            .is_empty(),
        "failed activation must not register a provider"
    );
    let port_probe = TcpListener::bind((Ipv4Addr::UNSPECIFIED, port))
        .expect("failed activation must drop every inert listener before compensation");
    drop(port_probe);
    let record = nimbus_network::LocalPortLeaseAuthority::open(&backend.config.network_state_root)
        .expect("port authority should open")
        .inspect(manifest.port_leases[0].lease_id())
        .expect("lease should inspect")
        .expect("reservation should remain durable");
    assert_eq!(record.phase(), nimbus_network::PortLeasePhase::Reserved);
    assert!(
        record.bind_claim().is_none(),
        "proven listener absence must abandon the exact durable bind claim"
    );
    assert!(
        record.binding().is_none(),
        "pre-activation failure must retain no observed provider binding"
    );

    backend
        .ensure_machine_port_proxies_running(&manifest.handle.id, &[Ipv4Addr::LOCALHOST], &manifest)
        .expect("the exact manifest should retry after claim compensation");
    backend
        .withdraw_and_stop_machine_port_proxies(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("the retry provider should stop");
    backend
        .port_lease_coordinator()
        .release_bindings(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("confirmed retry provider absence should release the test lease");
}

#[test]
fn machine_proxy_activation_ack_loss_inspects_active_binding_and_rebinds() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let port = unused_loopback_port();
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.machine_port_forwarder = Some(sample_forwarder(unused_loopback_port()));
    let backend = ContainerSandboxBackend::new(config);
    let manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("http", port, 8080)),
            &SandboxId::new("machine-activation-ack-loss"),
            None,
            None,
        )
        .expect("plan should reserve the machine listener")
        .manifest;

    let error = backend
        .ensure_machine_port_proxies_running_with_activation_ack_loss(
            &manifest.handle.id,
            &[Ipv4Addr::LOCALHOST],
            &manifest,
            || {
                Err(SandboxError::OperationFailed {
                    message: "injected activation acknowledgement loss".to_owned(),
                })
            },
        )
        .expect_err("ambiguous activation acknowledgement loss must fail startup");
    assert!(
        error
            .to_string()
            .contains("injected activation acknowledgement loss"),
        "the ambiguous activation error must remain primary: {error}"
    );
    assert!(
        backend
            .machine_port_proxies
            .lock()
            .expect("machine proxy registry should lock")
            .is_empty(),
        "ambiguous activation must not register a process-local provider"
    );
    let port_probe = TcpListener::bind((Ipv4Addr::UNSPECIFIED, port))
        .expect("compensation must drop every inert listener before durable inspection");
    drop(port_probe);
    let record = nimbus_network::LocalPortLeaseAuthority::open(&backend.config.network_state_root)
        .expect("port authority should open")
        .inspect(manifest.port_leases[0].lease_id())
        .expect("lease should inspect")
        .expect("reservation should remain durable");
    assert_eq!(
        record.phase(),
        nimbus_network::PortLeasePhase::Reserved,
        "exact Active inspection plus confirmed provider stop must prepare the lease for rebind"
    );
    assert!(
        record.bind_claim().is_none() && record.binding().is_none(),
        "rebind preparation must clear only obsolete attempt and provider evidence"
    );

    backend
        .ensure_machine_port_proxies_running(&manifest.handle.id, &[Ipv4Addr::LOCALHOST], &manifest)
        .expect("the exact manifest should retry after ambiguous-outcome reconciliation");
    backend
        .withdraw_and_stop_machine_port_proxies(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("the retry provider should stop");
    backend
        .port_lease_coordinator()
        .release_bindings(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("confirmed retry provider absence should release the test lease");
}

#[test]
fn machine_proxy_reuse_requires_exact_normalized_forwarding_plan() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let port = unused_loopback_port();
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.machine_port_forwarder = Some(sample_forwarder(unused_loopback_port()));
    let backend = ContainerSandboxBackend::new(config);
    let manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("http", port, 8080)),
            &SandboxId::new("machine-route-owner"),
            None,
            None,
        )
        .expect("plan should reserve the machine listener")
        .manifest;
    backend
        .ensure_machine_port_proxies_running(&manifest.handle.id, &[Ipv4Addr::LOCALHOST], &manifest)
        .expect("first exact forwarding plan should start");
    let published = Arc::new(AtomicBool::new(false));
    let published_by_call = Arc::clone(&published);

    let error = backend
        .ensure_machine_port_proxies_running_with_publication(
            &manifest.handle.id,
            &[Ipv4Addr::new(127, 0, 0, 2)],
            &manifest,
            MachinePortPreparationReleaseAuthority::FreshLaunch(
                manifest
                    .launch_reservation_claim
                    .as_ref()
                    .expect("planned launch should retain coordinator claim"),
            ),
            move || {
                published_by_call.store(true, Ordering::SeqCst);
                Ok(())
            },
        )
        .expect_err("a changed provider target must not reuse the prior live proxy");

    assert!(
        error.to_string().contains("exact listener generation"),
        "the rejection must identify mismatched provider evidence: {error}"
    );
    assert!(
        !published.load(Ordering::SeqCst),
        "a stale provider target must be rejected before publication"
    );
    backend
        .withdraw_and_stop_machine_port_proxies(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("the original exact provider should stop");
    backend
        .port_lease_coordinator()
        .release_bindings(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("confirmed provider absence should release the test lease");
}

#[test]
fn machine_publication_rejects_external_address_substitution_before_proxy_or_forwarder_effect() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let port = unused_loopback_port();
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.machine_port_forwarder = Some(sample_forwarder(unused_loopback_port()));
    let backend = ContainerSandboxBackend::new(config);
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(
                SandboxPortBinding::tcp("http", port, 8080)
                    .with_host_address(std::net::IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7))),
            ),
            &SandboxId::new("machine-publication-owner"),
            None,
            None,
        )
        .expect("plan should reserve the exact external publication intent")
        .manifest;
    manifest.spec.port_bindings[0].host_address = std::net::IpAddr::V4(Ipv4Addr::UNSPECIFIED);
    let published = Arc::new(AtomicBool::new(false));
    let published_by_call = Arc::clone(&published);

    let result = backend.ensure_machine_port_proxies_running_with_publication(
        &manifest.handle.id,
        &[Ipv4Addr::LOCALHOST],
        &manifest,
        MachinePortPreparationReleaseAuthority::FreshLaunch(
            manifest
                .launch_reservation_claim
                .as_ref()
                .expect("planned launch should retain coordinator claim"),
        ),
        move || {
            published_by_call.store(true, Ordering::SeqCst);
            Ok(())
        },
    );
    if result.is_ok() {
        let _ = backend.stop_machine_port_proxies(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        );
    }
    let error = result.expect_err(
        "a substituted external address must fail before proxy bind or forwarder publication",
    );
    assert!(
        error.to_string().contains("does not match the caller"),
        "the rejection must identify divergent durable publication intent: {error}"
    );
    assert!(
        !published.load(Ordering::SeqCst),
        "address substitution must fail before forwarder publication"
    );
    assert!(
        backend
            .machine_port_proxies
            .lock()
            .expect("machine proxy registry should lock")
            .is_empty(),
        "address substitution must not retain a provider effect"
    );
    let record = nimbus_network::LocalPortLeaseAuthority::open(&backend.config.network_state_root)
        .expect("authority should open")
        .inspect(manifest.port_leases[0].lease_id())
        .expect("lease should inspect")
        .expect("reservation should remain durable");
    assert_eq!(
        record.phase(),
        nimbus_network::PortLeasePhase::Reserved,
        "address rejection must precede durable provider adoption"
    );
    backend
        .port_lease_coordinator()
        .release_never_bound_requests(
            &manifest.port_leases,
            manifest
                .launch_reservation_claim
                .as_ref()
                .expect("planned launch should retain coordinator claim"),
        )
        .expect("test reservation should release after absence is proven");
}
