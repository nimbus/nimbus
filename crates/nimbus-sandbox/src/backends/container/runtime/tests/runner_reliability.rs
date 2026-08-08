use super::*;

use std::cell::Cell;
use std::sync::mpsc;
use std::time::Duration;

fn mark_prepared_service_runner(manifest: &mut ContainerSandboxManifest) {
    manifest.lifecycle_coordinator = ContainerLifecycleCoordinator::PreparedServiceRunner;
}

#[test]
fn runner_exit_finalization_waits_for_lifecycle_owner_and_preserves_terminal_state() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let lock_probe =
        super::super::runner::RunnerLifecycleLockTestProbe::new(Duration::from_secs(2));
    let backend = sample_plan_only_backend(temp_dir.path())
        .with_runner_lifecycle_lock_test_probe(lock_probe.clone());
    let mut runner_manifest = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("runner-finalization-lifecycle-lock"),
            None,
            None,
        )
        .expect("runner fixture should plan")
        .manifest;
    mark_prepared_service_runner(&mut runner_manifest);
    backend
        .write_manifest(&runner_manifest)
        .expect("prepared manifest should be durable");
    let handoff =
        super::super::runner::persist_runner_execution_ownership(&backend, &mut runner_manifest)
            .expect("runner should claim execution");
    super::super::runner::mark_runner_effects_started(&runner_manifest, &handoff)
        .expect("runner effect boundary should become durable");
    publish_present_runner_lifecycle(&runner_manifest, &handoff);
    drop(handoff);

    let lifecycle = super::super::runner::lock_execute_lifecycle(&runner_manifest)
        .expect("ordinary lifecycle owner should acquire the shared lock");
    let mut stopped = runner_manifest.clone();
    stopped.shutdown_requested = true;
    stopped.last_exit_code = Some(41);
    stopped.launch_reservation_claim = None;
    stopped.launch_artifact = None;
    stopped.network_cleanup_complete = true;
    synchronize_handle_status(&mut stopped, SandboxStatus::Stopped);
    backend
        .write_manifest(&stopped)
        .expect("ordinary stop should persist its terminal result under the lock");

    let contender = backend.clone();
    let (result_tx, result_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let result =
            super::super::runner::finalize_runner_exit(&contender, &mut runner_manifest, 0);
        result_tx
            .send((result, runner_manifest))
            .expect("finalizer result should send");
    });
    assert!(
        lock_probe.wait_until_contended(),
        "runner finalizer must reach the actual WouldBlock acquisition boundary"
    );
    drop(lifecycle);
    let (result, finalized) = result_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("runner finalization should resume after lifecycle release");
    worker.join().expect("runner finalizer should join");
    result.expect("a durable terminal stop should make runner finalization idempotent");

    assert_eq!(
        finalized, stopped,
        "runner finalization must adopt the current terminal manifest without stale cleanup"
    );
    let persisted = backend
        .read_manifest(&stopped.handle.id)
        .expect("terminal manifest should inspect")
        .expect("terminal manifest should remain durable");
    assert_eq!(
        persisted, stopped,
        "the later exit observation must not overwrite the explicit stop result"
    );
}

#[test]
fn explicit_stop_rejects_a_manifest_changed_while_waiting_for_lifecycle_authority() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let lock_probe =
        super::super::runner::RunnerLifecycleLockTestProbe::new(Duration::from_secs(2));
    let backend = sample_plan_only_backend(temp_dir.path())
        .with_runner_lifecycle_lock_test_probe(lock_probe.clone());
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("execute-stop-stale-snapshot-fence"),
            None,
            None,
        )
        .expect("runner fixture should plan")
        .manifest;
    mark_prepared_service_runner(&mut manifest);
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
        .expect("ordinary lifecycle owner should acquire the shared lock");
    let contender = backend.clone();
    let id = manifest.handle.id.clone();
    let (result_tx, result_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        result_tx
            .send(contender.stop_sync(&id))
            .expect("stop result should send");
    });
    assert!(
        lock_probe.wait_until_contended(),
        "stop must reach the actual Execute lifecycle-lock boundary"
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
        .expect("stop should resume after lifecycle release")
        .expect_err("a stale stop snapshot must remain fenced");
    worker.join().expect("stop worker should join");
    assert!(
        error.to_string().contains("changed durable manifest"),
        "the rejection must identify the stale lifecycle snapshot: {error}"
    );
    assert_eq!(
        std::fs::read(&changed.conmon_layout.manifest_path)
            .expect("newer manifest bytes should reread"),
        before,
        "a stale stop must not overwrite or clean up a newer durable manifest"
    );
}

#[test]
fn runner_finalization_lock_contention_returns_with_fenced_timeout() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = sample_plan_only_backend(temp_dir.path());
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("runner-finalization-lock-timeout"),
            None,
            None,
        )
        .expect("runner fixture should plan")
        .manifest;
    mark_prepared_service_runner(&mut manifest);
    manifest.start_mode = ContainerStartMode::Execute;
    backend
        .write_manifest(&manifest)
        .expect("Execute manifest should be durable");
    let lifecycle = super::super::runner::lock_execute_lifecycle(&manifest)
        .expect("ordinary lifecycle owner should hold the shared lock");

    let started = std::time::Instant::now();
    let error = super::super::runner::converge_runner_lifecycle_lock_with_timeout_for_test(
        &backend,
        &manifest,
        Duration::from_millis(25),
    )
    .expect_err("runner finalization must not wait forever behind another lifecycle owner");
    let elapsed = started.elapsed();
    drop(lifecycle);

    assert!(
        elapsed < Duration::from_millis(500),
        "the injected bounded acquisition should return promptly, took {elapsed:?}"
    );
    assert!(
        error.to_string().contains("timed out acquiring")
            && error
                .to_string()
                .contains("execution and cancellation remain fenced"),
        "the timeout must state that authority remains fenced: {error}"
    );
}

#[test]
fn runner_wait_failure_preserves_an_explicit_stop_terminal_result() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = sample_plan_only_backend(temp_dir.path());
    let mut runner_manifest = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("runner-wait-failure-after-stop"),
            None,
            None,
        )
        .expect("runner fixture should plan")
        .manifest;
    mark_prepared_service_runner(&mut runner_manifest);
    backend
        .write_manifest(&runner_manifest)
        .expect("prepared manifest should be durable");
    let handoff =
        super::super::runner::persist_runner_execution_ownership(&backend, &mut runner_manifest)
            .expect("runner should claim execution");
    super::super::runner::mark_runner_effects_started(&runner_manifest, &handoff)
        .expect("runner effect boundary should become durable");
    publish_present_runner_lifecycle(&runner_manifest, &handoff);
    drop(handoff);

    let lifecycle = super::super::runner::lock_execute_lifecycle(&runner_manifest)
        .expect("ordinary lifecycle owner should acquire");
    let mut stopped = runner_manifest.clone();
    stopped.shutdown_requested = true;
    stopped.last_exit_code = Some(17);
    stopped.launch_reservation_claim = None;
    stopped.launch_artifact = None;
    stopped.network_cleanup_complete = true;
    synchronize_handle_status(&mut stopped, SandboxStatus::Stopped);
    backend
        .write_manifest(&stopped)
        .expect("explicit stop should persist");
    drop(lifecycle);

    let primary = SandboxError::OperationFailed {
        message: "injected runner wait observation failure".to_owned(),
    };
    let error = super::super::runner::finalize_runner_failure_for_test(
        &backend,
        &mut runner_manifest,
        primary,
    );
    assert!(
        error
            .to_string()
            .contains("injected runner wait observation failure"),
        "the wait failure should remain primary: {error}"
    );
    assert_eq!(
        runner_manifest, stopped,
        "wait failure finalization must adopt an already-complete explicit stop"
    );
    assert_eq!(
        backend
            .read_manifest(&stopped.handle.id)
            .expect("terminal manifest should inspect")
            .expect("terminal manifest should remain durable"),
        stopped,
        "wait failure must not rewrite the explicit stop result"
    );
}

#[test]
fn runner_finalization_rejects_changed_execution_identity_without_mutation() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = sample_plan_only_backend(temp_dir.path());
    let mut runner_manifest = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("runner-finalization-identity-fence"),
            None,
            None,
        )
        .expect("runner fixture should plan")
        .manifest;
    mark_prepared_service_runner(&mut runner_manifest);
    backend
        .write_manifest(&runner_manifest)
        .expect("prepared manifest should be durable");
    let handoff =
        super::super::runner::persist_runner_execution_ownership(&backend, &mut runner_manifest)
            .expect("runner should claim execution");
    super::super::runner::mark_runner_effects_started(&runner_manifest, &handoff)
        .expect("runner effect boundary should become durable");
    publish_present_runner_lifecycle(&runner_manifest, &handoff);
    drop(handoff);

    let lifecycle = super::super::runner::lock_execute_lifecycle(&runner_manifest)
        .expect("ordinary lifecycle owner should acquire");
    let mut changed = runner_manifest.clone();
    changed.handle.name.push_str("-foreign-generation");
    backend
        .write_manifest(&changed)
        .expect("changed durable identity should model an external generation");
    let before = std::fs::read(&changed.conmon_layout.manifest_path)
        .expect("changed manifest bytes should read");
    drop(lifecycle);

    let error = super::super::runner::finalize_runner_exit(&backend, &mut runner_manifest, 0)
        .expect_err("stale runner identity must remain fenced");
    assert!(
        error.to_string().contains("changed lifecycle identity"),
        "the immutable identity fence should be explicit: {error}"
    );
    assert_eq!(
        std::fs::read(&changed.conmon_layout.manifest_path)
            .expect("changed manifest bytes should reread"),
        before,
        "identity rejection must precede persistence and provider cleanup"
    );
}

#[test]
fn runner_exit_after_egress_reload_authenticates_immutable_identity_and_converges_cleanup() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = sample_plan_only_backend(temp_dir.path());
    let mut runner_manifest = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("runner-egress-reload-identity"),
            None,
            None,
        )
        .expect("runner fixture should plan")
        .manifest;
    mark_prepared_service_runner(&mut runner_manifest);
    backend
        .write_manifest(&runner_manifest)
        .expect("prepared manifest should be durable");
    let handoff =
        super::super::runner::persist_runner_execution_ownership(&backend, &mut runner_manifest)
            .expect("runner should claim execution");
    super::super::runner::mark_runner_effects_started(&runner_manifest, &handoff)
        .expect("runner effect boundary should become durable");
    publish_present_runner_lifecycle(&runner_manifest, &handoff);
    drop(handoff);

    let mut reloaded = runner_manifest.clone();
    reloaded.spec.egress = nimbus_egress::EgressPolicy::new([nimbus_egress::EgressRule::new(
        "reloaded-policy",
        nimbus_egress::EgressProtocol::Https,
        "example.com",
        443,
    )]);
    backend
        .write_manifest(&reloaded)
        .expect("reloadable desired policy should persist under lifecycle ownership");

    super::super::runner::finalize_runner_exit_with_cleanup_for_test(
        &backend,
        &mut runner_manifest,
        0,
        |candidate| {
            candidate.launch_reservation_claim = None;
            candidate.launch_artifact = None;
            candidate.network_cleanup_complete = true;
            Ok(())
        },
    )
    .expect("reloadable desired policy must not invalidate immutable execution identity");

    let terminal = backend
        .read_manifest(&reloaded.handle.id)
        .expect("terminal manifest should inspect")
        .expect("terminal manifest should remain durable");
    assert_eq!(
        terminal.spec.egress, reloaded.spec.egress,
        "runner finalization must preserve the reloaded desired policy"
    );
    assert!(
        terminal.network_cleanup_complete
            && terminal.launch_reservation_claim.is_none()
            && terminal.launch_artifact.is_none(),
        "runner exit may publish terminal only after exact cleanup finality"
    );
}

#[test]
fn egress_reload_waits_for_execute_lifecycle_lock_and_uses_current_manifest() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let lock_probe =
        super::super::runner::RunnerLifecycleLockTestProbe::new(Duration::from_secs(2));
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.start_mode = ContainerStartMode::PlanOnly;
    config.node_network_supernet = "127.0.0.0/24".to_owned();
    let pep_port = unused_loopback_port();
    config.published_port_range = pep_port..=pep_port;
    let backend = ContainerSandboxBackend::new(config)
        .with_runner_lifecycle_lock_test_probe(lock_probe.clone());
    let mut plan = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("runner-egress-reload-lifecycle-lock"),
            None,
            None,
        )
        .expect("runner fixture should plan");
    backend
        .attach_runner_owned_egress_proxy(&mut plan)
        .expect("runner fixture should reserve exact PEP authority");
    mark_prepared_service_runner(&mut plan.manifest);
    backend
        .write_manifest(&plan.manifest)
        .expect("prepared manifest should be durable");
    let mut manifest = plan.manifest;
    let handoff = super::super::runner::persist_runner_execution_ownership(&backend, &mut manifest)
        .expect("runner should claim execution");
    super::super::runner::mark_runner_effects_started(&manifest, &handoff)
        .expect("runner effect boundary should become durable");
    backend
        .ensure_egress_proxy_running_with_release_authority(
            &manifest,
            PepPreAdoptionReleaseAuthority::FreshLaunch(
                manifest
                    .launch_reservation_claim
                    .as_ref()
                    .expect("runner launch should retain exact PEP reservation authority"),
            ),
        )
        .expect("running PEP should exist before live reload");
    manifest.launch_reservation_claim = None;
    backend
        .write_manifest(&manifest)
        .expect("running manifest should publish post-launch authority");
    publish_present_runner_lifecycle(&manifest, &handoff);
    drop(handoff);

    let lifecycle = super::super::runner::lock_execute_lifecycle(&manifest)
        .expect("ordinary lifecycle owner should acquire the shared lock");
    let reloaded_policy = nimbus_egress::EgressPolicy::new([nimbus_egress::EgressRule::new(
        "serialized-reload",
        nimbus_egress::EgressProtocol::Https,
        "example.com",
        443,
    )]);
    let contender = backend.clone();
    let id = manifest.handle.id.clone();
    let policy_for_worker = reloaded_policy.clone();
    let (result_tx, result_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        result_tx
            .send(contender.reload_egress_policy(&id, policy_for_worker))
            .expect("reload result should send");
    });
    assert!(
        lock_probe.wait_until_contended(),
        "reload must reach the existing Execute lifecycle-lock boundary"
    );
    let while_locked = backend
        .read_manifest(&manifest.handle.id)
        .expect("manifest should inspect while reload waits")
        .expect("manifest should remain durable");
    assert!(
        while_locked.spec.egress.is_deny_all(),
        "a blocked reload must not mutate provider or durable desired policy"
    );

    drop(lifecycle);
    result_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("reload should resume after lifecycle release")
        .expect("reload should use the canonical manifest read under the lock");
    worker.join().expect("reload worker should join");
    let persisted = backend
        .read_manifest(&manifest.handle.id)
        .expect("reloaded manifest should inspect")
        .expect("reloaded manifest should remain durable");
    assert_eq!(
        persisted.spec.egress, reloaded_policy,
        "serialized reload must preserve the requested desired policy"
    );
}

#[test]
fn runner_cleanup_resume_preserves_winning_exit_receipt() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = sample_plan_only_backend(temp_dir.path());
    let mut runner_manifest = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("runner-terminal-without-finality"),
            None,
            None,
        )
        .expect("runner fixture should plan")
        .manifest;
    mark_prepared_service_runner(&mut runner_manifest);
    backend
        .write_manifest(&runner_manifest)
        .expect("prepared manifest should be durable");
    let handoff =
        super::super::runner::persist_runner_execution_ownership(&backend, &mut runner_manifest)
            .expect("runner should claim execution");
    super::super::runner::mark_runner_effects_started(&runner_manifest, &handoff)
        .expect("runner effect boundary should become durable");
    publish_present_runner_lifecycle(&runner_manifest, &handoff);
    drop(handoff);

    let mut cleanup_pending = runner_manifest.clone();
    cleanup_pending.shutdown_requested = true;
    cleanup_pending.last_exit_code = Some(41);
    synchronize_handle_status(&mut cleanup_pending, SandboxStatus::Stopping);
    assert!(
        !cleanup_pending.has_terminal_network_finality(),
        "the crash cut must retain incomplete cleanup authority"
    );
    backend
        .write_manifest(&cleanup_pending)
        .expect("cleanup-pending crash cut should persist as nonterminal");

    let cleanup_called = Cell::new(false);
    super::super::runner::finalize_runner_exit_with_cleanup_for_test(
        &backend,
        &mut runner_manifest,
        0,
        |candidate| {
            cleanup_called.set(true);
            candidate.launch_reservation_claim = None;
            candidate.launch_artifact = None;
            candidate.network_cleanup_complete = true;
            Ok(())
        },
    )
    .expect("runner finalization should resume incomplete nonterminal cleanup");
    assert!(
        cleanup_called.get(),
        "cleanup-pending status must not bypass the cleanup adapter"
    );

    let terminal = backend
        .read_manifest(&cleanup_pending.handle.id)
        .expect("terminal manifest should inspect")
        .expect("terminal manifest should remain durable");
    assert_eq!(
        (terminal.status, terminal.last_exit_code),
        (SandboxStatus::Failed, Some(41)),
        "cleanup resumption must preserve the first durable Stopping exit receipt"
    );
    assert!(
        terminal.network_cleanup_complete
            && terminal.launch_reservation_claim.is_none()
            && terminal.launch_artifact.is_none(),
        "terminal publication is complete only after every cleanup receipt converges"
    );
}

#[test]
fn runner_cleanup_retry_preserves_immutable_exit_receipt_matrix() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = sample_plan_only_backend(temp_dir.path());
    let mut base = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("runner-immutable-exit-matrix"),
            None,
            None,
        )
        .expect("runner exit matrix should plan")
        .manifest;
    mark_prepared_service_runner(&mut base);
    base.start_mode = ContainerStartMode::Execute;
    base.shutdown_requested = true;
    synchronize_handle_status(&mut base, SandboxStatus::Stopping);

    let cases = [
        (
            "identical nonzero replay",
            Some(41),
            SandboxStatus::Failed,
            Some(41),
            SandboxStatus::Failed,
            Some(41),
        ),
        (
            "conflicting successful replay",
            Some(41),
            SandboxStatus::Stopped,
            Some(0),
            SandboxStatus::Failed,
            Some(41),
        ),
        (
            "conflicting failed replay",
            Some(0),
            SandboxStatus::Failed,
            Some(41),
            SandboxStatus::Stopped,
            Some(0),
        ),
        (
            "receipt-free failure replay",
            None,
            SandboxStatus::Stopped,
            Some(0),
            SandboxStatus::Failed,
            None,
        ),
    ];
    for (name, durable_exit, proposed_status, proposed_exit, expected_status, expected_exit) in
        cases
    {
        let mut manifest = base.clone();
        manifest.last_exit_code = durable_exit;
        super::super::runner::try_converge_runner_cleanup_with(
            &mut manifest,
            proposed_status,
            proposed_exit,
            |_| Ok(()),
            |_| Ok(()),
            |stage, error| panic!("{name} unexpectedly waited at {stage:?}: {error}"),
        )
        .unwrap_or_else(|error| panic!("{name} should converge: {error}"));
        assert_eq!(
            (manifest.status, manifest.last_exit_code),
            (expected_status, expected_exit),
            "{name} must preserve the first durable Stopping outcome"
        );
    }
}

#[test]
fn runner_ownership_bounded_convergence_stops_after_exact_attempt_limit() {
    let attempts = Cell::new(0_usize);
    let waits = Cell::new(0_usize);
    let error = super::super::runner::converge_runner_ownership_with(
        super::super::runner::RunnerOwnershipConvergenceStage::LifecyclePublished,
        || {
            attempts.set(attempts.get() + 1);
            Err(SandboxError::OperationFailed {
                message: "injected permanent lifecycle publication failure".to_owned(),
            })
        },
        |_stage, _error| waits.set(waits.get() + 1),
    )
    .expect_err("permanent lifecycle publication must return after the bounded attempts");

    assert_eq!(
        attempts.get(),
        4,
        "the final failed attempt must be bounded"
    );
    assert_eq!(
        waits.get(),
        3,
        "only retryable failures before the final attempt should wait"
    );
    assert!(
        error.to_string().contains("LifecyclePublished")
            && error.to_string().contains("after 4 attempts")
            && error
                .to_string()
                .contains("injected permanent lifecycle publication failure")
            && error.to_string().contains("inspect-before-retry"),
        "the error must preserve the stage, bound, last failure, and fenced recovery: {error}"
    );
}

#[test]
fn runner_launch_publication_failure_stops_after_exact_attempt_limit() {
    let attempts = Cell::new(0_usize);
    let waits = Cell::new(0_usize);
    let cleanup_calls = Cell::new(0_usize);
    let mut state = ();
    let error = super::super::runner::converge_runner_launch_result_with(
        &mut state,
        Err(SandboxError::OperationFailed {
            message: "injected primary launch failure".to_owned(),
        }),
        |_| cleanup_calls.set(cleanup_calls.get() + 1),
        |_| {
            attempts.set(attempts.get() + 1);
            Err(SandboxError::OperationFailed {
                message: "injected permanent lifecycle publication failure".to_owned(),
            })
        },
        |_state, _stage, _error| waits.set(waits.get() + 1),
    )
    .expect_err("permanent publication must preserve the primary launch failure and return");

    assert_eq!(cleanup_calls.get(), 1, "cleanup must not replay");
    assert_eq!(
        attempts.get(),
        4,
        "publication must attempt exactly four times"
    );
    assert_eq!(waits.get(), 3, "only the first three failures may wait");
    assert!(
        error
            .to_string()
            .contains("injected primary launch failure")
            && error
                .to_string()
                .contains("injected permanent lifecycle publication failure")
            && error.to_string().contains("after 4 attempts")
            && error.to_string().contains("both preserved"),
        "both the primary and convergence failures must remain visible: {error}"
    );
}

#[test]
fn runner_provider_cleanup_failure_stops_after_exact_attempt_limit() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = sample_plan_only_backend(temp_dir.path());
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("runner-provider-cleanup-bounded"),
            None,
            None,
        )
        .expect("runner convergence fixture should plan")
        .manifest;
    mark_prepared_service_runner(&mut manifest);
    manifest.start_mode = ContainerStartMode::Execute;
    let cleanup_attempts = Cell::new(0_usize);
    let persistence_attempts = Cell::new(0_usize);
    let waits = Cell::new(0_usize);
    let error = super::super::runner::try_converge_runner_cleanup_with(
        &mut manifest,
        SandboxStatus::Failed,
        Some(71),
        |_| {
            persistence_attempts.set(persistence_attempts.get() + 1);
            Ok(())
        },
        |_| {
            cleanup_attempts.set(cleanup_attempts.get() + 1);
            Err(SandboxError::OperationFailed {
                message: "injected permanent provider cleanup failure".to_owned(),
            })
        },
        |stage, _error| {
            if stage == super::super::runner::RunnerCleanupConvergenceStage::ProviderCleanup {
                waits.set(waits.get() + 1);
            }
        },
    )
    .expect_err("permanent provider cleanup must retain Stopping ownership and return");

    assert_eq!(
        cleanup_attempts.get(),
        4,
        "provider cleanup must attempt exactly four times"
    );
    assert_eq!(waits.get(), 3, "only the first three failures may wait");
    assert_eq!(
        persistence_attempts.get(),
        5,
        "the initial Stopping state and every failed cleanup mutation must become durable"
    );
    assert!(
        error.to_string().contains("ProviderCleanup")
            && error.to_string().contains("after 4 attempts")
            && error
                .to_string()
                .contains("injected permanent provider cleanup failure")
            && error
                .to_string()
                .contains("terminal lifecycle was not published"),
        "the error must preserve the exact unfinished cleanup authority: {error}"
    );
    assert_eq!(manifest.status, SandboxStatus::Stopping);
    assert_eq!(manifest.handle.status, SandboxStatus::Stopping);
    assert_eq!(manifest.last_exit_code, Some(71));
}

#[test]
fn runner_cleanup_preserves_provider_and_progress_persistence_failures() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = sample_plan_only_backend(temp_dir.path());
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("runner-cleanup-dual-failure"),
            None,
            None,
        )
        .expect("runner convergence fixture should plan")
        .manifest;
    mark_prepared_service_runner(&mut manifest);
    manifest.start_mode = ContainerStartMode::Execute;
    let persistence_attempts = Cell::new(0_usize);
    let cleanup_attempts = Cell::new(0_usize);
    let stopping_waits = Cell::new(0_usize);

    let error = super::super::runner::try_converge_runner_cleanup_with(
        &mut manifest,
        SandboxStatus::Failed,
        None,
        |_| {
            let attempt = persistence_attempts.get() + 1;
            persistence_attempts.set(attempt);
            if attempt == 1 {
                Ok(())
            } else {
                Err(SandboxError::OperationFailed {
                    message: "injected cleanup-progress persistence failure".to_owned(),
                })
            }
        },
        |_| {
            cleanup_attempts.set(cleanup_attempts.get() + 1);
            Err(SandboxError::OperationFailed {
                message: "injected provider cleanup primary failure".to_owned(),
            })
        },
        |stage, _error| {
            if stage == super::super::runner::RunnerCleanupConvergenceStage::StoppingPersistence {
                stopping_waits.set(stopping_waits.get() + 1);
            }
        },
    )
    .expect_err("cleanup-progress persistence exhaustion must return without replaying cleanup");

    assert_eq!(
        cleanup_attempts.get(),
        1,
        "ambiguous cleanup must not replay"
    );
    assert_eq!(
        persistence_attempts.get(),
        5,
        "the initial Stopping write and four progress writes must be bounded"
    );
    assert_eq!(
        stopping_waits.get(),
        3,
        "only retryable progress-persistence failures should wait"
    );
    assert!(
        error
            .to_string()
            .contains("injected provider cleanup primary failure")
            && error
                .to_string()
                .contains("injected cleanup-progress persistence failure")
            && error.to_string().contains("inspect-before-retry"),
        "both provider and persistence evidence must survive: {error}"
    );
    assert_eq!(manifest.status, SandboxStatus::Stopping);
    assert_eq!(manifest.handle.status, SandboxStatus::Stopping);
}

fn prepared_runner_effect_fence_fixture(
    root: &std::path::Path,
    id: &str,
) -> (
    ContainerSandboxBackend,
    ContainerSandboxManifest,
    super::super::runner::RunnerHandoffGuard,
) {
    let backend = sample_plan_only_backend(root);
    let mut plan = backend
        .plan_start_with_id(&sample_spec(), &SandboxId::new(id), None, None)
        .expect("runner fixture should plan");
    backend
        .attach_runner_owned_egress_proxy(&mut plan)
        .expect("runner fixture should reserve exact execution authority");
    mark_prepared_service_runner(&mut plan.manifest);
    backend
        .write_manifest(&plan.manifest)
        .expect("prepared manifest should be durable");
    let mut manifest = plan.manifest;
    let handoff = super::super::runner::persist_runner_execution_ownership(&backend, &mut manifest)
        .expect("runner should claim execution");
    (backend, manifest, handoff)
}

#[test]
fn runner_phase_query_rejects_plan_only_source_mode_without_mutation() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let (backend, manifest, handoff) =
        prepared_runner_effect_fence_fixture(temp_dir.path(), "runner-phase-mode-substitution");
    let mut substituted = manifest.clone();
    substituted.start_mode = ContainerStartMode::PlanOnly;
    backend
        .write_manifest(&substituted)
        .expect("substituted source-mode fixture should become durable");
    let decision_path = substituted
        .conmon_layout
        .container_state_dir
        .join(super::super::runner::RUNNER_HANDOFF_DECISION_FILE);
    let before_manifest = std::fs::read(&substituted.conmon_layout.manifest_path)
        .expect("substituted manifest bytes should read");
    let before_decision = std::fs::read(&decision_path).expect("runner decision bytes should read");

    let error = super::super::runner::execute_handoff_phase(&substituted)
        .expect_err("an Execute decision must reject a PlanOnly source manifest");
    assert!(
        error.to_string().contains("Execute") && error.to_string().contains("PlanOnly"),
        "the rejection must name the expected and actual source modes: {error}"
    );
    assert_eq!(
        std::fs::read(&substituted.conmon_layout.manifest_path)
            .expect("substituted manifest bytes should reread"),
        before_manifest
    );
    assert_eq!(
        std::fs::read(&decision_path).expect("runner decision bytes should reread"),
        before_decision,
        "source-mode rejection must not advance durable handoff authority"
    );
    drop(handoff);
}

#[test]
fn runner_lifecycle_publication_rejects_plan_only_post_effect_source() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let (backend, manifest, handoff) = prepared_runner_effect_fence_fixture(
        temp_dir.path(),
        "runner-publication-mode-substitution",
    );
    super::super::runner::mark_runner_effects_started(&manifest, &handoff)
        .expect("effect boundary should become durable");
    let mut substituted = manifest.clone();
    substituted.start_mode = ContainerStartMode::PlanOnly;
    backend
        .write_manifest(&substituted)
        .expect("substituted source-mode fixture should become durable");
    let decision_path = substituted
        .conmon_layout
        .container_state_dir
        .join(super::super::runner::RUNNER_HANDOFF_DECISION_FILE);
    let before_manifest = std::fs::read(&substituted.conmon_layout.manifest_path)
        .expect("substituted manifest bytes should read");
    let before_decision = std::fs::read(&decision_path).expect("runner decision bytes should read");

    let error = super::super::runner::publish_runner_lifecycle_ownership(&substituted, &handoff)
        .expect_err("post-effect publication must reject a PlanOnly source manifest");
    assert!(
        error.to_string().contains("Execute") && error.to_string().contains("PlanOnly"),
        "the rejection must name the expected and actual source modes: {error}"
    );
    assert_eq!(
        std::fs::read(&substituted.conmon_layout.manifest_path)
            .expect("substituted manifest bytes should reread"),
        before_manifest
    );
    assert_eq!(
        std::fs::read(&decision_path).expect("runner decision bytes should reread"),
        before_decision,
        "source-mode rejection must not advance durable handoff authority"
    );
}

#[test]
fn terminal_pre_effect_cleanup_rejects_substituted_desired_authority() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let (backend, mut terminal, handoff) = prepared_runner_effect_fence_fixture(
        temp_dir.path(),
        "runner-pre-effect-authority-substitution",
    );
    backend
        .release_unstarted_launch_artifacts(&mut terminal)
        .expect("confirmed no-effect fixture should release exact launch authority");
    terminal.shutdown_requested = true;
    terminal.last_exit_code = Some(0);
    terminal.next_restart_at_millis = None;
    synchronize_handle_status(&mut terminal, SandboxStatus::Stopped);
    terminal.spec.egress = nimbus_egress::EgressPolicy::new([nimbus_egress::EgressRule::new(
        "substituted-pre-effect-policy",
        nimbus_egress::EgressProtocol::Https,
        "example.com",
        443,
    )]);
    assert!(terminal.has_terminal_network_finality());
    backend
        .write_manifest(&terminal)
        .expect("substituted terminal fixture should become durable");
    let decision_path = terminal
        .conmon_layout
        .container_state_dir
        .join(super::super::runner::RUNNER_HANDOFF_DECISION_FILE);
    let before_manifest = std::fs::read(&terminal.conmon_layout.manifest_path)
        .expect("terminal manifest bytes should read");
    let before_decision = std::fs::read(&decision_path).expect("runner decision bytes should read");

    let error = super::super::runner::publish_runner_lifecycle_ownership(&terminal, &handoff)
        .expect_err("pre-effect publication must reject substituted desired authority");
    assert!(
        error.to_string().contains("does not authenticate")
            || error.to_string().contains("does not match"),
        "the rejection must identify the changed prepared authority: {error}"
    );
    assert_eq!(
        std::fs::read(&terminal.conmon_layout.manifest_path)
            .expect("terminal manifest bytes should reread"),
        before_manifest
    );
    assert_eq!(
        std::fs::read(&decision_path).expect("runner decision bytes should reread"),
        before_decision,
        "authority rejection must not publish lifecycle ownership"
    );
}

#[test]
fn effects_started_rejects_substituted_desired_authority_before_publication() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let (backend, mut manifest, handoff) = prepared_runner_effect_fence_fixture(
        temp_dir.path(),
        "runner-effects-started-authority-substitution",
    );
    super::super::runner::mark_runner_effects_started(&manifest, &handoff)
        .expect("effect boundary should become durable");
    manifest.spec.egress = nimbus_egress::EgressPolicy::new([nimbus_egress::EgressRule::new(
        "substituted-before-lifecycle-publication",
        nimbus_egress::EgressProtocol::Https,
        "example.com",
        443,
    )]);
    backend
        .write_manifest(&manifest)
        .expect("substituted EffectsStarted fixture should become durable");
    let decision_path = manifest
        .conmon_layout
        .container_state_dir
        .join(super::super::runner::RUNNER_HANDOFF_DECISION_FILE);
    let before_manifest = std::fs::read(&manifest.conmon_layout.manifest_path)
        .expect("substituted manifest bytes should read");
    let before_decision = std::fs::read(&decision_path).expect("runner decision bytes should read");

    let error = super::super::runner::publish_runner_lifecycle_ownership(&manifest, &handoff)
        .expect_err("EffectsStarted must reject substituted desired authority");
    assert!(
        error.to_string().contains("does not match"),
        "the rejection must identify the changed handoff authority: {error}"
    );
    assert_eq!(
        std::fs::read(&manifest.conmon_layout.manifest_path)
            .expect("substituted manifest bytes should reread"),
        before_manifest
    );
    assert_eq!(
        std::fs::read(&decision_path).expect("runner decision bytes should reread"),
        before_decision,
        "authority rejection must preserve the exact EffectsStarted decision"
    );
}

#[test]
fn runner_effect_fence_permanent_failure_returns_and_explicit_stop_converges() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let (backend, manifest, handoff) =
        prepared_runner_effect_fence_fixture(temp_dir.path(), "runner-effect-fence-permanent");
    let attempts = Cell::new(0_usize);
    let waits = Cell::new(0_usize);

    let error = super::super::runner::converge_runner_effects_started_with(
        &manifest,
        || {
            attempts.set(attempts.get() + 1);
            Err(SandboxError::OperationFailed {
                message: "injected runner state-volume failure".to_owned(),
            })
        },
        |_stage, _error| waits.set(waits.get() + 1),
    )
    .expect_err("permanent persistence failure must return before provider launch");
    assert_eq!((attempts.get(), waits.get()), (4, 3));
    assert!(
        error.to_string().contains("no provider effect began")
            && error.to_string().contains("explicit stop"),
        "the exact no-effect recovery must be explicit: {error}"
    );
    assert_eq!(
        super::super::runner::execute_handoff_phase(&manifest)
            .expect("handoff should authenticate"),
        Some(super::super::runner::RunnerHandoffPhase::ClaimedBeforeEffects)
    );
    assert!(
        !manifest.network_layout.netns_path.exists()
            && !manifest.network_layout.status_path.exists(),
        "provider launch must remain unentered"
    );
    drop(handoff);

    let recovery = ContainerSandboxBackend::new(manifest.runner_config.to_backend_config());
    recovery
        .stop_sync(&manifest.handle.id)
        .expect("explicit stop should acquire the released lock and compensate");
    let stopped = recovery
        .read_manifest(&manifest.handle.id)
        .expect("terminal manifest should inspect")
        .expect("terminal manifest should remain");
    assert!(stopped.network_cleanup_complete && stopped.launch_reservation_claim.is_none());
    recovery
        .stop_sync(&manifest.handle.id)
        .expect("terminal recovery should replay idempotently");
    drop(backend);
}

#[test]
fn runner_effect_fence_transient_failure_precedes_provider_sentinel() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let (_backend, manifest, handoff) =
        prepared_runner_effect_fence_fixture(temp_dir.path(), "runner-effect-fence-transient");
    let attempts = Cell::new(0_usize);

    super::super::runner::converge_runner_effects_started_with(
        &manifest,
        || {
            let attempt = attempts.get() + 1;
            attempts.set(attempt);
            if attempt < 3 {
                Err(SandboxError::OperationFailed {
                    message: "injected transient runner persistence failure".to_owned(),
                })
            } else {
                super::super::runner::mark_runner_effects_started(&manifest, &handoff)
            }
        },
        |_stage, _error| {},
    )
    .expect("transient persistence failure should converge");
    assert_eq!(attempts.get(), 3);
    let provider_calls = Cell::new(0_usize);
    assert_eq!(
        super::super::runner::execute_handoff_phase(&manifest)
            .expect("effect boundary should authenticate"),
        Some(super::super::runner::RunnerHandoffPhase::EffectsStarted),
        "the provider sentinel may run only after the durable effect boundary"
    );
    provider_calls.set(provider_calls.get() + 1);
    assert_eq!(
        provider_calls.get(),
        1,
        "the provider sentinel must execute exactly once after convergence"
    );
}

#[test]
fn runner_effect_fence_acknowledgement_loss_remains_fenced() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let (backend, manifest, handoff) =
        prepared_runner_effect_fence_fixture(temp_dir.path(), "runner-effect-fence-ambiguous");
    let attempts = Cell::new(0_usize);

    let error = super::super::runner::converge_runner_effects_started_with(
        &manifest,
        || {
            let attempt = attempts.get() + 1;
            attempts.set(attempt);
            if attempt == 1 {
                super::super::runner::mark_runner_effects_started(&manifest, &handoff)?;
            }
            Err(SandboxError::OperationFailed {
                message: "injected acknowledgement loss".to_owned(),
            })
        },
        |_stage, _error| {},
    )
    .expect_err("ambiguous durable publication must return without provider launch");
    assert_eq!(attempts.get(), 4);
    assert!(
        error.to_string().contains("may have published")
            && error.to_string().contains("inspect-before-retry"),
        "the ambiguous phase must not advertise no-effect cleanup: {error}"
    );
    assert_eq!(
        super::super::runner::execute_handoff_phase(&manifest)
            .expect("effect boundary should authenticate"),
        Some(super::super::runner::RunnerHandoffPhase::EffectsStarted)
    );
    drop(handoff);
    let stop_error = backend
        .stop_sync(&manifest.handle.id)
        .expect_err("external stop cannot release an EffectsStarted handoff");
    assert!(
        stop_error.to_string().contains("runtime state command")
            && stop_error.to_string().contains("remain fenced"),
        "provider ambiguity must retain exact authority: {stop_error}"
    );
}

#[test]
fn runner_effect_fence_corrupt_state_fails_closed() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let (_backend, manifest, handoff) =
        prepared_runner_effect_fence_fixture(temp_dir.path(), "runner-effect-fence-corrupt");
    let attempts = Cell::new(0_usize);
    let decision_path = manifest
        .conmon_layout
        .container_state_dir
        .join(super::super::runner::RUNNER_HANDOFF_DECISION_FILE);

    let error = super::super::runner::converge_runner_effects_started_with(
        &manifest,
        || {
            attempts.set(attempts.get() + 1);
            if attempts.get() == 1 {
                std::fs::write(&decision_path, b"{not-json\n")
                    .expect("corrupt phase fixture should write");
            }
            Err(SandboxError::OperationFailed {
                message: "injected persistence failure".to_owned(),
            })
        },
        |_stage, _error| {},
    )
    .expect_err("unreadable durable phase must fail closed");
    assert_eq!(attempts.get(), 4);
    assert!(
        error.to_string().contains("cannot authenticate")
            && error.to_string().contains("failed to parse")
            && error
                .to_string()
                .contains("lifecycle mutation remains fenced"),
        "both persistence and inspection evidence must remain visible: {error}"
    );
    drop(handoff);
}

#[test]
fn direct_effect_fence_persistence_exhaustion_stop_converges() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    let backend = ContainerSandboxBackend::new(config.clone())
        .with_runner_handoff_failure(RunnerHandoffFailure::DirectEffectFencePersistence);
    let plan = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("direct-effect-fence-persistence"),
            None,
            None,
        )
        .expect("direct launch should reserve complete authority");
    let planned = plan.manifest.clone();

    let mut manifest = plan.manifest;
    let error = backend
        .execute_direct_start(&mut manifest)
        .expect_err("permanent effect-fence persistence failure must return");
    assert!(
        error.to_string().contains("after 4 attempts")
            && error
                .to_string()
                .contains("injected direct effect-fence persistence failure")
            && error.to_string().contains("explicit stop"),
        "the direct caller must receive the bounded persistence diagnostic: {error}"
    );

    let persisted = backend
        .read_manifest(&planned.handle.id)
        .expect("direct manifest should inspect")
        .expect("the durable pre-effect manifest must remain");
    assert_eq!(
        super::super::runner::execute_handoff_phase(&persisted)
            .expect("durable handoff should authenticate"),
        Some(super::super::runner::RunnerHandoffPhase::ClaimedBeforeEffects),
        "failure must retain the exact no-provider-effect crash boundary"
    );
    assert!(
        !persisted.network_layout.netns_path.exists()
            && !persisted.network_layout.status_path.exists(),
        "bounded effect-fence failure must return before namespace or provider effects"
    );

    let recovering_backend = ContainerSandboxBackend::new(config);
    recovering_backend
        .stop_sync(&planned.handle.id)
        .expect("a reopened backend must stop the durable pre-effect claim");
    let stopped = recovering_backend
        .read_manifest(&planned.handle.id)
        .expect("stopped manifest should inspect")
        .expect("stopped manifest should remain durable");
    assert_eq!(stopped.status, SandboxStatus::Stopped);
    assert_eq!(stopped.handle.status, SandboxStatus::Stopped);
    assert!(stopped.shutdown_requested);
    assert!(stopped.network_cleanup_complete);
    assert!(stopped.launch_reservation_claim.is_none());
    assert!(stopped.launch_artifact.is_none());
    assert_eq!(
        super::super::runner::execute_handoff_phase(&stopped)
            .expect("terminal handoff should authenticate"),
        None,
        "explicit stop must publish terminal lifecycle ownership"
    );
    let authority = nimbus_network::LocalPortLeaseAuthority::open(
        &recovering_backend.config.network_state_root,
    )
    .expect("port authority should reopen");
    for request in planned.port_leases.iter().chain(
        planned
            .egress_proxy
            .as_ref()
            .map(|assignment| &assignment.port_lease),
    ) {
        assert_eq!(
            authority
                .inspect(request.lease_id())
                .expect("lease should inspect")
                .expect("released receipt should remain durable")
                .phase(),
            PortLeasePhase::Released,
            "explicit stop must release every never-bound listener exactly once"
        );
    }
    assert!(
        recovering_backend
            .segment_allocator
            .inspect_segments(&stopped.spec.tenant_id)
            .expect("segment authority should inspect")
            .unwrap_or_default()
            .is_empty(),
        "explicit stop must retire IPAM and the reserved attachment"
    );
    recovering_backend
        .stop_sync(&planned.handle.id)
        .expect("terminal pre-effect stop replay must be idempotent");
}

#[test]
fn direct_effect_fence_acknowledgement_loss_never_enters_provider() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(temp_dir.path()))
            .with_runner_handoff_failure(
                RunnerHandoffFailure::DirectEffectFenceAcknowledgementLoss,
            );
    let plan = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("direct-effect-fence-ack-loss"),
            None,
            None,
        )
        .expect("direct launch should reserve exact authority");
    let id = plan.manifest.handle.id.clone();

    let mut manifest = plan.manifest;
    let error = backend
        .execute_direct_start(&mut manifest)
        .expect_err("acknowledgement loss must return without provider launch");
    assert!(
        error.to_string().contains("may have published")
            && error.to_string().contains("inspect-before-retry"),
        "direct start must report the exact ambiguous fence: {error}"
    );
    let persisted = backend
        .read_manifest(&id)
        .expect("manifest should inspect")
        .expect("manifest should remain");
    assert_eq!(
        super::super::runner::execute_handoff_phase(&persisted)
            .expect("effect phase should authenticate"),
        Some(super::super::runner::RunnerHandoffPhase::EffectsStarted)
    );
    assert!(
        !persisted.network_layout.netns_path.exists()
            && !persisted.network_layout.status_path.exists(),
        "provider effects must remain absent despite the durable ambiguity"
    );
    let stop_error = backend
        .stop_sync(&id)
        .expect_err("ambiguous effect fence must reject external cleanup");
    assert!(
        stop_error.to_string().contains("runtime state command")
            && stop_error.to_string().contains("remain fenced"),
        "unobservable provider absence must retain exact authority: {stop_error}"
    );
}

#[test]
fn terminal_pre_effect_cleanup_reopen_publishes_lifecycle_once() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    let backend = ContainerSandboxBackend::new(config.clone())
        .with_runner_handoff_failure(RunnerHandoffFailure::DirectEffectFencePersistence);
    let plan = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("direct-pre-effect-terminal-publication"),
            None,
            None,
        )
        .expect("direct launch should reserve complete authority");
    let id = plan.manifest.handle.id.clone();
    let mut manifest = plan.manifest;
    backend
        .execute_direct_start(&mut manifest)
        .expect_err("effect-fence exhaustion must retain the pre-effect decision");
    let mut terminal = backend
        .read_manifest(&id)
        .expect("claimed manifest should inspect")
        .expect("claimed manifest should remain durable");
    backend
        .release_unstarted_launch_artifacts(&mut terminal)
        .expect("no-effect cleanup should converge before the simulated crash");
    terminal.shutdown_requested = true;
    terminal.last_exit_code = Some(0);
    terminal.next_restart_at_millis = None;
    synchronize_handle_status(&mut terminal, SandboxStatus::Stopped);
    backend
        .write_manifest(&terminal)
        .expect("terminal cleanup receipt should become durable");
    assert_eq!(
        super::super::runner::execute_handoff_phase(&terminal)
            .expect("terminal pre-effect cut should authenticate"),
        Some(super::super::runner::RunnerHandoffPhase::ClaimedBeforeEffects),
        "the simulated crash must occur before lifecycle publication"
    );

    let recovery = ContainerSandboxBackend::new(config);
    recovery
        .stop_sync(&id)
        .expect("reopened stop must publish the completed cleanup receipt");
    let published = recovery
        .read_manifest(&id)
        .expect("published manifest should inspect")
        .expect("published manifest should remain durable");
    assert_eq!(
        super::super::runner::execute_handoff_phase(&published)
            .expect("published lifecycle should authenticate"),
        None
    );
    assert_eq!(published, terminal);
    recovery
        .stop_sync(&id)
        .expect("terminal lifecycle publication replay must be idempotent");
}

#[test]
fn nonzero_runner_exit_persists_failed_status_after_owned_cleanup() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.netavark_path = PathBuf::from("/usr/bin/true");
    let backend = ContainerSandboxBackend::new(config);
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("runner-nonzero-exit"),
            None,
            None,
        )
        .expect("runner exit fixture should reserve complete authority")
        .manifest;
    mark_prepared_service_runner(&mut manifest);
    mark_runtime_absent_for_cleanup(&mut manifest);
    backend
        .write_manifest(&manifest)
        .expect("prepared runner fixture should be durable");
    let handoff = super::super::runner::persist_runner_execution_ownership(&backend, &mut manifest)
        .expect("runner fixture should claim execution");
    super::super::runner::mark_runner_effects_started(&manifest, &handoff)
        .expect("runner fixture should publish its provider-effect boundary");
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

    super::super::runner::finalize_runner_exit(&backend, &mut manifest, 23)
        .expect("nonzero exit should converge cleanup and terminal evidence");

    let persisted = backend
        .read_manifest(&manifest.handle.id)
        .expect("runner manifest should inspect")
        .expect("runner manifest should remain durable");
    assert_eq!(
        persisted.status,
        SandboxStatus::Failed,
        "a nonzero process exit must remain an observable workload failure"
    );
    assert_eq!(persisted.last_exit_code, Some(23));
    assert!(
        persisted.shutdown_requested
            && persisted.launch_reservation_claim.is_none()
            && persisted.launch_artifact.is_none(),
        "failed status may publish only after ordered provider cleanup converges"
    );
}
