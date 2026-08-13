//! Natural-exit observation proofs.

use super::support::*;

use std::sync::Arc;

use nimbus_network::{LocalPortLeaseAuthority, NetworkSegmentAllocator};

use crate::backends::oci::network::{
    OciSegmentAllocator, RecordingSegmentAllocator, default_network_attachment_id,
};

#[test]
fn natural_execute_exit_is_observed_without_cleanup_side_effects() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let spec = sample_spec_for_tenant("krun-natural-exit", "api");
    let recorder = Arc::new(RecordingSegmentAllocator::new(
        spec.tenant_id.clone(),
        "10.77.0.0/24",
        77,
    ));
    let injected: Arc<OciSegmentAllocator> = recorder.clone();
    let mut config = KrunSandboxBackendConfig::under_root(temp_dir.path().to_path_buf());
    config.netavark_path = PathBuf::from("/usr/bin/true");
    let backend = KrunSandboxBackend::with_segment_allocator(config, injected);
    let mut manifest = backend
        .plan_start_with_id(&spec, &SandboxId::new("krun-natural-exit"), None, None)
        .expect("execute launch should reserve exact network authority")
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
    backend
        .write_manifest(&manifest)
        .expect("running-shaped manifest should persist");
    fs::write(&manifest.conmon_layout.exit_status_file, b"0\n")
        .expect("natural exit should persist");
    let operations_before = recorder.operations();
    let manifest_before =
        fs::read(&manifest.conmon_layout.manifest_path).expect("manifest bytes should be readable");
    let authority = LocalPortLeaseAuthority::open(&backend.config.network_state_root)
        .expect("authority should open");
    let pep_request = &manifest
        .egress_proxy
        .as_ref()
        .expect("execute launch should reserve its PEP")
        .port_lease;

    let inspected = backend
        .inspect_sync(&manifest.handle.id)
        .expect("natural exit observation should succeed")
        .expect("exited workload should remain inspectable");
    assert_eq!(inspected.handle.status, SandboxStatus::Stopping);
    assert!(inspected.handle.published_endpoints.is_empty());
    assert_eq!(
        inspected.execution,
        SandboxExecutionObservation::Exited { exit_code: 0 }
    );
    assert_eq!(
        inspected.restart,
        SandboxRestartAssessment::Candidate {
            exit_code: 0,
            blocker: None,
        }
    );
    assert_eq!(inspected.cleanup, SandboxCleanupObservation::Retained);
    let repeated = backend
        .inspect_sync(&manifest.handle.id)
        .expect("repeated natural-exit observation should succeed")
        .expect("exited workload should remain inspectable");
    assert_eq!(repeated, inspected);
    assert_eq!(
        fs::read(&manifest.conmon_layout.manifest_path)
            .expect("manifest bytes should remain readable"),
        manifest_before,
        "inspection must not persist natural-exit cleanup state"
    );
    assert_eq!(
        recorder.operations(),
        operations_before,
        "inspection must not invoke allocator cleanup"
    );
    assert_eq!(
        authority
            .inspect(pep_request.lease_id())
            .expect("PEP lease should inspect")
            .expect("reserved evidence should remain durable")
            .phase(),
        nimbus_network::PortLeasePhase::Reserved,
        "inspection must not adopt, bind, or release listener authority"
    );
}
