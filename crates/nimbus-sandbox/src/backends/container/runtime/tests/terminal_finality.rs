//! Terminal workload projection must consume canonical network authority.

use super::*;

#[test]
fn terminal_manifest_publication_rejects_each_local_authority_until_released() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.start_mode = ContainerStartMode::PlanOnly;
    let backend = ContainerSandboxBackend::new(config);
    let mut terminal = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("terminal-local-authority-matrix"),
            None,
            None,
        )
        .expect("plan-only manifest should lower without network authority")
        .manifest;
    backend
        .write_manifest(&terminal)
        .expect("nonterminal checkpoint should persist");
    let checkpoint = std::fs::read(&terminal.conmon_layout.manifest_path)
        .expect("nonterminal checkpoint bytes should read");

    terminal.shutdown_requested = true;
    terminal.last_exit_code = Some(0);
    terminal.next_restart_at_millis = None;
    terminal.network_cleanup_complete = true;
    terminal.launch_reservation_claim = None;
    terminal.launch_artifact = None;
    synchronize_handle_status(&mut terminal, SandboxStatus::Stopped);
    assert!(terminal.has_terminal_network_finality());

    let mut retained_cleanup = terminal.clone();
    retained_cleanup.network_cleanup_complete = false;
    let mut retained_claim = terminal.clone();
    retained_claim.launch_reservation_claim = Some(
        crate::backends::oci::port_lease::new_launch_reservation_claim()
            .expect("retained claim should validate"),
    );
    let mut retained_artifact = terminal.clone();
    retained_artifact.launch_artifact = Some(sample_rootfs_artifact(
        temp_dir.path().join("retained-rootfs"),
    ));
    let mut retained_restart = terminal.clone();
    retained_restart.next_restart_at_millis = Some(1);
    let mut retained_shutdown = terminal.clone();
    retained_shutdown.shutdown_requested = false;
    let mut mismatched_projection = terminal.clone();
    mismatched_projection.handle.status = SandboxStatus::Stopping;
    let cases = [
        ("network_cleanup_complete=false", retained_cleanup),
        ("launch_reservation_claim_present=true", retained_claim),
        ("launch_artifact_present=true", retained_artifact),
        ("next_restart_at_millis=Some(1)", retained_restart),
        ("shutdown_requested=false", retained_shutdown),
        ("handle_status=Stopping", mismatched_projection),
    ];

    for (expected, retained) in cases {
        let error = backend
            .write_existing_workload_manifest(&retained)
            .expect_err("retained local authority must veto terminal publication");
        assert!(
            error.to_string().contains(expected),
            "diagnostic must name retained local authority {expected}: {error}"
        );
        assert_eq!(
            std::fs::read(&retained.conmon_layout.manifest_path)
                .expect("rejected publication must preserve prior bytes"),
            checkpoint,
            "terminal projection must remain nonterminal while {expected}"
        );
    }

    backend
        .write_existing_workload_manifest(&terminal)
        .expect("fully released local authority should publish terminal status");
    assert_eq!(
        backend
            .read_manifest(&terminal.handle.id)
            .expect("terminal manifest should inspect")
            .expect("terminal manifest should remain durable"),
        terminal
    );
}

#[test]
fn terminal_manifest_publication_rejects_a_retained_port_lease() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(temp_dir.path()));
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("terminal-retained-port-authority"),
            None,
            None,
        )
        .expect("execute planning should reserve exact launch authority")
        .manifest;
    backend
        .write_manifest(&manifest)
        .expect("nonterminal launch manifest should persist");
    let nonterminal_bytes = std::fs::read(&manifest.conmon_layout.manifest_path)
        .expect("nonterminal manifest bytes should read");
    let claim = manifest
        .launch_reservation_claim
        .as_ref()
        .expect("planned launch should retain its coordinator claim")
        .clone();
    let network_config = manifest
        .network_config
        .as_ref()
        .expect("planned launch should retain its attachment config");

    // Model a faulty upper coordinator claiming that port compensation
    // succeeded while the canonical port authority still says Reserved. The
    // lower attachment/IPAM owner accepts only the explicit result supplied by
    // its caller, so terminal publication must independently reject the
    // surviving port authority.
    crate::backends::oci::network::release_reserved_network_launch_after_ports(
        backend.segment_allocator.as_ref(),
        &manifest.network_layout,
        &manifest.spec.tenant_id,
        &manifest.handle.id,
        &network_config.reservation_claim,
        Ok(()),
    )
    .expect("fixture should release attachment authority while retaining ports");
    let port_authority =
        nimbus_network::LocalPortLeaseAuthority::open(&backend.config.network_state_root)
            .expect("port authority should reopen");
    for request in &manifest.port_leases {
        assert_eq!(
            port_authority
                .inspect(request.lease_id())
                .expect("lease should inspect")
                .expect("retained lease should remain durable")
                .phase(),
            nimbus_network::PortLeasePhase::Reserved
        );
    }

    manifest.shutdown_requested = true;
    manifest.last_exit_code = Some(0);
    manifest.next_restart_at_millis = None;
    manifest.network_cleanup_complete = true;
    manifest.launch_reservation_claim = None;
    manifest.launch_artifact = None;
    synchronize_handle_status(&mut manifest, SandboxStatus::Stopped);
    assert!(
        manifest.has_terminal_network_finality(),
        "the legacy manifest-only predicate demonstrates the acceptance gap"
    );

    let error = backend
        .write_existing_workload_manifest(&manifest)
        .expect_err("retained canonical port authority must veto terminal publication");
    assert!(
        error.to_string().contains("port lease")
            && error.to_string().contains("Reserved")
            && error
                .to_string()
                .contains(claim.coordinator_attempt().provider_id().as_str()),
        "the rejection must identify the retained authority and exact coordinator: {error}"
    );
    assert_eq!(
        std::fs::read(&manifest.conmon_layout.manifest_path)
            .expect("rejected terminal manifest must leave prior bytes readable"),
        nonterminal_bytes,
        "terminal projection must remain nonterminal until the exact lease is released"
    );
}
