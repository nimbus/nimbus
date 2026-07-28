//! Netavark published-listener restart transition proofs.

use super::*;

#[test]
fn confirmed_netavark_restart_detach_prepares_published_leases_for_rebind() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(temp_dir.path()));
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp(
                "http",
                unused_loopback_port(),
                8080,
            )),
            &SandboxId::new("netavark-restart-rebind"),
            None,
            None,
        )
        .expect("execute manifest should reserve the published listener")
        .manifest;
    manifest.conmon_launch.delete_command = CommandSpec::new("/usr/bin/true");
    manifest.conmon_launch.state_command =
        explicitly_absent_runtime_state_command(&manifest.handle.id);
    manifest.egress_proxy = None;

    let port_manager = backend
        .port_manager_for_manifest(&manifest)
        .expect("manifest provider context should authenticate");
    let initial_lifetimes = port_manager
        .claim_netavark_bindings_with_lifetimes(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("initial Netavark bind claims should persist");
    port_manager
        .activate_netavark_bindings_with_lifetimes(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
            &initial_lifetimes,
        )
        .expect("initial Netavark bindings should activate");
    backend
        .netavark_port_lifetimes
        .insert(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            initial_lifetimes,
        )
        .map_err(|(error, _batch)| error)
        .expect("fixture should retain its exact live Netavark lifetimes");
    let authority = nimbus_network::LocalPortLeaseAuthority::open(&backend.config.state_root)
        .expect("port authority should reopen");
    assert_eq!(
        authority
            .inspect(manifest.port_leases[0].lease_id())
            .expect("active lease should inspect")
            .expect("active lease should remain durable")
            .phase(),
        nimbus_network::PortLeasePhase::Active,
        "precondition: the initial Netavark provider owns the published listener"
    );
    std::fs::write(&manifest.conmon_layout.exit_status_file, b"42\n")
        .expect("restart exit receipt should persist");

    backend
        .reset_runtime_for_restart(&manifest)
        .expect("confirmed provider detach should prepare the published leases for restart");

    let retained = authority
        .inspect(manifest.port_leases[0].lease_id())
        .expect("retained lease should inspect")
        .expect("retained lease should remain durable");
    assert_eq!(
        retained.phase(),
        nimbus_network::PortLeasePhase::Reserved,
        "confirmed Netavark detach must replace Active provider ownership with rebind authority"
    );
    assert!(
        retained.confirmed_stopped_binding().is_some(),
        "restart retention must preserve the exact confirmed-stopped binding"
    );
    assert!(
        !manifest
            .conmon_layout
            .exit_status_file
            .try_exists()
            .expect("restart receipt metadata should remain readable"),
        "the restart receipt may be consumed only after confirmed detach and durable rebind preparation"
    );

    let restart_lifetimes = port_manager
        .claim_netavark_bindings_with_lifetimes(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("the same published listener should be claimable for restart");
    assert_eq!(
        restart_lifetimes.claims().len(),
        manifest.port_leases.len(),
        "restart must claim every retained listener exactly once"
    );
}

#[test]
fn already_absent_runtime_is_a_successful_restart_delete_replay() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(temp_dir.path()));
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("restart-delete-replay"),
            None,
            None,
        )
        .expect("execute manifest should reserve its network launch")
        .manifest;
    manifest.egress_proxy = None;
    manifest.conmon_launch.delete_command = CommandSpec::new("/usr/bin/false");
    manifest.conmon_launch.state_command =
        explicitly_absent_runtime_state_command(&manifest.handle.id);
    std::fs::write(&manifest.conmon_layout.exit_status_file, b"42\n")
        .expect("restart receipt should persist");

    backend
        .reset_runtime_for_restart(&manifest)
        .expect("explicit runtime absence must acknowledge an idempotent restart delete");
}

#[test]
fn restart_cleanup_retains_network_and_listeners_until_runtime_absence_is_observed() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let published_port = unused_loopback_port();
    let mut pep_port = unused_loopback_port();
    while pep_port == published_port {
        pep_port = unused_loopback_port();
    }
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.netavark_path = PathBuf::from("/usr/bin/true");
    config.node_network_supernet = "127.0.0.0/24".to_owned();
    config.published_port_range = pep_port..=pep_port;
    let backend = ContainerSandboxBackend::new(config);
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("http", published_port, 8080)),
            &SandboxId::new("restart-runtime-absence-fence"),
            None,
            None,
        )
        .expect("execute manifest should reserve network and listener authority")
        .manifest;
    let launch_claim = manifest
        .launch_reservation_claim
        .clone()
        .expect("initial launch should retain its exact reservation claim");
    backend
        .segment_allocator
        .adopt_reserved_attachment(
            &manifest.spec.tenant_id,
            &default_network_attachment_id(&manifest.handle.id),
            &launch_claim,
        )
        .expect("fixture should adopt the exact segment hold");
    backend
        .ensure_egress_proxy_running_with_release_authority(
            &manifest,
            PepPreAdoptionReleaseAuthority::FreshLaunch(&launch_claim),
        )
        .expect("fixture PEP should own its exact listener");
    let port_manager = backend
        .port_manager_for_manifest(&manifest)
        .expect("manifest provider context should authenticate");
    let netavark_lifetimes = port_manager
        .claim_netavark_bindings_with_lifetimes(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("fixture should claim the published listener");
    setup_container_network(
        &manifest.network_layout,
        manifest
            .require_network_config()
            .expect("manifest should carry its exact network config"),
        &manifest.handle.id,
        manifest.spec.display_name(),
        &hostname_for(&manifest.spec),
        &manifest.spec.port_bindings,
        None,
    )
    .expect("fixture should publish Ready Netavark authority");
    port_manager
        .activate_netavark_bindings_with_lifetimes(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
            &netavark_lifetimes,
        )
        .expect("fixture should activate the published listener");
    backend
        .netavark_port_lifetimes
        .insert(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            netavark_lifetimes,
        )
        .map_err(|(error, _batch)| error)
        .expect("fixture should retain its exact live Netavark lifetimes");
    manifest.launch_reservation_claim = None;
    manifest.conmon_launch.delete_command = CommandSpec::new("/usr/bin/true");
    manifest.conmon_launch.state_command = CommandSpec::new("/bin/sh").args([
        "-c".to_owned(),
        "printf '%s\\n' '{\"id\":\"restart-runtime-absence-fence\",\"status\":\"running\"}'"
            .to_owned(),
    ]);
    let receipts = [
        (&manifest.conmon_layout.exit_status_file, b"42\n".as_slice()),
        (&manifest.conmon_layout.pidfile, b"424242\n".as_slice()),
        (
            &manifest.conmon_layout.conmon_pidfile,
            b"434343\n".as_slice(),
        ),
    ];
    for (path, contents) in receipts {
        std::fs::write(path, contents).expect("restart receipt should persist");
    }
    let authority_path =
        nimbus_network::LocalNetworkStateStore::authority_path_for(&backend.config.state_root);
    let authority_before =
        std::fs::read(&authority_path).expect("active network authority should be durable");
    let readiness_before = backend
        .egress_proxies
        .readiness(&manifest.spec.tenant_id, &manifest.handle.id)
        .expect("fixture PEP readiness should inspect")
        .expect("fixture PEP should remain registered");
    assert!(
        manifest_port_lease_records(&backend.config.state_root, &manifest)
            .iter()
            .all(|record| record.phase() == nimbus_network::PortLeasePhase::Active),
        "fixture must begin with active PEP and published-listener authority"
    );

    let error = backend
        .reset_runtime_for_restart(&manifest)
        .expect_err("a still-running runtime must fence every network restart effect");
    assert!(
        error.to_string().contains("remains \"running\""),
        "runtime-presence evidence must remain the primary diagnostic: {error}"
    );
    assert!(
        std::fs::read(&authority_path).expect("fenced authority should remain readable")
            == authority_before,
        "unconfirmed runtime absence must leave every durable network authority byte unchanged"
    );
    assert_eq!(
        backend
            .egress_proxies
            .readiness(&manifest.spec.tenant_id, &manifest.handle.id)
            .expect("fenced PEP readiness should inspect")
            .expect("fenced PEP must remain registered"),
        readiness_before,
        "unconfirmed runtime absence must not stop or replace the exact PEP"
    );
    assert!(
        manifest
            .network_layout
            .status_path
            .try_exists()
            .expect("Netavark status metadata should remain readable"),
        "unconfirmed runtime absence must retain the observed Netavark projection"
    );
    for (path, contents) in receipts {
        assert_eq!(
            std::fs::read(path).expect("fenced restart receipt should remain"),
            contents,
            "unconfirmed runtime absence must retain {} byte-for-byte",
            path.display()
        );
    }
}

fn explicitly_absent_runtime_state_command(runtime_id: &SandboxId) -> CommandSpec {
    CommandSpec::new("/bin/sh").args([
        "-c".to_owned(),
        format!(
            "printf '%s\\n' 'container `{0}` does not exist: open \
             `/run/crun/{0}/status`: No such file or directory' >&2; exit 1",
            runtime_id
        ),
    ])
}
