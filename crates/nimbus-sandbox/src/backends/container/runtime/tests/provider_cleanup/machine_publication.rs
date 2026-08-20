//! Machine-forwarded partial-start, withdrawal, and restart cleanup proofs.

use super::assertions::{
    assert_machine_unexpose_request, assert_manifest_port_leases_released,
    manifest_port_lease_records,
};
use super::forwarder_observer::ForwarderObserver;
use super::*;

#[test]
fn machine_never_bound_final_cleanup_releases_without_publication_authority() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    // Offset 0 is the forwarder endpoint and offset 1 the never-bound
    // published port. Both stay exclusive for the whole proof, so the absence
    // this test asserts stays observable.
    let port_window = PortWindow::claim();
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.node_network_supernet = "127.0.0.0/24".to_owned();
    config.machine_port_forwarder = Some(sample_forwarder(port_window.port(0)));
    let backend = ContainerSandboxBackend::new(config);
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp(
                "never-bound",
                port_window.port(1),
                5432,
            )),
            &SandboxId::new("machine-never-bound-final-cleanup"),
            None,
            None,
        )
        .expect("machine launch should plan")
        .manifest;
    let claim = manifest
        .launch_reservation_claim
        .clone()
        .expect("planned launch must retain exact compensation authority");
    backend
        .segment_allocator
        .adopt_reserved_attachment(
            &manifest.spec.tenant_id,
            &default_network_attachment_id(&manifest.handle.id),
            &claim,
        )
        .expect("fixture must cross the attachment provider boundary");
    mark_runtime_absent_for_cleanup(&mut manifest);

    backend
        .release_execution_artifacts(&mut manifest)
        .expect("exact never-bound listener evidence must permit compensation");

    assert_manifest_port_leases_released(&backend.config.network_state_root, &manifest);
    assert!(
        manifest.launch_reservation_claim.is_none(),
        "successful compensation must retire the exact launch coordinator"
    );
    assert!(
        manifest.network_cleanup_complete,
        "successful compensation must publish network cleanup finality"
    );
    let attachment = nimbus_network::LocalNetworkAttachmentAuthority::open(
        &manifest.runner_config.network_state_root,
    )
    .expect("attachment authority should reopen")
    .get(
        &manifest.spec.tenant_id,
        &default_network_attachment_id(&manifest.handle.id),
    )
    .expect("never-bound attachment should inspect")
    .expect("cleanup should retain its durable terminal attachment record");
    assert_eq!(
        attachment.resource().phase(),
        nimbus_network::NetworkResourcePhase::Released,
        "never-bound final cleanup must release the shared attachment generation"
    );
    assert!(
        !manifest
            .conmon_layout
            .container_state_dir
            .join(".nimbus-machine-port-evidence.json")
            .exists(),
        "a provider effect that never existed must not fabricate publication authority"
    );
}

#[test]
fn fresh_machine_partial_start_shutdown_diagnostic_replays_terminal_release() {
    // Offset 0 is the published binding and offset 1 the one-port PEP range.
    // Distinct offsets replace the retry loop that used to reject a duplicate
    // draw, and the claim covers both for the whole proof.
    let port_window = PortWindow::claim();
    let published_port = port_window.port(0);
    let forwarder_listener =
        TcpListener::bind("127.0.0.1:0").expect("forwarder listener should bind");
    let forwarder_port = forwarder_listener
        .local_addr()
        .expect("forwarder address should resolve")
        .port();
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.node_network_supernet = "127.0.0.0/24".to_owned();
    let pep_port = port_window.port(1);
    config.published_port_range = pep_port..=pep_port;
    config.machine_port_forwarder = Some(sample_forwarder(forwarder_port));
    let backend = ContainerSandboxBackend::new(config);
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp(
                "partial-fresh",
                published_port,
                5432,
            )),
            &SandboxId::new("partial-fresh-machine-cleanup"),
            None,
            None,
        )
        .expect("fresh machine launch should plan")
        .manifest;
    let claim = manifest
        .launch_reservation_claim
        .clone()
        .expect("fresh launch must retain exact compensation authority");
    backend
        .segment_allocator
        .adopt_reserved_attachment(
            &manifest.spec.tenant_id,
            &default_network_attachment_id(&manifest.handle.id),
            &claim,
        )
        .expect("provider-effect fixture must adopt its exact segment hold");
    backend
        .ensure_egress_proxy_running_with_release_authority(
            &manifest,
            PepPreAdoptionReleaseAuthority::FreshLaunch(&claim),
        )
        .expect("provider-effect fixture must start its exact PEP");
    backend
        .ensure_machine_port_proxies_running(&manifest.handle.id, &[Ipv4Addr::LOCALHOST], &manifest)
        .expect("fixture must first establish an exact active machine listener");

    let first = backend
        .inject_partial_machine_proxy_start_shutdown_diagnostic(
            &manifest,
            MachinePortPreparationReleaseAuthority::FreshLaunch(&claim),
        )
        .expect_err("the injected first provider-stop acknowledgement must fail");
    assert!(
        first.to_string().contains("panicked"),
        "the exact provider-stop diagnostic must remain visible: {first}"
    );
    let authority =
        nimbus_network::LocalPortLeaseAuthority::open(&backend.config.network_state_root)
            .expect("port authority should reopen");
    assert_eq!(
        authority
            .inspect(manifest.port_leases[0].lease_id())
            .expect("lease should inspect")
            .expect("lease must remain durable")
            .phase(),
        nimbus_network::PortLeasePhase::Withdrawing,
        "fresh-launch compensation must withdraw before the retryable provider stop"
    );

    mark_runtime_absent_for_cleanup(&mut manifest);
    let absence_observer = ForwarderObserver::spawn_authenticated(
        forwarder_listener,
        manifest
            .runner_config
            .machine_port_forwarder
            .as_ref()
            .expect("manifest should retain forwarder authority"),
        &[],
        Vec::new(),
        0,
    );
    backend
        .release_execution_artifacts(&mut manifest)
        .expect("outer terminal compensation must resume the same Release tombstone");
    assert!(
        absence_observer.finish_exact().is_empty(),
        "exact provider absence must converge without a blind unexpose mutation"
    );
    assert_manifest_port_leases_released(&backend.config.network_state_root, &manifest);
    backend
        .release_execution_artifacts(&mut manifest)
        .expect("terminal compensation replay must remain idempotent");
}

#[test]
fn retained_machine_partial_start_shutdown_diagnostic_replays_restart() {
    // Offset 0 is the published binding and offset 1 the forwarder endpoint.
    let port_window = PortWindow::claim();
    let published_port = port_window.port(0);
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.node_network_supernet = "127.0.0.0/24".to_owned();
    config.machine_port_forwarder = Some(sample_forwarder(port_window.port(1)));
    let backend = ContainerSandboxBackend::new(config);
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp(
                "partial-restart",
                published_port,
                5432,
            )),
            &SandboxId::new("partial-restart-machine-cleanup"),
            None,
            None,
        )
        .expect("restart machine launch should plan")
        .manifest;
    backend
        .ensure_machine_port_proxies_running(&manifest.handle.id, &[Ipv4Addr::LOCALHOST], &manifest)
        .expect("fixture must first establish an exact active machine listener");
    manifest.launch_reservation_claim = None;

    let first = backend
        .inject_partial_machine_proxy_start_shutdown_diagnostic(
            &manifest,
            MachinePortPreparationReleaseAuthority::Retain,
        )
        .expect_err("the injected first provider-stop acknowledgement must fail");
    assert!(
        first.to_string().contains("panicked"),
        "the exact provider-stop diagnostic must remain visible: {first}"
    );
    let authority =
        nimbus_network::LocalPortLeaseAuthority::open(&backend.config.network_state_root)
            .expect("port authority should reopen");
    assert_eq!(
        authority
            .inspect(manifest.port_leases[0].lease_id())
            .expect("lease should inspect")
            .expect("lease must remain durable")
            .phase(),
        nimbus_network::PortLeasePhase::Active,
        "restart retention must not convert an active listener into terminal release"
    );

    backend
        .stop_machine_port_proxies(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("same Restart tombstone must converge on retry");
    let retained = authority
        .inspect(manifest.port_leases[0].lease_id())
        .expect("retained lease should inspect")
        .expect("retained lease must remain durable");
    assert_eq!(retained.phase(), nimbus_network::PortLeasePhase::Reserved);
    assert!(
        retained.confirmed_stopped_binding().is_some(),
        "restart replay must retain exact confirmed-stop evidence for rebind"
    );
    backend
        .stop_machine_port_proxies(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("confirmed restart retention must replay idempotently");
}

#[test]
fn failed_restart_teardown_retains_runtime_receipts_for_retry() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let backend =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(temp_dir.path()));
    let mut manifest = backend
        .plan_start_with_id(&sample_spec(), &sandbox_id(), None, None)
        .expect("execute manifest should plan")
        .manifest;
    let claim = manifest
        .launch_reservation_claim
        .as_ref()
        .expect("planned restart fixture should retain its exact claim");
    backend
        .segment_allocator
        .adopt_reserved_attachment(
            &manifest.spec.tenant_id,
            &default_network_attachment_id(&manifest.handle.id),
            claim,
        )
        .expect("restart fixture should adopt its exact attachment association");
    manifest.conmon_launch.delete_command = CommandSpec::new("/usr/bin/false");
    let receipts = [
        (&manifest.conmon_layout.exit_status_file, b"42\n".as_slice()),
        (&manifest.conmon_layout.pidfile, b"424242\n".as_slice()),
        (
            &manifest.conmon_layout.conmon_pidfile,
            b"434343\n".as_slice(),
        ),
    ];
    for (path, contents) in receipts {
        std::fs::write(path, contents).expect("runtime receipt should persist");
    }

    let error = backend
        .reset_runtime_for_restart(&manifest)
        .expect_err("failed provider teardown must abort restart reset");
    assert!(
        error.to_string().contains("/usr/bin/false"),
        "restart reset must report the provider failure: {error}"
    );
    for (path, contents) in receipts {
        assert_eq!(
            std::fs::read(path).expect("failed restart must retain the exact receipt"),
            contents,
            "failed restart teardown must not consume {}",
            path.display()
        );
    }
}

#[test]
fn terminal_cleanup_uses_manifest_machine_forwarder_after_backend_config_drift() {
    // Offset 0 is the published binding and offset 1 the one-port PEP range.
    // Distinct offsets replace the retry loop that used to reject a duplicate
    // draw. The two forwarder endpoints below stay ephemeral because their
    // listeners live for the whole test.
    let port_window = PortWindow::claim();
    let published_port = port_window.port(0);
    let launch_listener = TcpListener::bind("127.0.0.1:0").expect("launch forwarder should bind");
    let launch_forwarder_port = launch_listener
        .local_addr()
        .expect("launch forwarder address should resolve")
        .port();
    let drift_listener = TcpListener::bind("127.0.0.1:0").expect("drift forwarder should bind");
    let drift_forwarder_port = drift_listener
        .local_addr()
        .expect("drift forwarder address should resolve")
        .port();

    let temp_dir = TempDir::new().expect("tempdir should build");
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path())
        .with_network_state_root(temp_dir.path().join("node-network-state"));
    config.node_network_supernet = "127.0.0.0/24".to_owned();
    let egress_proxy_port = port_window.port(1);
    config.published_port_range = egress_proxy_port..=egress_proxy_port;
    config.machine_port_forwarder = Some(sample_forwarder(launch_forwarder_port));
    let mut backend = ContainerSandboxBackend::new(config);
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("db", published_port, 5432)),
            &SandboxId::new("manifest-forwarder-config-drift"),
            None,
            None,
        )
        .expect("plan should lower")
        .manifest;
    mark_runtime_absent_for_cleanup(&mut manifest);
    backend
        .segment_allocator
        .adopt_reserved_attachment(
            &manifest.spec.tenant_id,
            &default_network_attachment_id(&manifest.handle.id),
            manifest
                .launch_reservation_claim
                .as_ref()
                .expect("execute plan should retain launch claim"),
        )
        .expect("provider-effect fixture must adopt the segment hold");
    backend
        .ensure_egress_proxy_running_with_release_authority(
            &manifest,
            PepPreAdoptionReleaseAuthority::FreshLaunch(
                manifest
                    .launch_reservation_claim
                    .as_ref()
                    .expect("execute plan should retain launch claim"),
            ),
        )
        .expect("fixture PEP should own exact listener evidence");
    backend
        .ensure_machine_port_proxies_running(&manifest.handle.id, &[Ipv4Addr::LOCALHOST], &manifest)
        .expect("fixture should start machine provider under launch-time configuration");
    let bindings_before =
        manifest_port_lease_records(&manifest.runner_config.network_state_root, &manifest)
            .into_iter()
            .map(|record| record.binding().cloned())
            .collect::<Vec<_>>();
    assert!(
        bindings_before.iter().all(Option::is_some),
        "fixture must activate every published and PEP binding before teardown"
    );
    backend.config.machine_port_forwarder = Some(sample_forwarder(drift_forwarder_port));
    let launch_observer = ForwarderObserver::spawn_authenticated(
        launch_listener,
        manifest
            .runner_config
            .machine_port_forwarder
            .as_ref()
            .expect("launch-time forwarder should remain persisted"),
        &manifest.spec.port_bindings,
        vec![true],
        1,
    );
    let drift_observer = ForwarderObserver::spawn(drift_listener, Vec::new(), 0);

    backend
        .release_execution_artifacts(&mut manifest)
        .expect("teardown must use the persisted launch-time machine forwarder");
    let launch_requests = launch_observer.finish_exact();
    assert_machine_unexpose_request(
        &launch_requests[0],
        &manifest.spec.port_bindings[0],
        manifest
            .runner_config
            .machine_port_forwarder
            .as_ref()
            .expect("launch-time forwarder should remain persisted"),
    );
    let drift_requests = drift_observer.finish_exact();
    assert!(
        drift_requests.is_empty(),
        "backend configuration drift must not redirect teardown provider effects"
    );
    let released =
        assert_manifest_port_leases_released(&manifest.runner_config.network_state_root, &manifest);
    assert!(
        released.iter().all(|record| {
            record.reservation_claim().is_none() && record.bind_claim().is_none()
        }),
        "terminal provider cleanup must clear active coordinator and bind claims"
    );
    assert_eq!(
        released
            .iter()
            .map(|record| record.binding().cloned())
            .collect::<Vec<_>>(),
        bindings_before,
        "terminal records must retain the exact immutable provider binding as audit evidence"
    );
    assert!(
        manifest.network_cleanup_complete,
        "exact provider and authority cleanup must publish durable finality"
    );
    assert!(
        manifest.launch_reservation_claim.is_none(),
        "terminal cleanup must retire the exact launch coordinator"
    );
    let attachment = nimbus_network::LocalNetworkAttachmentAuthority::open(
        &manifest.runner_config.network_state_root,
    )
    .expect("attachment authority should reopen")
    .get(
        &manifest.spec.tenant_id,
        &default_network_attachment_id(&manifest.handle.id),
    )
    .expect("machine-forwarded attachment should inspect")
    .expect("machine-forwarded cleanup should retain its durable terminal record");
    assert_eq!(
        attachment.resource().phase(),
        nimbus_network::NetworkResourcePhase::Released,
        "machine-forwarded final cleanup must converge through the shared durable attachment lifecycle"
    );
    let network_config = manifest
        .require_network_config()
        .expect("terminal manifest should retain immutable network identity");
    assert_eq!(
        attachment.association().reservation_claim(),
        &network_config.reservation_claim
    );
    assert_eq!(
        attachment.association().segment_id().as_str(),
        network_config.segment_id
    );
}

#[test]
fn machine_forwarder_unexpose_failure_keeps_port_lease_fenced() {
    // Offset 0 is the published binding, offset 1 the one-port PEP range, and
    // offset 2 the forwarder endpoint. The forwarder port needs the claim
    // because the retry observer below re-binds it after the first observer
    // closes it, and only the claim keeps that window free of a foreign bind.
    let port_window = PortWindow::claim();
    let published_port = port_window.port(0);
    let port = port_window.port(2);
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).expect("listener should bind");

    let temp_dir = TempDir::new().expect("tempdir should build");
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.node_network_supernet = "127.0.0.0/24".to_owned();
    let egress_proxy_port = port_window.port(1);
    config.published_port_range = egress_proxy_port..=egress_proxy_port;
    let forwarder = sample_forwarder(port);
    config.machine_port_forwarder = Some(forwarder.clone());
    let backend = ContainerSandboxBackend::new(config);
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("db", published_port, 5432)),
            &sandbox_id(),
            None,
            None,
        )
        .expect("plan should lower")
        .manifest;
    mark_runtime_absent_for_cleanup(&mut manifest);
    backend
        .segment_allocator
        .adopt_reserved_attachment(
            &manifest.spec.tenant_id,
            &default_network_attachment_id(&manifest.handle.id),
            manifest
                .launch_reservation_claim
                .as_ref()
                .expect("execute plan should retain launch claim"),
        )
        .expect("provider-effect fixture must first adopt the segment hold");
    backend
        .ensure_egress_proxy_running_with_release_authority(
            &manifest,
            PepPreAdoptionReleaseAuthority::FreshLaunch(
                manifest
                    .launch_reservation_claim
                    .as_ref()
                    .expect("execute plan should retain launch claim"),
            ),
        )
        .expect("the test must retain exact PEP provider evidence through retryable teardown");
    backend
        .ensure_machine_port_proxies_running(&manifest.handle.id, &[Ipv4Addr::LOCALHOST], &manifest)
        .expect("the test must start an exact local provider before ambiguous unexpose");
    let failed_observer = ForwarderObserver::spawn_authenticated(
        listener,
        &forwarder,
        &manifest.spec.port_bindings,
        vec![false],
        1,
    );

    let error = backend
        .release_execution_artifacts(&mut manifest)
        .expect_err("failed provider unexpose must prevent lease release");
    let failed_requests = failed_observer.finish_exact();
    assert_machine_unexpose_request(
        &failed_requests[0],
        &manifest.spec.port_bindings[0],
        &forwarder,
    );
    assert!(
        error.to_string().contains("withdraw")
            && error.to_string().contains(&published_port.to_string()),
        "cleanup should report the provider failure: {error}"
    );
    let authority =
        nimbus_network::LocalPortLeaseAuthority::open(&backend.config.network_state_root)
            .expect("port authority should remain readable");
    let record = authority
        .inspect(manifest.port_leases[0].lease_id())
        .expect("lease inspection should succeed")
        .expect("lease must remain durable");
    assert_eq!(
        record.phase(),
        nimbus_network::PortLeasePhase::Withdrawing,
        "an ambiguous machine-provider unexpose must retain the host-port fence"
    );

    let retry_listener =
        TcpListener::bind((Ipv4Addr::LOCALHOST, port)).expect("retry listener should bind");
    let retry_observer = ForwarderObserver::spawn_authenticated(
        retry_listener,
        &forwarder,
        &manifest.spec.port_bindings,
        vec![true],
        1,
    );
    backend
        .release_execution_artifacts(&mut manifest)
        .expect("a later successful unexpose must resume the retained exact stop evidence");
    let retry_requests = retry_observer.finish_exact();
    assert_machine_unexpose_request(
        &retry_requests[0],
        &manifest.spec.port_bindings[0],
        manifest
            .runner_config
            .machine_port_forwarder
            .as_ref()
            .expect("machine forwarder should remain persisted"),
    );

    let released = authority
        .inspect(manifest.port_leases[0].lease_id())
        .expect("released lease inspection should succeed")
        .expect("released lease must remain durable");
    assert_eq!(
        released.phase(),
        nimbus_network::PortLeasePhase::Released,
        "successful retry must finish durable release"
    );
}

#[test]
fn machine_forwarder_unexpose_attempts_every_binding_and_retries_only_failures() {
    // Offsets 0 and 1 are the two published bindings, offset 2 the one-port PEP
    // range, and offset 3 the forwarder endpoint. The forwarder port needs the
    // claim because the retry observer below re-binds it after the first-pass
    // observer closes it.
    let port_window = PortWindow::claim();
    let first_port = port_window.port(0);
    let second_port = port_window.port(1);
    let forwarder_port = port_window.port(3);
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, forwarder_port))
        .expect("forwarder listener should bind");

    let temp_dir = TempDir::new().expect("tempdir should build");
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.node_network_supernet = "127.0.0.0/24".to_owned();
    let egress_proxy_port = port_window.port(2);
    config.published_port_range = egress_proxy_port..=egress_proxy_port;
    config.machine_port_forwarder = Some(sample_forwarder(forwarder_port));
    let backend = ContainerSandboxBackend::new(config);
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_bindings([
                SandboxPortBinding::tcp("first", first_port, 5432),
                SandboxPortBinding::tcp("second", second_port, 5433),
            ]),
            &SandboxId::new("machine-multi-withdraw"),
            None,
            None,
        )
        .expect("plan should lower")
        .manifest;
    mark_runtime_absent_for_cleanup(&mut manifest);
    backend
        .segment_allocator
        .adopt_reserved_attachment(
            &manifest.spec.tenant_id,
            &default_network_attachment_id(&manifest.handle.id),
            manifest
                .launch_reservation_claim
                .as_ref()
                .expect("execute plan should retain launch claim"),
        )
        .expect("provider-effect fixture must first adopt the segment hold");
    backend
        .ensure_egress_proxy_running_with_release_authority(
            &manifest,
            PepPreAdoptionReleaseAuthority::FreshLaunch(
                manifest
                    .launch_reservation_claim
                    .as_ref()
                    .expect("execute plan should retain launch claim"),
            ),
        )
        .expect("fixture PEP should own exact listener evidence");
    backend
        .ensure_machine_port_proxies_running(&manifest.handle.id, &[Ipv4Addr::LOCALHOST], &manifest)
        .expect("fixture should start both exact machine listeners");

    let first_pass_observer = ForwarderObserver::spawn_authenticated(
        listener,
        manifest
            .runner_config
            .machine_port_forwarder
            .as_ref()
            .expect("machine forwarder should remain persisted"),
        &manifest.spec.port_bindings,
        vec![false, true],
        2,
    );
    let error = backend
        .release_execution_artifacts(&mut manifest)
        .expect_err("one failed publication withdrawal must keep cleanup pending");
    let first_pass_requests = first_pass_observer.finish_exact();
    assert_eq!(
        first_pass_requests.len(),
        2,
        "one binding failure must not prevent withdrawal of later bindings"
    );
    let manifest_forwarder = manifest
        .runner_config
        .machine_port_forwarder
        .clone()
        .expect("machine forwarder should remain persisted");
    assert_machine_unexpose_request(
        &first_pass_requests[0],
        &manifest.spec.port_bindings[0],
        &manifest_forwarder,
    );
    assert_machine_unexpose_request(
        &first_pass_requests[1],
        &manifest.spec.port_bindings[1],
        &manifest_forwarder,
    );
    assert!(
        error.to_string().contains(&first_port.to_string())
            && !error.to_string().contains(&second_port.to_string()),
        "the aggregate diagnostic should identify only the failed binding: {error}"
    );

    let retry_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, forwarder_port))
        .expect("retry forwarder should bind");
    let retry_observer = ForwarderObserver::spawn_authenticated(
        retry_listener,
        manifest
            .runner_config
            .machine_port_forwarder
            .as_ref()
            .expect("machine forwarder should remain persisted"),
        std::slice::from_ref(&manifest.spec.port_bindings[0]),
        vec![true],
        1,
    );
    backend
        .release_execution_artifacts(&mut manifest)
        .expect("retry should withdraw only the still-pending binding and converge");
    let retry_requests = retry_observer.finish_exact();
    assert_eq!(
        retry_requests.len(),
        1,
        "successful first-pass withdrawals must not be replayed"
    );
    assert_machine_unexpose_request(
        &retry_requests[0],
        &manifest.spec.port_bindings[0],
        &manifest_forwarder,
    );
    let absent_receipts = backend
        .absent_machine_port_receipts(&manifest.handle.id)
        .expect("the converged retry must publish one complete absence batch");
    assert_eq!(
        absent_receipts
            .iter()
            .map(|receipt| receipt.binding.clone())
            .collect::<Vec<_>>(),
        manifest.spec.port_bindings,
        "retry convergence must preserve the original canonical binding order"
    );
    assert_manifest_port_leases_released(&manifest.runner_config.network_state_root, &manifest);
}

#[test]
fn restart_retained_machine_listener_releases_without_process_registry() {
    // Offset 0 is the published binding, offset 1 the one-port PEP range, and
    // offset 2 the forwarder endpoint.
    let port_window = PortWindow::claim();
    let published_port = port_window.port(0);
    let temp_dir = TempDir::new().expect("tempdir should build");
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.node_network_supernet = "127.0.0.0/24".to_owned();
    let egress_proxy_port = port_window.port(1);
    config.published_port_range = egress_proxy_port..=egress_proxy_port;
    config.machine_port_forwarder = Some(sample_forwarder(port_window.port(2)));
    let mut backend = ContainerSandboxBackend::new(config);
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("db", published_port, 5432)),
            &SandboxId::new("machine-restart-retained-terminal-release"),
            None,
            None,
        )
        .expect("plan should lower")
        .manifest;
    let launch_claim = manifest
        .launch_reservation_claim
        .clone()
        .expect("execute launch should retain exact reservation authority");
    backend
        .segment_allocator
        .adopt_reserved_attachment(
            &manifest.spec.tenant_id,
            &default_network_attachment_id(&manifest.handle.id),
            &launch_claim,
        )
        .expect("provider-effect fixture must adopt the segment hold");
    backend
        .ensure_egress_proxy_running_with_release_authority(
            &manifest,
            PepPreAdoptionReleaseAuthority::FreshLaunch(&launch_claim),
        )
        .expect("fixture PEP should own its exact listener");
    backend
        .ensure_machine_port_proxies_running(&manifest.handle.id, &[Ipv4Addr::LOCALHOST], &manifest)
        .expect("fixture should start the exact machine listener");
    manifest.launch_reservation_claim = None;

    backend
        .stop_machine_port_proxies(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("ordinary restart stop should retain exact durable receipts");
    let authority =
        nimbus_network::LocalPortLeaseAuthority::open(&backend.config.network_state_root)
            .expect("port authority should open");
    let retained = authority
        .inspect(manifest.port_leases[0].lease_id())
        .expect("retained lease should inspect")
        .expect("retained lease should remain");
    assert_eq!(retained.phase(), nimbus_network::PortLeasePhase::Reserved);
    assert!(
        retained.confirmed_stopped_binding().is_some(),
        "restart completion must retain exact provider-stop evidence"
    );

    backend.config.machine_port_forwarder = None;
    mark_runtime_absent_for_cleanup(&mut manifest);
    backend
        .release_execution_artifacts(&mut manifest)
        .expect("terminal cleanup should consume exact restart-stop evidence without a registry");
    let released = authority
        .inspect(manifest.port_leases[0].lease_id())
        .expect("released lease should inspect")
        .expect("released lease should remain");
    assert_eq!(released.phase(), nimbus_network::PortLeasePhase::Released);
    assert!(released.confirmed_stopped_binding().is_none());
}
