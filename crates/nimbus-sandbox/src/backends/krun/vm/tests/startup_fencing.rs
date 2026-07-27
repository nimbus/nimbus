//! Startup reconciliation fences admission while preserving exact cleanup.

use super::support::*;

use std::sync::Arc;

use nimbus_network::NetworkSegmentAllocator;

use crate::backends::oci::network::{
    OciSegmentAllocator, RecordingSegmentAllocator, default_network_attachment_id,
};

fn inject_startup_reconciliation_failure(backend: &mut KrunSandboxBackend) {
    backend.startup_network_reconciliation_error = Some(Arc::<str>::from(
        "injected retained krun startup reconciliation failure",
    ));
}

#[test]
fn startup_reconciliation_failure_allows_exact_explicit_stop() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let spec = sample_spec_for_tenant("krun-startup-stop-fence", "api");
    let recorder = Arc::new(RecordingSegmentAllocator::new(
        spec.tenant_id.clone(),
        "10.79.0.0/24",
        79,
    ));
    let injected: Arc<OciSegmentAllocator> = recorder.clone();
    let mut backend = KrunSandboxBackend::with_segment_allocator(
        KrunSandboxBackendConfig::under_root(temp_dir.path()),
        injected,
    );
    let manifest = backend
        .plan_start_with_id(
            &spec,
            &SandboxId::new("krun-startup-stop-fence"),
            None,
            None,
        )
        .expect("execute launch should reserve exact authority")
        .manifest;
    backend
        .write_manifest(&manifest)
        .expect("baseline manifest should persist");
    let authority_path =
        nimbus_network::LocalNetworkStateStore::authority_path_for(&backend.config.state_root);
    let authority_before =
        fs::read(&authority_path).expect("launch authority should remain durable");
    inject_startup_reconciliation_failure(&mut backend);

    backend
        .stop_sync(&manifest.handle.id)
        .expect("an existing workload must retain exact stop authority");
    assert_ne!(
        fs::read(&authority_path).expect("launch authority should remain readable"),
        authority_before,
        "successful stop must durably release exact launch authority"
    );
    let persisted = backend
        .read_manifest(&manifest.handle.id)
        .expect("terminal manifest should inspect")
        .expect("terminal manifest should remain durable");
    assert_eq!(persisted.status, SandboxStatus::Stopped);
    assert_eq!(persisted.launch_authority, KrunLaunchAuthority::Released);
    assert!(
        recorder.operations().len() > 1,
        "exact stop must invoke authenticated segment cleanup"
    );
}

#[test]
fn startup_reconciliation_failure_allows_exact_natural_exit_cleanup() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let spec = sample_spec_for_tenant("krun-startup-exit-fence", "api");
    let recorder = Arc::new(RecordingSegmentAllocator::new(
        spec.tenant_id.clone(),
        "10.80.0.0/24",
        80,
    ));
    let injected: Arc<OciSegmentAllocator> = recorder.clone();
    let mut config = KrunSandboxBackendConfig::under_root(temp_dir.path());
    config.netavark_path = PathBuf::from("/usr/bin/true");
    let mut backend = KrunSandboxBackend::with_segment_allocator(config, injected);
    let mut manifest = backend
        .plan_start_with_id(
            &spec,
            &SandboxId::new("krun-startup-exit-fence"),
            None,
            None,
        )
        .expect("execute launch should reserve exact authority")
        .manifest;
    let claim = manifest
        .require_reserved_claim()
        .expect("initial launch should retain its reservation claim")
        .clone();
    recorder
        .adopt_reserved_attachment(
            &manifest.spec.tenant_id,
            &default_network_attachment_id(&manifest.handle.id),
            &claim,
        )
        .expect("fixture should model adopted attachment authority");
    manifest.launch_authority = KrunLaunchAuthority::Adopted {
        reservation_claim: claim,
    };
    super::super::readiness::synchronize_handle_status(&mut manifest, SandboxStatus::Ready);
    fs::write(&manifest.conmon_layout.exit_status_file, b"17\n")
        .expect("natural-exit status should persist");
    backend
        .write_manifest(&manifest)
        .expect("running manifest should persist before inspection");
    let authority_path =
        nimbus_network::LocalNetworkStateStore::authority_path_for(&backend.config.state_root);
    let authority_before =
        fs::read(&authority_path).expect("active authority should remain durable");
    inject_startup_reconciliation_failure(&mut backend);

    let inspected = backend
        .inspect_sync(&manifest.handle.id)
        .expect("natural-exit cleanup should retain exact authority")
        .expect("terminal workload should remain inspectable");
    assert_eq!(inspected.status, SandboxStatus::Failed);
    assert_ne!(
        fs::read(&authority_path).expect("active authority should remain readable"),
        authority_before,
        "natural-exit cleanup must durably release exact network authority"
    );
    let persisted = backend
        .read_manifest(&manifest.handle.id)
        .expect("terminal manifest should inspect")
        .expect("terminal manifest should remain durable");
    assert_eq!(persisted.status, SandboxStatus::Failed);
    assert_eq!(persisted.launch_authority, KrunLaunchAuthority::Released);
    assert!(
        recorder.operations().len() > 1,
        "natural-exit cleanup must invoke authenticated segment convergence"
    );
}

#[test]
fn startup_reconciliation_failure_keeps_restart_eligible_inspection_read_only() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let spec = sample_spec_for_tenant("krun-startup-restart-fence", "api")
        .with_restart_policy(SandboxRestartPolicy::OnFailure { max_restarts: 1 });
    let recorder = Arc::new(RecordingSegmentAllocator::new(
        spec.tenant_id.clone(),
        "10.81.0.0/24",
        81,
    ));
    let injected: Arc<OciSegmentAllocator> = recorder.clone();
    let mut backend = KrunSandboxBackend::with_segment_allocator(
        KrunSandboxBackendConfig::under_root(temp_dir.path()),
        injected,
    );
    let mut manifest = backend
        .plan_start_with_id(
            &spec,
            &SandboxId::new("krun-startup-restart-fence"),
            None,
            None,
        )
        .expect("execute launch should reserve exact authority")
        .manifest;
    let claim = manifest
        .require_reserved_claim()
        .expect("initial launch should retain its reservation claim")
        .clone();
    recorder
        .adopt_reserved_attachment(
            &manifest.spec.tenant_id,
            &default_network_attachment_id(&manifest.handle.id),
            &claim,
        )
        .expect("fixture should model adopted attachment authority");
    manifest.launch_authority = KrunLaunchAuthority::Adopted {
        reservation_claim: claim,
    };
    super::super::readiness::synchronize_handle_status(&mut manifest, SandboxStatus::Ready);
    fs::write(&manifest.conmon_layout.exit_status_file, b"17\n")
        .expect("restart-eligible exit status should persist");
    backend
        .write_manifest(&manifest)
        .expect("running manifest should persist before inspection");
    let manifest_before = fs::read(&manifest.conmon_layout.manifest_path)
        .expect("canonical manifest bytes should read");
    let authority_path =
        nimbus_network::LocalNetworkStateStore::authority_path_for(&backend.config.state_root);
    let authority_before = fs::read(&authority_path).expect("active authority should be durable");
    let operations_before = recorder.operations();
    inject_startup_reconciliation_failure(&mut backend);

    let inspected = backend
        .inspect_sync(&manifest.handle.id)
        .expect("startup-fenced inspection should remain available")
        .expect("restart-eligible workload should remain inspectable");
    assert_eq!(
        inspected, manifest.handle,
        "inspection must return the unchanged durable projection"
    );
    assert_eq!(
        fs::read(&manifest.conmon_layout.manifest_path)
            .expect("canonical manifest bytes should remain readable"),
        manifest_before,
        "startup-fenced inspection must not publish a restart decision"
    );
    assert_eq!(
        fs::read(&authority_path).expect("active authority should remain readable"),
        authority_before,
        "startup-fenced inspection must not mutate network authority"
    );
    assert_eq!(
        recorder.operations(),
        operations_before,
        "startup-fenced inspection must not invoke provider or segment effects"
    );
}
