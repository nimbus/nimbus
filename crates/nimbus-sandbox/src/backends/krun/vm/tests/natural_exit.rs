//! Natural-exit network convergence proofs.

use super::support::*;

use nimbus_network::{LocalPortLeaseAuthority, NetworkSegmentAllocator};
use std::sync::Arc;

use crate::backends::oci::network::{
    OciSegmentAllocator, RecordingSegmentAllocator, SegmentAllocatorOperation,
    default_network_attachment_id,
};

#[test]
fn natural_execute_exit_releases_exact_network_authority_before_terminal_status() {
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
        .expect("initial launch should retain its reservation claim");
    let claim = claim.clone();
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
    let before_cleanup = recorder.operations().len();

    let inspected = backend
        .inspect_sync(&manifest.handle.id)
        .expect("natural exit reconciliation should succeed")
        .expect("terminal workload should remain inspectable");
    assert_eq!(inspected.status, SandboxStatus::Stopped);
    assert!(inspected.published_endpoints.is_empty());
    let persisted = backend
        .read_manifest(&manifest.handle.id)
        .expect("terminal manifest should inspect")
        .expect("terminal manifest should remain durable");
    assert_eq!(persisted.status, SandboxStatus::Stopped);
    assert_eq!(persisted.last_exit_code, Some(0));
    assert!(
        persisted.shutdown_requested,
        "natural exit must durably publish shutdown intent before terminal cleanup"
    );
    assert_eq!(
        &recorder.operations()[before_cleanup..],
        [
            SegmentAllocatorOperation::Quarantine(
                manifest.spec.tenant_id.clone(),
                default_network_attachment_id(&manifest.handle.id),
            ),
            SegmentAllocatorOperation::Quarantine(
                manifest.spec.tenant_id.clone(),
                default_network_attachment_id(&manifest.handle.id),
            ),
            SegmentAllocatorOperation::Release(
                manifest.spec.tenant_id.clone(),
                default_network_attachment_id(&manifest.handle.id),
            ),
            SegmentAllocatorOperation::FinalizeRelease(
                manifest.spec.tenant_id.clone(),
                vec!["netsegment_01ARZ3NDEKTSV4RRFFQ69G5FAV".to_owned()],
            ),
            SegmentAllocatorOperation::InspectAttachment(
                manifest.spec.tenant_id.clone(),
                default_network_attachment_id(&manifest.handle.id),
            ),
        ],
        "terminal status may publish only after exact tenant/workload attachment convergence and \
         read-only absence verification"
    );
    let authority =
        LocalPortLeaseAuthority::open(&backend.config.state_root).expect("authority should reopen");
    let pep_request = &manifest
        .egress_proxy
        .as_ref()
        .expect("execute launch should reserve its PEP")
        .port_lease;
    assert_eq!(
        authority
            .inspect(pep_request.lease_id())
            .expect("PEP lease should inspect")
            .expect("released evidence should remain durable")
            .phase(),
        nimbus_network::PortLeasePhase::Released
    );

    let operations_after_first_inspection = recorder.operations();
    let repeated = backend
        .inspect_sync(&manifest.handle.id)
        .expect("released natural-exit cleanup must be idempotently inspectable")
        .expect("terminal workload should remain inspectable");
    assert_eq!(repeated.status, SandboxStatus::Stopped);
    assert!(repeated.published_endpoints.is_empty());
    assert_eq!(
        &recorder.operations()[operations_after_first_inspection.len()..],
        [SegmentAllocatorOperation::InspectAttachment(
            manifest.spec.tenant_id.clone(),
            default_network_attachment_id(&manifest.handle.id),
        )],
        "repeated terminal inspection may verify exact absence read-only, but must not replay \
         provider or allocation cleanup"
    );
}

#[test]
fn natural_execute_exit_cleanup_failure_remains_stopping_with_exact_fence() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let spec = sample_spec_for_tenant("krun-natural-exit-failure", "api");
    let recorder = Arc::new(
        RecordingSegmentAllocator::new(spec.tenant_id.clone(), "10.78.0.0/24", 78)
            .with_quarantine_failure("forced exact attachment quarantine failure"),
    );
    let injected: Arc<OciSegmentAllocator> = recorder.clone();
    let mut config = KrunSandboxBackendConfig::under_root(temp_dir.path().to_path_buf());
    config.netavark_path = PathBuf::from("/usr/bin/true");
    let backend = KrunSandboxBackend::with_segment_allocator(config, injected);
    let mut manifest = backend
        .plan_start_with_id(
            &spec,
            &SandboxId::new("krun-natural-exit-failure"),
            None,
            None,
        )
        .expect("execute launch should reserve exact network authority")
        .manifest;
    let claim = manifest
        .require_reserved_claim()
        .expect("initial launch should retain its reservation claim");
    let claim = claim.clone();
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

    let error = backend
        .inspect_sync(&manifest.handle.id)
        .expect_err("failed cleanup must not publish a terminal state");
    assert!(
        error
            .to_string()
            .contains("forced exact attachment quarantine failure"),
        "the cleanup failure must remain observable: {error}"
    );
    let persisted = backend
        .read_manifest(&manifest.handle.id)
        .expect("retry checkpoint should inspect")
        .expect("retry checkpoint should remain durable");
    assert_eq!(persisted.status, SandboxStatus::Stopping);
    assert_eq!(persisted.handle.status, SandboxStatus::Stopping);
    assert_eq!(persisted.last_exit_code, Some(0));
    assert!(persisted.handle.published_endpoints.is_empty());
    assert_eq!(
        persisted.launch_authority,
        KrunLaunchAuthority::Adopted {
            reservation_claim: manifest
                .reservation_claim()
                .expect("fixture should retain its adoption receipt")
                .clone(),
        },
        "failed convergence must retain authenticated provider teardown authority"
    );
    let authority =
        LocalPortLeaseAuthority::open(&backend.config.state_root).expect("authority should reopen");
    let pep_request = &manifest
        .egress_proxy
        .as_ref()
        .expect("execute launch should reserve its PEP")
        .port_lease;
    assert_eq!(
        authority
            .inspect(pep_request.lease_id())
            .expect("PEP lease should inspect")
            .expect("fenced evidence should remain durable")
            .phase(),
        nimbus_network::PortLeasePhase::Reserved,
        "failed attachment cleanup must not release sibling listener authority"
    );
}
