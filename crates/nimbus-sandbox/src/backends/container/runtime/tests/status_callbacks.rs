//! External service-runner status callback finality and monotonicity proofs.

use super::*;
use std::sync::mpsc;
use std::time::Duration;

fn mark_prepared_service_runner(manifest: &mut ContainerSandboxManifest) {
    manifest.lifecycle_coordinator = ContainerLifecycleCoordinator::PreparedServiceRunner;
}

#[test]
fn plan_only_stopped_callback_publishes_terminal_finality_and_replays_byte_exact() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = sample_plan_only_backend(temp_dir.path());
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("plan-only-status-terminal-finality"),
            None,
            None,
        )
        .expect("prepared workload should plan")
        .manifest;
    mark_prepared_service_runner(&mut manifest);
    backend
        .write_manifest(&manifest)
        .expect("prepared manifest should become durable");

    let stopped = backend
        .mark_plan_only_service_workload_stopped(&manifest.handle.id)
        .expect("first stopped callback should converge")
        .expect("prepared workload should remain observable");
    assert_eq!(stopped.status, SandboxStatus::Stopped);
    let terminal = backend
        .read_manifest(&manifest.handle.id)
        .expect("terminal manifest should inspect")
        .expect("terminal manifest should remain durable");
    assert!(
        terminal.shutdown_requested,
        "a stopped callback must publish terminal shutdown intent"
    );
    assert_eq!(
        terminal.last_exit_code,
        Some(0),
        "an acknowledged service-runner stop must publish its successful terminal outcome"
    );
    assert!(
        terminal.has_terminal_network_finality(),
        "the callback may return Stopped only after every network fence reaches finality"
    );
    let before = std::fs::read(&terminal.conmon_layout.manifest_path)
        .expect("terminal manifest bytes should read");

    let replayed = backend
        .mark_plan_only_service_workload_stopped(&manifest.handle.id)
        .expect("terminal callback replay should be idempotent")
        .expect("terminal workload should remain observable");
    assert_eq!(replayed.status, SandboxStatus::Stopped);
    let inspected = backend
        .inspect_sync(&manifest.handle.id)
        .expect("authenticated terminal cancellation should inspect")
        .expect("terminal workload should remain observable");
    assert_eq!(inspected.status, SandboxStatus::Stopped);
    assert_eq!(
        std::fs::read(&terminal.conmon_layout.manifest_path)
            .expect("terminal manifest bytes should reread"),
        before,
        "callback replay and inspection must not rewrite terminal authority"
    );
}

#[test]
fn delayed_ready_callback_preserves_published_terminal_execute_bytes() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = sample_plan_only_backend(temp_dir.path());
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("execute-status-terminal-monotonicity"),
            None,
            None,
        )
        .expect("prepared workload should plan")
        .manifest;
    mark_prepared_service_runner(&mut manifest);
    backend
        .write_manifest(&manifest)
        .expect("prepared manifest should become durable");
    let handoff = super::super::runner::persist_runner_execution_ownership(&backend, &mut manifest)
        .expect("runner should claim execution");
    super::super::runner::mark_runner_effects_started(&manifest, &handoff)
        .expect("effect boundary should become durable");
    publish_present_runner_lifecycle(&manifest, &handoff);
    drop(handoff);

    manifest.shutdown_requested = true;
    manifest.last_exit_code = Some(0);
    manifest.next_restart_at_millis = None;
    manifest.launch_reservation_claim = None;
    manifest.launch_artifact = None;
    manifest.network_cleanup_complete = true;
    synchronize_handle_status(&mut manifest, SandboxStatus::Stopped);
    assert!(manifest.has_terminal_network_finality());
    backend
        .write_manifest(&manifest)
        .expect("terminal lifecycle receipt should become durable");
    let before = std::fs::read(&manifest.conmon_layout.manifest_path)
        .expect("terminal manifest bytes should read");

    for _ in 0..2 {
        let observed = backend
            .refresh_plan_only_service_workload_status(&manifest.handle.id, SandboxStatus::Ready)
            .expect("a delayed callback should become an idempotent terminal observation")
            .expect("terminal workload should remain observable");
        assert_eq!(
            observed.status,
            SandboxStatus::Stopped,
            "a delayed Ready observation must not resurrect a terminal workload"
        );
        assert_eq!(
            std::fs::read(&manifest.conmon_layout.manifest_path)
                .expect("terminal manifest bytes should reread"),
            before,
            "a delayed callback must preserve exact terminal authority"
        );
    }
}

#[test]
fn delayed_ready_callback_preserves_cleanup_pending_execute_bytes() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = sample_plan_only_backend(temp_dir.path());
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("execute-status-cleanup-pending-monotonicity"),
            None,
            None,
        )
        .expect("prepared workload should plan")
        .manifest;
    mark_prepared_service_runner(&mut manifest);
    backend
        .write_manifest(&manifest)
        .expect("prepared manifest should become durable");
    let handoff = super::super::runner::persist_runner_execution_ownership(&backend, &mut manifest)
        .expect("runner should claim execution");
    super::super::runner::mark_runner_effects_started(&manifest, &handoff)
        .expect("effect boundary should become durable");
    publish_present_runner_lifecycle(&manifest, &handoff);
    drop(handoff);

    manifest.shutdown_requested = true;
    manifest.last_exit_code = Some(0);
    manifest.next_restart_at_millis = None;
    manifest.network_cleanup_complete = false;
    synchronize_handle_status(&mut manifest, SandboxStatus::Stopping);
    assert!(!manifest.has_terminal_network_finality());
    backend
        .write_manifest(&manifest)
        .expect("cleanup-pending receipt should become durable");
    let before = std::fs::read(&manifest.conmon_layout.manifest_path)
        .expect("cleanup-pending manifest bytes should read");

    let observed = backend
        .refresh_plan_only_service_workload_status(&manifest.handle.id, SandboxStatus::Ready)
        .expect("a delayed callback should preserve canonical shutdown progress")
        .expect("cleanup-pending workload should remain observable");
    assert_eq!(
        observed.status,
        SandboxStatus::Stopping,
        "a delayed Ready observation must not erase cleanup-pending shutdown"
    );
    assert_eq!(
        std::fs::read(&manifest.conmon_layout.manifest_path)
            .expect("cleanup-pending manifest bytes should reread"),
        before,
        "a delayed callback must preserve exact cleanup authority"
    );
}

#[test]
fn execute_status_callback_rejects_a_manifest_changed_while_waiting_for_lifecycle_authority() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let lock_probe =
        super::super::runner::RunnerLifecycleLockTestProbe::new(Duration::from_secs(2));
    let backend = sample_plan_only_backend(temp_dir.path())
        .with_runner_lifecycle_lock_test_probe(lock_probe.clone());
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("execute-status-stale-snapshot-fence"),
            None,
            None,
        )
        .expect("prepared workload should plan")
        .manifest;
    mark_prepared_service_runner(&mut manifest);
    backend
        .write_manifest(&manifest)
        .expect("prepared manifest should become durable");
    let handoff = super::super::runner::persist_runner_execution_ownership(&backend, &mut manifest)
        .expect("runner should claim execution");
    super::super::runner::mark_runner_effects_started(&manifest, &handoff)
        .expect("effect boundary should become durable");
    publish_present_runner_lifecycle(&manifest, &handoff);
    drop(handoff);

    let lifecycle = super::super::runner::lock_execute_lifecycle(&manifest)
        .expect("ordinary lifecycle owner should acquire the shared lock");
    let contender = backend.clone();
    let id = manifest.handle.id.clone();
    let (result_tx, result_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        result_tx
            .send(contender.refresh_plan_only_service_workload_status(&id, SandboxStatus::Ready))
            .expect("callback result should send");
    });
    assert!(
        lock_probe.wait_until_contended(),
        "status callback must reach the actual Execute lifecycle-lock boundary"
    );

    let mut changed = manifest.clone();
    changed.spec.egress = nimbus_egress::EgressPolicy::new([nimbus_egress::EgressRule::new(
        "concurrent-policy",
        nimbus_egress::EgressProtocol::Https,
        "example.com",
        443,
    )]);
    backend
        .write_manifest(&changed)
        .expect("concurrent lifecycle owner should publish the newer manifest");
    let before = std::fs::read(&changed.conmon_layout.manifest_path)
        .expect("newer manifest bytes should read");
    drop(lifecycle);

    let error = result_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("callback should resume after lifecycle release")
        .expect_err("a stale callback snapshot must remain fenced");
    worker.join().expect("callback worker should join");
    assert!(
        error.to_string().contains("changed durable manifest"),
        "the rejection must identify the stale lifecycle snapshot: {error}"
    );
    assert_eq!(
        std::fs::read(&changed.conmon_layout.manifest_path)
            .expect("newer manifest bytes should reread"),
        before,
        "a stale callback must not overwrite a newer durable manifest"
    );
}

#[test]
fn plan_only_failed_callback_cancels_and_releases_prepared_authority() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = sample_plan_only_backend(temp_dir.path());
    let prepared = backend
        .prepare_plan_only_service_workload(sample_spec())
        .expect("service workload should prepare");
    let pointer_path = prepared
        .bundle_dir
        .join(super::super::runner::RUNNER_MANIFEST_POINTER_FILE);
    assert!(pointer_path.exists(), "prepared runner pointer must exist");

    let failed = backend
        .refresh_plan_only_service_workload_status(&prepared.handle.id, SandboxStatus::Failed)
        .expect("failed callback should converge cancellation")
        .expect("failed workload should remain observable");
    assert_eq!(failed.status, SandboxStatus::Failed);
    let terminal = backend
        .read_manifest(&prepared.handle.id)
        .expect("terminal manifest should inspect")
        .expect("terminal manifest should remain durable");
    assert!(terminal.shutdown_requested);
    assert_eq!(
        terminal.last_exit_code, None,
        "node failure without an exit receipt must not invent a numeric outcome"
    );
    assert!(terminal.has_terminal_network_finality());
    assert!(
        !pointer_path.exists(),
        "terminal cancellation must remove the exact runner pointer"
    );

    let mut replay = terminal.clone();
    let error = super::super::runner::persist_runner_execution_ownership(&backend, &mut replay)
        .expect_err("a durable failed cancellation must fence a later Execute winner");
    assert!(
        error.to_string().contains("Cancel"),
        "the rejection must name the durable cancellation winner: {error}"
    );
}

#[test]
fn stopped_retry_preserves_nonzero_execute_outcome_as_failed() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let mut planner_config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    planner_config.netavark_path = PathBuf::from("/usr/bin/true");
    let planner = ContainerSandboxBackend::new(planner_config);
    let mut observer_config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    observer_config.start_mode = ContainerStartMode::PlanOnly;
    observer_config.netavark_path = PathBuf::from("/usr/bin/true");
    let backend = ContainerSandboxBackend::new(observer_config);
    let mut manifest = planner
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("execute-status-preserve-nonzero-exit"),
            None,
            None,
        )
        .expect("prepared workload should plan")
        .manifest;
    mark_prepared_service_runner(&mut manifest);
    mark_runtime_absent_for_cleanup(&mut manifest);
    backend
        .write_manifest(&manifest)
        .expect("prepared manifest should become durable");
    let handoff = super::super::runner::persist_runner_execution_ownership(&backend, &mut manifest)
        .expect("runner should claim execution");
    super::super::runner::mark_runner_effects_started(&manifest, &handoff)
        .expect("effect boundary should become durable");
    let claim = manifest
        .launch_reservation_claim
        .clone()
        .expect("runner launch should retain its coordinator claim");
    backend
        .segment_allocator
        .adopt_reserved_attachment(
            &manifest.spec.tenant_id,
            &default_network_attachment_id(&manifest.handle.id),
            &claim,
        )
        .expect("runner fixture should model the post-adoption boundary");
    backend
        .write_manifest(&manifest)
        .expect("post-adoption runner manifest should be durable");
    publish_present_runner_lifecycle(&manifest, &handoff);
    drop(handoff);

    manifest.shutdown_requested = true;
    manifest.last_exit_code = Some(23);
    manifest.next_restart_at_millis = None;
    manifest.network_cleanup_complete = false;
    synchronize_handle_status(&mut manifest, SandboxStatus::Stopping);
    backend
        .write_manifest(&manifest)
        .expect("failed cleanup checkpoint should become durable");

    let failed = backend
        .mark_plan_only_service_workload_stopped(&manifest.handle.id)
        .expect("stopped retry should converge exact cleanup")
        .expect("failed workload should remain observable");
    assert_eq!(
        failed.status,
        SandboxStatus::Failed,
        "a prior nonzero exit must remain the canonical terminal outcome"
    );
    let terminal = backend
        .read_manifest(&manifest.handle.id)
        .expect("terminal manifest should inspect")
        .expect("terminal manifest should remain durable");
    assert_eq!(terminal.last_exit_code, Some(23));
    assert!(terminal.has_terminal_network_finality());
}

#[test]
fn cancelled_callback_cleanup_reopens_and_converges_exact_authority() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let state_root = temp_dir.path().join("state");
    let bundle_root = temp_dir.path().join("bundles");
    let tenant = sample_spec().tenant_id;
    let failing_allocator = Arc::new(
        RecordingSegmentAllocator::new(tenant.clone(), "10.83.0.0/24", 83)
            .with_release_reserved_failure("injected callback cleanup failure"),
    );
    let injected: Arc<OciSegmentAllocator> = failing_allocator;
    let first = ContainerSandboxBackend::with_segment_allocator(
        ContainerSandboxBackendConfig::plan_only(&bundle_root, &state_root),
        injected,
    )
    .with_runner_handoff_failure(RunnerHandoffFailure::Pointer);

    let error = first
        .prepare_plan_only_service_workload(sample_spec())
        .expect_err("pointer failure should retain the durable Cancel cleanup checkpoint");
    assert!(
        error.to_string().contains("runner pointer")
            && error
                .to_string()
                .contains("injected callback cleanup failure"),
        "both the primary handoff failure and retained cleanup must remain visible: {error}"
    );
    let manifest_path = crate::artifact_paths::all_manifest_paths(&state_root)
        .expect("failed workload manifest should enumerate")
        .into_iter()
        .next()
        .expect("failed workload manifest should remain durable");
    let fenced: ContainerSandboxManifest = serde_json::from_slice(
        &std::fs::read(&manifest_path).expect("fenced manifest should read"),
    )
    .expect("fenced manifest should parse");
    assert!(fenced.shutdown_requested);
    assert_eq!(fenced.status, SandboxStatus::Stopping);
    assert!(fenced.launch_reservation_claim.is_some());

    let recovery_allocator = Arc::new(RecordingSegmentAllocator::new(tenant, "10.83.0.0/24", 83));
    let recovery_injected: Arc<OciSegmentAllocator> = recovery_allocator;
    let recovery = ContainerSandboxBackend::with_segment_allocator(
        ContainerSandboxBackendConfig::plan_only(&bundle_root, &state_root),
        recovery_injected,
    );
    let stopped = recovery
        .mark_plan_only_service_workload_stopped(&fenced.handle.id)
        .expect("reopened callback must resume the exact durable Cancel")
        .expect("recovered workload should remain observable");
    assert_eq!(stopped.status, SandboxStatus::Stopped);
    let terminal = recovery
        .read_manifest(&fenced.handle.id)
        .expect("terminal manifest should inspect")
        .expect("terminal manifest should remain durable");
    assert_eq!(terminal.last_exit_code, Some(0));
    assert!(terminal.has_terminal_network_finality());
    recovery
        .mark_plan_only_service_workload_stopped(&fenced.handle.id)
        .expect("terminal callback replay should remain idempotent");
}
