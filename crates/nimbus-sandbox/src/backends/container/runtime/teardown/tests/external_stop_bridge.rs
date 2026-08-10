use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plan_only_external_stop_bridge_inspects_then_records_one_terminal_fence() {
    let fixture = TeardownFixture::materialized_plan_only("external-stop-terminal");
    drain_plan_only(&fixture, "external-stop-drain").await;
    let command = fixture.command(
        SandboxExecutionTeardownOperation::Stop,
        "external-stop-terminal",
        1,
    );
    let host_terminal = exact_host_terminal_evidence(&fixture, &command);
    let host_evidence_sha256 = host_terminal.evidence_sha256().to_owned();
    let crossed_host_terminal = ContainerHostTerminalEvidence::new(
        command.tenant_id().clone(),
        fixture.id.clone(),
        fixture.execution_attempt_id.clone(),
        command.provider_claim().clone(),
        b"different exact Systemd terminal evidence".to_vec(),
    )
    .expect("different host terminal evidence should validate in isolation");
    assert_ne!(
        crossed_host_terminal.evidence_sha256(),
        host_evidence_sha256
    );
    let journal = fixture
        .backend
        .attempt_idempotency_journal()
        .expect("the Container-rooted provider journal should open");
    let execution = claim_teardown_execution(&journal, &command);
    let claimed_bytes = provider_journal_files(&fixture);
    let network_before = fixture.network_authority();
    let runtime = ScriptedRuntime::live(fixture.backend.clone(), 100);
    let executor_runtime = runtime.clone();
    let backend = fixture.backend.clone();
    let child_command = command.clone();
    let child_finished = Arc::new(tokio::sync::Notify::new());
    let publish_allowed = Arc::new(tokio::sync::Notify::new());
    let executor_finished = Arc::clone(&child_finished);
    let executor_allowed = Arc::clone(&publish_allowed);
    let executor_journal = journal.clone();

    let executor = tokio::spawn(async move {
        executor_journal
            .execute_current_claim_async(execution, move |current| {
                Box::pin(async move {
                    let live = backend.record_externally_stopped_execution_substep_with_runtime(
                        &child_command,
                        current,
                        &host_terminal,
                        &executor_runtime,
                    );
                    assert!(matches!(
                        live,
                        SandboxExecutionTeardownObservation::InProgress { .. }
                    ));
                    assert!(matches!(
                        backend
                            .read_manifest(child_command.sandbox_id())
                            .expect("live manifest should read")
                            .expect("live manifest should exist")
                            .execution_teardown
                            .stop(),
                        ContainerStopProgress::NotRequested
                    ));
                    executor_runtime.terminal.store(true, Ordering::Release);
                    let terminal = backend
                        .record_externally_stopped_execution_substep_with_runtime(
                            &child_command,
                            current,
                            &host_terminal,
                            &executor_runtime,
                        );
                    assert!(matches!(
                        terminal,
                        SandboxExecutionTeardownObservation::Succeeded { .. }
                    ));
                    let terminal_evidence: serde_json::Value =
                        serde_json::from_slice(terminal.evidence())
                            .expect("terminal evidence should decode");
                    assert_eq!(
                        terminal_evidence["hostTerminalEvidenceSha256"].as_str(),
                        Some(host_evidence_sha256.as_str()),
                        "the durable Container fence must bind the exact Systemd evidence bytes"
                    );
                    let manifest_after_terminal = backend
                        .read_manifest(child_command.sandbox_id())
                        .expect("terminal manifest should read")
                        .expect("terminal manifest should exist");
                    let inspections_after_terminal = executor_runtime.terminal_inspections();
                    let crossed_replay = backend
                        .record_externally_stopped_execution_substep_with_runtime(
                            &child_command,
                            current,
                            &crossed_host_terminal,
                            &executor_runtime,
                        );
                    assert!(matches!(
                        crossed_replay,
                        SandboxExecutionTeardownObservation::DefiniteFailure { code, .. }
                            if code == "sandbox_teardown_command_crossed"
                    ));
                    assert_eq!(
                        backend
                            .read_manifest(child_command.sandbox_id())
                            .expect("crossed replay manifest should read")
                            .expect("crossed replay manifest should exist"),
                        manifest_after_terminal,
                        "crossed child evidence must not replace the durable Container fence"
                    );
                    assert_eq!(
                        executor_runtime.terminal_inspections(),
                        inspections_after_terminal,
                        "crossed child evidence must fail before runtime inspection"
                    );
                    let replay = backend.record_externally_stopped_execution_substep_with_runtime(
                        &child_command,
                        current,
                        &host_terminal,
                        &executor_runtime,
                    );
                    assert_eq!(replay, terminal);
                    assert_eq!(
                        executor_runtime.terminal_inspections(),
                        inspections_after_terminal,
                        "exact terminal replay must not inspect or stop the runtime twice"
                    );
                    assert!(executor_runtime.signals().is_empty());
                    executor_finished.notify_one();
                    executor_allowed.notified().await;
                    let kind = execution_observation_kind(&terminal);
                    let failure_code = terminal.failure_code().map(str::to_owned);
                    let evidence = terminal.evidence().to_vec();
                    (terminal, kind, failure_code, evidence)
                })
            })
            .await
    });

    child_finished.notified().await;
    assert!(matches!(
        fixture.manifest().execution_teardown.stop(),
        ContainerStopProgress::ExecutionStopped { fence, .. }
            if fence == command.provider_claim()
    ));
    assert_eq!(fixture.network_authority(), network_before);
    assert_eq!(
        provider_journal_files(&fixture),
        claimed_bytes,
        "the external-stop child must leave result publication to the composite owner"
    );
    assert_eq!(runtime.terminal_inspections(), 2);
    assert!(runtime.signals().is_empty());

    publish_allowed.notify_one();
    let (child, published) = executor
        .await
        .expect("the generic execution task should join")
        .expect("the composite owner should publish after its child settles");
    assert!(matches!(
        child,
        SandboxExecutionTeardownObservation::Succeeded { .. }
    ));
    assert_eq!(published.kind(), ProviderCommandObservationKind::Succeeded);
}

#[tokio::test]
async fn plan_only_external_stop_bridge_keeps_unknown_runtime_ambiguous() {
    let fixture = TeardownFixture::materialized_plan_only("external-stop-unknown");
    drain_plan_only(&fixture, "external-stop-unknown-drain").await;
    let command = fixture.command(
        SandboxExecutionTeardownOperation::Stop,
        "external-stop-unknown",
        1,
    );
    let host_terminal = exact_host_terminal_evidence(&fixture, &command);
    let journal = fixture
        .backend
        .attempt_idempotency_journal()
        .expect("the Container-rooted provider journal should open");
    let execution = claim_teardown_execution(&journal, &command);
    let manifest_before = fixture.manifest();
    let network_before = fixture.network_authority();
    let runtime = ScriptedRuntime::live(fixture.backend.clone(), 100);
    runtime.set_terminal_unknown(true);
    let executor_runtime = runtime.clone();
    let backend = fixture.backend.clone();

    let (child, published) = journal
        .execute_current_claim_async(execution, move |current| {
            Box::pin(async move {
                let observation = backend.record_externally_stopped_execution_substep_with_runtime(
                    &command,
                    current,
                    &host_terminal,
                    &executor_runtime,
                );
                let kind = execution_observation_kind(&observation);
                let failure_code = observation.failure_code().map(str::to_owned);
                let evidence = observation.evidence().to_vec();
                (observation, kind, failure_code, evidence)
            })
        })
        .await
        .expect("unknown runtime observation should become durable");

    assert!(matches!(
        child,
        SandboxExecutionTeardownObservation::Ambiguous { .. }
    ));
    assert_eq!(published.kind(), ProviderCommandObservationKind::Ambiguous);
    assert_eq!(fixture.manifest(), manifest_before);
    assert_eq!(fixture.network_authority(), network_before);
    assert_eq!(runtime.terminal_inspections(), 1);
    assert!(runtime.signals().is_empty());
}

#[tokio::test]
async fn plan_only_external_stop_inspection_is_read_only_and_byte_stable() {
    let fixture = TeardownFixture::materialized_plan_only("external-stop-inspection");
    drain_plan_only(&fixture, "external-stop-inspection-drain").await;
    let command = fixture.command(
        SandboxExecutionTeardownOperation::Stop,
        "external-stop-inspection",
        1,
    );
    let host_terminal = exact_host_terminal_evidence(&fixture, &command);
    let journal = fixture
        .backend
        .attempt_idempotency_journal()
        .expect("the Container-rooted provider journal should open");
    let execution = claim_teardown_execution(&journal, &command);
    let network_before = fixture.network_authority();
    let runtime = ScriptedRuntime::live(fixture.backend.clone(), 100);
    let (executed_live, current) = journal
        .execute_current_claim(execution, |current| {
            let observation = fixture
                .backend
                .record_externally_stopped_execution_substep_with_runtime(
                    &command,
                    current,
                    &host_terminal,
                    &runtime,
                );
            let kind = execution_observation_kind(&observation);
            let failure_code = observation.failure_code().map(str::to_owned);
            let evidence = observation.evidence().to_vec();
            (observation, kind, failure_code, evidence)
        })
        .expect("the live Container child should publish in progress");
    assert!(matches!(
        executed_live,
        SandboxExecutionTeardownObservation::InProgress { .. }
    ));
    assert_eq!(current.kind(), ProviderCommandObservationKind::InProgress);
    let files_before = snapshot_files(&fixture.backend.config.workload_state_root);

    let live_first = fixture
        .backend
        .inspect_externally_stopped_execution_substep_with_runtime(
            &command,
            &current,
            &host_terminal,
            &runtime,
        );
    let live_second = fixture
        .backend
        .inspect_externally_stopped_execution_substep_with_runtime(
            &command,
            &current,
            &host_terminal,
            &runtime,
        );
    assert_eq!(live_second, live_first);
    assert!(matches!(
        live_first,
        SandboxExecutionTeardownObservation::InProgress { .. }
    ));

    runtime.set_terminal_unknown(true);
    let unknown_first = fixture
        .backend
        .inspect_externally_stopped_execution_substep_with_runtime(
            &command,
            &current,
            &host_terminal,
            &runtime,
        );
    let unknown_second = fixture
        .backend
        .inspect_externally_stopped_execution_substep_with_runtime(
            &command,
            &current,
            &host_terminal,
            &runtime,
        );
    assert_eq!(unknown_second, unknown_first);
    assert!(matches!(
        unknown_first,
        SandboxExecutionTeardownObservation::Ambiguous { .. }
    ));

    runtime.set_terminal_unknown(false);
    runtime.terminal.store(true, Ordering::Release);
    let terminal_first = fixture
        .backend
        .inspect_externally_stopped_execution_substep_with_runtime(
            &command,
            &current,
            &host_terminal,
            &runtime,
        );
    let terminal_second = fixture
        .backend
        .inspect_externally_stopped_execution_substep_with_runtime(
            &command,
            &current,
            &host_terminal,
            &runtime,
        );
    assert_eq!(terminal_second, terminal_first);
    assert!(matches!(
        terminal_first,
        SandboxExecutionTeardownObservation::Absent { .. }
    ));
    assert_eq!(
        snapshot_files(&fixture.backend.config.workload_state_root),
        files_before,
        "external-stop inspection must not write provider or manifest state"
    );
    assert_eq!(fixture.network_authority(), network_before);
    assert!(runtime.signals().is_empty());

    let retry = fixture.command(
        SandboxExecutionTeardownOperation::Stop,
        "external-stop-inspection",
        2,
    );
    let retry_execution = match journal
        .claim_dispatch_epoch_after_inspected_absence(
            retry.provider_claim(),
            command.provider_claim().dispatch_epoch(),
            terminal_first.evidence(),
        )
        .expect("the exact absent child inspection should authorize one adjacent retry")
    {
        ProviderCommandClaimDecision::ExecuteClaimed(execution) => execution,
        ProviderCommandClaimDecision::AdoptExactAttempt(_) => {
            panic!("the adjacent external-stop retry should receive execute authority")
        }
    };
    let retry_host_terminal = exact_host_terminal_evidence(&fixture, &retry);
    let ((recorded, replay), published) = journal
        .execute_current_claim(retry_execution, |current| {
            let recorded = fixture
                .backend
                .record_externally_stopped_execution_substep_with_runtime(
                    &retry,
                    current,
                    &retry_host_terminal,
                    &runtime,
                );
            let inspections_after_record = runtime.terminal_inspections();
            let replay = fixture
                .backend
                .record_externally_stopped_execution_substep_with_runtime(
                    &retry,
                    current,
                    &retry_host_terminal,
                    &runtime,
                );
            assert_eq!(runtime.terminal_inspections(), inspections_after_record);
            let kind = execution_observation_kind(&replay);
            let failure_code = replay.failure_code().map(str::to_owned);
            let evidence = replay.evidence().to_vec();
            ((recorded, replay), kind, failure_code, evidence)
        })
        .expect("the adjacent Execute should record one Container terminal fence");
    assert_eq!(recorded, replay);
    assert!(matches!(
        recorded,
        SandboxExecutionTeardownObservation::Succeeded { .. }
    ));
    assert_eq!(published.kind(), ProviderCommandObservationKind::Succeeded);
    assert!(matches!(
        fixture.manifest().execution_teardown.stop(),
        ContainerStopProgress::ExecutionStopped { fence, .. }
            if fence == retry.provider_claim()
    ));
    assert_eq!(fixture.network_authority(), network_before);
    assert_eq!(runtime.terminal_inspections(), 8);
    assert!(runtime.signals().is_empty());
}

#[tokio::test]
async fn plan_only_external_stop_inspection_replays_durable_terminal_bytes() {
    let fixture = TeardownFixture::materialized_plan_only("external-stop-inspect-terminal");
    drain_plan_only(&fixture, "external-stop-inspect-terminal-drain").await;
    let command = fixture.command(
        SandboxExecutionTeardownOperation::Stop,
        "external-stop-inspect-terminal",
        1,
    );
    let host_terminal = exact_host_terminal_evidence(&fixture, &command);
    let crossed_host_terminal = ContainerHostTerminalEvidence::new(
        command.tenant_id().clone(),
        fixture.id.clone(),
        fixture.execution_attempt_id.clone(),
        command.provider_claim().clone(),
        b"different Systemd terminal inspection evidence".to_vec(),
    )
    .expect("crossed host terminal evidence should validate in isolation");
    let journal = fixture
        .backend
        .attempt_idempotency_journal()
        .expect("the Container-rooted provider journal should open");
    let execution = claim_teardown_execution(&journal, &command);
    let runtime = ScriptedRuntime::live(fixture.backend.clone(), 100);
    runtime.terminal.store(true, Ordering::Release);
    let (terminal, current) = journal
        .execute_current_claim(execution, |current| {
            let terminal = fixture
                .backend
                .record_externally_stopped_execution_substep_with_runtime(
                    &command,
                    current,
                    &host_terminal,
                    &runtime,
                );
            (
                terminal,
                ProviderCommandObservationKind::InProgress,
                None,
                b"a composite sibling remains in progress".to_vec(),
            )
        })
        .expect("the composite owner should retain an in-progress outer result");
    assert!(matches!(
        terminal,
        SandboxExecutionTeardownObservation::Succeeded { .. }
    ));
    let inspections_after_record = runtime.terminal_inspections();
    let files_before = snapshot_files(&fixture.backend.config.workload_state_root);

    let first = fixture
        .backend
        .inspect_externally_stopped_execution_substep_with_runtime(
            &command,
            &current,
            &host_terminal,
            &runtime,
        );
    let second = fixture
        .backend
        .inspect_externally_stopped_execution_substep_with_runtime(
            &command,
            &current,
            &host_terminal,
            &runtime,
        );
    assert_eq!(first, terminal);
    assert_eq!(second, first);
    let crossed = fixture
        .backend
        .inspect_externally_stopped_execution_substep_with_runtime(
            &command,
            &current,
            &crossed_host_terminal,
            &runtime,
        );
    assert!(matches!(
        crossed,
        SandboxExecutionTeardownObservation::DefiniteFailure { code, .. }
            if code == "sandbox_teardown_command_crossed"
    ));
    assert_eq!(runtime.terminal_inspections(), inspections_after_record);
    assert!(runtime.signals().is_empty());
    assert_eq!(
        snapshot_files(&fixture.backend.config.workload_state_root),
        files_before,
        "durable external-stop inspection must replay exact bytes without writes"
    );
}

#[tokio::test]
async fn plan_only_external_stop_inspection_rejects_crossed_durable_fence() {
    let fixture = TeardownFixture::materialized_plan_only("external-stop-inspect-crossed");
    drain_plan_only(&fixture, "external-stop-inspect-crossed-drain").await;
    let command = fixture.command(
        SandboxExecutionTeardownOperation::Stop,
        "external-stop-inspect-current",
        1,
    );
    let crossed = fixture.command(
        SandboxExecutionTeardownOperation::Stop,
        "external-stop-inspect-crossed",
        1,
    );
    let mut manifest = fixture.manifest();
    manifest
        .execution_teardown
        .set_stop(ContainerStopProgress::ExecutionStopped {
            fence: crossed.provider_claim().clone(),
            evidence: b"crossed durable Container stop evidence".to_vec(),
        });
    fixture
        .backend
        .write_manifest(&manifest)
        .expect("crossed stop fence fixture should persist");
    let current = publish_in_progress_current(&fixture, &command);
    let host_terminal = exact_host_terminal_evidence(&fixture, &command);
    let files_before = snapshot_files(&fixture.backend.config.workload_state_root);
    let runtime = ScriptedRuntime::live(fixture.backend.clone(), 100);

    let observation = fixture
        .backend
        .inspect_externally_stopped_execution_substep_with_runtime(
            &command,
            &current,
            &host_terminal,
            &runtime,
        );
    assert!(matches!(
        observation,
        SandboxExecutionTeardownObservation::DefiniteFailure { code, .. }
            if code == "sandbox_teardown_command_crossed"
    ));
    assert_eq!(runtime.terminal_inspections(), 0);
    assert!(runtime.signals().is_empty());
    assert_eq!(
        snapshot_files(&fixture.backend.config.workload_state_root),
        files_before,
        "crossed stop inspection must fail before a durable write"
    );
}

#[tokio::test]
async fn plan_only_external_stop_bridge_rejects_crossed_host_evidence_before_mutation() {
    let fixture = TeardownFixture::materialized_plan_only("external-stop-crossed");
    drain_plan_only(&fixture, "external-stop-crossed-drain").await;
    let command = fixture.command(
        SandboxExecutionTeardownOperation::Stop,
        "external-stop-current",
        1,
    );
    let crossed_command = fixture.command(
        SandboxExecutionTeardownOperation::Stop,
        "external-stop-crossed",
        1,
    );
    let crossed_claim = exact_host_terminal_evidence(&fixture, &crossed_command);
    let crossed_tenant = ContainerHostTerminalEvidence::new(
        nimbus_core::TenantId::new("crossed-host-terminal-tenant")
            .expect("crossed host tenant should validate"),
        fixture.id.clone(),
        fixture.execution_attempt_id.clone(),
        command.provider_claim().clone(),
        b"exact crossed Systemd terminal evidence".to_vec(),
    )
    .expect("crossed host evidence should validate in isolation");
    let journal = fixture
        .backend
        .attempt_idempotency_journal()
        .expect("the Container-rooted provider journal should open");
    let execution = claim_teardown_execution(&journal, &command);
    let manifest_before = fixture.manifest();
    let network_before = fixture.network_authority();
    let runtime = ScriptedRuntime::live(fixture.backend.clone(), 100);
    runtime.terminal.store(true, Ordering::Release);
    let executor_runtime = runtime.clone();
    let backend = fixture.backend.clone();

    let (children, _) = journal
        .execute_current_claim_async(execution, move |current| {
            Box::pin(async move {
                let claim_rejection = backend
                    .record_externally_stopped_execution_substep_with_runtime(
                        &command,
                        current,
                        &crossed_claim,
                        &executor_runtime,
                    );
                let tenant_rejection = backend
                    .record_externally_stopped_execution_substep_with_runtime(
                        &command,
                        current,
                        &crossed_tenant,
                        &executor_runtime,
                    );
                let kind = execution_observation_kind(&tenant_rejection);
                let failure_code = tenant_rejection.failure_code().map(str::to_owned);
                let evidence = tenant_rejection.evidence().to_vec();
                (
                    (claim_rejection, tenant_rejection),
                    kind,
                    failure_code,
                    evidence,
                )
            })
        })
        .await
        .expect("crossed host evidence rejection should become durable");

    for child in [&children.0, &children.1] {
        assert!(matches!(
            child,
            SandboxExecutionTeardownObservation::DefiniteFailure { code, .. }
                if code == "sandbox_teardown_command_crossed"
        ));
    }
    assert_eq!(fixture.manifest(), manifest_before);
    assert_eq!(fixture.network_authority(), network_before);
    assert_eq!(runtime.terminal_inspections(), 0);
    assert!(runtime.signals().is_empty());
}

async fn drain_plan_only(fixture: &TeardownFixture, attempt: &str) {
    let command = fixture.command(SandboxExecutionTeardownOperation::Drain, attempt, 1);
    let journal = fixture
        .backend
        .attempt_idempotency_journal()
        .expect("the Container-rooted provider journal should open");
    let execution = claim_teardown_execution(&journal, &command);
    let backend = fixture.backend.clone();
    let (child, _) = journal
        .execute_current_claim_async(execution, move |current| {
            Box::pin(async move {
                let observation = backend.execute_execution_teardown_substep(&command, current);
                let kind = execution_observation_kind(&observation);
                let failure_code = observation.failure_code().map(str::to_owned);
                let evidence = observation.evidence().to_vec();
                (observation, kind, failure_code, evidence)
            })
        })
        .await
        .expect("PlanOnly drain should publish after its child settles");
    assert!(matches!(
        child,
        SandboxExecutionTeardownObservation::Succeeded { .. }
    ));
}

fn publish_in_progress_current(
    fixture: &TeardownFixture,
    command: &SandboxExecutionTeardownCommand,
) -> ProviderCommandObservation {
    let journal = fixture
        .backend
        .attempt_idempotency_journal()
        .expect("the Container-rooted provider journal should open");
    let _execution = claim_teardown_execution(&journal, command);
    persist_teardown_observation(
        &journal,
        command,
        &SandboxExecutionTeardownObservation::InProgress {
            evidence: b"Systemd is absent while Container runtime remains live".to_vec(),
        },
    );
    journal
        .adopt_exact_attempt(command.provider_claim())
        .expect("the current in-progress observation should read")
        .expect("the current in-progress observation should exist")
}

fn exact_host_terminal_evidence(
    fixture: &TeardownFixture,
    command: &SandboxExecutionTeardownCommand,
) -> ContainerHostTerminalEvidence {
    ContainerHostTerminalEvidence::new(
        command.tenant_id().clone(),
        fixture.id.clone(),
        fixture.execution_attempt_id.clone(),
        command.provider_claim().clone(),
        b"exact Systemd terminal evidence".to_vec(),
    )
    .expect("exact host terminal evidence should validate")
}
