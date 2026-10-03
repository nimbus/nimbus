use super::*;

#[test]
fn delayed_stop_claim_fails_before_manifest_or_effect_after_epoch_advances() {
    let fixture = TeardownFixture::reserved("delayed-stop-claim");
    let drain = fixture.command(
        SandboxExecutionTeardownOperation::Drain,
        "delayed-stop-claim",
        1,
    );
    assert!(matches!(
        fixture.backend.execute_execution_teardown(&drain),
        SandboxExecutionTeardownObservation::Succeeded { .. }
    ));
    let journal = fixture
        .backend
        .attempt_idempotency_journal()
        .expect("the Container provider journal should open");
    let first = fixture.command(
        SandboxExecutionTeardownOperation::Stop,
        "delayed-stop-claim",
        1,
    );
    let stale_execution = match journal
        .claim_dispatch_epoch(first.provider_claim())
        .expect("the first stop epoch should claim")
    {
        ProviderCommandClaimDecision::ExecuteClaimed(execution) => execution,
        ProviderCommandClaimDecision::AdoptExactAttempt(_) => {
            panic!("the first stop epoch must receive execute authority")
        }
    };
    journal
        .record_observation(
            first.provider_claim(),
            ProviderCommandObservationKind::RetryAuthorized,
            b"inspection authorized the adjacent stop epoch",
        )
        .expect("retry authority should become durable");
    let second = fixture.command(
        SandboxExecutionTeardownOperation::Stop,
        "delayed-stop-claim",
        2,
    );
    let _current_execution = match journal
        .claim_dispatch_epoch(second.provider_claim())
        .expect("the adjacent stop epoch should claim")
    {
        ProviderCommandClaimDecision::ExecuteClaimed(execution) => execution,
        ProviderCommandClaimDecision::AdoptExactAttempt(_) => {
            panic!("the adjacent stop epoch must receive execute authority")
        }
    };
    let before = snapshot_files(fixture.root.path());

    assert!(matches!(
        fixture
            .backend
            .execute_execution_teardown_with_claim(&first, stale_execution),
        Err(ProviderCommandJournalError::StaleDispatchEpoch {
            current: 2,
            candidate: 1,
        })
    ));
    assert_eq!(snapshot_files(fixture.root.path()), before);
    assert!(matches!(
        fixture.manifest().execution_teardown.stop(),
        ContainerStopProgress::NotRequested
    ));
}

#[test]
fn journal_claim_crashes_reconcile_exact_retry_lineage_before_manifest_progress() {
    let fixture = TeardownFixture::attached("claim-before-manifest");
    let journal = fixture
        .backend
        .attempt_idempotency_journal()
        .expect("the Container provider journal should open");
    let first = fixture.command(
        SandboxExecutionTeardownOperation::Drain,
        "claim-before-manifest",
        1,
    );
    assert!(matches!(
        journal
            .claim_dispatch_epoch(first.provider_claim())
            .expect("the first drain epoch should claim"),
        ProviderCommandClaimDecision::ExecuteClaimed(_)
    ));
    let mut manifest = fixture.manifest();
    manifest
        .execution_teardown
        .set_drain(ContainerDrainProgress::BarrierPersisted {
            fence: first.provider_claim().clone(),
        });
    fixture
        .backend
        .write_existing_workload_manifest(&manifest)
        .expect("the initial drain barrier should become durable");
    journal
        .record_observation(
            first.provider_claim(),
            ProviderCommandObservationKind::Absent,
            b"first drain epoch has no effect",
        )
        .expect("the first no-effect observation should become durable");

    for epoch in [2, 3] {
        let claimed = fixture.command(
            SandboxExecutionTeardownOperation::Drain,
            "claim-before-manifest",
            epoch,
        );
        assert!(matches!(
            journal
                .claim_dispatch_epoch(claimed.provider_claim())
                .expect("the retry epoch should claim before the simulated crash"),
            ProviderCommandClaimDecision::ExecuteClaimed(_)
        ));
        let durable = journal
            .adopt_exact_attempt(claimed.provider_claim())
            .expect("the claimed retry should inspect")
            .expect("the claimed retry should exist");
        let stable_files = snapshot_files(fixture.root.path());
        let inspected = fixture
            .backend
            .inspect_execution_teardown_with_observation(&claimed, &durable);
        assert!(matches!(
            inspected,
            SandboxExecutionTeardownObservation::Absent { .. }
        ));
        assert_eq!(
            snapshot_files(fixture.root.path()),
            stable_files,
            "claim-only crash inspection must not change a durable byte"
        );
        let evidence = inspected.evidence().to_vec();
        journal
            .record_observation(
                claimed.provider_claim(),
                ProviderCommandObservationKind::Absent,
                &evidence,
            )
            .expect("the claim-only crash should record exact absence");
    }

    let final_command = fixture.command(
        SandboxExecutionTeardownOperation::Drain,
        "claim-before-manifest",
        4,
    );
    let final_execution = match journal
        .claim_dispatch_epoch(final_command.provider_claim())
        .expect("the final retry should receive execute authority")
    {
        ProviderCommandClaimDecision::ExecuteClaimed(execution) => execution,
        ProviderCommandClaimDecision::AdoptExactAttempt(_) => {
            panic!("the final retry must not adopt an older attempt")
        }
    };
    assert!(
        final_execution
            .observation()
            .authenticates_retry_progress(first.provider_claim()),
        "the one journal must retain an exact receipt for every no-effect retry"
    );
    assert!(matches!(
        fixture
            .backend
            .execute_execution_teardown_with_claim(&final_command, final_execution),
        Ok(observation) if observation.kind() == ProviderCommandObservationKind::Succeeded
    ));
    assert!(matches!(
        fixture.manifest().execution_teardown.drain(),
        ContainerDrainProgress::Drained { fence, .. } if fence == final_command.provider_claim()
    ));
}

#[test]
fn journal_receipts_reconcile_two_epoch_lag_for_every_stop_progress_state() {
    enum LagState {
        Intent,
        Term,
        Kill,
    }

    for (label, state, first_kind, expected_signal) in [
        (
            "intent",
            LagState::Intent,
            ProviderCommandObservationKind::Absent,
            libc::SIGTERM,
        ),
        (
            "term",
            LagState::Term,
            ProviderCommandObservationKind::RetryAuthorized,
            libc::SIGKILL,
        ),
        (
            "kill",
            LagState::Kill,
            ProviderCommandObservationKind::RetryAuthorized,
            libc::SIGKILL,
        ),
    ] {
        let fixture = TeardownFixture::reserved(&format!("stop-lag-{label}"));
        let drain = fixture.command(SandboxExecutionTeardownOperation::Drain, "stop-lag", 1);
        assert!(matches!(
            fixture.backend.execute_execution_teardown(&drain),
            SandboxExecutionTeardownObservation::Succeeded { .. }
        ));
        let journal = fixture
            .backend
            .attempt_idempotency_journal()
            .expect("the stop-lag provider journal should open");
        let first = fixture.command(SandboxExecutionTeardownOperation::Stop, "stop-lag", 1);
        assert!(matches!(
            journal
                .claim_dispatch_epoch(first.provider_claim())
                .expect("the first stop epoch should claim"),
            ProviderCommandClaimDecision::ExecuteClaimed(_)
        ));
        let mut manifest = fixture.manifest();
        manifest.shutdown_requested = true;
        let process =
            RuntimeProcessIdentity::fixture(manifest.handle.id.as_str(), "stop-lag-process", 42);
        let progress = match state {
            LagState::Intent => ContainerStopProgress::IntentPersisted {
                fence: first.provider_claim().clone(),
            },
            LagState::Term => ContainerStopProgress::TermMayExist {
                fence: first.provider_claim().clone(),
                process,
                grace_deadline_unix_millis: 1_000,
            },
            LagState::Kill => ContainerStopProgress::KillMayExist {
                fence: first.provider_claim().clone(),
                process,
                redelivery_not_before_unix_millis: 1_000,
            },
        };
        manifest.execution_teardown.set_stop(progress);
        fixture
            .backend
            .write_existing_workload_manifest(&manifest)
            .expect("the first stop progress should become durable");
        journal
            .record_observation(first.provider_claim(), first_kind, label.as_bytes())
            .expect("the first stop retry authority should become durable");

        let runtime = ScriptedRuntime::live(fixture.backend.clone(), 10_000);
        let second = fixture.command(SandboxExecutionTeardownOperation::Stop, "stop-lag", 2);
        let second_execution = match journal
            .claim_dispatch_epoch(second.provider_claim())
            .expect("the first adjacent stop retry should claim")
        {
            ProviderCommandClaimDecision::ExecuteClaimed(execution) => execution,
            ProviderCommandClaimDecision::AdoptExactAttempt(_) => {
                panic!("the first adjacent stop retry must receive execute authority")
            }
        };
        let before_inspect = snapshot_files(fixture.root.path());
        let inspected = fixture
            .backend
            .inspect_execution_teardown_inner_with_runtime_and_authorization(
                &second,
                &runtime,
                Some(second_execution.observation()),
            )
            .expect("the first claim-only stop retry should inspect older progress");
        assert_eq!(snapshot_files(fixture.root.path()), before_inspect);
        let inspected_kind = match &inspected {
            SandboxExecutionTeardownObservation::Absent { .. } => {
                ProviderCommandObservationKind::Absent
            }
            SandboxExecutionTeardownObservation::RetryAuthorized { .. } => {
                ProviderCommandObservationKind::RetryAuthorized
            }
            other => panic!("older stop progress must authorize a safe retry: {other:?}"),
        };
        journal
            .record_observation(
                second.provider_claim(),
                inspected_kind,
                inspected.evidence(),
            )
            .expect("the inspected stop retry authority should become durable");

        let third = fixture.command(SandboxExecutionTeardownOperation::Stop, "stop-lag", 3);
        let third_execution = match journal
            .claim_dispatch_epoch(third.provider_claim())
            .expect("the second adjacent stop retry should claim")
        {
            ProviderCommandClaimDecision::ExecuteClaimed(execution) => execution,
            ProviderCommandClaimDecision::AdoptExactAttempt(_) => {
                panic!("the second adjacent stop retry must receive execute authority")
            }
        };
        let before_untrusted = snapshot_files(fixture.root.path());
        assert!(matches!(
            fixture.backend.execute_execution_teardown(&third),
            SandboxExecutionTeardownObservation::DefiniteFailure { .. }
        ));
        assert_eq!(snapshot_files(fixture.root.path()), before_untrusted);
        assert!(runtime.signals().is_empty());

        assert!(matches!(
            fixture
                .backend
                .execute_execution_teardown_inner_with_runtime_and_authorization(
                    &third,
                    &runtime,
                    Some(third_execution.observation()),
                )
                .expect("journal-authenticated stop retry should advance older progress"),
            SandboxExecutionTeardownObservation::InProgress { .. }
        ));
        assert_eq!(runtime.signals(), vec![expected_signal]);
        assert!(match fixture.manifest().execution_teardown.stop() {
            ContainerStopProgress::TermMayExist { fence, .. }
            | ContainerStopProgress::KillMayExist { fence, .. } => {
                fence == third.provider_claim()
            }
            _ => false,
        });
    }
}
