//! Provider cleanup finality, ordering, and generation-fencing proofs.

use super::*;
use crate::inspection::{SandboxCleanupObservation, SandboxExecutionObservation};

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
        .segment_allocator
        .adopt_reserved_attachment(
            &manifest.spec.tenant_id,
            &default_network_attachment_id(&manifest.handle.id),
            manifest
                .launch_reservation_claim
                .as_ref()
                .expect("provider cleanup fixture should retain its exact claim"),
        )
        .expect("provider cleanup fixture must adopt its bound segment association");

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
        &backend.ipam_authority,
        &stale.network_layout,
        &stale.handle.id,
        &stale_network_config.attachment_id,
        &stale_network_config.reservation_claim,
        stale_network_config.provider_kind(),
    )
    .expect("first IPAM generation should release exactly");
    let mut replacement_network_config = stale_network_config.clone();
    replacement_network_config.reservation_claim =
        crate::backends::oci::port_lease::new_launch_reservation_claim()
            .expect("replacement IPAM claim should mint");
    crate::backends::oci::network::allocate_container_ips(
        &backend.ipam_authority,
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
    let authority_path = nimbus_network::LocalNetworkStateStore::authority_path_for(
        &backend.config.network_state_root,
    );
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
        &backend.ipam_authority,
        &stale.network_layout,
        &stale.handle.id,
        &replacement_network_config.attachment_id,
        &replacement_network_config.reservation_claim,
        replacement_network_config.provider_kind(),
    )
    .expect("replacement IPAM should release exactly for fixture cleanup");
    crate::backends::oci::network::allocate_container_ips(
        &backend.ipam_authority,
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
    let manifest_before = std::fs::read(&manifest.conmon_layout.manifest_path)
        .expect("manifest bytes should be readable");
    let operations_before = recorder.operations();

    let inspected = backend
        .inspect_sync(&id)
        .expect("inspection must not enter segment finalization")
        .expect("exited workload should remain inspectable");
    assert_eq!(inspected.handle.status, SandboxStatus::Stopping);
    assert!(inspected.handle.published_endpoints.is_empty());
    assert_eq!(
        inspected.execution,
        SandboxExecutionObservation::Exited { exit_code: 17 }
    );
    assert_eq!(inspected.cleanup, SandboxCleanupObservation::Retained);
    assert_eq!(recorder.operations(), operations_before);
    assert_eq!(
        std::fs::read(&manifest.conmon_layout.manifest_path)
            .expect("manifest bytes should remain readable"),
        manifest_before,
        "natural-exit inspection must be byte-stable"
    );

    let error = backend
        .stop_sync(&id)
        .expect_err("segment finalization failure must abort explicit terminal publication");
    assert!(
        error
            .to_string()
            .contains("injected segment finalization failure"),
        "explicit stop must surface the cleanup failure: {error}"
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
        &backend.ipam_authority,
        &manifest.network_layout,
        manifest
            .network_config
            .as_ref()
            .expect("manifest should retain exact network generation"),
        &id,
    )
    .expect("exact terminal IPAM evidence must remain available for retry");

    recorder.clear_finalize_release_failure();
    backend
        .stop_sync(&id)
        .expect("explicit cleanup retry should converge");
    let stopped = backend
        .inspect_sync(&id)
        .expect("terminal inspection should succeed")
        .expect("manifest should remain visible");
    assert_eq!(
        stopped.handle.status,
        SandboxStatus::Stopped,
        "explicit stop must publish terminal cleanup finality"
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
            &backend.ipam_authority,
            &terminal.network_layout,
            &id,
            &terminal
                .network_config
                .as_ref()
                .expect("terminal manifest should retain generation identity")
                .attachment_id,
            &terminal
                .network_config
                .as_ref()
                .expect("terminal manifest should retain generation identity")
                .reservation_claim,
            terminal
                .network_config
                .as_ref()
                .expect("terminal manifest should retain generation identity")
                .provider_kind(),
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
        &backend.ipam_authority,
        &manifest.network_layout,
        &id,
        &network_config.attachment_id,
        &network_config.reservation_claim,
        network_config.provider_kind(),
    )
    .expect("fixture should persist terminal IPAM retry evidence");
    manifest.shutdown_requested = true;
    manifest.network_cleanup_complete = true;
    manifest.launch_reservation_claim = None;
    manifest.launch_artifact = None;
    manifest.next_restart_at_millis = None;
    synchronize_handle_status(&mut manifest, SandboxStatus::Stopped);
    assert!(manifest.has_terminal_network_finality());

    let authority_path = nimbus_network::LocalNetworkStateStore::authority_path_for(
        &backend.config.network_state_root,
    );
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
            &backend.ipam_authority,
            &manifest.network_layout,
            &id,
            &network_config.attachment_id,
            &network_config.reservation_claim,
            network_config.provider_kind(),
        )
        .expect("retirement replay should inspect"),
        "same-process terminal replay must already have retired the exact receipt"
    );
}
