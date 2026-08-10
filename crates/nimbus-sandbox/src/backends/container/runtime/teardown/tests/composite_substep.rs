use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plan_only_container_drain_requires_composite_authority() {
    let direct = TeardownFixture::materialized_plan_only("plan-only-direct");
    let direct_command = direct.command(
        SandboxExecutionTeardownOperation::Drain,
        "plan-only-direct",
        1,
    );
    assert!(matches!(
        direct.backend.execute_execution_teardown(&direct_command),
        SandboxExecutionTeardownObservation::DefiniteFailure { .. }
    ));
    assert!(matches!(
        direct.backend.inspect_execution_teardown(&direct_command),
        SandboxExecutionTeardownObservation::DefiniteFailure { .. }
    ));
    let direct_journal = direct
        .backend
        .attempt_idempotency_journal()
        .expect("the Container-rooted provider journal should open");
    let direct_execution = claim_teardown_execution(&direct_journal, &direct_command);
    assert!(matches!(
        direct.backend.inspect_execution_teardown_with_observation(
            &direct_command,
            direct_execution.observation(),
        ),
        SandboxExecutionTeardownObservation::DefiniteFailure { .. }
    ));
    assert_eq!(
        direct
            .backend
            .execute_execution_teardown_with_claim(&direct_command, direct_execution)
            .expect("the direct adapter should publish its PlanOnly rejection")
            .kind(),
        ProviderCommandObservationKind::DefiniteFailure
    );
    assert!(matches!(
        direct.manifest().execution_teardown.drain(),
        ContainerDrainProgress::Open
    ));

    let fixture = TeardownFixture::materialized_plan_only("plan-only-composite");
    let command = fixture.command(
        SandboxExecutionTeardownOperation::Drain,
        "plan-only-composite",
        1,
    );
    let journal = fixture
        .backend
        .attempt_idempotency_journal()
        .expect("the Container-rooted provider journal should open");
    let execution = claim_teardown_execution(&journal, &command);
    let claimed_bytes = provider_journal_files(&fixture);
    let backend = fixture.backend.clone();
    let child_command = command.clone();
    let child_finished = std::sync::Arc::new(tokio::sync::Notify::new());
    let publish_allowed = std::sync::Arc::new(tokio::sync::Notify::new());
    let executor_finished = std::sync::Arc::clone(&child_finished);
    let executor_allowed = std::sync::Arc::clone(&publish_allowed);
    let executor_journal = journal.clone();

    let executor = tokio::spawn(async move {
        executor_journal
            .execute_current_claim_async(execution, move |current| {
                Box::pin(async move {
                    let observation =
                        backend.execute_execution_teardown_substep(&child_command, current);
                    executor_finished.notify_one();
                    assert!(matches!(
                        observation,
                        SandboxExecutionTeardownObservation::Succeeded { .. }
                    ));
                    let files_before_inspection =
                        snapshot_files(&backend.config.workload_state_root);
                    let inspected = backend
                        .inspect_execution_teardown_substep(&child_command, current.observation());
                    assert_eq!(inspected, observation);
                    assert_eq!(
                        snapshot_files(&backend.config.workload_state_root),
                        files_before_inspection,
                        "PlanOnly child inspection must stay read-only"
                    );
                    executor_allowed.notified().await;
                    let kind = execution_observation_kind(&observation);
                    let failure_code = observation.failure_code().map(str::to_owned);
                    let evidence = observation.evidence().to_vec();
                    (observation, kind, failure_code, evidence)
                })
            })
            .await
    });

    child_finished.notified().await;
    assert!(matches!(
        fixture.manifest().execution_teardown.drain(),
        ContainerDrainProgress::Drained { fence, .. } if fence == command.provider_claim()
    ));
    assert_eq!(
        provider_journal_files(&fixture),
        claimed_bytes,
        "the PlanOnly child must leave result publication to the composite owner"
    );

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
async fn plan_only_container_substep_rejects_crossed_fences_before_mutation() {
    let fixture = TeardownFixture::materialized_plan_only("plan-only-crossed-claim");
    let current = fixture.command(
        SandboxExecutionTeardownOperation::Drain,
        "plan-only-current",
        1,
    );
    let crossed = fixture.command(
        SandboxExecutionTeardownOperation::Drain,
        "plan-only-crossed",
        1,
    );
    assert_plan_only_substep_rejected_before_mutation(fixture, current, crossed).await;

    let fixture = TeardownFixture::materialized_plan_only("plan-only-crossed-tenant");
    let current = fixture.command(
        SandboxExecutionTeardownOperation::Drain,
        "plan-only-tenant",
        1,
    );
    let crossed = SandboxExecutionTeardownCommand::new(
        nimbus_core::TenantId::new("crossed-container-tenant")
            .expect("crossed tenant should validate"),
        fixture.id.clone(),
        fixture.execution_attempt_id.clone(),
        CONTAINER_EXECUTION_TEARDOWN_PROVIDER_KEY,
        SandboxExecutionTeardownOperation::Drain,
        current.provider_claim().clone(),
    )
    .expect("crossed tenant command should validate in isolation");
    assert_plan_only_substep_rejected_before_mutation(fixture, current, crossed).await;

    let fixture = TeardownFixture::materialized_plan_only("plan-only-crossed-attempt");
    let current = fixture.command(
        SandboxExecutionTeardownOperation::Drain,
        "plan-only-attempt",
        1,
    );
    let crossed = SandboxExecutionTeardownCommand::new(
        current.tenant_id().clone(),
        fixture.id.clone(),
        crate::SandboxExecutionAttemptId::new("crossed-container-attempt")
            .expect("crossed execution attempt should validate"),
        CONTAINER_EXECUTION_TEARDOWN_PROVIDER_KEY,
        SandboxExecutionTeardownOperation::Drain,
        current.provider_claim().clone(),
    )
    .expect("crossed execution-attempt command should validate in isolation");
    assert_plan_only_substep_rejected_before_mutation(fixture, current, crossed).await;

    let fixture = TeardownFixture::materialized_plan_only("plan-only-crossed-plan");
    let manifest = fixture.manifest();
    let exact_plan = manifest
        .provision_network_plan
        .as_ref()
        .expect("PlanOnly fixture should retain its plan");
    let crossed = fixture.command_with_execution_and_plan(
        &fixture.execution_attempt_id,
        SandboxExecutionTeardownOperation::Drain,
        "plan-only-plan",
        1,
        exact_plan.generation().as_u64(),
        "4".repeat(64),
    );
    assert_plan_only_substep_rejected_before_mutation(fixture, crossed.clone(), crossed).await;

    let fixture = TeardownFixture::materialized_plan_only("plan-only-crossed-generation");
    let manifest = fixture.manifest();
    let exact_plan = manifest
        .provision_network_plan
        .as_ref()
        .expect("PlanOnly fixture should retain its plan");
    let crossed = fixture.command_with_execution_and_plan(
        &fixture.execution_attempt_id,
        SandboxExecutionTeardownOperation::Drain,
        "plan-only-generation",
        1,
        exact_plan.generation().as_u64() + 1,
        exact_plan.network_plan().digest().to_string(),
    );
    assert_plan_only_substep_rejected_before_mutation(fixture, crossed.clone(), crossed).await;

    let fixture = TeardownFixture::materialized_plan_only("plan-only-crossed-stop");
    let stop = fixture.command(SandboxExecutionTeardownOperation::Stop, "plan-only-stop", 1);
    assert_plan_only_substep_rejected_before_mutation(fixture, stop.clone(), stop).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn container_execution_teardown_substep_does_not_publish_generic_success() {
    let fixture = TeardownFixture::reserved("composite-drain-publication");
    let command = fixture.command(
        SandboxExecutionTeardownOperation::Drain,
        "composite-drain",
        1,
    );
    let journal = fixture
        .backend
        .attempt_idempotency_journal()
        .expect("the Container-rooted provider journal should open");
    let execution = claim_teardown_execution(&journal, &command);
    let claimed_bytes = provider_journal_files(&fixture);
    let backend = fixture.backend.clone();
    let child_command = command.clone();
    let child_finished = std::sync::Arc::new(tokio::sync::Notify::new());
    let publish_allowed = std::sync::Arc::new(tokio::sync::Notify::new());
    let executor_finished = std::sync::Arc::clone(&child_finished);
    let executor_allowed = std::sync::Arc::clone(&publish_allowed);
    let executor_journal = journal.clone();

    let executor = tokio::spawn(async move {
        executor_journal
            .execute_current_claim_async(execution, move |current| {
                Box::pin(async move {
                    let observation =
                        backend.execute_execution_teardown_substep(&child_command, current);
                    executor_finished.notify_one();
                    executor_allowed.notified().await;
                    let kind = execution_observation_kind(&observation);
                    let failure_code = observation.failure_code().map(str::to_owned);
                    let evidence = observation.evidence().to_vec();
                    (observation, kind, failure_code, evidence)
                })
            })
            .await
    });

    child_finished.notified().await;
    assert!(matches!(
        fixture.manifest().execution_teardown.drain(),
        ContainerDrainProgress::Drained { fence, .. } if fence == command.provider_claim()
    ));
    assert_eq!(
        provider_journal_files(&fixture),
        claimed_bytes,
        "one completed Container child must not publish the generic result"
    );

    publish_allowed.notify_one();
    let (child, published) = executor
        .await
        .expect("the generic execution task should join")
        .expect("the caller should publish after its child settles");
    assert!(matches!(
        child,
        SandboxExecutionTeardownObservation::Succeeded { .. }
    ));
    assert_eq!(published.kind(), ProviderCommandObservationKind::Succeeded);
}

#[tokio::test]
async fn container_execution_teardown_substep_rejects_crossed_claim_before_manifest_access() {
    let fixture = TeardownFixture::reserved("composite-crossed-claim");
    let exact = fixture.command(
        SandboxExecutionTeardownOperation::Drain,
        "composite-exact",
        1,
    );
    let crossed = fixture.command(
        SandboxExecutionTeardownOperation::Drain,
        "composite-crossed",
        1,
    );
    let journal = fixture
        .backend
        .attempt_idempotency_journal()
        .expect("the Container-rooted provider journal should open");
    let execution = claim_teardown_execution(&journal, &exact);
    let manifest_path = crate::artifact_paths::manifest_path(
        &fixture.backend.config.workload_state_root,
        exact.tenant_id(),
        exact.sandbox_id(),
    );
    let manifest_before = std::fs::read(&manifest_path).expect("manifest should read");
    let backend = fixture.backend.clone();

    let (child, _) = journal
        .execute_current_claim_async(execution, move |current| {
            Box::pin(async move {
                let observation = backend.execute_execution_teardown_substep(&crossed, current);
                let kind = execution_observation_kind(&observation);
                let failure_code = observation.failure_code().map(str::to_owned);
                let evidence = observation.evidence().to_vec();
                (observation, kind, failure_code, evidence)
            })
        })
        .await
        .expect("the crossed child result should become durable");

    assert!(matches!(
        child,
        SandboxExecutionTeardownObservation::DefiniteFailure { ref code, .. }
            if code == "sandbox_teardown_command_crossed"
    ));
    assert_eq!(
        std::fs::read(&manifest_path).expect("manifest should remain readable"),
        manifest_before,
        "crossed child authority must fail before manifest access or mutation"
    );
}

#[tokio::test]
async fn container_execution_teardown_substep_exact_replay_has_no_second_child_effect() {
    let fixture = TeardownFixture::reserved("composite-exact-replay");
    let command = fixture.command(
        SandboxExecutionTeardownOperation::Drain,
        "composite-replay",
        1,
    );
    let journal = fixture
        .backend
        .attempt_idempotency_journal()
        .expect("the Container-rooted provider journal should open");
    let execution = claim_teardown_execution(&journal, &command);
    let backend = fixture.backend.clone();

    let (observations, _) = journal
        .execute_current_claim_async(execution, move |current| {
            Box::pin(async move {
                let first = backend.execute_execution_teardown_substep(&command, current);
                let after_first = snapshot_files(&backend.config.workload_state_root);
                let second = backend.execute_execution_teardown_substep(&command, current);
                assert_eq!(
                    snapshot_files(&backend.config.workload_state_root),
                    after_first,
                    "exact child replay must not add a second durable effect"
                );
                let kind = execution_observation_kind(&second);
                let failure_code = second.failure_code().map(str::to_owned);
                let evidence = second.evidence().to_vec();
                ((first, second), kind, failure_code, evidence)
            })
        })
        .await
        .expect("exact child replay should publish once");

    assert!(matches!(
        observations.0,
        SandboxExecutionTeardownObservation::Succeeded { .. }
    ));
    assert_eq!(observations.0, observations.1);
}

#[tokio::test]
async fn container_stop_substep_requires_the_exact_durable_drain() {
    let fixture = TeardownFixture::reserved("composite-stop-order");
    let command = fixture.command(SandboxExecutionTeardownOperation::Stop, "composite-stop", 1);
    let journal = fixture
        .backend
        .attempt_idempotency_journal()
        .expect("the Container-rooted provider journal should open");
    let execution = claim_teardown_execution(&journal, &command);
    let manifest_before = fixture.manifest();
    let network_before = fixture.network_authority();
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
        .expect("the ordered failure should become durable");

    assert!(matches!(
        child,
        SandboxExecutionTeardownObservation::DefiniteFailure { ref code, .. }
            if code == "sandbox_teardown_command_crossed"
    ));
    assert_eq!(fixture.manifest(), manifest_before);
    assert_eq!(fixture.network_authority(), network_before);
}

async fn assert_plan_only_substep_rejected_before_mutation(
    fixture: TeardownFixture,
    current_command: SandboxExecutionTeardownCommand,
    child_command: SandboxExecutionTeardownCommand,
) {
    let journal = fixture
        .backend
        .attempt_idempotency_journal()
        .expect("the Container-rooted provider journal should open");
    let execution = claim_teardown_execution(&journal, &current_command);
    let manifest_before = fixture.manifest();
    let backend = fixture.backend.clone();

    let (child, _) = journal
        .execute_current_claim_async(execution, move |current| {
            Box::pin(async move {
                let observation =
                    backend.execute_execution_teardown_substep(&child_command, current);
                let kind = execution_observation_kind(&observation);
                let failure_code = observation.failure_code().map(str::to_owned);
                let evidence = observation.evidence().to_vec();
                (observation, kind, failure_code, evidence)
            })
        })
        .await
        .expect("the PlanOnly child rejection should become durable");

    assert!(matches!(
        child,
        SandboxExecutionTeardownObservation::DefiniteFailure { ref code, .. }
            if code == "sandbox_teardown_command_crossed"
    ));
    assert_eq!(
        fixture.manifest(),
        manifest_before,
        "rejected PlanOnly child authority must fail before manifest mutation"
    );
}
