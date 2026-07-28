//! Restart fencing while exact runtime absence remains unproven.

use super::*;

#[test]
fn restart_cleanup_retains_network_and_listeners_until_runtime_absence_is_observed() {
    assert_restart_cleanup_is_fenced(
        "krun-restart-runtime-present",
        CommandSpec::new("/bin/sh").args([
            "-c",
            "printf '%s\\n' '{\"id\":\"krun-restart-runtime-present\",\"status\":\"running\"}'",
        ]),
        "remains \"running\"",
    );
}

#[test]
fn restart_cleanup_retains_network_and_listeners_when_runtime_absence_is_ambiguous() {
    assert_restart_cleanup_is_fenced(
        "krun-restart-runtime-ambiguous",
        CommandSpec::new("/bin/sh")
            .args(["-c", "printf '%s\\n' 'temporarily unavailable' >&2; exit 1"]),
        "cannot confirm krun runtime",
    );
}

fn assert_restart_cleanup_is_fenced(
    sandbox_name: &str,
    state_command: CommandSpec,
    expected_error: &str,
) {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let published_port = unused_loopback_port();
    let mut pep_port = unused_loopback_port();
    while pep_port == published_port {
        pep_port = unused_loopback_port();
    }
    let mut config = KrunSandboxBackendConfig::under_root(temp_dir.path().to_path_buf());
    config.netavark_path = PathBuf::from("/usr/bin/true");
    config.node_network_supernet = "127.0.0.0/24".to_owned();
    config.published_port_range = pep_port..=pep_port;
    let backend = KrunSandboxBackend::new(config);
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec_for_tenant(sandbox_name, "api")
                .with_port_binding(SandboxPortBinding::tcp("http", published_port, 8080)),
            &SandboxId::new(sandbox_name),
            None,
            None,
        )
        .expect("execute manifest should reserve network and listener authority")
        .manifest;
    let launch_claim = adopt_launch_network(&backend, &mut manifest);
    crate::backends::oci::egress::ensure_egress_proxy_running_with_release_authority(
        &backend.egress_proxies,
        &manifest.spec.tenant_id,
        &manifest.handle.id,
        manifest.egress_proxy.as_ref(),
        &manifest.spec.egress,
        crate::backends::oci::egress::PepPreAdoptionReleaseAuthority::FreshLaunch(&launch_claim),
    )
    .expect("fixture PEP should own its exact listener");
    manifest.launch_authority = KrunLaunchAuthority::ProviderOwned;
    manifest.creator_handoff = KrunCreatorHandoffState::RuntimeObserved {
        receipt: crate::backends::conmon::creator::CreatorAttemptReceipt::for_test(
            "runtime-observed-fixture",
        ),
    };
    let port_lease_coordinator = backend.port_lease_coordinator();
    let netavark_claims = port_lease_coordinator
        .claim_netavark_bindings(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("fixture should claim the published listener");
    crate::backends::oci::network::setup_container_network(
        &manifest.network_layout,
        manifest
            .require_network_config()
            .expect("manifest should carry exact network config"),
        &manifest.handle.id,
        manifest.spec.display_name(),
        &crate::backends::krun::vm::start::hostname_for(&manifest.spec),
        &manifest.spec.port_bindings,
        None,
    )
    .expect("fixture should publish Ready Netavark authority");
    fs::create_dir_all(
        manifest
            .network_layout
            .netns_path
            .parent()
            .expect("netns path should have a parent"),
    )
    .expect("netns parent should exist");
    fs::write(&manifest.network_layout.netns_path, b"owned krun netns\n")
        .expect("fixture should retain an owned netns retry handle");
    port_lease_coordinator
        .activate_netavark_bindings(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
            &netavark_claims,
        )
        .expect("fixture should activate the published listener");
    manifest.conmon_launch.delete_command = CommandSpec::new("/usr/bin/true");
    manifest.conmon_launch.state_command = state_command;
    let receipts = [
        (&manifest.conmon_layout.exit_status_file, b"42\n".as_slice()),
        (&manifest.conmon_layout.pidfile, b"424242\n".as_slice()),
        (
            &manifest.conmon_layout.conmon_pidfile,
            b"434343\n".as_slice(),
        ),
    ];
    for (path, contents) in receipts {
        fs::write(path, contents).expect("restart receipt should persist");
    }
    let authority_path =
        nimbus_network::LocalNetworkStateStore::authority_path_for(&backend.config.state_root);
    let authority_before =
        fs::read(&authority_path).expect("active network authority should be durable");
    let lease_records_before = manifest
        .port_leases
        .iter()
        .chain(
            manifest
                .egress_proxy
                .as_ref()
                .map(|assignment| &assignment.port_lease),
        )
        .map(|request| {
            nimbus_network::LocalPortLeaseAuthority::open(&backend.config.state_root)
                .expect("port authority should reopen")
                .inspect(request.lease_id())
                .expect("lease should inspect")
                .expect("active lease should remain durable")
        })
        .collect::<Vec<_>>();
    assert!(
        lease_records_before
            .iter()
            .all(|record| record.phase() == PortLeasePhase::Active),
        "fixture must begin with active PEP and published-listener authority"
    );
    let readiness_before = backend
        .egress_proxies
        .readiness(&manifest.spec.tenant_id, &manifest.handle.id)
        .expect("fixture PEP readiness should inspect")
        .expect("fixture PEP should remain registered");
    let netavark_before =
        fs::read(&manifest.network_layout.status_path).expect("Netavark status should persist");
    let netns_before =
        fs::read(&manifest.network_layout.netns_path).expect("owned netns should persist");

    let error = backend
        .reset_runtime_for_restart(&manifest)
        .expect_err("unproven runtime absence must fence every network restart effect");
    assert!(
        error.to_string().contains(expected_error),
        "runtime-absence evidence must remain the primary diagnostic: {error}"
    );
    assert!(
        fs::read(&authority_path).expect("fenced authority should remain readable")
            == authority_before,
        "unproven runtime absence must leave all durable network authority byte-identical"
    );
    assert_eq!(
        backend
            .egress_proxies
            .readiness(&manifest.spec.tenant_id, &manifest.handle.id)
            .expect("fenced PEP readiness should inspect")
            .expect("fenced PEP must remain registered"),
        readiness_before,
        "unproven runtime absence must not stop or replace the exact PEP"
    );
    assert_eq!(
        fs::read(&manifest.network_layout.status_path)
            .expect("fenced Netavark status should remain"),
        netavark_before
    );
    assert_eq!(
        fs::read(&manifest.network_layout.netns_path).expect("fenced netns should remain"),
        netns_before
    );
    let authority = nimbus_network::LocalPortLeaseAuthority::open(&backend.config.state_root)
        .expect("port authority should reopen");
    for (request, expected) in manifest
        .port_leases
        .iter()
        .chain(
            manifest
                .egress_proxy
                .as_ref()
                .map(|assignment| &assignment.port_lease),
        )
        .zip(lease_records_before)
    {
        assert_eq!(
            authority
                .inspect(request.lease_id())
                .expect("lease should inspect")
                .expect("fenced lease should remain durable"),
            expected,
            "unproven absence must preserve exact PEP/listener authority"
        );
    }
    for (path, contents) in receipts {
        assert_eq!(
            fs::read(path).expect("fenced restart receipt should remain"),
            contents,
            "unproven absence must retain {} byte-for-byte",
            path.display()
        );
    }
}

fn unused_loopback_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("ephemeral test listener should bind")
        .local_addr()
        .expect("ephemeral listener should have an address")
        .port()
}
