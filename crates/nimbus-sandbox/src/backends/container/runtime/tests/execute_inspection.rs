//! Execute inspection lifecycle-lock proofs.

use super::*;

#[test]
fn execute_inspection_waits_for_lifecycle_owner_and_rejects_stale_manifest() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let lock_probe =
        super::super::runner::RunnerLifecycleLockTestProbe::new(Duration::from_secs(2));
    let backend = sample_plan_only_backend(temp_dir.path())
        .with_runner_lifecycle_lock_test_probe(lock_probe.clone());
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("execute-inspection-lifecycle-lock"),
            None,
            None,
        )
        .expect("runner fixture should plan")
        .manifest;
    manifest.lifecycle_coordinator = ContainerLifecycleCoordinator::PreparedServiceRunner;
    backend
        .write_manifest(&manifest)
        .expect("prepared manifest should be durable");
    let handoff = super::super::runner::persist_runner_execution_ownership(&backend, &mut manifest)
        .expect("runner should claim execution");
    super::super::runner::mark_runner_effects_started(&manifest, &handoff)
        .expect("runner effect boundary should become durable");
    publish_present_runner_lifecycle(&manifest, &handoff);
    drop(handoff);

    let lifecycle = super::super::runner::lock_execute_lifecycle(&manifest)
        .expect("coordinator should own the shared lifecycle lock");
    let inspect_backend = backend.clone();
    let inspect_id = manifest.handle.id.clone();
    let inspect_thread = std::thread::spawn(move || inspect_backend.inspect_sync(&inspect_id));

    if !lock_probe.wait_until_contended() {
        drop(lifecycle);
        let inspect_result = inspect_thread
            .join()
            .expect("unfenced inspection thread should still join");
        panic!(
            "Execute inspection must acquire the shared lifecycle lock before any mutable \
             fallthrough; inspection completed without contention as {inspect_result:?}"
        );
    }

    let mut withdrawn = manifest.clone();
    withdrawn.shutdown_requested = true;
    withdrawn.last_exit_code = Some(0);
    withdrawn.launch_reservation_claim = None;
    withdrawn.launch_artifact = None;
    withdrawn.network_cleanup_complete = true;
    synchronize_handle_status(&mut withdrawn, SandboxStatus::Stopped);
    backend
        .write_manifest(&withdrawn)
        .expect("coordinator withdrawal should persist under the lifecycle lock");
    drop(lifecycle);

    let error = inspect_thread
        .join()
        .expect("inspection thread should join")
        .expect_err("stale inspection must reject the changed canonical manifest");
    assert!(
        error.to_string().contains("changed durable manifest"),
        "stale inspection must name the canonical-reread fence: {error}"
    );
    assert_eq!(
        backend
            .read_manifest(&withdrawn.handle.id)
            .expect("terminal manifest should inspect")
            .expect("terminal manifest should remain durable"),
        withdrawn,
        "stale inspection must not overwrite the coordinator's terminal state"
    );
}

#[test]
fn detect_runtime_status_marks_stale_pidfiles_as_failed() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let backend =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(temp_dir.path()));
    let mut manifest = backend
        .plan_start_with_id(&sample_spec(), &sandbox_id(), None, None)
        .expect("plan should lower")
        .manifest;
    manifest.conmon_launch.state_command = CommandSpec::new("/bin/sh").args([
        "-c".to_owned(),
        format!(
            "printf '%s\\n' 'container `{0}` does not exist: open `/run/crun/{0}/status`: No such file or directory' >&2; exit 1",
            manifest.handle.id
        ),
    ]);
    std::fs::write(&manifest.conmon_layout.pidfile, "999999\n").expect("pidfile should write");

    assert_eq!(
        backend
            .detect_runtime_status(&manifest)
            .expect("status should resolve"),
        SandboxStatus::Failed
    );
}
