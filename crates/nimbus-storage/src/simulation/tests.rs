use std::num::NonZeroU64;

use nimbus_core::Timestamp;

use super::*;

#[tokio::test]
async fn scenario_signal_wait_returns_after_trigger_even_if_triggered_first() {
    let harness = DeterministicHarness::scenario("signal-wait", 7, Timestamp(1_000));
    let signal = harness.cancellation("client-drop");
    signal.trigger();
    signal.wait().await;
    assert!(signal.is_triggered());
    assert_eq!(signal.describe(), "cancellation signal 'client-drop'");
}

#[test]
fn harness_reuses_named_signals_and_preserves_metadata() {
    let harness = DeterministicHarness::scenario("metadata", 42, Timestamp(5_000));
    let left = harness.disconnect("socket-1");
    let right = harness.disconnect("socket-1");

    assert_eq!(harness.name(), "metadata");
    assert_eq!(harness.seed(), 42);
    assert_eq!(harness.describe(), "metadata (seed 42)");
    assert_eq!(left.name(), right.name());
    assert_eq!(left.kind(), ScenarioSignalKind::Disconnect);
    assert!(!left.is_triggered());

    left.trigger();
    assert!(right.is_triggered());
}

#[test]
fn seeded_harness_replays_the_same_fault_schedule_for_the_same_seed() {
    let left = DeterministicHarness::seeded(
        "left",
        11,
        Timestamp(10_000),
        NonZeroU64::new(3).expect("period should be non-zero"),
    );
    let right = DeterministicHarness::seeded(
        "right",
        11,
        Timestamp(10_000),
        NonZeroU64::new(3).expect("period should be non-zero"),
    );

    let left_results = [
        FaultPoint::StorageCommitBeforeVisibility,
        FaultPoint::JournalAppendBeforeDurableFlush,
        FaultPoint::StorageCommitBeforeVisibility,
        FaultPoint::CheckpointPublishBeforeManifestUpdate,
        FaultPoint::StorageCommitBeforeVisibility,
        FaultPoint::CompactionStartBeforePublish,
    ]
    .into_iter()
    .map(|point| left.check_fault(point).is_err())
    .collect::<Vec<_>>();
    let right_results = [
        FaultPoint::StorageCommitBeforeVisibility,
        FaultPoint::JournalAppendBeforeDurableFlush,
        FaultPoint::StorageCommitBeforeVisibility,
        FaultPoint::CheckpointPublishBeforeManifestUpdate,
        FaultPoint::StorageCommitBeforeVisibility,
        FaultPoint::CompactionStartBeforePublish,
    ]
    .into_iter()
    .map(|point| right.check_fault(point).is_err())
    .collect::<Vec<_>>();

    assert_eq!(left_results, right_results);
}

#[test]
fn generated_task_history_is_reproducible_for_the_same_seed() {
    let left = GeneratedTaskHistory::seeded("left", 23, 12);
    let right = GeneratedTaskHistory::seeded("right", 23, 12);

    assert_eq!(left.steps(), right.steps());
    assert_eq!(left.model(), right.model());
    assert_eq!(left.query_status(), right.query_status());
    assert!(matches!(
        left.query_status(),
        "todo" | "done" | "in_progress"
    ));
    assert_eq!(left.page_size(), 2);
}

#[test]
fn scripted_restart_schedule_is_reproducible_for_the_same_seed() {
    let left = ScriptedRestartSchedule::seeded(
        "left",
        91,
        12,
        3,
        &[
            RestartBoundary::DurableAppendBeforeApply,
            RestartBoundary::SchedulerClaim,
            RestartBoundary::SchedulerCompletion,
        ],
    );
    let right = ScriptedRestartSchedule::seeded(
        "right",
        91,
        12,
        3,
        &[
            RestartBoundary::DurableAppendBeforeApply,
            RestartBoundary::SchedulerClaim,
            RestartBoundary::SchedulerCompletion,
        ],
    );

    assert_eq!(left.restart_points(), right.restart_points());
    assert_eq!(left.restart_points().len(), 3);
    assert!(left.describe().contains("seed 91"));
}

#[tokio::test]
async fn generated_task_history_async_replay_preserves_slot_bindings() {
    let history = GeneratedTaskHistory::seeded("async-runner", 23, 12);
    let remaining =
        replay_generated_task_history_async(
            &history,
            |slot, _record| async move {
                Ok::<String, std::convert::Infallible>(format!("slot-{slot}"))
            },
            |_slot, _id, _record| async move { Ok::<(), std::convert::Infallible>(()) },
            |_slot, _id| async move { Ok::<(), std::convert::Infallible>(()) },
        )
        .await
        .expect("async replay should succeed");

    assert_eq!(remaining.len(), history.model().final_documents().len());
}

#[test]
fn verification_harness_seed_corpus_has_explicit_required_and_nightly_modes() {
    let required = generated_task_history_seed_corpus(VerificationHarnessMode::Required);
    let nightly = generated_task_history_seed_corpus(VerificationHarnessMode::Nightly);

    assert_eq!(required.len(), 2);
    assert!(
        required
            .iter()
            .all(|case| case.mode == VerificationHarnessMode::Required)
    );
    assert!(nightly.len() > required.len());
    assert!(
        nightly
            .iter()
            .all(|case| case.mode == VerificationHarnessMode::Nightly)
    );
    assert!(required.iter().all(|case| {
        nightly
            .iter()
            .any(|nightly_case| nightly_case.id == case.id)
    }));
    assert!(nightly.iter().any(|case| case.regression));
}

#[test]
fn verification_harness_seed_corpus_can_filter_to_one_named_case() {
    let selected = filter_generated_task_history_seed_corpus(
        generated_task_history_seed_corpus(VerificationHarnessMode::Nightly),
        Some("regression-two-page-pagination-41"),
    )
    .expect("seed corpus filter should accept a named case");

    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].id, "regression-two-page-pagination-41");
    assert!(selected[0].regression);
}

#[test]
fn verification_harness_seed_case_formats_deterministic_repro_command() {
    let case = generated_task_history_seed_corpus(VerificationHarnessMode::Required)[1];
    assert_eq!(
        case.repro_command(
            "nimbus-engine",
            "verification_harness_required_generated_history_seed_corpus_matches_model"
        ),
        "NIMBUS_VERIFY_CASE=regression-two-page-pagination-41 cargo test -p nimbus-engine verification_harness_required_generated_history_seed_corpus_matches_model -- --ignored --nocapture"
    );
}
