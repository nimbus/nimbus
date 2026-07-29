use super::*;
use std::cell::{Cell, RefCell};
use std::sync::Barrier;

fn mark_prepared_service_runner(manifest: &mut ContainerSandboxManifest) {
    manifest.lifecycle_coordinator = ContainerLifecycleCoordinator::PreparedServiceRunner;
}

#[test]
fn runner_execution_ownership_is_an_exclusive_durable_claim() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = sample_plan_only_backend(temp_dir.path());
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("http", 18078, 8080)),
            &SandboxId::new("runner-exclusive-execution"),
            None,
            None,
        )
        .expect("runner fixture should reserve its complete launch authority")
        .manifest;
    mark_prepared_service_runner(&mut manifest);
    backend
        .write_manifest(&manifest)
        .expect("the prepared manifest should be the durable handoff barrier");

    let start = Arc::new(Barrier::new(3));
    let mut workers = Vec::new();
    for mut candidate in [manifest.clone(), manifest.clone()] {
        let backend = backend.clone();
        let start = Arc::clone(&start);
        workers.push(thread::spawn(move || {
            start.wait();
            let result =
                super::super::runner::persist_runner_execution_ownership(&backend, &mut candidate);
            (result, candidate.start_mode)
        }));
    }
    start.wait();
    let outcomes = workers
        .into_iter()
        .map(|worker| worker.join().expect("runner contender should join"))
        .collect::<Vec<_>>();

    assert_eq!(
        outcomes.iter().filter(|(result, _)| result.is_ok()).count(),
        1,
        "exactly one runner may durably take execution ownership: {outcomes:?}"
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|(result, _)| {
                result.as_ref().is_err_and(|error| {
                    error
                        .to_string()
                        .contains("timed out acquiring container runner handoff lock")
                })
            })
            .count(),
        1,
        "the losing runner must remain fenced while the live owner retains its OS lock: \
         {outcomes:?}"
    );
    assert!(
        manifest
            .conmon_layout
            .container_state_dir
            .join(super::super::runner::RUNNER_HANDOFF_DECISION_FILE)
            .is_file(),
        "execution ownership must remain durable for crash reconciliation"
    );
    let persisted = backend
        .read_manifest(&manifest.handle.id)
        .expect("claimed manifest should inspect")
        .expect("claimed manifest should remain durable");
    assert_eq!(persisted.start_mode, ContainerStartMode::Execute);
}

#[test]
fn runner_handoff_reconciles_bounded_staging_orphan_before_publication() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = sample_plan_only_backend(temp_dir.path());
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("runner-bounded-stage-recovery"),
            None,
            None,
        )
        .expect("runner fixture should plan")
        .manifest;
    mark_prepared_service_runner(&mut manifest);
    backend
        .write_manifest(&manifest)
        .expect("prepared manifest should be durable");
    let staged = manifest
        .conmon_layout
        .container_state_dir
        .join(".nimbus-runner-handoff-decision.stage");
    std::fs::write(&staged, b"orphaned crash-cut bytes")
        .expect("bounded staging orphan should exist");

    super::super::runner::claim_runner_execution_for_test(&manifest)
        .expect("the next lock owner should reconcile and publish from fresh bytes");
    assert!(
        !staged.exists(),
        "successful publication must remove the bounded staging name"
    );
    let entries = std::fs::read_dir(&manifest.conmon_layout.container_state_dir)
        .expect("runner state directory should inspect")
        .map(|entry| {
            entry
                .expect("runner state entry should inspect")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect::<Vec<_>>();
    assert!(
        entries
            .iter()
            .all(|name| !name.starts_with(".nimbus-runner-handoff-decision.")
                || name == super::super::runner::RUNNER_HANDOFF_DECISION_FILE),
        "no unique or stale decision staging files may survive publication: {entries:?}"
    );
}

#[test]
fn runner_handoff_stage_failures_leave_no_orphans_and_retry() {
    for (suffix, fault) in [
        (
            "create",
            super::super::runner::RunnerDecisionStageFault::AfterCreate,
        ),
        (
            "write",
            super::super::runner::RunnerDecisionStageFault::AfterWrite,
        ),
    ] {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let backend = sample_plan_only_backend(temp_dir.path());
        let mut manifest = backend
            .plan_start_with_id(
                &sample_spec(),
                &SandboxId::new(format!("runner-stage-failure-{suffix}")),
                None,
                None,
            )
            .expect("runner fixture should plan")
            .manifest;
        mark_prepared_service_runner(&mut manifest);
        backend
            .write_manifest(&manifest)
            .expect("prepared manifest should be durable");

        let error = super::super::runner::claim_runner_execution_with_stage_fault_for_test(
            &manifest, fault,
        )
        .expect_err("injected staging boundary should fail");
        assert!(
            error
                .to_string()
                .contains("injected runner decision failure"),
            "the exact injected boundary should be visible: {error}"
        );
        let staged = manifest
            .conmon_layout
            .container_state_dir
            .join(".nimbus-runner-handoff-decision.stage");
        assert!(
            !staged.exists(),
            "failed staging must durably remove its bounded temporary file"
        );
        assert!(
            !manifest
                .conmon_layout
                .container_state_dir
                .join(super::super::runner::RUNNER_HANDOFF_DECISION_FILE)
                .exists(),
            "a pre-publication failure must not synthesize a durable decision"
        );

        super::super::runner::claim_runner_execution_for_test(&manifest)
            .expect("a clean retry should publish the authenticated decision");
        assert!(
            !staged.exists(),
            "successful retry must also leave no staging residue"
        );
    }
}

#[test]
fn durable_execute_manifest_allows_explicit_stop_after_owner_death_before_effects() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = sample_plan_only_backend(temp_dir.path());
    let mut plan = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("http", 18091, 8080)),
            &SandboxId::new("runner-owner-loss-before-effects"),
            None,
            None,
        )
        .expect("runner fixture should plan");
    backend
        .attach_runner_owned_egress_proxy(&mut plan)
        .expect("runner fixture should reserve its complete launch authority");
    let mut first_owner = plan.manifest;
    mark_prepared_service_runner(&mut first_owner);
    backend
        .write_manifest(&first_owner)
        .expect("the prepared manifest should be the durable handoff barrier");
    super::super::runner::persist_runner_execution_ownership(&backend, &mut first_owner)
        .expect("first owner should durably claim execution");
    assert_eq!(first_owner.start_mode, ContainerStartMode::Execute);
    let before_inspect = std::fs::read(&first_owner.conmon_layout.manifest_path)
        .expect("execute manifest bytes should read");
    backend
        .inspect_sync(&first_owner.handle.id)
        .expect("runner-owned pre-effect inspection should remain read-only")
        .expect("runner-owned workload should remain visible");
    assert_eq!(
        std::fs::read(&first_owner.conmon_layout.manifest_path)
            .expect("execute manifest bytes should reread"),
        before_inspect,
        "inspection must not mutate or launch behind the runner handoff fence"
    );
    let status_error = backend
        .refresh_plan_only_service_workload_status(&first_owner.handle.id, SandboxStatus::Ready)
        .expect_err("external status writes must not race the runner-owned manifest");
    assert!(
        status_error
            .to_string()
            .contains("external status mutation remains fenced"),
        "status rejection must name the runner fence: {status_error}"
    );

    backend
        .stop_sync(&first_owner.handle.id)
        .expect("explicit stop must own authenticated pre-effect compensation");
    let stopped = backend
        .read_manifest(&first_owner.handle.id)
        .expect("stopped manifest should inspect")
        .expect("stopped manifest should remain durable");
    assert_eq!(stopped.status, SandboxStatus::Stopped);
    assert!(stopped.shutdown_requested);
    assert!(stopped.network_cleanup_complete);
    assert!(stopped.launch_reservation_claim.is_none());
    assert_eq!(
        super::super::runner::execute_handoff_phase(&stopped)
            .expect("terminal handoff should authenticate"),
        None,
        "explicit stop must publish terminal lifecycle ownership"
    );
}

#[test]
fn owner_death_after_effects_started_fails_closed_for_inspect_before_retry() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = sample_plan_only_backend(temp_dir.path());
    let mut first_owner = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("http", 18092, 8080)),
            &SandboxId::new("runner-owner-loss-after-effects"),
            None,
            None,
        )
        .expect("runner fixture should reserve its complete launch authority")
        .manifest;
    mark_prepared_service_runner(&mut first_owner);
    backend
        .write_manifest(&first_owner)
        .expect("the prepared manifest should be the durable handoff barrier");
    let handoff =
        super::super::runner::persist_runner_execution_ownership(&backend, &mut first_owner)
            .expect("first owner should durably claim execution");
    super::super::runner::mark_runner_effects_started(&first_owner, &handoff)
        .expect("the effect boundary must become durable before provider execution");
    drop(handoff);

    let mut successor = backend
        .read_manifest(&first_owner.handle.id)
        .expect("execute manifest should inspect")
        .expect("execute manifest should remain durable");
    let error = super::super::runner::persist_runner_execution_ownership(&backend, &mut successor)
        .expect_err("owner loss after the effect boundary must not replay provider effects");
    assert!(
        error.to_string().contains("inspect-before-retry"),
        "the fenced replay must name its reconciliation obligation: {error}"
    );
    assert_eq!(
        successor, first_owner,
        "rejected replay must not mutate the durable Execute manifest"
    );
}

#[test]
fn durable_execute_decision_fences_plan_only_cancellation_before_manifest_transition() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = sample_plan_only_backend(temp_dir.path());
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("http", 18086, 8080)),
            &SandboxId::new("runner-execute-cancel-race"),
            None,
            None,
        )
        .expect("runner fixture should reserve its complete launch authority")
        .manifest;
    mark_prepared_service_runner(&mut manifest);
    backend
        .write_manifest(&manifest)
        .expect("the prepared manifest should be the durable handoff barrier");
    let mut launch_batch = manifest.port_leases.clone();
    launch_batch.extend(
        manifest
            .egress_proxy
            .as_ref()
            .map(|assignment| assignment.port_lease.clone()),
    );
    let decision_path = super::super::runner::claim_runner_execution_for_test(&manifest)
        .expect("runner should durably win execution before the manifest transition");

    let cancellation_error = backend
        .stop_sync(&manifest.handle.id)
        .expect_err("cancellation must honor the durable Execute decision");
    assert!(
        cancellation_error
            .to_string()
            .contains("already decided as Execute"),
        "the losing cancellation must name the handoff winner: {cancellation_error}"
    );
    let authority =
        nimbus_network::LocalPortLeaseAuthority::open(&backend.config.network_state_root)
            .expect("port authority should reopen");
    for request in &launch_batch {
        assert_eq!(
            authority
                .inspect(request.lease_id())
                .expect("fenced lease should inspect")
                .expect("fenced lease should remain durable")
                .phase(),
            PortLeasePhase::Reserved,
            "the losing cancellation must not release runner-owned authority"
        );
    }
    assert_eq!(
        backend
            .read_manifest(&manifest.handle.id)
            .expect("manifest should inspect")
            .expect("manifest should remain durable")
            .start_mode,
        ContainerStartMode::PlanOnly,
        "the test boundary must remain parked before the runner manifest transition"
    );

    super::super::runner::persist_claimed_runner_execution_for_test(
        &backend,
        &mut manifest,
        &decision_path,
    )
    .expect("the Execute winner should complete its durable manifest transition");
    assert_eq!(
        backend
            .read_manifest(&manifest.handle.id)
            .expect("manifest should inspect")
            .expect("manifest should remain durable")
            .start_mode,
        ContainerStartMode::Execute
    );
}

#[test]
fn durable_execute_decision_fences_plan_only_status_refresh_without_manifest_mutation() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = sample_plan_only_backend(temp_dir.path());
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("http", 18089, 8080)),
            &SandboxId::new("runner-execute-status-race"),
            None,
            None,
        )
        .expect("runner fixture should reserve its complete launch authority")
        .manifest;
    mark_prepared_service_runner(&mut manifest);
    backend
        .write_manifest(&manifest)
        .expect("the prepared manifest should be the durable handoff barrier");
    super::super::runner::claim_runner_execution_for_test(&manifest)
        .expect("runner should durably win execution before the manifest transition");
    let before = std::fs::read(&manifest.conmon_layout.manifest_path)
        .expect("prepared manifest bytes should read");

    let error = backend
        .refresh_plan_only_service_workload_status(&manifest.handle.id, SandboxStatus::Ready)
        .expect_err("status refresh must honor the durable Execute decision");
    assert!(
        error.to_string().contains("already decided as Execute"),
        "the losing status refresh must name the handoff winner: {error}"
    );
    assert_eq!(
        std::fs::read(&manifest.conmon_layout.manifest_path)
            .expect("fenced manifest bytes should read"),
        before,
        "a losing status refresh must not rewrite the prepared-manifest fingerprint"
    );
}

#[test]
fn durable_cancel_decision_fences_a_stale_runner_and_replays_idempotently() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = sample_plan_only_backend(temp_dir.path());
    let mut stale_runner_manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("http", 18087, 8080)),
            &SandboxId::new("runner-cancel-execute-race"),
            None,
            None,
        )
        .expect("runner fixture should reserve its complete launch authority")
        .manifest;
    mark_prepared_service_runner(&mut stale_runner_manifest);
    backend
        .write_manifest(&stale_runner_manifest)
        .expect("the prepared manifest should be the durable handoff barrier");

    backend
        .stop_sync(&stale_runner_manifest.handle.id)
        .expect("cancellation should durably win the unclaimed handoff");
    backend
        .stop_sync(&stale_runner_manifest.handle.id)
        .expect("terminal cancellation replay should be idempotent");
    let runner_error = super::super::runner::persist_runner_execution_ownership(
        &backend,
        &mut stale_runner_manifest,
    )
    .expect_err("the stale runner must honor the durable Cancel decision");
    assert!(
        runner_error
            .to_string()
            .contains("already decided as Cancel"),
        "the losing runner must name the handoff winner: {runner_error}"
    );
    let persisted = backend
        .read_manifest(&stale_runner_manifest.handle.id)
        .expect("manifest should inspect")
        .expect("manifest should remain durable");
    assert_eq!(persisted.start_mode, ContainerStartMode::PlanOnly);
    assert_eq!(persisted.status, SandboxStatus::Stopped);
    assert!(persisted.shutdown_requested);
}

#[test]
fn corrupt_runner_handoff_decision_fences_execution_and_cancellation_without_release() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = sample_plan_only_backend(temp_dir.path());
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("http", 18088, 8080)),
            &SandboxId::new("runner-corrupt-handoff"),
            None,
            None,
        )
        .expect("runner fixture should reserve its complete launch authority")
        .manifest;
    mark_prepared_service_runner(&mut manifest);
    backend
        .write_manifest(&manifest)
        .expect("the prepared manifest should be the durable handoff barrier");
    let decision_path = manifest
        .conmon_layout
        .container_state_dir
        .join(super::super::runner::RUNNER_HANDOFF_DECISION_FILE);
    std::fs::write(&decision_path, b"{not-json\n").expect("corrupt decision fixture should write");

    let runner_error =
        super::super::runner::persist_runner_execution_ownership(&backend, &mut manifest)
            .expect_err("corrupt handoff evidence must fence execution");
    let cancellation_error = backend
        .stop_sync(&manifest.handle.id)
        .expect_err("corrupt handoff evidence must fence cancellation");
    for error in [runner_error, cancellation_error] {
        assert!(
            error.to_string().contains("failed to parse durable"),
            "corrupt handoff rejection must name the durable evidence: {error}"
        );
    }
    let authority =
        nimbus_network::LocalPortLeaseAuthority::open(&backend.config.network_state_root)
            .expect("port authority should reopen");
    assert!(
        manifest.port_leases.iter().all(|request| {
            authority
                .inspect(request.lease_id())
                .expect("lease should inspect")
                .is_some_and(|record| record.phase() == PortLeasePhase::Reserved)
        }),
        "corrupt handoff evidence must not release any launch port"
    );
}

#[test]
fn direct_execute_effect_fence_precedes_every_provider_probe() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(temp_dir.path()))
            .with_runner_handoff_failure(RunnerHandoffFailure::DirectAfterEffectFence);
    let plan = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("http", 18101, 8080)),
            &SandboxId::new("direct-effect-fence"),
            None,
            None,
        )
        .expect("direct launch should reserve complete authority");
    let planned = plan.manifest.clone();

    let error = backend
        .finish_start(plan)
        .expect_err("the injected boundary must stop before provider execution");
    assert!(
        error.to_string().contains("after durable effect fence"),
        "the injected crash cut must remain visible: {error}"
    );
    let persisted = backend
        .read_manifest(&planned.handle.id)
        .expect("direct manifest should inspect")
        .expect("direct manifest must be durable before the effect fence");
    let mut expected = planned;
    expected.runner_handoff_id = persisted.runner_handoff_id.clone();
    assert!(
        expected.runner_handoff_id.is_some(),
        "the effect fence must persist the exact winning runner generation"
    );
    assert_eq!(
        persisted, expected,
        "claiming execution may add only the exact winning runner generation before provider effects"
    );
    assert_eq!(
        super::super::runner::execute_handoff_phase(&persisted)
            .expect("durable decision should authenticate"),
        Some(super::super::runner::RunnerHandoffPhase::EffectsStarted),
        "provider ambiguity must be fenced before the first effect"
    );
    assert!(
        !persisted.network_layout.netns_path.exists()
            && !persisted.network_layout.status_path.exists(),
        "no Netavark or namespace effect may precede the durable effect fence"
    );
    let authority =
        nimbus_network::LocalPortLeaseAuthority::open(&backend.config.network_state_root)
            .expect("port authority should reopen");
    for request in persisted.port_leases.iter().chain(
        persisted
            .egress_proxy
            .as_ref()
            .map(|assignment| &assignment.port_lease),
    ) {
        let record = authority
            .inspect(request.lease_id())
            .expect("lease should inspect")
            .expect("fenced lease should remain durable");
        assert_eq!(record.phase(), PortLeasePhase::Reserved);
        assert!(
            record.bind_claim().is_none() && record.binding().is_none(),
            "the effect-fence crash cut must precede every provider bind attempt"
        );
    }
}

#[test]
fn effects_started_phase_remains_verifiable_after_manifest_progress() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = sample_plan_only_backend(temp_dir.path());
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("http", 18102, 8080)),
            &SandboxId::new("runner-stable-effect-identity"),
            None,
            None,
        )
        .expect("runner fixture should reserve complete authority")
        .manifest;
    mark_prepared_service_runner(&mut manifest);
    backend
        .write_manifest(&manifest)
        .expect("prepared manifest should be durable");
    let handoff = super::super::runner::persist_runner_execution_ownership(&backend, &mut manifest)
        .expect("runner should claim execution");
    super::super::runner::mark_runner_effects_started(&manifest, &handoff)
        .expect("effect boundary should become durable");
    drop(handoff);

    manifest.launch_reservation_claim = None;
    manifest.launch_artifact = None;
    manifest.last_exit_code = Some(0);
    manifest.restart_count = 2;
    manifest.next_restart_at_millis = Some(42);
    manifest.shutdown_requested = true;
    synchronize_handle_status(&mut manifest, SandboxStatus::Stopping);
    backend
        .write_manifest(&manifest)
        .expect("ordinary lifecycle progress should persist");

    assert_eq!(
        super::super::runner::execute_handoff_phase(&manifest)
            .expect("immutable execution identity should survive lifecycle progress"),
        Some(super::super::runner::RunnerHandoffPhase::EffectsStarted)
    );
    let error = backend
        .refresh_plan_only_service_workload_status(&manifest.handle.id, SandboxStatus::Ready)
        .expect_err("external status mutation must remain fenced after lifecycle progress");
    assert!(
        error
            .to_string()
            .contains("external status mutation remains fenced"),
        "the stable phase identity must drive an explicit fence: {error}"
    );
}

#[test]
fn published_runner_lifecycle_releases_start_lock_and_fences_execution_replay() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = sample_plan_only_backend(temp_dir.path());
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("http", 18104, 8080)),
            &SandboxId::new("runner-published-lifecycle"),
            None,
            None,
        )
        .expect("runner fixture should reserve complete authority")
        .manifest;
    mark_prepared_service_runner(&mut manifest);
    backend
        .write_manifest(&manifest)
        .expect("prepared manifest should be durable");
    let handoff = super::super::runner::persist_runner_execution_ownership(&backend, &mut manifest)
        .expect("runner should claim execution");
    super::super::runner::mark_runner_effects_started(&manifest, &handoff)
        .expect("effect boundary should become durable");
    publish_present_runner_lifecycle(&manifest, &handoff);
    drop(handoff);

    assert_eq!(
        super::super::runner::execute_handoff_phase(&manifest)
            .expect("published decision should authenticate"),
        None,
        "ordinary lifecycle operations must not treat publication as an active start fence"
    );
    let lifecycle = super::super::runner::lock_execute_lifecycle(&manifest)
        .expect("stop/inspect lifecycle ownership should be immediately acquirable");
    drop(lifecycle);

    let mut replay = manifest.clone();
    let error = super::super::runner::persist_runner_execution_ownership(&backend, &mut replay)
        .expect_err("published lifecycle must never replay initial provider effects");
    assert!(
        error.to_string().contains("lifecycle is already published"),
        "replay rejection must name the durable terminal handoff phase: {error}"
    );
}

#[test]
fn plan_only_status_update_rejects_direct_coordinator_before_locking() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = sample_plan_only_backend(temp_dir.path());
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("execute-status-owner-lock"),
            None,
            None,
        )
        .expect("plan-only fixture should prepare")
        .manifest;
    manifest.start_mode = ContainerStartMode::Execute;
    backend
        .write_manifest(&manifest)
        .expect("execute-shaped crash cut should persist");
    let before =
        std::fs::read(&manifest.conmon_layout.manifest_path).expect("manifest bytes should read");
    let owner = super::super::runner::lock_execute_lifecycle(&manifest)
        .expect("direct owner should hold the lifecycle lock");
    let result = backend
        .refresh_plan_only_service_workload_status(&manifest.handle.id, SandboxStatus::Ready);
    drop(owner);

    let error = result.expect_err("status mutation must not pass a live Execute owner");
    assert!(
        error
            .to_string()
            .contains("requires the PreparedServiceRunner lifecycle coordinator"),
        "the rejection must name the authenticated lifecycle coordinator: {error}"
    );
    assert_eq!(
        std::fs::read(&manifest.conmon_layout.manifest_path).expect("manifest bytes should reread"),
        before,
        "the losing status writer must not mutate the direct-owner crash cut"
    );
}

fn direct_status_callback_fixture(
    root: &std::path::Path,
    id: &str,
) -> (
    ContainerSandboxBackend,
    ContainerSandboxBackend,
    ContainerSandboxManifest,
) {
    let direct = ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(root));
    let manifest = direct
        .plan_start_with_id(&sample_spec(), &SandboxId::new(id), None, None)
        .expect("direct fixture should reserve complete authority")
        .manifest;
    direct
        .write_manifest(&manifest)
        .expect("direct Execute manifest should be durable");
    (direct, sample_plan_only_backend(root), manifest)
}

#[test]
fn plan_only_status_callback_rejects_direct_execute_no_decision_without_mutation() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let (_direct, observer, manifest) =
        direct_status_callback_fixture(temp_dir.path(), "direct-status-no-decision");
    let before = std::fs::read(&manifest.conmon_layout.manifest_path)
        .expect("direct manifest bytes should read");

    let error = observer
        .refresh_plan_only_service_workload_status(&manifest.handle.id, SandboxStatus::Ready)
        .expect_err("a plan-only callback must not mutate a direct Execute crash cut");

    assert!(
        error.to_string().contains("PreparedServiceRunner")
            && error.to_string().contains("DirectBackend"),
        "the rejection must name both lifecycle coordinators: {error}"
    );
    assert_eq!(
        std::fs::read(&manifest.conmon_layout.manifest_path)
            .expect("direct manifest bytes should reread"),
        before,
        "coordinator rejection must precede direct-manifest persistence"
    );
}

#[test]
fn plan_only_status_callback_rejects_published_direct_execute_without_mutation() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let (direct, observer, mut manifest) =
        direct_status_callback_fixture(temp_dir.path(), "direct-status-published");
    let handoff = super::super::runner::persist_direct_execution_ownership(&direct, &mut manifest)
        .expect("direct fixture should claim execution");
    super::super::runner::mark_runner_effects_started(&manifest, &handoff)
        .expect("direct effect boundary should become durable");
    publish_present_runner_lifecycle(&manifest, &handoff);
    drop(handoff);
    let before = std::fs::read(&manifest.conmon_layout.manifest_path)
        .expect("published direct manifest bytes should read");

    let error = observer
        .refresh_plan_only_service_workload_status(&manifest.handle.id, SandboxStatus::Ready)
        .expect_err("a plan-only callback must not mutate a published direct lifecycle");

    assert!(
        error.to_string().contains("PreparedServiceRunner")
            && error.to_string().contains("DirectBackend"),
        "the rejection must name both lifecycle coordinators: {error}"
    );
    assert_eq!(
        std::fs::read(&manifest.conmon_layout.manifest_path)
            .expect("published direct manifest bytes should reread"),
        before,
        "LifecyclePublished must not erase the immutable coordinator boundary"
    );
}

#[test]
fn lifecycle_coordinator_fences_cross_coordinator_execution_claims() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let direct =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(temp_dir.path()));
    let mut direct_manifest = direct
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("direct-runner-claim"),
            None,
            None,
        )
        .expect("direct fixture should reserve complete authority")
        .manifest;
    direct
        .write_manifest(&direct_manifest)
        .expect("direct manifest should be durable");
    let runner_error =
        super::super::runner::persist_runner_execution_ownership(&direct, &mut direct_manifest)
            .expect_err("runner ownership must reject a direct lifecycle coordinator");
    assert!(
        runner_error.to_string().contains("PreparedServiceRunner")
            && runner_error.to_string().contains("DirectBackend"),
        "runner rejection must name the coordinator mismatch: {runner_error}"
    );

    let prepared_backend = sample_plan_only_backend(temp_dir.path());
    let mut prepared_manifest = prepared_backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("runner-direct-claim"),
            None,
            None,
        )
        .expect("prepared fixture should plan")
        .manifest;
    mark_prepared_service_runner(&mut prepared_manifest);
    prepared_manifest.start_mode = ContainerStartMode::Execute;
    prepared_backend
        .write_manifest(&prepared_manifest)
        .expect("prepared-runner Execute fixture should be durable");
    let direct_error = super::super::runner::persist_direct_execution_ownership(
        &prepared_backend,
        &mut prepared_manifest,
    )
    .expect_err("direct ownership must reject a prepared-runner lifecycle coordinator");
    assert!(
        direct_error.to_string().contains("DirectBackend")
            && direct_error.to_string().contains("PreparedServiceRunner"),
        "direct rejection must name the coordinator mismatch: {direct_error}"
    );
    for manifest in [&direct_manifest, &prepared_manifest] {
        assert!(
            !manifest
                .conmon_layout
                .container_state_dir
                .join(super::super::runner::RUNNER_HANDOFF_DECISION_FILE)
                .exists(),
            "coordinator rejection must precede durable execution-claim publication"
        );
    }
}

#[test]
fn lifecycle_coordinator_is_required_durable_manifest_evidence() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let (_direct, _observer, manifest) =
        direct_status_callback_fixture(temp_dir.path(), "missing-lifecycle-coordinator");
    let mut rendered = serde_json::to_value(manifest).expect("manifest should serialize");
    rendered
        .as_object_mut()
        .expect("manifest should be an object")
        .remove("lifecycle_coordinator");

    let error = serde_json::from_value::<ContainerSandboxManifest>(rendered)
        .expect_err("a manifest without its lifecycle coordinator must fail closed");

    assert!(
        error.to_string().contains("lifecycle_coordinator"),
        "missing durable coordinator evidence must be explicit: {error}"
    );
}

#[test]
fn plan_only_stop_callback_rejects_direct_execute_before_cleanup_authority() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let tenant = sample_spec().tenant_id;
    let recorder = Arc::new(RecordingSegmentAllocator::new(tenant, "10.79.0.0/24", 79));
    let direct_allocator: Arc<OciSegmentAllocator> = recorder.clone();
    let observer_allocator: Arc<OciSegmentAllocator> = recorder.clone();
    let direct = ContainerSandboxBackend::with_segment_allocator(
        ContainerSandboxBackendConfig::under_root(temp_dir.path()),
        direct_allocator,
    );
    let observer = ContainerSandboxBackend::with_segment_allocator(
        ContainerSandboxBackendConfig::plan_only(
            temp_dir.path().join("bundles"),
            temp_dir.path().join("state"),
        ),
        observer_allocator,
    );
    let manifest = direct
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("direct-stop-callback"),
            None,
            None,
        )
        .expect("direct fixture should reserve complete authority")
        .manifest;
    direct
        .write_manifest(&manifest)
        .expect("direct Execute manifest should be durable");
    let before_manifest = std::fs::read(&manifest.conmon_layout.manifest_path)
        .expect("direct manifest bytes should read");
    let before_operations = recorder.operations();
    let authority =
        nimbus_network::LocalPortLeaseAuthority::open(&direct.config.network_state_root)
            .expect("port authority should reopen");
    let before_ports = authority.list().expect("port leases should list");

    let error = observer
        .mark_plan_only_service_workload_stopped(&manifest.handle.id)
        .expect_err("a plan-only stop callback must not acquire direct cleanup authority");

    assert!(
        error.to_string().contains("PreparedServiceRunner")
            && error.to_string().contains("DirectBackend"),
        "the rejection must name both lifecycle coordinators: {error}"
    );
    assert_eq!(
        std::fs::read(&manifest.conmon_layout.manifest_path)
            .expect("direct manifest bytes should reread"),
        before_manifest
    );
    assert_eq!(
        recorder.operations(),
        before_operations,
        "coordinator rejection must precede segment or provider cleanup"
    );
    assert_eq!(
        authority.list().expect("port leases should relist"),
        before_ports,
        "coordinator rejection must preserve exact port authority"
    );
}

#[test]
fn direct_stop_after_predecision_owner_death_releases_only_unstarted_authority() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(temp_dir.path()));
    let manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("http", 18103, 8080)),
            &SandboxId::new("direct-predecision-stop"),
            None,
            None,
        )
        .expect("direct fixture should reserve complete authority")
        .manifest;
    backend
        .write_manifest(&manifest)
        .expect("Execute manifest should become durable before the decision");

    backend
        .stop_sync(&manifest.handle.id)
        .expect("owner death before a decision may compensate only unstarted authority");
    let stopped = backend
        .read_manifest(&manifest.handle.id)
        .expect("stopped manifest should inspect")
        .expect("stopped manifest should remain durable");
    assert_eq!(stopped.status, SandboxStatus::Stopped);
    assert!(stopped.shutdown_requested);
    assert!(stopped.launch_reservation_claim.is_none());
    assert_eq!(
        super::super::runner::execute_handoff_phase(&stopped)
            .expect("no decision should remain after no-effect compensation"),
        None
    );
    let authority =
        nimbus_network::LocalPortLeaseAuthority::open(&backend.config.network_state_root)
            .expect("port authority should reopen");
    for request in manifest.port_leases.iter().chain(
        manifest
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
            PortLeasePhase::Released
        );
    }
}

#[test]
fn terminal_manifest_write_failure_converges_before_lifecycle_publication() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(temp_dir.path()))
            .with_runner_handoff_failure(RunnerHandoffFailure::DirectTerminalManifest);
    let mut plan = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("direct-terminal-persistence-fence"),
            None,
            None,
        )
        .expect("direct launch should reserve complete authority");
    mark_runtime_absent_for_cleanup(&mut plan.manifest);
    backend
        .write_manifest(&plan.manifest)
        .expect("runtime-absence fixture must remain the durable launch plan");
    let initial = plan.manifest.clone();

    let error = backend
        .finish_start(plan)
        .expect_err("terminal persistence injection must fail the launch");
    assert!(
        error
            .to_string()
            .contains("injected direct terminal manifest failure"),
        "the persistence failure must remain visible: {error}"
    );
    let persisted = backend
        .read_manifest(&initial.handle.id)
        .expect("initial manifest should inspect")
        .expect("initial manifest must remain durable");
    assert_eq!(persisted.status, SandboxStatus::Stopped);
    assert_eq!(persisted.handle.status, SandboxStatus::Stopped);
    assert!(persisted.shutdown_requested);
    assert!(persisted.launch_reservation_claim.is_none());
    assert!(persisted.launch_artifact.is_none());
    assert_eq!(
        super::super::runner::execute_handoff_phase(&persisted)
            .expect("the terminal execution identity should remain verifiable"),
        None,
        "lifecycle ownership may publish only after the terminal cleanup receipt is durable"
    );
    backend
        .stop_sync(&initial.handle.id)
        .expect("terminal cleanup replay should be idempotent after ownership publication");
}

#[test]
fn container_preflight_failure_compensates_all_unstarted_launch_artifacts() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(temp_dir.path()));
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("http", 18079, 8080)),
            &SandboxId::new("container-preflight-cleanup"),
            None,
            None,
        )
        .expect("launch should reserve complete network authority")
        .manifest;
    let trust_anchor = egress_trust_anchor_root(&backend.config.workload_state_root)
        .join(manifest.spec.tenant_id.as_str())
        .join(format!("{}.pem", manifest.handle.id.as_str()));
    assert!(
        trust_anchor.is_file(),
        "planning should materialize the unactivated trust anchor"
    );

    let error = backend
        .execute_start_after_preflight(
            &mut manifest,
            Err(SandboxError::BackendUnavailable {
                message: "forced container pre-provider rejection".to_owned(),
            }),
        )
        .expect_err("pre-provider rejection should fail the launch");
    assert!(
        error
            .to_string()
            .contains("forced container pre-provider rejection"),
        "the original preflight failure must remain primary: {error}"
    );

    let authority =
        nimbus_network::LocalPortLeaseAuthority::open(&backend.config.network_state_root)
            .expect("port authority should reopen");
    let records = authority.list().expect("port leases should list");
    assert!(
        !records.is_empty()
            && records
                .iter()
                .all(|record| record.phase() == PortLeasePhase::Released),
        "every never-bound publication and PEP reservation must be released: {records:?}"
    );
    assert!(
        backend
            .segment_allocator
            .inspect_segments(&manifest.spec.tenant_id)
            .expect("segment authority should inspect")
            .unwrap_or_default()
            .is_empty(),
        "pre-provider compensation must remove IPAM and finalize the reserved attachment"
    );
    assert!(
        !trust_anchor.exists(),
        "pre-provider compensation must remove the never-activated trust anchor"
    );
}

#[test]
fn adopted_container_attachment_cleanup_releases_never_bound_launch_authority() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.netavark_path = PathBuf::from("/usr/bin/true");
    let backend = ContainerSandboxBackend::new(config);
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("http", 18080, 8080)),
            &SandboxId::new("container-adopted-cleanup"),
            None,
            None,
        )
        .expect("launch should reserve attachment, IPAM, publication, and PEP authority")
        .manifest;
    let claim = manifest
        .launch_reservation_claim
        .clone()
        .expect("initial launch should retain its coordinator claim");
    backend
        .segment_allocator
        .adopt_reserved_attachment(
            &manifest.spec.tenant_id,
            &default_network_attachment_id(&manifest.handle.id),
            &claim,
        )
        .expect("the fail-before boundary follows attachment adoption");
    let mut launch_batch = manifest.port_leases.clone();
    launch_batch.push(
        manifest
            .egress_proxy
            .as_ref()
            .expect("execute launch should reserve its PEP")
            .port_lease
            .clone(),
    );
    mark_runtime_absent_for_cleanup(&mut manifest);

    backend
        .release_execution_artifacts(&mut manifest)
        .expect("confirmed provider absence should compensate mixed adopted/reserved authority");

    let authority =
        nimbus_network::LocalPortLeaseAuthority::open(&backend.config.network_state_root)
            .expect("port authority should reopen");
    for request in &launch_batch {
        let record = authority
            .inspect(request.lease_id())
            .expect("lease should inspect")
            .expect("released evidence should remain durable");
        assert_eq!(
            record.phase(),
            PortLeasePhase::Released,
            "every never-bound launch port must release only after provider absence"
        );
    }
    assert!(
        backend
            .segment_allocator
            .inspect_segments(&manifest.spec.tenant_id)
            .expect("segment authority should inspect")
            .unwrap_or_default()
            .is_empty(),
        "cleanup must remove IPAM before releasing and finalizing the adopted attachment"
    );
}

#[test]
fn container_cleanup_retains_network_authority_until_runtime_absence_is_observed() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.netavark_path = PathBuf::from("/usr/bin/true");
    let backend = ContainerSandboxBackend::new(config);
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("http", 18094, 8080)),
            &SandboxId::new("container-runtime-still-present"),
            None,
            None,
        )
        .expect("launch should reserve complete network authority")
        .manifest;
    let claim = manifest
        .launch_reservation_claim
        .clone()
        .expect("initial launch should retain exact reservation authority");
    backend
        .segment_allocator
        .adopt_reserved_attachment(
            &manifest.spec.tenant_id,
            &default_network_attachment_id(&manifest.handle.id),
            &claim,
        )
        .expect("fixture should cross the attachment-adoption boundary");
    manifest.conmon_launch.delete_command = CommandSpec::new("/usr/bin/true");
    manifest.conmon_launch.state_command = CommandSpec::new("/bin/sh").args([
        "-c",
        "printf '%s' '{\"id\":\"container-runtime-still-present\",\"status\":\"running\"}'",
    ]);

    let error = backend
        .release_execution_artifacts(&mut manifest)
        .expect_err("a still-observed runtime must fence provider detach");

    assert!(
        error
            .to_string()
            .contains("remains \"running\" after delete attempt"),
        "cleanup failure must report the exact observed runtime state: {error}"
    );
    assert_eq!(
        manifest.launch_reservation_claim.as_ref(),
        Some(&claim),
        "the retry capability must remain durable while runtime absence is unconfirmed"
    );
    assert!(
        !backend
            .segment_allocator
            .inspect_segments(&manifest.spec.tenant_id)
            .expect("segment authority should inspect")
            .unwrap_or_default()
            .is_empty(),
        "the adopted attachment must remain fenced beneath an observed live runtime"
    );
    assert!(
        crate::backends::oci::network::inspect_container_ips(
            &backend.ipam_authority,
            &manifest.network_layout,
            &manifest.handle.id,
        )
        .is_ok(),
        "IPAM must remain fenced for a later cleanup retry"
    );
}

#[test]
fn container_cleanup_rejects_unknown_runtime_observation_and_retains_authority() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.netavark_path = PathBuf::from("/usr/bin/true");
    let backend = ContainerSandboxBackend::new(config);
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("http", 18095, 8080)),
            &SandboxId::new("container-runtime-observation-unknown"),
            None,
            None,
        )
        .expect("launch should reserve complete network authority")
        .manifest;
    let claim = manifest
        .launch_reservation_claim
        .clone()
        .expect("initial launch should retain exact reservation authority");
    backend
        .segment_allocator
        .adopt_reserved_attachment(
            &manifest.spec.tenant_id,
            &default_network_attachment_id(&manifest.handle.id),
            &claim,
        )
        .expect("fixture should cross the attachment-adoption boundary");
    manifest.conmon_launch.delete_command =
        CommandSpec::new("/bin/sh").args(["-c", "printf 'delete denied\n' >&2; exit 1"]);
    manifest.conmon_launch.state_command = CommandSpec::new("/bin/sh").args([
        "-c",
        "printf 'runtime state database does not exist: permission denied\n' >&2; exit 1",
    ]);

    let error = backend
        .release_execution_artifacts(&mut manifest)
        .expect_err("unknown runtime state must fence every provider release");

    assert!(
        error
            .to_string()
            .contains("without explicit absence evidence")
            && error.to_string().contains("permission denied")
            && error.to_string().contains("delete denied"),
        "cleanup must retain both provider diagnostics: {error}"
    );
    let authority =
        nimbus_network::LocalPortLeaseAuthority::open(&backend.config.network_state_root)
            .expect("port authority should reopen");
    for request in manifest.port_leases.iter().chain(
        manifest
            .egress_proxy
            .iter()
            .map(|assignment| &assignment.port_lease),
    ) {
        let record = authority
            .inspect(request.lease_id())
            .expect("port lease should inspect")
            .expect("port lease must remain durable");
        assert_ne!(
            record.phase(),
            PortLeasePhase::Released,
            "unknown runtime state must retain every host-port fence"
        );
    }
    assert!(
        !backend
            .segment_allocator
            .inspect_segments(&manifest.spec.tenant_id)
            .expect("segment authority should inspect")
            .unwrap_or_default()
            .is_empty(),
        "unknown runtime state must retain the adopted attachment"
    );
    assert!(
        crate::backends::oci::network::inspect_container_ips(
            &backend.ipam_authority,
            &manifest.network_layout,
            &manifest.handle.id,
        )
        .is_ok(),
        "unknown runtime state must retain exact IPAM"
    );
}

#[test]
fn cleanup_failure_checkpoint_remains_stopping_with_exact_authority() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(temp_dir.path()));
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("http", 18096, 8080)),
            &SandboxId::new("container-cleanup-pending-checkpoint"),
            None,
            None,
        )
        .expect("launch should reserve complete network authority")
        .manifest;
    let claim = manifest
        .launch_reservation_claim
        .clone()
        .expect("launch should retain its exact cleanup authority");

    let error = backend.persist_failed_initial_launch(
        &mut manifest,
        SandboxError::OperationFailed {
            message: "injected launch failure".to_owned(),
        },
        Some(SandboxError::OperationFailed {
            message: "injected cleanup failure".to_owned(),
        }),
    );
    assert!(
        error.to_string().contains("injected launch failure")
            && error.to_string().contains("injected cleanup failure"),
        "both failure causes must remain visible: {error}"
    );

    let persisted = backend
        .read_manifest(&manifest.handle.id)
        .expect("cleanup checkpoint should inspect")
        .expect("cleanup checkpoint must remain durable");
    assert_eq!(
        persisted.status,
        SandboxStatus::Stopping,
        "cleanup-pending authority must never be published as terminal"
    );
    assert_eq!(persisted.handle.status, SandboxStatus::Stopping);
    assert!(persisted.shutdown_requested);
    assert_eq!(
        persisted.launch_reservation_claim.as_ref(),
        Some(&claim),
        "the exact retry capability must survive the crash checkpoint"
    );
}

#[test]
fn failed_netavark_setup_claims_reconcile_only_after_confirmed_detach() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.netavark_path = PathBuf::from("/usr/bin/true");
    let backend = ContainerSandboxBackend::new(config);
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("http", 18090, 8080)),
            &SandboxId::new("container-netavark-claim-reconcile"),
            None,
            None,
        )
        .expect("launch should reserve attachment, IPAM, publication, and PEP authority")
        .manifest;
    let launch_claim = manifest
        .launch_reservation_claim
        .clone()
        .expect("initial launch should retain its coordinator claim");
    backend
        .segment_allocator
        .adopt_reserved_attachment(
            &manifest.spec.tenant_id,
            &default_network_attachment_id(&manifest.handle.id),
            &launch_claim,
        )
        .expect("provider-attempt fixture follows attachment adoption");
    let port_lease_coordinator = backend.port_lease_coordinator();
    let lifetimes = port_lease_coordinator
        .claim_netavark_bindings_with_lifetimes(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("Netavark claims must be durable before its setup effect");
    let claims = lifetimes.claims().to_vec();
    assert!(
        !claims.is_empty(),
        "fixture must exercise a claimed publication"
    );
    drop(lifetimes);
    std::fs::create_dir_all(
        manifest
            .network_layout
            .netns_path
            .parent()
            .expect("netns path should have a parent"),
    )
    .expect("netns parent should exist");
    std::fs::write(&manifest.network_layout.netns_path, b"ambiguous-netns\n")
        .expect("ambiguous provider boundary should retain the netns handle");
    assert!(
        crate::backends::oci::network::inspect_container_ips(
            &backend.ipam_authority,
            &manifest.network_layout,
            &manifest.handle.id,
        )
        .is_ok(),
        "provider-attempt fixture must retain exact IPAM"
    );
    mark_runtime_absent_for_cleanup(&mut manifest);

    backend
        .release_execution_artifacts(&mut manifest)
        .expect("confirmed detach should abandon claims, release ports, then delete IPAM");

    let authority =
        nimbus_network::LocalPortLeaseAuthority::open(&backend.config.network_state_root)
            .expect("port authority should reopen");
    let mut launch_batch = manifest.port_leases.clone();
    launch_batch.push(
        manifest
            .egress_proxy
            .as_ref()
            .expect("execute launch should reserve its PEP")
            .port_lease
            .clone(),
    );
    let first_records = launch_batch
        .iter()
        .map(|request| {
            authority
                .inspect(request.lease_id())
                .expect("lease should inspect")
                .expect("terminal evidence should remain durable")
        })
        .collect::<Vec<_>>();
    assert!(
        first_records.iter().all(|record| {
            record.phase() == PortLeasePhase::Released && record.bind_claim().is_none()
        }),
        "exact detach reconciliation must release the complete launch batch: {first_records:?}"
    );
    assert!(
        crate::backends::oci::network::inspect_container_ips(
            &backend.ipam_authority,
            &manifest.network_layout,
            &manifest.handle.id,
        )
        .is_err(),
        "IPAM may be deleted only after provider detach and exact claim abandonment"
    );
    assert!(
        backend
            .segment_allocator
            .inspect_segments(&manifest.spec.tenant_id)
            .expect("segment authority should inspect")
            .unwrap_or_default()
            .is_empty(),
        "segment finalization must follow IPAM deletion"
    );
    assert!(manifest.launch_reservation_claim.is_none());

    if manifest.network_layout.netns_path.exists() {
        std::fs::remove_file(&manifest.network_layout.netns_path)
            .expect("non-Linux fake netns fixture should clean up");
    }
    backend
        .release_execution_artifacts(&mut manifest)
        .expect("terminal cleanup replay should be idempotent");
    let replayed = launch_batch
        .iter()
        .map(|request| {
            authority
                .inspect(request.lease_id())
                .expect("lease should inspect")
                .expect("terminal evidence should remain durable")
        })
        .collect::<Vec<_>>();
    assert_eq!(
        replayed, first_records,
        "cleanup replay must preserve exact terminal authority"
    );
}

#[test]
fn runner_launch_failure_after_attachment_adoption_compensates_network_authority() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.netavark_path = PathBuf::from("/usr/bin/true");
    let backend = ContainerSandboxBackend::new(config)
        .with_restart_launch_test_probe(RestartLaunchTestProbe::new(Duration::from_secs(1)));
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("http", 18081, 8080)),
            &SandboxId::new("runner-launch-cleanup"),
            None,
            None,
        )
        .expect("runner launch should reserve complete authority")
        .manifest;
    mark_prepared_service_runner(&mut manifest);
    let mut launch_batch = manifest.port_leases.clone();
    launch_batch.push(
        manifest
            .egress_proxy
            .as_ref()
            .expect("runner launch should reserve its PEP")
            .port_lease
            .clone(),
    );
    mark_runtime_absent_for_cleanup(&mut manifest);

    let error = backend
        .execute_start_after_preflight(&mut manifest, Ok(()))
        .expect_err("the injected post-adoption launch failure should be returned");
    assert!(
        error
            .to_string()
            .contains("cannot substitute for initial provider adoption"),
        "the primary launch failure must survive successful compensation: {error}"
    );

    let authority =
        nimbus_network::LocalPortLeaseAuthority::open(&backend.config.network_state_root)
            .expect("port authority should reopen");
    for request in &launch_batch {
        assert_eq!(
            authority
                .inspect(request.lease_id())
                .expect("lease should inspect")
                .expect("released evidence should remain durable")
                .phase(),
            PortLeasePhase::Released,
            "runner failure compensation must release every never-bound port"
        );
    }
    assert!(
        backend
            .segment_allocator
            .inspect_segments(&manifest.spec.tenant_id)
            .expect("segment authority should inspect")
            .unwrap_or_default()
            .is_empty(),
        "runner failure compensation must remove IPAM and finalize the adopted attachment"
    );
    let persisted = backend
        .read_manifest(&manifest.handle.id)
        .expect("compensated runner manifest should inspect")
        .expect("runner launch failure must retain terminal durable evidence");
    assert_eq!(persisted.start_mode, ContainerStartMode::Execute);
    assert_eq!(persisted.status, SandboxStatus::Stopped);
    assert!(persisted.shutdown_requested);
    assert!(persisted.launch_reservation_claim.is_none());
    assert!(persisted.launch_artifact.is_none());
}

#[test]
fn runner_exit_persists_execute_mode_after_owned_network_cleanup() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.netavark_path = PathBuf::from("/usr/bin/true");
    let backend = ContainerSandboxBackend::new(config);
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("http", 18082, 8080)),
            &SandboxId::new("runner-exit-cleanup"),
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

    super::super::runner::finalize_runner_exit(&backend, &mut manifest, 0)
        .expect("the effect-owning runner should complete teardown and persist it");

    let persisted = backend
        .read_manifest(&manifest.handle.id)
        .expect("runner manifest should inspect")
        .expect("runner manifest should remain durable");
    assert_eq!(persisted.start_mode, ContainerStartMode::Execute);
    assert_eq!(persisted.status, SandboxStatus::Stopped);
    assert_eq!(persisted.last_exit_code, Some(0));
    assert!(
        persisted.launch_reservation_claim.is_none(),
        "successful cleanup must retire the launch compensation capability"
    );
    assert!(
        backend
            .segment_allocator
            .inspect_segments(&manifest.spec.tenant_id)
            .expect("segment authority should inspect")
            .unwrap_or_default()
            .is_empty(),
        "runner exit must remove IPAM and finalize the adopted attachment"
    );
}

#[test]
fn runner_cleanup_retries_in_stopping_before_publishing_terminal_state() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(temp_dir.path()));
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("runner-cleanup-convergence"),
            None,
            None,
        )
        .expect("runner convergence fixture should plan")
        .manifest;
    mark_prepared_service_runner(&mut manifest);
    manifest.start_mode = ContainerStartMode::Execute;

    let events = RefCell::new(Vec::new());
    let cleanup_calls = Cell::new(0_u32);
    let fail_stopping_persistence_once = Cell::new(true);
    let fail_terminal_persistence_once = Cell::new(true);
    super::super::runner::converge_runner_cleanup_with(
        &mut manifest,
        SandboxStatus::Stopped,
        Some(7),
        |candidate| {
            events
                .borrow_mut()
                .push(format!("persist:{:?}", candidate.status));
            if candidate.status == SandboxStatus::Stopping
                && fail_stopping_persistence_once.replace(false)
            {
                return Err(SandboxError::OperationFailed {
                    message: "injected stopping persistence failure".to_owned(),
                });
            }
            if candidate.status == SandboxStatus::Stopped
                && fail_terminal_persistence_once.replace(false)
            {
                return Err(SandboxError::OperationFailed {
                    message: "injected terminal persistence failure".to_owned(),
                });
            }
            Ok(())
        },
        |candidate| {
            let call = cleanup_calls.get() + 1;
            cleanup_calls.set(call);
            events.borrow_mut().push(format!("cleanup:{call}"));
            if call == 1 {
                return Err(SandboxError::OperationFailed {
                    message: "injected provider cleanup failure".to_owned(),
                });
            }
            candidate.launch_reservation_claim = None;
            Ok(())
        },
        |stage, _error| events.borrow_mut().push(format!("wait:{stage:?}")),
    );

    assert_eq!(
        events.into_inner(),
        [
            "persist:Stopping",
            "wait:StoppingPersistence",
            "persist:Stopping",
            "cleanup:1",
            "persist:Stopping",
            "wait:ProviderCleanup",
            "cleanup:2",
            "persist:Stopped",
            "wait:TerminalPersistence",
            "persist:Stopped",
        ],
        "stopping persistence, cleanup, and terminal persistence retries must retain the same \
         owner and publish a terminal state only after provider cleanup succeeds"
    );
    assert_eq!(manifest.status, SandboxStatus::Stopped);
    assert_eq!(manifest.last_exit_code, Some(7));
    assert!(manifest.shutdown_requested);
    assert!(
        manifest.launch_reservation_claim.is_none(),
        "the terminal manifest must contain the successfully converged cleanup state"
    );
}

#[test]
fn runner_launch_result_converges_cleanup_and_handoff_before_return() {
    #[derive(Default)]
    struct LaunchState {
        cleaned: bool,
    }

    let events = RefCell::new(Vec::new());
    let publish_attempts = Cell::new(0_u32);
    let mut failed = LaunchState::default();
    let error = super::super::runner::converge_runner_launch_result_with(
        &mut failed,
        Err(SandboxError::OperationFailed {
            message: "injected runner launch failure".to_owned(),
        }),
        |state| {
            events.borrow_mut().push("cleanup");
            state.cleaned = true;
        },
        |state| {
            assert!(
                state.cleaned,
                "a failed launch must converge cleanup before lifecycle publication"
            );
            let attempt = publish_attempts.get() + 1;
            publish_attempts.set(attempt);
            events.borrow_mut().push(if attempt == 1 {
                "publish:1:error"
            } else {
                "publish:2:ok"
            });
            if attempt == 1 {
                Err(SandboxError::OperationFailed {
                    message: "injected lifecycle publication failure".to_owned(),
                })
            } else {
                Ok(())
            }
        },
        |_state, stage, _error| {
            events.borrow_mut().push(match stage {
                super::super::runner::RunnerOwnershipConvergenceStage::LifecyclePublished => {
                    "wait:LifecyclePublished"
                }
                super::super::runner::RunnerOwnershipConvergenceStage::EffectsStarted => {
                    panic!("launch completion must not retry the already-published effect fence")
                }
            });
        },
    )
    .expect_err("the original launch failure must remain primary after convergence");
    assert!(
        error.to_string().contains("injected runner launch failure"),
        "cleanup and publication convergence must preserve the primary launch error: {error}"
    );
    assert_eq!(
        events.into_inner(),
        [
            "cleanup",
            "publish:1:error",
            "wait:LifecyclePublished",
            "publish:2:ok",
        ],
        "the runner must not release its handoff before cleanup and publication converge"
    );

    let events = RefCell::new(Vec::new());
    let publish_attempts = Cell::new(0_u32);
    let mut launched = LaunchState::default();
    super::super::runner::converge_runner_launch_result_with(
        &mut launched,
        Ok(()),
        |_| panic!("a successful launch must not be torn down after a publication retry"),
        |_state| {
            let attempt = publish_attempts.get() + 1;
            publish_attempts.set(attempt);
            events.borrow_mut().push(format!("publish:{attempt}"));
            (attempt > 1)
                .then_some(())
                .ok_or_else(|| SandboxError::OperationFailed {
                    message: "injected acknowledgement loss after provider launch".to_owned(),
                })
        },
        |_state, stage, _error| {
            assert_eq!(
                stage,
                super::super::runner::RunnerOwnershipConvergenceStage::LifecyclePublished
            );
            events.borrow_mut().push("wait".to_owned());
        },
    )
    .expect("a successful launch should retry publication without relaunch or cleanup");
    assert_eq!(
        events.into_inner(),
        ["publish:1", "wait", "publish:2"],
        "acknowledgement loss must retry only durable lifecycle publication"
    );
}

#[test]
fn direct_planning_cleanup_failure_retains_claim_across_restart() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let state_root = temp_dir.path().join("state");
    let blocked_bundle_root = temp_dir.path().join("blocked-bundle-root");
    std::fs::write(&blocked_bundle_root, b"not a directory")
        .expect("bundle-root obstacle should write");
    let tenant = sample_spec().tenant_id;
    let first_allocator = Arc::new(
        RecordingSegmentAllocator::new(tenant.clone(), "10.76.0.0/24", 76)
            .with_release_reserved_failure("injected reserved-attachment cleanup failure"),
    );
    let first_injected: Arc<OciSegmentAllocator> = first_allocator;
    let mut first_config = ContainerSandboxBackendConfig::under_root(&state_root);
    first_config.bundle_root = blocked_bundle_root.clone();
    let first = ContainerSandboxBackend::with_segment_allocator(first_config, first_injected);
    let id = SandboxId::new("direct-planning-cleanup-restart");

    let error = first
        .plan_start_with_id(&sample_spec(), &id, None, None)
        .expect_err("post-reservation bundle failure should trigger injected cleanup failure");
    assert!(
        error
            .to_string()
            .contains("injected reserved-attachment cleanup failure"),
        "cleanup failure must remain visible: {error}"
    );
    let fenced = first
        .read_manifest(&id)
        .expect("failed manifest should inspect")
        .expect("failed manifest must remain durable");
    let retained_claim = fenced
        .launch_reservation_claim
        .clone()
        .expect("failed cleanup must retain the exact retry claim");
    assert_eq!(
        fenced.status,
        SandboxStatus::Stopping,
        "cleanup-pending planning authority must not be published as terminal"
    );
    assert!(
        crate::backends::oci::network::inspect_container_ips(
            &first.ipam_authority,
            &fenced.network_layout,
            &id,
        )
        .is_ok(),
        "failed compensation must safe-leak claim-fenced IPAM for exact retry"
    );
    std::fs::remove_file(&blocked_bundle_root)
        .expect("recovery should remove the injected bundle-root obstacle");
    std::fs::create_dir_all(&blocked_bundle_root)
        .expect("recovery should restore a traversable bundle root");

    let recovery_allocator = Arc::new(RecordingSegmentAllocator::new(tenant, "10.76.0.0/24", 76));
    let recovery_injected: Arc<OciSegmentAllocator> = recovery_allocator;
    let recovery = ContainerSandboxBackend::with_segment_allocator(
        ContainerSandboxBackendConfig::under_root(&state_root),
        recovery_injected,
    );
    recovery
        .stop_sync(&id)
        .expect("restart recovery should compensate the retained exact claim");
    let stopped = recovery
        .read_manifest(&id)
        .expect("recovered manifest should inspect")
        .expect("recovered manifest should remain durable");
    assert_eq!(stopped.status, SandboxStatus::Stopped);
    assert!(stopped.launch_reservation_claim.is_none());
    assert_ne!(
        Some(&retained_claim),
        stopped.launch_reservation_claim.as_ref(),
        "successful exact retry must retire the retained claim"
    );
    assert!(
        crate::backends::oci::network::inspect_container_ips(
            &recovery.ipam_authority,
            &stopped.network_layout,
            &id,
        )
        .is_err(),
        "exact recovery must delete the matching IPAM generation"
    );
}

#[test]
fn runner_planning_cleanup_failure_retains_claim_across_restart() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let state_root = temp_dir.path().join("state");
    let tenant = sample_spec().tenant_id;
    let first_allocator = Arc::new(
        RecordingSegmentAllocator::new(tenant.clone(), "10.77.0.0/24", 77)
            .with_release_reserved_failure("injected runner attachment cleanup failure"),
    );
    let first_injected: Arc<OciSegmentAllocator> = first_allocator;
    let first = ContainerSandboxBackend::with_segment_allocator(
        ContainerSandboxBackendConfig::plan_only(temp_dir.path().join("bundles"), &state_root),
        first_injected,
    )
    .with_runner_handoff_failure(RunnerHandoffFailure::Manifest);

    let error = first
        .prepare_plan_only_service_workload(sample_spec())
        .expect_err("runner handoff failure should trigger injected cleanup failure");
    assert!(
        error
            .to_string()
            .contains("injected runner attachment cleanup failure"),
        "runner cleanup failure must remain visible: {error}"
    );
    let manifest_paths = crate::artifact_paths::all_manifest_paths(&state_root)
        .expect("failed runner manifest should enumerate");
    assert_eq!(manifest_paths.len(), 1);
    let fenced: ContainerSandboxManifest = serde_json::from_slice(
        &std::fs::read(&manifest_paths[0]).expect("failed runner manifest should read"),
    )
    .expect("failed runner manifest should parse");
    let id = fenced.handle.id.clone();
    assert_eq!(fenced.status, SandboxStatus::Stopping);
    assert_eq!(fenced.handle.status, SandboxStatus::Stopping);
    assert!(
        fenced.launch_reservation_claim.is_some(),
        "failed runner cleanup must retain exact compensation authority"
    );
    assert!(
        crate::backends::oci::network::inspect_container_ips(
            &first.ipam_authority,
            &fenced.network_layout,
            &id,
        )
        .is_ok(),
        "failed runner compensation must retain claim-fenced IPAM"
    );

    let recovery_allocator = Arc::new(RecordingSegmentAllocator::new(tenant, "10.77.0.0/24", 77));
    let recovery_injected: Arc<OciSegmentAllocator> = recovery_allocator;
    let recovery = ContainerSandboxBackend::with_segment_allocator(
        ContainerSandboxBackendConfig::plan_only(temp_dir.path().join("bundles"), &state_root),
        recovery_injected,
    );
    recovery
        .stop_sync(&id)
        .expect("runner restart recovery should compensate the retained claim");
    let stopped = recovery
        .read_manifest(&id)
        .expect("recovered runner manifest should inspect")
        .expect("recovered runner manifest should remain durable");
    assert_eq!(stopped.status, SandboxStatus::Stopped);
    assert!(stopped.launch_reservation_claim.is_none());
    assert!(
        crate::backends::oci::network::inspect_container_ips(
            &recovery.ipam_authority,
            &stopped.network_layout,
            &id,
        )
        .is_err(),
        "runner recovery must delete the matching IPAM generation"
    );
}
