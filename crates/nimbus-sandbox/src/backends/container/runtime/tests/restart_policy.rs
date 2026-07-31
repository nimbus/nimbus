//! Container restart-policy decisions and inspection boundaries.

use super::*;

#[test]
fn restart_decision_keeps_failed_container_starting_until_backoff_elapses() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let backend =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(temp_dir.path()));
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec().with_restart_policy(SandboxRestartPolicy::OnFailure { max_restarts: 1 }),
            &sandbox_id(),
            None,
            None,
        )
        .expect("plan should lower")
        .manifest;
    std::fs::write(&manifest.conmon_layout.exit_status_file, "42\n")
        .expect("exit status should write");
    manifest.next_restart_at_millis = Some(1_500);

    let decision =
        mark_restart_decision_after_exit(&mut manifest, 1_000).expect("restart should evaluate");

    assert_eq!(decision, ContainerRestartDecision::WaitingForBackoff);
    assert_eq!(manifest.last_exit_code, Some(42));
    assert_eq!(manifest.restart_count, 0);
    assert_eq!(manifest.next_restart_at_millis, Some(1_500));
    assert_eq!(manifest.status, SandboxStatus::Starting);
    assert_eq!(manifest.handle.status, SandboxStatus::Starting);
}

#[test]
fn restart_decision_counts_due_failed_container_restart() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let backend =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(temp_dir.path()));
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec().with_restart_policy(SandboxRestartPolicy::OnFailure { max_restarts: 2 }),
            &sandbox_id(),
            None,
            None,
        )
        .expect("plan should lower")
        .manifest;
    std::fs::write(&manifest.conmon_layout.exit_status_file, "42\n")
        .expect("exit status should write");
    manifest.next_restart_at_millis = Some(0);

    let decision =
        mark_restart_decision_after_exit(&mut manifest, 1_000).expect("restart should evaluate");

    assert_eq!(decision, ContainerRestartDecision::RestartNow);
    assert_eq!(manifest.last_exit_code, Some(42));
    assert_eq!(manifest.restart_count, 1);
    assert_eq!(manifest.next_restart_at_millis, None);
    assert_eq!(manifest.status, SandboxStatus::Starting);
    assert_eq!(manifest.handle.status, SandboxStatus::Starting);
}

/// NNC0.6a fail-before baseline for NNCF20. Inspection owns a stale manifest
/// copy, reaches the provider-launch entry through restart policy, and parks.
/// The coordinator then durably withdraws the workload before releasing that
/// launch. No readiness outcome can satisfy this side-effect assertion.
#[test]
#[ignore = "NNC0.6a expected red until NNC5.6/NNC6.4a make inspect side-effect-free and fence restart"]
fn nnc0_6a_container_inspect_must_not_restart_after_withdrawal() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let restart_probe = RestartLaunchTestProbe::new(Duration::from_secs(1));
    let backend =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(temp_dir.path()))
            .with_restart_launch_test_probe(restart_probe.clone());
    let sandbox_id = SandboxId::new("nnc0-6a-container");
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec().with_restart_policy(SandboxRestartPolicy::OnFailure { max_restarts: 1 }),
            &sandbox_id,
            None,
            None,
        )
        .expect("execute manifest should plan")
        .manifest;
    manifest.conmon_launch.delete_command = CommandSpec::new("/usr/bin/true");
    manifest.next_restart_at_millis = Some(0);
    std::fs::write(&manifest.conmon_layout.exit_status_file, "42\n")
        .expect("failed exit should persist");
    backend
        .write_manifest(&manifest)
        .expect("restart-eligible manifest should persist");

    let inspect_backend = backend.clone();
    let inspect_id = sandbox_id.clone();
    let inspect_thread = thread::spawn(move || inspect_backend.inspect_sync(&inspect_id));
    if !restart_probe.wait_until_entered() {
        let inspect_result = inspect_thread
            .join()
            .expect("inspect thread should join after a missing barrier");
        panic!(
            "inspect must reach the provider-launch barrier through restart policy; \
             inspect completed instead with {inspect_result:?}"
        );
    }

    let mut withdrawn = manifest;
    withdrawn.shutdown_requested = true;
    withdrawn.next_restart_at_millis = None;
    withdrawn.status = SandboxStatus::Stopped;
    withdrawn.handle.status = SandboxStatus::Stopped;
    withdrawn.handle.published_endpoints.clear();
    backend
        .write_manifest(&withdrawn)
        .expect("coordinator withdrawal should persist before launch release");

    restart_probe.release();
    let inspected = inspect_thread
        .join()
        .expect("inspect thread should join")
        .expect("current inspect restart should complete through the test provider")
        .expect("manifest should remain inspectable");
    assert_eq!(
        inspected.status,
        SandboxStatus::Starting,
        "precondition: stale inspection currently reactivates the withdrawn manifest"
    );

    assert_eq!(
        restart_probe.effect_count(),
        0,
        "NNCF20: inspect must be side-effect-free; a withdrawal/fence persisted before \
         release must veto the stale container restart provider effect"
    );
}
