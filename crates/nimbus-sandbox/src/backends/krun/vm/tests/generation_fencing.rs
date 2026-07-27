use super::*;
use crate::backends::krun::vm::lifecycle::NetworkArtifactTeardownMode;
use crate::backends::oci::network::{
    OciNetworkConfig, OciNetworkLayout, deallocate_container_ips_after_confirmed_detach,
};

#[test]
fn stale_krun_cleanup_cannot_mutate_replacement_network_generation() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let spec = sample_spec_for_tenant("krun-cleanup-generation-fence", "api");
    let recorder = Arc::new(RecordingSegmentAllocator::new(
        spec.tenant_id.clone(),
        "10.77.0.0/24",
        77,
    ));
    let injected: Arc<OciSegmentAllocator> = recorder.clone();
    let backend = KrunSandboxBackend::with_segment_allocator(
        KrunSandboxBackendConfig::plan_only(
            temp_dir.path().join("bundles"),
            temp_dir.path().join("state"),
        ),
        injected,
    );
    let mut manifest = sample_manifest(spec, KrunStartMode::Execute);
    let layout = OciNetworkLayout::new(
        &backend.config.state_root,
        &manifest.spec.tenant_id,
        &manifest.handle.id,
    );
    layout
        .ensure_directories()
        .expect("network layout should exist");
    let stale_config = OciNetworkConfig::default();
    allocate_container_ips(&layout, &stale_config, &manifest.handle.id)
        .expect("first generation should reserve IPAM");
    deallocate_container_ips_after_confirmed_detach(
        &layout,
        &manifest.handle.id,
        &stale_config.reservation_claim,
    )
    .expect("first generation should release exact IPAM");
    let mut replacement_config = stale_config.clone();
    replacement_config.reservation_claim =
        crate::backends::oci::port_lease::new_launch_reservation_claim()
            .expect("replacement claim should mint");
    allocate_container_ips(&layout, &replacement_config, &manifest.handle.id)
        .expect("replacement generation should reserve the stable attachment");
    std::fs::write(&layout.netns_path, b"replacement-netns")
        .expect("replacement netns should persist");
    std::fs::write(&layout.status_path, b"replacement-status")
        .expect("replacement status should persist");
    let authority_path =
        nimbus_network::LocalNetworkStateStore::authority_path_for(&backend.config.state_root);
    let authority_before =
        std::fs::read(&authority_path).expect("replacement authority should read");
    let allocator_before = recorder.operations();
    manifest.network_layout = layout;
    manifest.network_config = Some(stale_config);

    let error = backend
        .release_network_artifacts(&manifest, NetworkArtifactTeardownMode::Final)
        .expect_err("stale krun cleanup must fail before replacement effects");
    assert!(
        error.to_string().contains("different launch coordinator"),
        "stale cleanup must identify the attachment generation fence: {error}"
    );
    assert_eq!(
        std::fs::read(&authority_path).expect("authority should remain readable"),
        authority_before,
        "stale cleanup must not rewrite replacement network authority"
    );
    assert_eq!(
        recorder.operations(),
        allocator_before,
        "stale cleanup must not quarantine or release replacement segment authority"
    );
    assert_eq!(
        std::fs::read(&manifest.network_layout.netns_path)
            .expect("replacement netns should remain"),
        b"replacement-netns"
    );
    assert_eq!(
        std::fs::read(&manifest.network_layout.status_path)
            .expect("replacement status should remain"),
        b"replacement-status"
    );
}
