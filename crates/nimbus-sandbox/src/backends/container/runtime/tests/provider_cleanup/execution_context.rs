//! Persisted execution-context and provider-selection cleanup proofs.

use super::assertions::{assert_manifest_port_leases_released, manifest_port_lease_records};
use super::forwarder_observer::ForwarderObserver;
use super::*;

#[test]
fn unstarted_artifact_cleanup_failure_retains_claim_for_idempotent_retry() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(temp_dir.path()));
    let spec = sample_spec();
    let sandbox_id = SandboxId::new("unstarted-artifact-cleanup-retry");
    let artifact_path = crate::artifact_paths::rootfs_root(
        &backend.config.workload_state_root,
        &spec.tenant_id,
        &sandbox_id,
    )
    .join(sandbox_id.as_str());
    std::fs::create_dir_all(
        artifact_path
            .parent()
            .expect("artifact parent should exist"),
    )
    .expect("artifact parent should create");
    std::fs::write(&artifact_path, b"not a directory")
        .expect("rootfs cleanup obstacle should write");
    let rootfs_path = artifact_path.join("rootfs");
    let mut manifest = backend
        .plan_start_with_id(
            &spec,
            &sandbox_id,
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
            .contains("filesystem identity is no longer an owned directory"),
        "cleanup must report the exact artifact failure: {error}"
    );
    assert_eq!(
        manifest.launch_reservation_claim.as_ref(),
        Some(&claim),
        "secondary artifact failure must retain the idempotent network-release claim"
    );
    assert!(!manifest.network_cleanup_complete);

    std::fs::remove_file(&artifact_path).expect("cleanup obstacle should be removable");
    backend
        .release_unstarted_launch_artifacts(&mut manifest)
        .expect("same-claim replay should converge after the transient artifact failure");
    assert!(manifest.launch_reservation_claim.is_none());
    assert!(manifest.launch_artifact.is_none());
    assert!(manifest.network_cleanup_complete);
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
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path())
        .with_network_state_root(temp_dir.path().join("node-network-state"));
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
        manifest_port_lease_records(&manifest.runner_config.network_state_root, &manifest)
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
    let manifest_state_root = manifest.runner_config.network_state_root.clone();
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
    let workload_state_root = temp_dir.path().join("state");
    let network_state_root = temp_dir.path().join("node-network-state");
    let backend = ContainerSandboxBackend::new(
        ContainerSandboxBackendConfig::under_root(temp_dir.path())
            .with_network_state_root(&network_state_root),
    );
    assert_eq!(backend.config.workload_state_root, workload_state_root);
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
    let authority_path = nimbus_network::LocalNetworkStateStore::authority_path_for(
        &backend.config.network_state_root,
    );
    let authority_before =
        std::fs::read(&authority_path).expect("launch authority should be durable");
    manifest.runner_config.workload_state_root = temp_dir.path().join("substituted-workload-state");

    let error = backend
        .release_unstarted_launch_artifacts(&mut manifest)
        .expect_err("substituted workload context must fail before every cleanup effect");
    assert!(
        error.to_string().contains("workload root") && error.to_string().contains("does not match"),
        "context rejection must name the mismatched workload root: {error}"
    );
    manifest.runner_config.workload_state_root = workload_state_root;
    manifest.runner_config.network_state_root = temp_dir.path().join("substituted-network-state");
    let error = backend
        .release_unstarted_launch_artifacts(&mut manifest)
        .expect_err("substituted network context must fail before every cleanup effect");
    assert!(
        error.to_string().contains("network authority root")
            && error.to_string().contains("does not match"),
        "context rejection must name the mismatched network root: {error}"
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
    let authority =
        nimbus_network::LocalPortLeaseAuthority::open(&backend.config.network_state_root)
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
            &backend.ipam_authority,
            &manifest.network_layout,
            &manifest.handle.id,
        )
        .is_ok(),
        "pending creator must retain exact IPAM"
    );
}
