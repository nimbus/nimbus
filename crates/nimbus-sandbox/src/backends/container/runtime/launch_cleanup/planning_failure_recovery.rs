use super::*;

#[test]
fn direct_planning_cleanup_failure_retains_claim_across_restart() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let state_root = temp_dir.path().join("state");
    let blocked_bundle_root = temp_dir.path().join("blocked-bundle-root");
    std::fs::write(&blocked_bundle_root, b"not a directory")
        .expect("bundle-root obstacle should write");
    let tenant = sample_spec().tenant_id;
    let first_allocator = Arc::new(
        RecordingSegmentAllocator::new(tenant.clone(), "10.76.0.0/24", 76)
            .with_release_reserved_failure("injected reserved-attachment cleanup failure"),
    );
    let first_injected: Arc<OciSegmentAllocator> = first_allocator;
    let mut first_config = ContainerSandboxBackendConfig::under_root(&state_root);
    first_config.bundle_root = blocked_bundle_root.clone();
    let first = ContainerSandboxBackend::with_segment_allocator(first_config, first_injected);
    let id = SandboxId::new("direct-planning-cleanup-restart");

    let error = first
        .plan_start_with_id(&sample_spec(), &id, None, None)
        .expect_err("post-reservation bundle failure should trigger injected cleanup failure");
    assert!(
        error
            .to_string()
            .contains("injected reserved-attachment cleanup failure"),
        "cleanup failure must remain visible: {error}"
    );
    let fenced = first
        .read_manifest(&id)
        .expect("failed manifest should inspect")
        .expect("failed manifest must remain durable");
    let retained_claim = fenced
        .launch_reservation_claim
        .clone()
        .expect("failed cleanup must retain the exact retry claim");
    assert_eq!(
        fenced.status,
        SandboxStatus::Stopping,
        "cleanup-pending planning authority must not be published as terminal"
    );
    assert!(
        crate::backends::oci::network::inspect_container_ips(
            &first.ipam_authority,
            &fenced.network_layout,
            &id,
        )
        .is_ok(),
        "failed compensation must safe-leak claim-fenced IPAM for exact retry"
    );
    std::fs::remove_file(&blocked_bundle_root)
        .expect("recovery should remove the injected bundle-root obstacle");
    std::fs::create_dir_all(&blocked_bundle_root)
        .expect("recovery should restore a traversable bundle root");

    let recovery_allocator = Arc::new(RecordingSegmentAllocator::new(tenant, "10.76.0.0/24", 76));
    let recovery_injected: Arc<OciSegmentAllocator> = recovery_allocator;
    let recovery = ContainerSandboxBackend::with_segment_allocator(
        ContainerSandboxBackendConfig::under_root(&state_root),
        recovery_injected,
    );
    let mut retained = recovery
        .read_manifest(&id)
        .expect("recovery manifest should inspect")
        .expect("recovery manifest should remain durable");
    recovery
        .release_unstarted_launch_artifacts(&mut retained)
        .expect("restart recovery should compensate the retained exact claim");
    retained.shutdown_requested = true;
    retained.last_exit_code = Some(0);
    synchronize_handle_status(&mut retained, SandboxStatus::Stopped);
    recovery
        .write_existing_workload_manifest(&retained)
        .expect("recovered compensation result must become durable");
    let stopped = recovery
        .read_manifest(&id)
        .expect("recovered manifest should inspect")
        .expect("recovered manifest should remain durable");
    assert_eq!(stopped.status, SandboxStatus::Stopped);
    assert!(stopped.launch_reservation_claim.is_none());
    assert_ne!(
        Some(&retained_claim),
        stopped.launch_reservation_claim.as_ref(),
        "successful exact retry must retire the retained claim"
    );
    assert!(
        crate::backends::oci::network::inspect_container_ips(
            &recovery.ipam_authority,
            &stopped.network_layout,
            &id,
        )
        .is_err(),
        "exact recovery must delete the matching IPAM generation"
    );
}

#[test]
fn runner_planning_cleanup_failure_retains_claim_across_restart() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let state_root = temp_dir.path().join("state");
    let tenant = sample_spec().tenant_id;
    let first_allocator = Arc::new(
        RecordingSegmentAllocator::new(tenant.clone(), "10.77.0.0/24", 77)
            .with_release_reserved_failure("injected runner attachment cleanup failure"),
    );
    let first_injected: Arc<OciSegmentAllocator> = first_allocator;
    let first = ContainerSandboxBackend::with_segment_allocator(
        ContainerSandboxBackendConfig::plan_only(temp_dir.path().join("bundles"), &state_root),
        first_injected,
    )
    .with_runner_handoff_failure(RunnerHandoffFailure::Manifest);

    let error = first
        .prepare_plan_only_service_workload(sample_spec())
        .expect_err("runner handoff failure should trigger injected cleanup failure");
    assert!(
        error
            .to_string()
            .contains("injected runner attachment cleanup failure"),
        "runner cleanup failure must remain visible: {error}"
    );
    let manifest_paths = crate::artifact_paths::all_manifest_paths(&state_root)
        .expect("failed runner manifest should enumerate");
    assert_eq!(manifest_paths.len(), 1);
    let fenced: ContainerSandboxManifest = serde_json::from_slice(
        &std::fs::read(&manifest_paths[0]).expect("failed runner manifest should read"),
    )
    .expect("failed runner manifest should parse");
    let id = fenced.handle.id.clone();
    assert_eq!(fenced.status, SandboxStatus::Stopping);
    assert_eq!(fenced.handle.status, SandboxStatus::Stopping);
    assert!(
        fenced.launch_reservation_claim.is_some(),
        "failed runner cleanup must retain exact compensation authority"
    );
    assert!(
        crate::backends::oci::network::inspect_container_ips(
            &first.ipam_authority,
            &fenced.network_layout,
            &id,
        )
        .is_ok(),
        "failed runner compensation must retain claim-fenced IPAM"
    );

    let recovery_allocator = Arc::new(RecordingSegmentAllocator::new(tenant, "10.77.0.0/24", 77));
    let recovery_injected: Arc<OciSegmentAllocator> = recovery_allocator;
    let recovery = ContainerSandboxBackend::with_segment_allocator(
        ContainerSandboxBackendConfig::plan_only(temp_dir.path().join("bundles"), &state_root),
        recovery_injected,
    );
    let retained = recovery
        .read_manifest(&id)
        .expect("runner recovery manifest should inspect")
        .expect("runner recovery manifest should remain durable");
    recovery
        .mark_plan_only_service_workload_stopped(&retained.handle.id)
        .expect("runner restart recovery should compensate the retained claim");
    let stopped = recovery
        .read_manifest(&id)
        .expect("recovered runner manifest should inspect")
        .expect("recovered runner manifest should remain durable");
    assert_eq!(stopped.status, SandboxStatus::Stopped);
    assert!(stopped.launch_reservation_claim.is_none());
    assert!(
        crate::backends::oci::network::inspect_container_ips(
            &recovery.ipam_authority,
            &stopped.network_layout,
            &id,
        )
        .is_err(),
        "runner recovery must delete the matching IPAM generation"
    );
}
