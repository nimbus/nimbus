//! Container provider-cleanup and durable authority-ordering proofs.

use super::*;

#[path = "provider_cleanup/forwarder_observer.rs"]
mod forwarder_observer;
#[path = "provider_cleanup/netavark_restart.rs"]
mod netavark_restart;
#[path = "provider_cleanup/startup_fencing.rs"]
mod startup_fencing;

use crate::backends::oci::network::OciMachinePortForwarderConfig;
use forwarder_observer::ForwarderObserver;

fn assert_machine_unexpose_request(
    request: &[u8],
    binding: &SandboxPortBinding,
    forwarder: &OciMachinePortForwarderConfig,
) {
    let header_end = request
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("forwarder request should contain complete headers");
    let headers = std::str::from_utf8(&request[..header_end])
        .expect("forwarder request headers should be UTF-8");
    assert_eq!(
        headers.lines().next(),
        Some("POST /services/forwarder/unexpose HTTP/1.0"),
        "cleanup must target the exact persisted forwarder path"
    );
    let body: serde_json::Value = serde_json::from_slice(&request[header_end + 4..])
        .expect("forwarder request body should be valid JSON");
    assert_eq!(
        body,
        serde_json::json!({
            "provider_instance": forwarder.provider_instance(),
            "provider_generation": forwarder.provider_generation(),
            "local": format!("{}:{}", binding.host_address, binding.host_port),
            "protocol": "tcp",
        }),
        "cleanup must withdraw the exact persisted publication"
    );
}

fn manifest_port_lease_records(
    state_root: &std::path::Path,
    manifest: &ContainerSandboxManifest,
) -> Vec<nimbus_network::PortLeaseRecord> {
    let authority = nimbus_network::LocalPortLeaseAuthority::open(state_root)
        .expect("manifest port authority should reopen");
    manifest
        .port_leases
        .iter()
        .chain(
            manifest
                .egress_proxy
                .as_ref()
                .map(|assignment| &assignment.port_lease),
        )
        .map(|request| {
            authority
                .inspect(request.lease_id())
                .expect("manifest lease should inspect")
                .expect("manifest lease record should remain durable")
        })
        .collect()
}

fn assert_manifest_port_leases_released(
    state_root: &std::path::Path,
    manifest: &ContainerSandboxManifest,
) -> Vec<nimbus_network::PortLeaseRecord> {
    let records = manifest_port_lease_records(state_root, manifest);
    for record in &records {
        assert_eq!(
            record.phase(),
            nimbus_network::PortLeasePhase::Released,
            "terminal cleanup must release exact lease {}",
            record.request().lease_id()
        );
        assert!(record.confirmed_stopped_binding().is_none());
    }
    records
}

#[test]
fn fresh_machine_partial_start_shutdown_diagnostic_replays_terminal_release() {
    let published_port = unused_loopback_port();
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.node_network_supernet = "127.0.0.0/24".to_owned();
    let mut pep_port = unused_loopback_port();
    while pep_port == published_port {
        pep_port = unused_loopback_port();
    }
    config.published_port_range = pep_port..=pep_port;
    config.machine_port_forwarder = Some(sample_forwarder(unused_loopback_port()));
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
    let authority = nimbus_network::LocalPortLeaseAuthority::open(&backend.config.state_root)
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
    backend
        .release_execution_artifacts(&mut manifest)
        .expect("outer terminal compensation must resume the same Release tombstone");
    assert_manifest_port_leases_released(&backend.config.state_root, &manifest);
    backend
        .release_execution_artifacts(&mut manifest)
        .expect("terminal compensation replay must remain idempotent");
}

#[test]
fn retained_machine_partial_start_shutdown_diagnostic_replays_restart() {
    let published_port = unused_loopback_port();
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.node_network_supernet = "127.0.0.0/24".to_owned();
    config.machine_port_forwarder = Some(sample_forwarder(unused_loopback_port()));
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
    let authority = nimbus_network::LocalPortLeaseAuthority::open(&backend.config.state_root)
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
fn unstarted_artifact_cleanup_failure_retains_claim_for_idempotent_retry() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let rootfs_path = temp_dir.path().join("transient-rootfs-obstacle");
    std::fs::write(&rootfs_path, b"not a directory").expect("rootfs cleanup obstacle should write");
    let backend =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(temp_dir.path()));
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("unstarted-artifact-cleanup-retry"),
            None,
            Some(sample_rootfs_artifact(rootfs_path.clone())),
        )
        .expect("execute manifest should reserve exact launch authority")
        .manifest;
    let claim = manifest
        .launch_reservation_claim
        .clone()
        .expect("unstarted launch must retain its exact coordinator claim");

    let error = backend
        .release_unstarted_launch_artifacts(&mut manifest)
        .expect_err("invalid rootfs shape must fail after exact network compensation");
    assert!(
        error
            .to_string()
            .contains("failed to remove materialized rootfs"),
        "cleanup must report the exact artifact failure: {error}"
    );
    assert_eq!(
        manifest.launch_reservation_claim.as_ref(),
        Some(&claim),
        "secondary artifact failure must retain the idempotent network-release claim"
    );
    assert!(!manifest.network_cleanup_complete);

    std::fs::remove_file(&rootfs_path).expect("cleanup obstacle should be removable");
    backend
        .release_unstarted_launch_artifacts(&mut manifest)
        .expect("same-claim replay should converge after the transient artifact failure");
    assert!(manifest.launch_reservation_claim.is_none());
    assert!(manifest.launch_artifact.is_none());
    assert!(manifest.network_cleanup_complete);
}

#[test]
fn terminal_cleanup_uses_manifest_machine_forwarder_after_backend_config_drift() {
    let published_port = unused_loopback_port();
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
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.node_network_supernet = "127.0.0.0/24".to_owned();
    let mut egress_proxy_port = unused_loopback_port();
    while egress_proxy_port == published_port {
        egress_proxy_port = unused_loopback_port();
    }
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
        manifest_port_lease_records(&manifest.runner_config.state_root, &manifest)
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
        assert_manifest_port_leases_released(&manifest.runner_config.state_root, &manifest);
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
}

#[test]
fn terminal_netavark_cleanup_ignores_current_machine_forwarder() {
    let published_port = unused_loopback_port();
    let drift_listener =
        TcpListener::bind("127.0.0.1:0").expect("current machine forwarder should bind");
    let drift_forwarder_port = drift_listener
        .local_addr()
        .expect("current machine forwarder address should resolve")
        .port();

    let temp_dir = TempDir::new().expect("tempdir should build");
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.node_network_supernet = "127.0.0.0/24".to_owned();
    let mut egress_proxy_port = unused_loopback_port();
    while egress_proxy_port == published_port {
        egress_proxy_port = unused_loopback_port();
    }
    config.published_port_range = egress_proxy_port..=egress_proxy_port;
    let mut backend = ContainerSandboxBackend::new(config);
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("db", published_port, 5432)),
            &SandboxId::new("manifest-netavark-config-drift"),
            None,
            None,
        )
        .expect("Netavark plan should lower")
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
        .expect("fixture must adopt the exact segment hold");
    backend
        .ensure_egress_proxy_running_with_release_authority(
            &manifest,
            PepPreAdoptionReleaseAuthority::FreshLaunch(&launch_claim),
        )
        .expect("fixture PEP should own exact listener evidence");
    let port_lease_coordinator = backend.port_lease_coordinator();
    let lifetimes = port_lease_coordinator
        .claim_netavark_bindings_with_lifetimes(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("fixture should claim the exact Netavark publication batch");
    port_lease_coordinator
        .activate_netavark_bindings_with_lifetimes(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
            &lifetimes,
        )
        .expect("fixture should activate the exact Netavark publication batch");
    backend
        .netavark_port_lifetimes
        .insert(&manifest.spec.tenant_id, &manifest.handle.id, lifetimes)
        .map_err(|(error, _batch)| error)
        .expect("fixture should retain the exact live Netavark lifetimes");
    let bindings_before =
        manifest_port_lease_records(&manifest.runner_config.state_root, &manifest)
            .into_iter()
            .map(|record| record.binding().cloned())
            .collect::<Vec<_>>();
    assert!(
        bindings_before.iter().all(Option::is_some),
        "fixture must activate every published and PEP binding before teardown"
    );
    manifest.launch_reservation_claim = None;
    backend.config.machine_port_forwarder = Some(sample_forwarder(drift_forwarder_port));
    mark_runtime_absent_for_cleanup(&mut manifest);
    let drift_observer = ForwarderObserver::spawn(drift_listener, Vec::new(), 0);

    backend
        .release_execution_artifacts(&mut manifest)
        .expect("persisted Netavark authority must survive current machine-mode drift");
    let drift_requests = drift_observer.finish_exact();
    assert!(
        drift_requests.is_empty(),
        "persisted Netavark cleanup must not contact the current machine forwarder"
    );
    let released =
        assert_manifest_port_leases_released(&manifest.runner_config.state_root, &manifest);
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
        "Netavark provider absence and exact lease release must publish finality"
    );
}

#[test]
fn unstarted_cancellation_uses_manifest_provider_context_after_backend_config_drift() {
    let drift_listener =
        TcpListener::bind("127.0.0.1:0").expect("current machine forwarder should bind");
    let drift_forwarder_port = drift_listener
        .local_addr()
        .expect("current machine forwarder address should resolve")
        .port();
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let mut backend =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(temp_dir.path()));
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("http", 18081, 8080)),
            &SandboxId::new("manifest-cancellation-config-drift"),
            None,
            None,
        )
        .expect("execute manifest should reserve exact launch authority")
        .manifest;
    let launch_claim = manifest
        .launch_reservation_claim
        .clone()
        .expect("claim-only fixture must retain exact reservation authority");
    let manifest_state_root = manifest.runner_config.state_root.clone();
    backend.config.published_port_range = 28000..=28001;
    backend.config.max_published_ports_per_tenant = Some(1);
    backend.config.machine_port_forwarder = Some(sample_forwarder(drift_forwarder_port));
    assert_ne!(
        manifest.runner_config.published_port_range, backend.config.published_port_range,
        "fixture must separate persisted and current allocation ranges"
    );
    assert!(
        manifest.runner_config.machine_port_forwarder.is_none(),
        "fixture manifest must retain its persisted Netavark provider family"
    );
    let drift_observer = ForwarderObserver::spawn(drift_listener, Vec::new(), 0);

    backend
        .release_unstarted_launch_artifacts(&mut manifest)
        .expect("claim-only cancellation must use the persisted manifest port authority");

    let drift_requests = drift_observer.finish_exact();
    assert!(
        drift_requests.is_empty(),
        "claim-only cancellation must not contact the current machine forwarder"
    );
    let released = assert_manifest_port_leases_released(&manifest_state_root, &manifest);
    assert!(
        released.iter().all(|record| {
            record.reservation_claim() == Some(&launch_claim)
                && record.bind_claim().is_none()
                && record.binding().is_none()
        }),
        "never-bound release must retain its exact coordinator fence for idempotent replay"
    );
    assert!(manifest.launch_reservation_claim.is_none());
    assert!(manifest.network_cleanup_complete);
}

#[test]
fn substituted_execution_context_fails_before_cancellation_effects() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let rootfs_path = temp_dir.path().join("substituted-context-rootfs");
    std::fs::create_dir_all(&rootfs_path).expect("rootfs fixture should create");
    let rootfs_sentinel = rootfs_path.join("sentinel");
    std::fs::write(&rootfs_sentinel, b"owned-artifact").expect("rootfs sentinel should persist");
    let backend =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(temp_dir.path()));
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("http", 18082, 8080)),
            &SandboxId::new("substituted-cancellation-context"),
            None,
            Some(sample_rootfs_artifact(rootfs_path.clone())),
        )
        .expect("execute manifest should reserve exact launch authority")
        .manifest;
    let launch_claim = manifest
        .launch_reservation_claim
        .clone()
        .expect("launch coordinator should remain durable");
    let pointer_path = manifest
        .bundle_layout
        .bundle_dir
        .join(super::super::runner::RUNNER_MANIFEST_POINTER_FILE);
    std::fs::write(&pointer_path, b"owned-pointer\n")
        .expect("runner pointer sentinel should persist");
    let authority_path =
        nimbus_network::LocalNetworkStateStore::authority_path_for(&backend.config.state_root);
    let authority_before =
        std::fs::read(&authority_path).expect("launch authority should be durable");
    manifest.runner_config.state_root = temp_dir.path().join("substituted-authority");

    let error = backend
        .release_unstarted_launch_artifacts(&mut manifest)
        .expect_err("substituted execution context must fail before every cleanup effect");
    assert!(
        error.to_string().contains("authority root")
            && error.to_string().contains("does not match"),
        "context rejection must name the mismatched authority: {error}"
    );
    assert_eq!(
        std::fs::read(&authority_path).expect("launch authority should remain readable"),
        authority_before,
        "substituted context must not release or rewrite durable authority"
    );
    assert_eq!(
        std::fs::read(&pointer_path).expect("runner pointer should remain"),
        b"owned-pointer\n",
        "context validation must precede runner-pointer removal"
    );
    assert_eq!(
        std::fs::read(&rootfs_sentinel).expect("rootfs sentinel should remain"),
        b"owned-artifact",
        "context validation must precede launch-artifact cleanup"
    );
    assert_eq!(
        manifest.launch_reservation_claim.as_ref(),
        Some(&launch_claim),
        "context rejection must retain exact compensation authority"
    );
    assert!(manifest.launch_artifact.is_some());
    assert!(!manifest.network_cleanup_complete);
}

#[test]
fn mounted_rootfs_cleanup_uses_manifest_buildah_context_after_config_drift() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let mut launch_config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    launch_config.buildah_path = "/usr/bin/true".into();
    launch_config.use_buildah_unshare = false;
    let launch_backend = ContainerSandboxBackend::new(launch_config);
    let manifest = launch_backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("manifest-buildah-config-drift"),
            None,
            Some(ContainerLaunchArtifact::MountedRootfs(
                crate::backends::oci::buildah::MountedRootfsSession {
                    session_name: "persisted-buildah-session".to_owned(),
                    image_reference: "example.invalid/test:latest".to_owned(),
                },
            )),
        )
        .expect("execute manifest should persist its launch-time Buildah context")
        .manifest;
    let mut drift_config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    drift_config.buildah_path = "/usr/bin/false".into();
    drift_config.use_buildah_unshare = true;
    let recovery = ContainerSandboxBackend::new(drift_config);

    recovery
        .cleanup_manifest_launch_artifacts(&manifest)
        .expect(
            "cleanup must use the exact Buildah path and unshare mode persisted in the manifest",
        );
}

#[test]
fn pending_creator_retains_network_authority_despite_runtime_absence() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(temp_dir.path()));
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("http", 18080, 8080)),
            &SandboxId::new("pending-creator-cleanup-fence"),
            None,
            None,
        )
        .expect("launch should reserve complete authority")
        .manifest;
    mark_runtime_absent_for_cleanup(&mut manifest);
    manifest.creator_handoff = ContainerCreatorHandoffState::Pending {
        receipt: crate::backends::conmon::creator::CreatorAttemptReceipt::for_test(
            "creator-attempt-pending",
        ),
    };
    let launch_claim = manifest
        .launch_reservation_claim
        .clone()
        .expect("launch coordinator claim should remain");
    let authority = nimbus_network::LocalPortLeaseAuthority::open(&backend.config.state_root)
        .expect("port authority should open");

    let error = backend
        .release_execution_artifacts(&mut manifest)
        .expect_err("runtime absence cannot outrun a pending creator");
    assert!(
        error.to_string().contains("creator")
            && error
                .to_string()
                .contains("runtime absence cannot authorize"),
        "cleanup denial must name the competing authority: {error}"
    );
    for request in manifest.port_leases.iter().chain(
        manifest
            .egress_proxy
            .as_ref()
            .map(|assignment| &assignment.port_lease),
    ) {
        let record = authority
            .inspect(request.lease_id())
            .expect("lease should inspect")
            .expect("reserved lease should remain");
        assert_eq!(record.phase(), nimbus_network::PortLeasePhase::Reserved);
        assert_eq!(record.reservation_claim(), Some(&launch_claim));
        assert!(record.bind_claim().is_none() && record.binding().is_none());
    }
    assert!(
        crate::backends::oci::network::inspect_container_ips(
            &manifest.network_layout,
            &manifest.handle.id,
        )
        .is_ok(),
        "pending creator must retain exact IPAM"
    );
}

#[test]
fn machine_forwarder_unexpose_failure_keeps_port_lease_fenced() {
    let published_port = unused_loopback_port();
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
    let port = listener
        .local_addr()
        .expect("listener address should resolve")
        .port();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("connection should arrive");
        read_complete_http_request(&mut stream);
        stream
            .write_all(
                b"HTTP/1.0 500 Internal Server Error\r\nContent-Length: 16\r\n\r\nproxy not found",
            )
            .expect("response should write");
    });

    let temp_dir = TempDir::new().expect("tempdir should build");
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.node_network_supernet = "127.0.0.0/24".to_owned();
    let mut egress_proxy_port = unused_loopback_port();
    while egress_proxy_port == published_port {
        egress_proxy_port = unused_loopback_port();
    }
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

    let error = backend
        .release_execution_artifacts(&mut manifest)
        .expect_err("failed provider unexpose must prevent lease release");
    server.join().expect("server thread should join");
    assert!(
        error.to_string().contains("machine forwarder unexpose"),
        "cleanup should report the provider failure: {error}"
    );
    let authority = nimbus_network::LocalPortLeaseAuthority::open(&backend.config.state_root)
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
    let retry_observer =
        ForwarderObserver::spawn_authenticated(retry_listener, &forwarder, vec![true], 1);
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
    let first_port = unused_loopback_port();
    let mut second_port = unused_loopback_port();
    while second_port == first_port {
        second_port = unused_loopback_port();
    }
    let listener = TcpListener::bind("127.0.0.1:0").expect("forwarder listener should bind");
    let forwarder_port = listener
        .local_addr()
        .expect("forwarder address should resolve")
        .port();

    let temp_dir = TempDir::new().expect("tempdir should build");
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.node_network_supernet = "127.0.0.0/24".to_owned();
    let mut egress_proxy_port = unused_loopback_port();
    while [first_port, second_port].contains(&egress_proxy_port) {
        egress_proxy_port = unused_loopback_port();
    }
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
    assert_manifest_port_leases_released(&manifest.runner_config.state_root, &manifest);
}

#[test]
fn restart_retained_machine_listener_releases_without_process_registry() {
    let published_port = unused_loopback_port();
    let temp_dir = TempDir::new().expect("tempdir should build");
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.node_network_supernet = "127.0.0.0/24".to_owned();
    let mut egress_proxy_port = unused_loopback_port();
    while egress_proxy_port == published_port {
        egress_proxy_port = unused_loopback_port();
    }
    config.published_port_range = egress_proxy_port..=egress_proxy_port;
    config.machine_port_forwarder = Some(sample_forwarder(unused_loopback_port()));
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
    let authority = nimbus_network::LocalPortLeaseAuthority::open(&backend.config.state_root)
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
        .expect("egress proxy should start on loopback test subnet");
    assert!(
        backend
            .egress_proxies
            .contains(&manifest.spec.tenant_id, &manifest.handle.id)
            .expect("registry lock should hold"),
        "egress proxy should be registered after ensure"
    );

    backend
        .release_execution_artifacts(&mut manifest)
        .expect("cleanup should stop proxy after explicit runtime-absence evidence");

    assert!(
        !backend
            .egress_proxies
            .contains(&manifest.spec.tenant_id, &manifest.handle.id)
            .expect("registry lock should hold"),
        "cleanup should drop the live egress proxy handle"
    );
}

#[test]
fn release_execution_artifacts_uses_quarantine_release_finalize_order() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let spec = sample_spec();
    let sandbox_id = sandbox_id();
    let recorder = Arc::new(RecordingSegmentAllocator::new(
        spec.tenant_id.clone(),
        "10.73.0.0/24",
        73,
    ));
    let injected: Arc<OciSegmentAllocator> = recorder.clone();
    let backend = ContainerSandboxBackend::with_segment_allocator(
        ContainerSandboxBackendConfig::under_root(temp_dir.path()),
        injected,
    );
    let mut manifest = backend
        .plan_start_with_id(&spec, &sandbox_id, None, None)
        .expect("plan should lower")
        .manifest;
    mark_runtime_absent_for_cleanup(&mut manifest);

    backend
        .release_execution_artifacts(&mut manifest)
        .expect("explicitly absent runtime cleanup should release");

    let attachment = default_network_attachment_id(&sandbox_id);
    let operations = recorder.operations();
    let expected_tail = [
        SegmentAllocatorOperation::Quarantine(spec.tenant_id.clone(), attachment.clone()),
        SegmentAllocatorOperation::Release(spec.tenant_id.clone(), attachment),
        SegmentAllocatorOperation::FinalizeRelease(
            spec.tenant_id,
            vec!["netsegment_01ARZ3NDEKTSV4RRFFQ69G5FAV".to_owned()],
        ),
    ];
    assert_eq!(
        operations.get(operations.len().saturating_sub(expected_tail.len())..),
        Some(expected_tail.as_slice()),
        "provider/netns teardown must be enclosed by durable quarantine and identity-fenced finalization"
    );
}

#[test]
fn stale_container_cleanup_cannot_mutate_replacement_network_generation() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.node_network_supernet = "127.0.0.0/24".to_owned();
    let backend = ContainerSandboxBackend::new(config);
    let id = SandboxId::new("container-cleanup-generation-fence");
    let spec = sample_spec();
    let mut stale = backend
        .plan_start_with_id(&spec, &id, None, None)
        .expect("first generation should reserve launch authority")
        .manifest;
    let stale_claim = stale
        .launch_reservation_claim
        .as_ref()
        .expect("first launch claim should remain")
        .clone();
    backend
        .ensure_egress_proxy_running_with_release_authority(
            &stale,
            PepPreAdoptionReleaseAuthority::FreshLaunch(&stale_claim),
        )
        .expect("non-IPAM provider evidence should be live before the generation changes");
    let stale_network_config = stale
        .network_config
        .as_ref()
        .expect("execute manifest should carry network config")
        .clone();
    crate::backends::oci::network::deallocate_container_ips_after_confirmed_detach(
        &stale.network_layout,
        &stale.handle.id,
        &stale_network_config.reservation_claim,
    )
    .expect("first IPAM generation should release exactly");
    let mut replacement_network_config = stale_network_config.clone();
    replacement_network_config.reservation_claim =
        crate::backends::oci::port_lease::new_launch_reservation_claim()
            .expect("replacement IPAM claim should mint");
    crate::backends::oci::network::allocate_container_ips(
        &stale.network_layout,
        &replacement_network_config,
        &stale.handle.id,
    )
    .expect("replacement IPAM generation should reuse the stable attachment");
    std::fs::create_dir_all(
        stale
            .network_layout
            .netns_path
            .parent()
            .expect("replacement netns parent should exist"),
    )
    .expect("replacement netns parent should create");
    std::fs::write(&stale.network_layout.netns_path, b"replacement-netns")
        .expect("replacement netns should persist");
    std::fs::create_dir_all(
        stale
            .network_layout
            .status_path
            .parent()
            .expect("replacement status parent should exist"),
    )
    .expect("replacement status parent should create");
    std::fs::write(&stale.network_layout.status_path, b"replacement-status")
        .expect("replacement status should persist");
    let authority_path =
        nimbus_network::LocalNetworkStateStore::authority_path_for(&backend.config.state_root);
    let authority_before =
        std::fs::read(&authority_path).expect("replacement authority should read");

    mark_runtime_absent_for_cleanup(&mut stale);
    let error = backend
        .release_execution_artifacts(&mut stale)
        .expect_err("stale cleanup must fail before any non-IPAM effect");
    assert!(
        error.to_string().contains("different launch coordinator"),
        "stale cleanup must identify the attachment generation fence: {error}"
    );
    assert_eq!(
        std::fs::read(&authority_path).expect("authority should remain readable"),
        authority_before,
        "stale cleanup must not rewrite port, segment, or IPAM authority"
    );
    assert!(
        backend
            .egress_proxies
            .contains(&stale.spec.tenant_id, &stale.handle.id)
            .expect("PEP registry should inspect"),
        "stale cleanup must not stop provider evidence while generations disagree"
    );
    assert_eq!(
        std::fs::read(&stale.network_layout.netns_path).expect("replacement netns should remain"),
        b"replacement-netns"
    );
    assert_eq!(
        std::fs::read(&stale.network_layout.status_path).expect("replacement status should remain"),
        b"replacement-status"
    );

    crate::backends::oci::network::deallocate_container_ips_after_confirmed_detach(
        &stale.network_layout,
        &stale.handle.id,
        &replacement_network_config.reservation_claim,
    )
    .expect("replacement IPAM should release exactly for fixture cleanup");
    crate::backends::oci::network::allocate_container_ips(
        &stale.network_layout,
        &stale_network_config,
        &stale.handle.id,
    )
    .expect("original generation should be restored for exact fixture cleanup");
    std::fs::remove_file(&stale.network_layout.netns_path)
        .expect("test should model confirmed provider absence");
    std::fs::remove_file(&stale.network_layout.status_path)
        .expect("test should remove the replacement generation's conflicting status");
    backend
        .segment_allocator
        .adopt_reserved_attachment(
            &stale.spec.tenant_id,
            &default_network_attachment_id(&stale.handle.id),
            &stale_claim,
        )
        .expect("original attachment should adopt before final cleanup");
    backend
        .release_execution_artifacts(&mut stale)
        .expect("original generation should clean up with its restored exact witness");
}

#[test]
fn natural_exit_preserves_terminal_ipam_until_segment_cleanup_finalizes() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let spec = sample_spec();
    let id = SandboxId::new("natural-exit-cleanup-retry");
    let recorder = Arc::new(
        RecordingSegmentAllocator::new(spec.tenant_id.clone(), "127.0.0.0/24", 73)
            .with_finalize_release_failure("injected segment finalization failure"),
    );
    let injected: Arc<OciSegmentAllocator> = recorder.clone();
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    let proxy_port = unused_loopback_port();
    config.published_port_range = proxy_port..=proxy_port;
    let backend = ContainerSandboxBackend::with_segment_allocator(config, injected);
    let mut manifest = backend
        .plan_start_with_id(&spec, &id, None, None)
        .expect("plan should lower")
        .manifest;
    mark_runtime_absent_for_cleanup(&mut manifest);
    let launch_claim = manifest
        .launch_reservation_claim
        .as_ref()
        .expect("execute launch should retain exact reservation authority")
        .clone();
    backend
        .segment_allocator
        .adopt_reserved_attachment(
            &manifest.spec.tenant_id,
            &default_network_attachment_id(&manifest.handle.id),
            &launch_claim,
        )
        .expect("fixture provider must adopt the segment hold");
    backend
        .ensure_egress_proxy_running_with_release_authority(
            &manifest,
            PepPreAdoptionReleaseAuthority::FreshLaunch(&launch_claim),
        )
        .expect("fixture PEP should own its exact listener");
    manifest.launch_reservation_claim = None;
    synchronize_handle_status(&mut manifest, SandboxStatus::Ready);
    std::fs::write(&manifest.conmon_layout.exit_status_file, b"17\n")
        .expect("natural-exit status should persist");
    backend
        .write_manifest(&manifest)
        .expect("running manifest should persist before inspection");

    let error = backend
        .inspect_sync(&id)
        .expect_err("segment finalization failure must abort terminal publication");
    assert!(
        error
            .to_string()
            .contains("injected segment finalization failure"),
        "inspection must surface the cleanup failure: {error}"
    );
    let persisted = backend
        .read_manifest(&id)
        .expect("manifest should remain readable")
        .expect("pre-cleanup manifest should remain durable");
    assert_eq!(
        persisted.status,
        SandboxStatus::Ready,
        "failed cleanup must not publish a terminal observed projection"
    );
    assert!(
        !persisted.network_cleanup_complete,
        "failed segment finalization must retain a nonterminal cleanup witness"
    );
    authenticate_container_network_generation_for_cleanup(
        &manifest.network_layout,
        manifest
            .network_config
            .as_ref()
            .expect("manifest should retain exact network generation"),
        &id,
    )
    .expect("exact terminal IPAM evidence must remain available for retry");

    recorder.clear_finalize_release_failure();
    let stopped = backend
        .inspect_sync(&id)
        .expect("inspection retry should converge")
        .expect("manifest should remain visible");
    assert_eq!(
        stopped.status,
        SandboxStatus::Failed,
        "a naturally absent runtime without a surviving pid file is observed as failed"
    );
    let terminal = backend
        .read_manifest(&id)
        .expect("terminal manifest should read")
        .expect("terminal manifest should remain durable");
    assert!(
        terminal.network_cleanup_complete,
        "successful retry must durably record network cleanup finality"
    );
    assert!(
        !crate::backends::oci::network::retire_terminal_container_ipam_release(
            &terminal.network_layout,
            &id,
            &terminal
                .network_config
                .as_ref()
                .expect("terminal manifest should retain generation identity")
                .reservation_claim,
        )
        .expect("terminal retirement replay should inspect"),
        "manifest publication must already have retired the exact terminal retry witness"
    );
}

#[test]
fn terminal_stop_replay_retries_ipam_receipt_retirement() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let backend =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(temp_dir.path()));
    let id = SandboxId::new("terminal-ipam-retirement-replay");
    let mut manifest = backend
        .plan_start_with_id(&sample_spec(), &id, None, None)
        .expect("execute planning should create exact network authority")
        .manifest;
    let network_config = manifest
        .network_config
        .as_ref()
        .expect("execute manifest should carry IPAM authority")
        .clone();
    crate::backends::oci::network::deallocate_container_ips_after_confirmed_detach(
        &manifest.network_layout,
        &id,
        &network_config.reservation_claim,
    )
    .expect("fixture should persist terminal IPAM retry evidence");
    manifest.shutdown_requested = true;
    manifest.network_cleanup_complete = true;
    manifest.launch_reservation_claim = None;
    manifest.launch_artifact = None;
    manifest.next_restart_at_millis = None;
    synchronize_handle_status(&mut manifest, SandboxStatus::Stopped);
    assert!(manifest.has_terminal_network_finality());

    let authority_path =
        nimbus_network::LocalNetworkStateStore::authority_path_for(&backend.config.state_root);
    let saved_authority = authority_path.with_extension("saved-for-terminal-replay");
    std::fs::rename(&authority_path, &saved_authority)
        .expect("authority should move behind deterministic fault");
    std::fs::create_dir(&authority_path)
        .expect("directory should force terminal authority read failure");
    let result = backend.write_manifest(&manifest);
    std::fs::remove_dir(&authority_path).expect("fault directory should remove");
    std::fs::rename(&saved_authority, &authority_path)
        .expect("authority should restore after deterministic fault");
    result.expect_err("terminal publication should report pending IPAM retirement");

    backend
        .stop_sync(&id)
        .expect("terminal stop replay should retire the exact pending IPAM receipt");
    assert!(
        !crate::backends::oci::network::retire_terminal_container_ipam_release(
            &manifest.network_layout,
            &id,
            &network_config.reservation_claim,
        )
        .expect("retirement replay should inspect"),
        "same-process terminal replay must already have retired the exact receipt"
    );
}
