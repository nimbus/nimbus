//! Machine-port proxy publication and withdrawal linearization.

use super::*;

#[test]
fn machine_proxy_withdrawal_waits_for_inflight_active_validation() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let port = unused_loopback_port();
    let tenant =
        nimbus_core::TenantId::new("tenant-machine-linearization").expect("tenant should validate");
    let id = SandboxId::new("machine-linearization");
    let spec = SandboxSpec::new(
        tenant.clone(),
        crate::spec::SandboxOwnerSpec::service("machine-linearization"),
        crate::backend::SandboxBackendKind::Container,
        crate::spec::SandboxRootSpec::Rootfs(crate::spec::SandboxRootfsSpec::new("/tmp/rootfs")),
        crate::spec::SandboxProcessSpec::new(["/bin/sh", "-c", "sleep 60"]),
    )
    .with_port_binding(SandboxPortBinding::tcp("http", port, 8080));
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.machine_port_forwarder = Some(sample_forwarder(unused_loopback_port()));
    let backend = ContainerSandboxBackend::new(config);
    let manifest = backend
        .plan_start_with_id(&spec, &id, None, None)
        .expect("plan should reserve the machine listener")
        .manifest;
    backend
        .ensure_machine_port_proxies_running(&id, &[Ipv4Addr::LOCALHOST], &manifest)
        .expect("initial ensure should own and activate the machine proxy");

    let (validated_tx, validated_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let ensuring_backend = backend.clone();
    let ensuring_id = id.clone();
    let ensuring_manifest = manifest.clone();
    let ensure_thread = thread::spawn(move || {
        ensuring_backend.ensure_machine_port_proxies_running_at_validation_barrier(
            &ensuring_id,
            &[Ipv4Addr::LOCALHOST],
            &ensuring_manifest,
            move || {
                validated_tx
                    .send(())
                    .expect("validation barrier should signal");
                release_rx
                    .recv_timeout(Duration::from_secs(1))
                    .expect("validation barrier should release");
            },
        )
    });
    validated_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("ensure should reach the post-validation barrier");

    let (lock_tx, lock_rx) = mpsc::channel();
    let withdrawing_backend = backend.clone();
    let withdrawing_id = id.clone();
    let withdrawing_manifest = manifest.clone();
    let withdraw_thread = thread::spawn(move || {
        withdrawing_backend.withdraw_and_stop_machine_port_proxies_at_lock_barrier(
            &tenant,
            &withdrawing_id,
            &withdrawing_manifest.spec.port_bindings,
            &withdrawing_manifest.port_leases,
            move || {
                lock_tx
                    .send(())
                    .expect("withdrawal lock barrier should signal");
            },
        )
    });
    lock_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("withdrawal should reach the registry lock barrier");

    let phase_during_validation =
        nimbus_network::LocalPortLeaseAuthority::open(&backend.config.network_state_root)
            .expect("authority should open")
            .inspect(manifest.port_leases[0].lease_id())
            .expect("lease should inspect")
            .expect("lease should remain durable")
            .phase();

    release_tx
        .send(())
        .expect("inflight validation should release");
    ensure_thread
        .join()
        .expect("ensure thread should join")
        .expect("the already-validated ensure should complete");
    withdraw_thread
        .join()
        .expect("withdraw thread should join")
        .expect("withdrawal should stop the exact proxy after validation completes");

    assert_eq!(
        phase_during_validation,
        nimbus_network::PortLeasePhase::Active,
        "withdrawal must acquire the same lifecycle lock before changing durable authority"
    );
}

#[test]
fn machine_proxy_withdrawal_waits_for_inflight_publication() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let port = unused_loopback_port();
    let tenant = nimbus_core::TenantId::new("tenant-machine-publish-linearization")
        .expect("tenant should validate");
    let id = SandboxId::new("machine-publish-linearization");
    let spec = SandboxSpec::new(
        tenant.clone(),
        crate::spec::SandboxOwnerSpec::service("machine-publish-linearization"),
        crate::backend::SandboxBackendKind::Container,
        crate::spec::SandboxRootSpec::Rootfs(crate::spec::SandboxRootfsSpec::new("/tmp/rootfs")),
        crate::spec::SandboxProcessSpec::new(["/bin/sh", "-c", "sleep 60"]),
    )
    .with_port_binding(SandboxPortBinding::tcp("http", port, 8080));
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.machine_port_forwarder = Some(sample_forwarder(unused_loopback_port()));
    let backend = ContainerSandboxBackend::new(config);
    let manifest = backend
        .plan_start_with_id(&spec, &id, None, None)
        .expect("plan should reserve the machine listener")
        .manifest;

    let (publishing_tx, publishing_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let publishing_backend = backend.clone();
    let publishing_id = id.clone();
    let publishing_manifest = manifest.clone();
    let publish_thread = thread::spawn(move || {
        publishing_backend.ensure_machine_port_proxies_running_at_publication_barrier(
            &publishing_id,
            &[Ipv4Addr::LOCALHOST],
            &publishing_manifest,
            move || {
                publishing_tx
                    .send(())
                    .expect("publication barrier should signal");
                release_rx
                    .recv_timeout(Duration::from_secs(1))
                    .expect("publication barrier should release");
                Ok(())
            },
        )
    });
    publishing_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("ensure should reach the publication barrier");

    let withdrawing_backend = backend.clone();
    let lock_probe_backend = backend.clone();
    let withdrawing_id = id.clone();
    let withdrawing_manifest = manifest.clone();
    let (at_lock_tx, at_lock_rx) = mpsc::channel();
    let (withdrawn_tx, withdrawn_rx) = mpsc::channel();
    let withdraw_thread = thread::spawn(move || {
        let result = withdrawing_backend.withdraw_and_stop_machine_port_proxies_at_lock_barrier(
            &tenant,
            &withdrawing_id,
            &withdrawing_manifest.spec.port_bindings,
            &withdrawing_manifest.port_leases,
            move || {
                assert!(
                    matches!(
                        lock_probe_backend.machine_port_proxies.try_lock(),
                        Err(std::sync::TryLockError::WouldBlock)
                    ),
                    "publication must hold the exact registry mutex immediately before withdrawal tries to acquire it"
                );
                at_lock_tx
                    .send(())
                    .expect("withdrawal lock barrier should signal");
            },
        );
        withdrawn_tx
            .send(())
            .expect("withdrawal completion should signal");
        result
    });
    at_lock_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("withdrawal should reach the registry-lock boundary");

    let phase_during_publication =
        nimbus_network::LocalPortLeaseAuthority::open(&backend.config.network_state_root)
            .expect("authority should open")
            .inspect(manifest.port_leases[0].lease_id())
            .expect("lease should inspect")
            .expect("lease should remain durable")
            .phase();

    release_tx
        .send(())
        .expect("inflight publication should release");
    publish_thread
        .join()
        .expect("publication thread should join")
        .expect("publication should complete");
    withdrawn_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("withdrawal should complete after publication releases the registry");
    withdraw_thread
        .join()
        .expect("withdraw thread should join")
        .expect("withdrawal should stop only after publication completes");

    assert_eq!(
        phase_during_publication,
        nimbus_network::PortLeasePhase::Active,
        "publication and withdrawal must share one lifecycle lock"
    );
}
