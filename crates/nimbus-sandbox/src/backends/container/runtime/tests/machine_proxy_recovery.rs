//! Machine-port proxy cleanup and restart reconciliation.

use super::*;

#[test]
fn machine_proxy_cleanup_targets_only_the_tenant_qualified_registry_entry() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let backend =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(temp_dir.path()));
    let tenant_a = nimbus_core::TenantId::new("tenant-machine-a").expect("tenant id");
    let tenant_b = nimbus_core::TenantId::new("tenant-machine-b").expect("tenant id");
    let id = SandboxId::new("shared-local-sandbox-id");
    {
        let mut registry = backend
            .machine_port_proxies
            .lock()
            .expect("machine proxy registry should lock");
        registry.insert(
            (tenant_a.clone(), id.clone()),
            MachinePortProxyEntry::Running(MachinePortProxyRegistration {
                port_bindings: Vec::new(),
                port_leases: Vec::new(),
                routes: Vec::new(),
                proxies: Vec::new(),
                lease_authority: None,
            }),
        );
        registry.insert(
            (tenant_b.clone(), id.clone()),
            MachinePortProxyEntry::Running(MachinePortProxyRegistration {
                port_bindings: Vec::new(),
                port_leases: Vec::new(),
                routes: Vec::new(),
                proxies: Vec::new(),
                lease_authority: None,
            }),
        );
    }

    let _tenant_a_cleanup = backend
        .begin_machine_port_proxy_restart(&tenant_a, &id, &[], &[])
        .expect("tenant-a proxy cleanup should begin")
        .expect("tenant-a running entry should yield a cleanup owner");
    let registry = backend
        .machine_port_proxies
        .lock()
        .expect("machine proxy registry should lock");
    assert!(
        matches!(
            registry.get(&(tenant_a, id.clone())),
            Some(MachinePortProxyEntry::Stopping(_))
        ),
        "tenant-a entry must become its own stopping tombstone"
    );
    assert!(
        matches!(
            registry.get(&(tenant_b, id)),
            Some(MachinePortProxyEntry::Running(_))
        ),
        "tenant-a cleanup must not mutate tenant-b's equal local sandbox id"
    );
}

#[test]
fn machine_proxy_restart_rebinds_exact_active_lease() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    // Offset 0 is the published binding the machine proxy binds; offset 1 is
    // the forwarder endpoint. The claim outlives every proxy generation below.
    let port_window = PortWindow::claim();
    let port = port_window.port(0);
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.machine_port_forwarder = Some(sample_forwarder(port_window.port(1)));
    let backend = ContainerSandboxBackend::new(config);
    let manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("http", port, 8080)),
            &SandboxId::new("machine-restart-owner"),
            None,
            None,
        )
        .expect("plan should reserve the restart listener")
        .manifest;
    backend
        .ensure_machine_port_proxies_running(&manifest.handle.id, &[Ipv4Addr::LOCALHOST], &manifest)
        .expect("first provider generation should start");
    backend
        .stop_machine_port_proxies(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("restart stop should acknowledge provider absence");

    let authority =
        nimbus_network::LocalPortLeaseAuthority::open(&backend.config.network_state_root)
            .expect("authority should open");
    let rebound = authority
        .inspect(manifest.port_leases[0].lease_id())
        .expect("rebound lease should inspect")
        .expect("rebound lease should remain durable");
    assert_eq!(
        rebound.phase(),
        nimbus_network::PortLeasePhase::Reserved,
        "acknowledged restart stop must retain the selected port as exact rebind authority"
    );

    backend
        .ensure_machine_port_proxies_running_for_restart(
            &manifest.handle.id,
            &[Ipv4Addr::LOCALHOST],
            &manifest,
        )
        .expect("the same incarnation must claim and restart its retained listener");
    let active = authority
        .inspect(manifest.port_leases[0].lease_id())
        .expect("active lease should inspect")
        .expect("active lease should remain durable");
    assert_eq!(active.phase(), nimbus_network::PortLeasePhase::Active);

    backend
        .withdraw_and_stop_machine_port_proxies(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("final provider stop should succeed");
    backend
        .port_lease_coordinator()
        .release_bindings(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("final provider absence should release the test lease");
}

#[test]
fn machine_proxy_accept_worker_panic_reports_then_cleanup_converges_on_retry() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    // Offset 0 is the published binding the machine proxy binds; offset 1 is
    // the forwarder endpoint. The claim outlives every proxy generation below.
    let port_window = PortWindow::claim();
    let port = port_window.port(0);
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.machine_port_forwarder = Some(sample_forwarder(port_window.port(1)));
    let backend = ContainerSandboxBackend::new(config);
    let manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("http", port, 8080)),
            &SandboxId::new("machine-worker-panic"),
            None,
            None,
        )
        .expect("plan should reserve the listener")
        .manifest;
    backend
        .ensure_machine_port_proxies_running(&manifest.handle.id, &[Ipv4Addr::LOCALHOST], &manifest)
        .expect("provider generation should start");
    {
        let mut registry = backend
            .machine_port_proxies
            .lock()
            .expect("machine proxy registry should lock");
        let entry = registry
            .get_mut(&(manifest.spec.tenant_id.clone(), manifest.handle.id.clone()))
            .expect("running registration should exist");
        let MachinePortProxyEntry::Running(registration) = entry else {
            panic!("provider generation should still be running");
        };
        let replacement = panicking_machine_port_proxy_for_test(SocketAddr::new(
            Ipv4Addr::UNSPECIFIED.into(),
            port,
        ));
        let mut original = std::mem::replace(&mut registration.proxies[0], replacement);
        original
            .shutdown()
            .expect("the real provider should stop before failure injection");
    }

    let deadline = std::time::Instant::now() + Duration::from_secs(1);
    loop {
        let provider_is_running = {
            let registry = backend
                .machine_port_proxies
                .lock()
                .expect("machine proxy registry should lock");
            let MachinePortProxyEntry::Running(registration) = registry
                .get(&(manifest.spec.tenant_id.clone(), manifest.handle.id.clone()))
                .expect("injected registration should remain")
            else {
                panic!("injected registration should remain running");
            };
            registration.proxies[0].provider_is_running()
        };
        if !provider_is_running {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "injected provider worker did not exit within one second"
        );
        thread::sleep(Duration::from_millis(5));
    }
    let published = Arc::new(AtomicBool::new(false));
    let publish_probe = Arc::clone(&published);
    let ensure_error = backend
        .ensure_machine_port_proxies_running_with_publication(
            &manifest.handle.id,
            &[Ipv4Addr::LOCALHOST],
            &manifest,
            MachinePortPreparationReleaseAuthority::Retain,
            move || {
                publish_probe.store(true, Ordering::SeqCst);
                Ok(())
            },
        )
        .expect_err("an exited retained provider must fence publication");
    assert!(
        ensure_error.to_string().contains("provider worker exited"),
        "the liveness fence should name the failed process-local provider: {ensure_error}"
    );
    assert!(
        !published.load(Ordering::SeqCst),
        "durable Active evidence must not republish an exited process-local provider"
    );

    let first = backend
        .stop_machine_port_proxies(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect_err("accept-worker panic must deny restart cleanup");
    backend
        .stop_machine_port_proxies(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("cleanup retry must consume the joined provider-absence proof");
    assert!(
        first
            .to_string()
            .contains("accept worker panicked during shutdown"),
        "the first attempt must preserve the provider diagnostic: {first}"
    );

    let authority =
        nimbus_network::LocalPortLeaseAuthority::open(&backend.config.network_state_root)
            .expect("authority should open");
    assert_eq!(
        authority
            .inspect(manifest.port_leases[0].lease_id())
            .expect("lease should inspect")
            .expect("listener authority should remain durable")
            .phase(),
        nimbus_network::PortLeasePhase::Reserved,
        "joined provider absence must authorize the exact restart rebind"
    );
    let registry = backend
        .machine_port_proxies
        .lock()
        .expect("machine proxy registry should lock");
    assert!(
        !registry.contains_key(&(manifest.spec.tenant_id.clone(), manifest.handle.id.clone())),
        "completed cleanup must retire its generation-qualified tombstone"
    );
}

#[test]
fn machine_proxy_restart_waits_for_external_unexpose_before_rebind() {
    // The claim holds the published port across the restart the provider script
    // below drives, so the rebind observes the exact port the routes name.
    let port_window = PortWindow::claim();
    let published_port = port_window.port(0);
    let listener = TcpListener::bind("127.0.0.1:0").expect("forwarder should bind");
    let forwarder_port = listener
        .local_addr()
        .expect("forwarder address should resolve")
        .port();
    let configured_forwarder = sample_forwarder(forwarder_port);
    let (request_tx, request_rx) = mpsc::channel();
    let (response_tx, response_rx) = mpsc::channel();
    let initially_exposed = serde_json::to_vec(&vec![serde_json::json!({
        "local": format!("127.0.0.1:{published_port}"),
        "remote": format!(":{published_port}"),
        "protocol": "tcp",
    })])
    .expect("initial provider route should encode");
    let server = thread::spawn(move || {
        let (mut initial_inspection, _) = listener
            .accept()
            .expect("initial inspection should connect");
        read_complete_http_request(&mut initial_inspection);
        let initial_response = format!(
            "HTTP/1.0 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
            initially_exposed.len()
        );
        initial_inspection
            .write_all(initial_response.as_bytes())
            .and_then(|()| initial_inspection.write_all(&initially_exposed))
            .expect("initial exposed list should write");

        let (mut unexpose, _) = listener.accept().expect("unexpose should connect");
        read_complete_http_request(&mut unexpose);
        request_tx
            .send(())
            .expect("unexpose receipt should be observable");
        response_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("unexpose response should be released");
        unexpose
            .write_all(b"HTTP/1.0 200 OK\r\nContent-Length: 0\r\n\r\n")
            .expect("native unexpose response should write");
        let (mut inspection, _) = listener
            .accept()
            .expect("absence inspection should connect");
        read_complete_http_request(&mut inspection);
        inspection
            .write_all(
                b"HTTP/1.0 200 OK\r\nContent-Type: application/json\r\n\
                  Content-Length: 2\r\n\r\n[]",
            )
            .expect("native absence list should write");
    });

    let temp_dir = TempDir::new().expect("tempdir should build");
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.machine_port_forwarder = Some(configured_forwarder);
    let backend = ContainerSandboxBackend::new(config);
    let manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("http", published_port, 8080)),
            &SandboxId::new("machine-restart-unexpose"),
            None,
            None,
        )
        .expect("plan should reserve the restart listener")
        .manifest;
    backend
        .ensure_machine_port_proxies_running(&manifest.handle.id, &[Ipv4Addr::LOCALHOST], &manifest)
        .expect("provider generation should start");
    let cleanup = backend
        .begin_machine_port_proxy_restart(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("restart cleanup should begin")
        .expect("the running provider should yield exact cleanup evidence");
    let authority =
        nimbus_network::LocalPortLeaseAuthority::open(&backend.config.network_state_root)
            .expect("authority should open");
    assert_eq!(
        authority
            .inspect(manifest.port_leases[0].lease_id())
            .expect("active lease should inspect")
            .expect("active lease should remain")
            .phase(),
        nimbus_network::PortLeasePhase::Active,
        "local provider stop alone must not authorize rebind before external unexpose"
    );
    let disposition_substitution = match backend.begin_machine_port_proxy_release(
        &manifest.spec.tenant_id,
        &manifest.handle.id,
        &manifest.spec.port_bindings,
        &manifest.port_leases,
    ) {
        Ok(_) => panic!("a restart tombstone must reject release-disposition substitution"),
        Err(error) => error,
    };
    assert!(
        disposition_substitution
            .to_string()
            .contains("different exact listener generation or disposition"),
        "the tombstone must authenticate its exact disposition: {disposition_substitution}"
    );

    let unexpose_backend = backend.clone();
    let forwarder = backend
        .config
        .machine_port_forwarder
        .clone()
        .expect("forwarder should remain configured");
    let unexpose_thread = thread::spawn(move || {
        let result =
            unexpose_backend.unexpose_machine_port_proxy_publications(&cleanup, &forwarder);
        (cleanup, result)
    });
    request_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("unexpose should reach the external provider");

    assert_eq!(
        authority
            .inspect(manifest.port_leases[0].lease_id())
            .expect("fenced lease should inspect")
            .expect("fenced lease should remain")
            .phase(),
        nimbus_network::PortLeasePhase::Active,
        "an in-flight unexpose must retain the exact active generation fence"
    );
    let replacement = backend
        .ensure_machine_port_proxies_running(&manifest.handle.id, &[Ipv4Addr::LOCALHOST], &manifest)
        .expect_err("a stopping tombstone must reject replacement publication");
    assert!(
        replacement
            .to_string()
            .contains("cleanup is still in progress"),
        "replacement rejection should identify the stopping tombstone: {replacement}"
    );

    response_tx
        .send(())
        .expect("external unexpose acknowledgement should release");
    let (cleanup, unexpose_result) = unexpose_thread.join().expect("unexpose thread should join");
    unexpose_result.expect("external unexpose should be acknowledged");
    server.join().expect("forwarder server should join");
    backend
        .complete_machine_port_proxy_cleanup(&cleanup)
        .expect("acknowledged unexpose may complete the atomic rebind transition");
    assert_eq!(
        authority
            .inspect(manifest.port_leases[0].lease_id())
            .expect("rebind lease should inspect")
            .expect("rebind lease should remain")
            .phase(),
        nimbus_network::PortLeasePhase::Reserved,
        "only external unexpose acknowledgement may authorize exact restart rebind"
    );
    let manager = backend.port_lease_coordinator();
    manager
        .withdraw_bindings(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("restart authority should withdraw after confirmed provider absence");
    manager
        .release_bindings(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("withdrawn restart authority should release");
}

#[test]
fn empty_overlapping_machine_proxy_registry_keeps_live_provider_fenced() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    // Offset 0 is the published binding the machine proxy binds; offset 1 is
    // the forwarder endpoint. The claim outlives every proxy generation below.
    let port_window = PortWindow::claim();
    let port = port_window.port(0);
    let tenant =
        nimbus_core::TenantId::new("tenant-machine-overlap").expect("tenant should validate");
    let id = SandboxId::new("machine-overlap");
    let spec = SandboxSpec::new(
        tenant.clone(),
        crate::spec::SandboxOwnerSpec::service("machine-overlap"),
        crate::backend::SandboxBackendKind::Container,
        crate::spec::SandboxRootSpec::Rootfs(crate::spec::SandboxRootfsSpec::new("/tmp/rootfs")),
        crate::spec::SandboxProcessSpec::new(["/bin/sh", "-c", "sleep 60"]),
    )
    .with_port_binding(SandboxPortBinding::tcp("http", port, 8080));
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.machine_port_forwarder = Some(sample_forwarder(port_window.port(1)));
    let first = ContainerSandboxBackend::new(config.clone());
    let manifest = first
        .plan_start_with_id(&spec, &id, None, None)
        .expect("plan should reserve the machine listener")
        .manifest;
    first
        .ensure_machine_port_proxies_running(&id, &[Ipv4Addr::LOCALHOST], &manifest)
        .expect("first backend should own and activate the machine proxy");

    let overlapping = ContainerSandboxBackend::new(config);
    assert!(
        overlapping
            .machine_port_proxies
            .lock()
            .expect("overlapping registry should lock")
            .is_empty(),
        "a fresh backend has no process-local provider evidence"
    );
    overlapping
        .port_lease_coordinator()
        .withdraw_bindings(
            &tenant,
            &id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("teardown must fence the listener before attempting stop");
    let stale_fast_path = first
        .ensure_machine_port_proxies_running(&id, &[Ipv4Addr::LOCALHOST], &manifest)
        .expect_err("a matching local registry must revalidate durable Active authority");
    assert!(
        stale_fast_path
            .to_string()
            .contains("expected exact Active"),
        "the fast path must reject a concurrently withdrawn generation: {stale_fast_path}"
    );
    let ambiguity = overlapping
        .stop_machine_port_proxies(
            &tenant,
            &id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect_err("an empty overlapping registry cannot confirm provider shutdown");
    assert!(
        ambiguity.to_string().contains("live process lifetime"),
        "the error must identify the live process-owner fence: {ambiguity}"
    );

    let authority = nimbus_network::LocalPortLeaseAuthority::open(&first.config.network_state_root)
        .expect("authority");
    let record = authority
        .inspect(manifest.port_leases[0].lease_id())
        .expect("lease should inspect")
        .expect("lease should remain durable");
    assert_eq!(
        record.phase(),
        nimbus_network::PortLeasePhase::Withdrawing,
        "ambiguous stop must retain the host-global fence"
    );
    let collision = TcpListener::bind((Ipv4Addr::UNSPECIFIED, port))
        .expect_err("the original provider must still own the real socket");
    assert_eq!(collision.kind(), std::io::ErrorKind::AddrInUse);

    first
        .withdraw_and_stop_machine_port_proxies(
            &tenant,
            &id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("the exact local registry should resume withdrawal and release its provider");
    first
        .port_lease_coordinator()
        .release_bindings(
            &tenant,
            &id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("confirmed provider stop may release durable authority");
    TcpListener::bind((Ipv4Addr::UNSPECIFIED, port))
        .expect("confirmed stop and release must make the real port reusable");
}

#[test]
fn independent_machine_backend_cannot_withdraw_another_process_provider() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    // Offset 0 is the published binding the machine proxy binds; offset 1 is
    // the forwarder endpoint. The claim outlives every proxy generation below.
    let port_window = PortWindow::claim();
    let port = port_window.port(0);
    let tenant = nimbus_core::TenantId::new("tenant-machine-foreign-withdraw")
        .expect("tenant should validate");
    let id = SandboxId::new("machine-foreign-withdraw");
    let spec = SandboxSpec::new(
        tenant.clone(),
        crate::spec::SandboxOwnerSpec::service("machine-foreign-withdraw"),
        crate::backend::SandboxBackendKind::Container,
        crate::spec::SandboxRootSpec::Rootfs(crate::spec::SandboxRootfsSpec::new("/tmp/rootfs")),
        crate::spec::SandboxProcessSpec::new(["/bin/sh", "-c", "sleep 60"]),
    )
    .with_port_binding(SandboxPortBinding::tcp("http", port, 8080));
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.machine_port_forwarder = Some(sample_forwarder(port_window.port(1)));
    let owner = ContainerSandboxBackend::new(config.clone());
    let manifest = owner
        .plan_start_with_id(&spec, &id, None, None)
        .expect("plan should reserve the machine listener")
        .manifest;
    owner
        .ensure_machine_port_proxies_running(&id, &[Ipv4Addr::LOCALHOST], &manifest)
        .expect("owner backend should start the machine proxy");

    let foreign = ContainerSandboxBackend::new(config);
    let error = foreign
        .withdraw_and_stop_machine_port_proxies(
            &tenant,
            &id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect_err("a backend without the provider registration must not withdraw it");
    assert!(
        error.to_string().contains("live process lifetime"),
        "the failure must identify the still-live foreign provider owner: {error}"
    );

    let authority = nimbus_network::LocalPortLeaseAuthority::open(&owner.config.network_state_root)
        .expect("authority should open");
    let record = authority
        .inspect(manifest.port_leases[0].lease_id())
        .expect("lease should inspect")
        .expect("lease should remain durable");
    assert_eq!(
        record.phase(),
        nimbus_network::PortLeasePhase::Active,
        "foreign teardown must prove provider ownership before changing durable authority"
    );
    let collision = TcpListener::bind((Ipv4Addr::UNSPECIFIED, port))
        .expect_err("the owner provider must remain bound");
    assert_eq!(collision.kind(), std::io::ErrorKind::AddrInUse);

    owner
        .withdraw_and_stop_machine_port_proxies(
            &tenant,
            &id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("the backend with the exact registration may withdraw and stop");
    owner
        .port_lease_coordinator()
        .release_bindings(
            &tenant,
            &id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("confirmed provider stop may release durable authority");
}

#[test]
fn machine_proxy_lifetime_fences_live_owner_and_recovers_after_owner_drop() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    // Offset 0 is the published binding the machine proxy binds; offset 1 is
    // the forwarder endpoint. The claim outlives every proxy generation below.
    let port_window = PortWindow::claim();
    let port = port_window.port(0);
    let tenant =
        nimbus_core::TenantId::new("tenant-machine-lifetime").expect("tenant should validate");
    let id = SandboxId::new("machine-lifetime");
    let spec = SandboxSpec::new(
        tenant.clone(),
        crate::spec::SandboxOwnerSpec::service("machine-lifetime"),
        crate::backend::SandboxBackendKind::Container,
        crate::spec::SandboxRootSpec::Rootfs(crate::spec::SandboxRootfsSpec::new("/tmp/rootfs")),
        crate::spec::SandboxProcessSpec::new(["/bin/sh", "-c", "sleep 60"]),
    )
    .with_port_binding(SandboxPortBinding::tcp("http", port, 8080));
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.machine_port_forwarder = Some(sample_forwarder(port_window.port(1)));
    let owner = ContainerSandboxBackend::new(config.clone());
    let manifest = owner
        .plan_start_with_id(&spec, &id, None, None)
        .expect("plan should reserve the machine listener")
        .manifest;
    owner
        .ensure_machine_port_proxies_running(&id, &[Ipv4Addr::LOCALHOST], &manifest)
        .expect("owner backend should start the machine proxy");

    let authority = nimbus_network::LocalPortLeaseAuthority::open(&owner.config.network_state_root)
        .expect("authority");
    let active = authority
        .inspect(manifest.port_leases[0].lease_id())
        .expect("lease should inspect")
        .expect("lease should remain durable");
    let lifetime = active
        .active_lifetime()
        .expect("every machine provider effect must retain a process-owner generation");
    assert_eq!(
        lifetime.effect_scope(),
        nimbus_network::PortLeaseEffectScope::ProviderManaged,
        "the external machine publication may outlive its process coordinator"
    );

    let recovery = ContainerSandboxBackend::new(config);
    let before_live_rebuild = authority
        .inspect(manifest.port_leases[0].lease_id())
        .expect("lease should inspect")
        .expect("lease should remain durable");
    let live_rebuild_error = recovery
        .ensure_machine_port_proxies_running_for_restart(&id, &[Ipv4Addr::LOCALHOST], &manifest)
        .expect_err("a restart coordinator must not reclaim a live local-listener owner");
    assert!(
        live_rebuild_error
            .to_string()
            .contains("live process lifetime"),
        "live-owner route rebuild must identify the exact lifetime fence: {live_rebuild_error}"
    );
    assert_eq!(
        authority
            .inspect(manifest.port_leases[0].lease_id())
            .expect("lease should inspect")
            .expect("lease should remain durable"),
        before_live_rebuild,
        "a live-owner route rebuild attempt must leave durable authority byte-equivalent"
    );

    let live_error = match recovery.begin_machine_port_proxy_release(
        &tenant,
        &id,
        &manifest.spec.port_bindings,
        &manifest.port_leases,
    ) {
        Ok(_) => panic!("a second coordinator must fail closed while the lifetime owner is live"),
        Err(error) => error,
    };
    assert!(
        live_error.to_string().contains("live process lifetime"),
        "live-owner rejection must identify the exact lifetime fence: {live_error}"
    );
    assert_eq!(
        authority
            .inspect(manifest.port_leases[0].lease_id())
            .expect("lease should inspect")
            .expect("lease should remain durable")
            .phase(),
        nimbus_network::PortLeasePhase::Active,
        "a live-owner recovery attempt must not mutate durable authority"
    );

    drop(owner);
    let cleanup = recovery
        .begin_machine_port_proxy_release(
            &tenant,
            &id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("one successor should acquire dead-owner recovery")
        .expect("dead provider-managed authority requires exact cleanup");
    assert_eq!(
        authority
            .inspect(manifest.port_leases[0].lease_id())
            .expect("lease should inspect")
            .expect("lease should remain durable")
            .phase(),
        nimbus_network::PortLeasePhase::CleanupPending,
        "owner death must quarantine provider-managed authority before inspection"
    );
    recovery
        .confirm_machine_port_proxy_publication_absent(&cleanup)
        .expect("test provider should authenticate exact publication absence");
    recovery
        .complete_machine_port_proxy_cleanup(&cleanup)
        .expect("exact absence should complete dead-owner cleanup");
    assert_eq!(
        authority
            .inspect(manifest.port_leases[0].lease_id())
            .expect("lease should inspect")
            .expect("lease should remain durable")
            .phase(),
        nimbus_network::PortLeasePhase::Released
    );
}

#[test]
fn absent_machine_registry_accepts_only_an_entire_terminal_no_effect_batch() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    // The window owns the two-port published range plus the forwarder endpoint
    // at offset 2. The claim holds for the whole test, so the range the
    // coordinator walks names ports no other test process can draw.
    let port_window = PortWindow::claim();
    let first_port = port_window.port(0);
    let second_port = port_window.port(1);
    assert_ne!(first_port, second_port);

    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.machine_port_forwarder = Some(sample_forwarder(port_window.port(2)));
    config.published_port_range = first_port.min(second_port)..=first_port.max(second_port);
    let backend = ContainerSandboxBackend::new(config);
    let manager = backend.port_lease_coordinator();
    let tenant =
        nimbus_core::TenantId::new("tenant-machine-terminal").expect("tenant should validate");
    let id = SandboxId::new("machine-terminal");
    let bindings = [
        SandboxPortBinding::tcp("released", first_port, 8080),
        SandboxPortBinding::tcp("failed", second_port, 8081),
    ];
    let reservation_claim = crate::backends::oci::port_lease::new_launch_reservation_claim()
        .expect("terminal batch launch claim should mint");
    let mut reservations = manager
        .reserve_launch_ports_for_sandbox(
            crate::backends::oci::port_lifecycle::SandboxLaunchPortPlan::new(
                &tenant,
                &id,
                &bindings,
                &[],
            ),
            &reservation_claim,
        )
        .expect("terminal batch should reserve atomically");
    reservations
        .confirm_manifest_published()
        .expect("fixture should publish its exact launch request set");
    manager
        .release_never_bound_requests(
            std::slice::from_ref(&reservations.published_leases[0]),
            &reservations.reservation_claim,
        )
        .expect("first never-bound listener should release");
    let failed_claim = manager
        .claim_machine_bindings(
            &tenant,
            &id,
            std::slice::from_ref(&bindings[1]),
            std::slice::from_ref(&reservations.published_leases[1]),
        )
        .expect("failed listener should claim its provider attempt")
        .pop()
        .expect("one listener should return one claim");
    manager
        .record_machine_proxy_bind_failure(
            &reservations.published_leases[1],
            &failed_claim,
            SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), second_port),
            std::io::ErrorKind::AddrInUse,
        )
        .expect("second listener should record terminal no-effect failure");

    manager
        .classify_machine_cleanup_batch(&tenant, &id, &bindings, &reservations.published_leases)
        .expect("a Failed/Released batch must classify as uniformly terminal without effect");
    backend
        .stop_machine_port_proxies(&tenant, &id, &bindings, &reservations.published_leases)
        .expect("an absent registry is idempotent when every exact listener is terminal");

    let phases = reservations
        .published_leases
        .iter()
        .map(|request| {
            nimbus_network::LocalPortLeaseAuthority::open(&backend.config.network_state_root)
                .expect("authority should open")
                .inspect(request.lease_id())
                .expect("lease should inspect")
                .expect("terminal record should persist")
                .phase()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        phases,
        [
            nimbus_network::PortLeasePhase::Released,
            nimbus_network::PortLeasePhase::Failed,
        ],
        "idempotent cleanup must preserve exact terminal evidence"
    );
}

fn read_complete_http_request(stream: &mut TcpStream) {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("request read timeout should set");
    let mut request = Vec::new();
    let mut expected_len = None;
    let mut chunk = [0_u8; 1024];
    loop {
        let read = stream.read(&mut chunk).expect("request should read");
        assert!(read > 0, "request closed before its complete body arrived");
        request.extend_from_slice(&chunk[..read]);
        if expected_len.is_none()
            && let Some(header_end) = request.windows(4).position(|window| window == b"\r\n\r\n")
        {
            let headers = std::str::from_utf8(&request[..header_end])
                .expect("request headers should be UTF-8");
            let content_len = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().expect("valid content length"))
                })
                .unwrap_or(0);
            expected_len = Some(header_end + 4 + content_len);
        }
        if expected_len.is_some_and(|expected| request.len() >= expected) {
            return;
        }
    }
}
