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
fn startup_reconciliation_failure_fences_direct_initial_launch_before_effects() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let spec = sample_spec_for_tenant("krun-startup-launch-fence", "api");
    let recorder = Arc::new(RecordingSegmentAllocator::new(
        spec.tenant_id.clone(),
        "10.78.0.0/24",
        78,
    ));
    let injected: Arc<OciSegmentAllocator> = recorder.clone();
    let mut backend = KrunSandboxBackend::with_segment_allocator(
        KrunSandboxBackendConfig::under_root(temp_dir.path()),
        injected,
    );
    let mut manifest = backend
        .plan_start_with_id(
            &spec,
            &SandboxId::new("krun-startup-launch-fence"),
            None,
            None,
        )
        .expect("initial planning should reserve exact launch authority")
        .manifest;
    let authority_path = nimbus_network::LocalNetworkStateStore::authority_path_for(
        &backend.config.network_state_root,
    );
    let authority_before = fs::read(&authority_path).expect("reserved authority should be durable");
    let operations_before = recorder.operations();
    let manifest_before =
        serde_json::to_vec(&manifest).expect("unstarted manifest should serialize");
    inject_startup_reconciliation_failure(&mut backend);

    let error = backend
        .launch_manifest(&mut manifest, true)
        .expect_err("retained startup failure must fence direct initial launch");

    assert!(
        error
            .to_string()
            .contains("refuses new network work because startup reconciliation did not complete"),
        "the retained startup diagnostic must remain primary: {error}"
    );
    assert_eq!(
        serde_json::to_vec(&manifest).expect("fenced manifest should serialize"),
        manifest_before,
        "the launch fence must precede in-memory lifecycle mutation"
    );
    assert_eq!(
        fs::read(&authority_path).expect("reserved authority should remain readable"),
        authority_before,
        "the launch fence must not mutate portable network authority"
    );
    assert_eq!(
        recorder.operations(),
        operations_before,
        "the launch fence must precede allocator or provider effects"
    );
    assert!(!manifest.network_layout.netns_path.exists());
    assert!(!manifest.network_layout.status_path.exists());
}

#[test]
fn nnc5_2d_krun_startup_durably_fences_unmatched_no_hold_evidence() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let config = KrunSandboxBackendConfig::under_root(temp_dir.path());
    let unmatched = config
        .workload_state_root
        .join("tenants")
        .join("tenant-unmatched-krun")
        .join("networks")
        .join("netns")
        .join("orphan-without-hold");
    fs::create_dir_all(
        unmatched
            .parent()
            .expect("unmatched netns parent should exist"),
    )
    .expect("unmatched netns parent should create");
    fs::write(&unmatched, b"persistent-netns").expect("unmatched durable evidence should write");

    for attempt in 0..2 {
        let backend = KrunSandboxBackend::new(config.clone());
        let error = backend
            .plan_start_with_id(
                &sample_spec(),
                &SandboxId::new(format!("krun-admission-{attempt}")),
                None,
                None,
            )
            .expect_err("unmatched no-hold evidence must fence every fresh backend");
        let message = error.to_string();
        assert!(
            message.contains("startup reconciliation did not complete")
                && message.contains("unmatched artifact"),
            "the durable admission fence must name the retained unmatched evidence: {message}"
        );
        assert!(
            unmatched.is_file(),
            "startup quarantine must preserve unmatched evidence for later cleanup convergence"
        );
    }
}

#[test]
fn startup_reconciliation_failure_allows_exact_unstarted_launch_compensation() {
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
    let mut manifest = backend
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
    let authority_path = nimbus_network::LocalNetworkStateStore::authority_path_for(
        &backend.config.network_state_root,
    );
    let authority_before =
        fs::read(&authority_path).expect("launch authority should remain durable");
    inject_startup_reconciliation_failure(&mut backend);

    let error = backend.persist_unstarted_launch_failure(
        &mut manifest,
        crate::error::SandboxError::OperationFailed {
            message: "injected fenced launch cancellation".to_owned(),
        },
    );
    assert!(error.to_string().contains("fenced launch cancellation"));
    assert_ne!(
        fs::read(&authority_path).expect("launch authority should remain readable"),
        authority_before,
        "successful stop must durably release exact launch authority"
    );
    let persisted = backend
        .read_manifest(&manifest.handle.id)
        .expect("terminal manifest should inspect")
        .expect("terminal manifest should remain durable");
    assert_eq!(persisted.status, SandboxStatus::Failed);
    assert_eq!(persisted.launch_authority, KrunLaunchAuthority::Released);
    assert!(
        recorder.operations().len() > 1,
        "exact stop must invoke authenticated segment cleanup"
    );
}

#[test]
fn startup_reconciliation_failure_keeps_natural_exit_read_only_until_owned_compensation() {
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
    let authority_path = nimbus_network::LocalNetworkStateStore::authority_path_for(
        &backend.config.network_state_root,
    );
    let authority_before =
        fs::read(&authority_path).expect("active authority should remain durable");
    let operations_before = recorder.operations();
    inject_startup_reconciliation_failure(&mut backend);

    let inspected = backend
        .inspect_sync(&manifest.handle.id)
        .expect("natural-exit inspection should retain exact authority")
        .expect("exited workload should remain inspectable");
    assert_eq!(inspected.handle.status, SandboxStatus::Stopping);
    assert!(inspected.handle.published_endpoints.is_empty());
    assert_eq!(
        inspected.execution,
        SandboxExecutionObservation::Exited { exit_code: 17 }
    );
    assert_eq!(inspected.cleanup, SandboxCleanupObservation::Retained);
    assert_eq!(
        fs::read(&authority_path).expect("active authority should remain readable"),
        authority_before,
        "natural-exit inspection must not release exact network authority"
    );
    let persisted = backend
        .read_manifest(&manifest.handle.id)
        .expect("retained manifest should inspect")
        .expect("retained manifest should remain durable");
    assert_eq!(persisted, manifest);
    assert_eq!(
        recorder.operations(),
        operations_before,
        "inspection must not invoke allocator cleanup"
    );

    let error = backend.persist_provider_launch_failure(
        &mut manifest,
        crate::error::SandboxError::OperationFailed {
            message: "injected provider cleanup after startup fence".to_owned(),
        },
    );
    assert!(
        error
            .to_string()
            .contains("provider cleanup after startup fence")
    );
    assert_ne!(
        fs::read(&authority_path).expect("released authority should remain readable"),
        authority_before,
        "explicit stop must durably release exact network authority"
    );
    let persisted = backend
        .read_manifest(&manifest.handle.id)
        .expect("terminal manifest should inspect")
        .expect("terminal manifest should remain durable");
    assert_eq!(persisted.status, SandboxStatus::Failed);
    assert_eq!(persisted.launch_authority, KrunLaunchAuthority::Released);
    assert!(
        recorder.operations().len() > operations_before.len(),
        "explicit stop must invoke authenticated segment convergence"
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
    let authority_path = nimbus_network::LocalNetworkStateStore::authority_path_for(
        &backend.config.network_state_root,
    );
    let authority_before = fs::read(&authority_path).expect("active authority should be durable");
    let operations_before = recorder.operations();
    inject_startup_reconciliation_failure(&mut backend);

    let inspected = backend
        .inspect_sync(&manifest.handle.id)
        .expect("startup-fenced inspection should remain available")
        .expect("restart-eligible workload should remain inspectable");
    assert_eq!(inspected.handle.status, SandboxStatus::Stopping);
    assert!(inspected.handle.published_endpoints.is_empty());
    assert_eq!(
        inspected.execution,
        SandboxExecutionObservation::Exited { exit_code: 17 }
    );
    assert_eq!(
        inspected.restart,
        SandboxRestartAssessment::Candidate {
            exit_code: 17,
            blocker: Some(SandboxRestartBlocker::StartupReconciliationUnavailable),
        }
    );
    assert_eq!(inspected.cleanup, SandboxCleanupObservation::Retained);
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
