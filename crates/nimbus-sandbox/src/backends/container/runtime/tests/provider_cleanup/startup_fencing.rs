//! Startup-reconciliation admission fences preserve existing-workload cleanup.

use super::*;
use crate::inspection::{
    SandboxCleanupObservation, SandboxExecutionObservation, SandboxRestartAssessment,
    SandboxRestartBlocker,
};

fn inject_startup_reconciliation_failure(backend: &mut ContainerSandboxBackend) {
    backend.startup_reconciliation_error = Some(Arc::<str>::from(
        "injected retained startup reconciliation failure",
    ));
}

#[test]
fn startup_reconciliation_failure_fences_direct_initial_launch_before_effects() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let spec = sample_spec();
    let recorder = Arc::new(RecordingSegmentAllocator::new(
        spec.tenant_id.clone(),
        "127.0.0.0/24",
        73,
    ));
    let injected: Arc<OciSegmentAllocator> = recorder.clone();
    let mut backend = ContainerSandboxBackend::with_segment_allocator(
        ContainerSandboxBackendConfig::under_root(temp_dir.path()),
        injected,
    );
    let mut manifest = backend
        .plan_start_with_id(&spec, &SandboxId::new("launch-startup-fence"), None, None)
        .expect("initial planning should reserve exact launch authority")
        .manifest;
    let authority_path = nimbus_network::LocalNetworkStateStore::authority_path_for(
        &backend.config.network_state_root,
    );
    let authority_before =
        std::fs::read(&authority_path).expect("reserved authority should be durable");
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
            .contains("refuses new durable work because startup reconciliation did not complete"),
        "the retained startup diagnostic must remain primary: {error}"
    );
    assert_eq!(
        serde_json::to_vec(&manifest).expect("fenced manifest should serialize"),
        manifest_before,
        "the launch fence must precede in-memory lifecycle mutation"
    );
    assert_eq!(
        std::fs::read(&authority_path).expect("reserved authority should remain readable"),
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
fn startup_reconciliation_failure_allows_exact_plan_only_status_cleanup() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let mut backend = sample_plan_only_backend(temp_dir.path());
    let spec = sample_spec();
    let sandbox_id = SandboxId::new("plan-only-startup-fence");
    let artifact_path = crate::artifact_paths::rootfs_root(
        &backend.config.workload_state_root,
        &spec.tenant_id,
        &sandbox_id,
    )
    .join(sandbox_id.as_str());
    let rootfs_path = artifact_path.join("rootfs");
    std::fs::create_dir_all(&rootfs_path).expect("rootfs fixture should create");
    let sentinel = rootfs_path.join("sentinel");
    std::fs::write(&sentinel, b"owned-rootfs").expect("rootfs sentinel should persist");
    let mut manifest = backend
        .plan_start_with_id(
            &spec,
            &sandbox_id,
            None,
            Some(sample_rootfs_artifact(rootfs_path)),
        )
        .expect("plan-only manifest should lower")
        .manifest;
    manifest.lifecycle_coordinator = ContainerLifecycleCoordinator::PreparedServiceRunner;
    backend
        .write_manifest(&manifest)
        .expect("baseline manifest should persist");
    inject_startup_reconciliation_failure(&mut backend);

    let stopped = backend
        .update_plan_only_service_workload_status(&manifest.handle.id, SandboxStatus::Stopped)
        .expect("an existing workload must retain exact cleanup authority")
        .expect("the stopped workload should remain inspectable");
    assert_eq!(stopped.status, SandboxStatus::Stopped);
    assert!(
        !sentinel.exists(),
        "exact status cleanup should remove its owned launch artifact"
    );
    let persisted = backend
        .read_manifest(&manifest.handle.id)
        .expect("terminal manifest should inspect")
        .expect("terminal manifest should remain durable");
    assert!(
        persisted.has_terminal_network_finality(),
        "cleanup must durably publish exact terminal network finality"
    );
    assert!(persisted.launch_artifact.is_none());
}

#[test]
fn startup_reconciliation_failure_allows_exact_explicit_stop() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let mut backend =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(temp_dir.path()));
    let manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp(
                "http",
                unused_loopback_port(),
                8080,
            )),
            &SandboxId::new("stop-startup-fence"),
            None,
            None,
        )
        .expect("execute manifest should reserve exact launch authority")
        .manifest;
    backend
        .write_manifest(&manifest)
        .expect("baseline manifest should persist");
    let authority_path = nimbus_network::LocalNetworkStateStore::authority_path_for(
        &backend.config.network_state_root,
    );
    let authority_before =
        std::fs::read(&authority_path).expect("launch authority should be durable");
    inject_startup_reconciliation_failure(&mut backend);

    backend
        .stop_sync(&manifest.handle.id)
        .expect("an existing workload must retain exact stop authority");
    assert_ne!(
        std::fs::read(&authority_path).expect("launch authority should remain readable"),
        authority_before,
        "successful stop must durably release exact launch authority"
    );
    let persisted = backend
        .read_manifest(&manifest.handle.id)
        .expect("terminal manifest should inspect")
        .expect("terminal manifest should remain durable");
    assert_eq!(persisted.status, SandboxStatus::Stopped);
    assert!(
        persisted.has_terminal_network_finality(),
        "explicit stop must durably publish exact cleanup finality"
    );
    assert!(persisted.launch_reservation_claim.is_none());
}

#[test]
fn startup_reconciliation_failure_keeps_natural_exit_read_only_until_explicit_stop() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let spec = sample_spec();
    let id = SandboxId::new("natural-exit-startup-fence");
    let recorder = Arc::new(RecordingSegmentAllocator::new(
        spec.tenant_id.clone(),
        "127.0.0.0/24",
        74,
    ));
    let injected: Arc<OciSegmentAllocator> = recorder;
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    let proxy_port = unused_loopback_port();
    config.published_port_range = proxy_port..=proxy_port;
    let mut backend = ContainerSandboxBackend::with_segment_allocator(config, injected);
    let mut manifest = backend
        .plan_start_with_id(&spec, &id, None, None)
        .expect("execute manifest should reserve exact authority")
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
    let readiness_before = backend
        .egress_proxies
        .readiness(&manifest.spec.tenant_id, &manifest.handle.id)
        .expect("baseline readiness should inspect")
        .expect("baseline PEP should remain registered");
    let authority_path = nimbus_network::LocalNetworkStateStore::authority_path_for(
        &backend.config.network_state_root,
    );
    let authority_before =
        std::fs::read(&authority_path).expect("active authority should be durable");
    inject_startup_reconciliation_failure(&mut backend);

    let inspected = backend
        .inspect_sync(&id)
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
        std::fs::read(&authority_path).expect("active authority should remain readable"),
        authority_before,
        "natural-exit inspection must not release exact network authority"
    );
    assert!(
        backend
            .egress_proxies
            .readiness(&manifest.spec.tenant_id, &manifest.handle.id)
            .expect("post-inspection readiness should inspect")
            .is_some(),
        "natural-exit inspection must not remove the exact PEP registration"
    );
    assert!(
        readiness_before.is_ready(),
        "the fixture must begin with a live PEP before cleanup"
    );
    let persisted = backend
        .read_manifest(&id)
        .expect("retained manifest should inspect")
        .expect("retained manifest should remain durable");
    assert_eq!(persisted, manifest);

    backend
        .stop_sync(&id)
        .expect("explicit stop must retain cleanup authority despite startup failure");
    assert_ne!(
        std::fs::read(&authority_path).expect("released authority should remain readable"),
        authority_before,
        "explicit stop must durably release exact network authority"
    );
    assert!(
        backend
            .egress_proxies
            .readiness(&manifest.spec.tenant_id, &manifest.handle.id)
            .expect("post-stop readiness should inspect")
            .is_none(),
        "explicit stop must remove the exact PEP registration"
    );
    let persisted = backend
        .read_manifest(&id)
        .expect("terminal manifest should inspect")
        .expect("terminal manifest should remain durable");
    assert_eq!(persisted.status, SandboxStatus::Stopped);
    assert!(
        persisted.has_terminal_network_finality(),
        "explicit stop must publish terminal status only after exact cleanup"
    );
}

#[test]
fn startup_reconciliation_failure_keeps_restart_eligible_inspection_read_only() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let spec = sample_spec()
        .with_restart_policy(crate::spec::SandboxRestartPolicy::OnFailure { max_restarts: 1 });
    let id = SandboxId::new("restart-startup-fence");
    let recorder = Arc::new(RecordingSegmentAllocator::new(
        spec.tenant_id.clone(),
        "127.0.0.0/24",
        75,
    ));
    let injected: Arc<OciSegmentAllocator> = recorder.clone();
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    let proxy_port = unused_loopback_port();
    config.published_port_range = proxy_port..=proxy_port;
    let mut backend = ContainerSandboxBackend::with_segment_allocator(config, injected);
    let mut manifest = backend
        .plan_start_with_id(&spec, &id, None, None)
        .expect("execute manifest should reserve exact authority")
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
        .expect("restart-eligible exit status should persist");
    backend
        .write_manifest(&manifest)
        .expect("running manifest should persist before inspection");
    let manifest_before = std::fs::read(&manifest.conmon_layout.manifest_path)
        .expect("canonical manifest bytes should read");
    let authority_path = nimbus_network::LocalNetworkStateStore::authority_path_for(
        &backend.config.network_state_root,
    );
    let authority_before =
        std::fs::read(&authority_path).expect("active authority should be durable");
    let operations_before = recorder.operations();
    inject_startup_reconciliation_failure(&mut backend);

    let inspected = backend
        .inspect_sync(&id)
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
            completed_restarts: 0,
            retry_delay_millis: 1_000,
            persisted_not_before_millis: None,
            blocker: Some(SandboxRestartBlocker::StartupReconciliationUnavailable),
        }
    );
    assert_eq!(inspected.cleanup, SandboxCleanupObservation::Retained);
    assert_eq!(
        std::fs::read(&manifest.conmon_layout.manifest_path)
            .expect("canonical manifest bytes should remain readable"),
        manifest_before,
        "startup-fenced inspection must not publish a restart decision"
    );
    assert_eq!(
        std::fs::read(&authority_path).expect("active authority should remain readable"),
        authority_before,
        "startup-fenced inspection must not mutate network authority"
    );
    assert_eq!(
        recorder.operations(),
        operations_before,
        "startup-fenced inspection must not invoke provider or segment effects"
    );
}
