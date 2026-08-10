use super::*;

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

fn provider_journal_files(fixture: &TeardownFixture) -> BTreeMap<PathBuf, Vec<u8>> {
    snapshot_files(&fixture.backend.config.workload_state_root)
        .into_iter()
        .filter(|(path, _)| {
            path.components()
                .any(|component| component.as_os_str() == ".nimbus-provider-command-attempts")
        })
        .collect()
}
