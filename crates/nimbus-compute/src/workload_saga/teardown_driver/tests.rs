use nimbus_workloads::{
    ProposedWorkloadTeardownTransition, WorkloadSagaPhase, WorkloadTeardownCommandMode,
    WorkloadTeardownDecision, WorkloadTeardownStep,
};

use super::*;
use crate::workload_saga::recovery::tests::{
    cleanup_pending_record, no_reference_teardown_record, restart_settlement_pending_record,
    teardown_record,
};
use crate::workload_saga::teardown_test_support::{
    CasFault, DurableTeardownStore, RecordingTeardownProvider, TeardownProviderBehavior, driver,
};

const ORDERED_STEPS: [WorkloadTeardownStep; 5] = [
    WorkloadTeardownStep::WithdrawPublication,
    WorkloadTeardownStep::DrainExecution,
    WorkloadTeardownStep::StopExecution,
    WorkloadTeardownStep::DetachNetwork,
    WorkloadTeardownStep::ReleaseNetwork,
];

fn withdrawal_record(label: &str) -> nimbus_workloads::WorkloadSagaRecord {
    teardown_record(label, WorkloadSagaPhase::WithdrawalCommitted)
}

fn claimed_withdrawal(label: &str) -> nimbus_workloads::WorkloadSagaRecord {
    let record = withdrawal_record(label);
    let WorkloadTeardownDecision::PersistCandidate(
        proposed @ ProposedWorkloadTeardownTransition::Claim { .. },
    ) = record.decide_teardown().expect("withdrawal is reducible")
    else {
        panic!("withdrawal fixture must require an exact claim");
    };
    materialize_teardown_candidate(&record, &proposed).expect("claim fixture must materialize")
}

#[tokio::test]
async fn teardown_driver_records_exact_five_step_order() {
    let initial = withdrawal_record("teardown-order");
    let store = DurableTeardownStore::with_record(initial.clone());
    let provider = RecordingTeardownProvider::new(TeardownProviderBehavior::Succeed);
    let driver = driver(store, &initial, provider.clone());

    let run = driver
        .resume(initial.key())
        .await
        .expect("exact teardown should complete");

    assert_eq!(run.record().phase(), WorkloadSagaPhase::Recorded);
    assert_eq!(run.disposition(), WorkloadTeardownRunDisposition::Completed);
    let calls = provider.calls();
    assert_eq!(calls.len(), ORDERED_STEPS.len());
    assert_eq!(
        calls.iter().map(|call| call.step).collect::<Vec<_>>(),
        ORDERED_STEPS
    );
    assert!(
        calls
            .iter()
            .all(|call| call.mode == WorkloadTeardownCommandMode::Execute)
    );
}

#[tokio::test]
async fn teardown_driver_confirms_each_result_before_next_capability() {
    let initial = withdrawal_record("teardown-confirm-each");
    let store = DurableTeardownStore::with_record(initial.clone());
    let provider = RecordingTeardownProvider::new(TeardownProviderBehavior::Succeed);
    let driver = driver(store.clone(), &initial, provider.clone());

    driver
        .resume(initial.key())
        .await
        .expect("exact teardown should complete");

    assert_eq!(provider.calls().len(), 5);
    assert_eq!(store.record().phase(), WorkloadSagaPhase::Recorded);
    assert!(store.counts().1 >= 11, "each claim and result must commit");
}

#[tokio::test]
async fn recovered_pending_claim_persists_inspection_before_provider_read() {
    let pending = claimed_withdrawal("teardown-recovered-pending");
    let store = DurableTeardownStore::with_record(pending.clone());
    let provider = RecordingTeardownProvider::new(TeardownProviderBehavior::Succeed);
    let driver = driver(store, &pending, provider.clone());

    driver
        .resume(pending.key())
        .await
        .expect("recovered teardown should complete by inspection");

    let first = provider
        .calls()
        .into_iter()
        .next()
        .expect("inspection occurs");
    assert_eq!(first.step, WorkloadTeardownStep::WithdrawPublication);
    assert_eq!(first.mode, WorkloadTeardownCommandMode::Inspect);
}

#[tokio::test]
async fn ambiguous_effect_result_persists_inspection_required() {
    let initial = withdrawal_record("teardown-effect-ambiguity");
    let store = DurableTeardownStore::with_record(initial.clone());
    let provider = RecordingTeardownProvider::new(
        TeardownProviderBehavior::AmbiguousExecuteThenSatisfiedInspectAt(
            WorkloadTeardownStep::WithdrawPublication,
        ),
    );
    let driver = driver(store, &initial, provider.clone());

    driver
        .resume(initial.key())
        .await
        .expect("ambiguous effect should converge by inspection");

    let calls = provider.calls();
    assert_eq!(calls[0].mode, WorkloadTeardownCommandMode::Execute);
    assert_eq!(calls[1].mode, WorkloadTeardownCommandMode::Inspect);
    assert_eq!(calls[0].step, calls[1].step);
}

#[tokio::test]
async fn not_completed_inspection_authorizes_same_attempt_next_epoch_once() {
    let pending = claimed_withdrawal("teardown-not-completed");
    let original_claim = pending
        .teardown_disposition()
        .and_then(nimbus_workloads::WorkloadTeardownDisposition::claim)
        .expect("pending fixture retains a claim")
        .clone();
    let store = DurableTeardownStore::with_record(pending.clone());
    let provider = RecordingTeardownProvider::new(TeardownProviderBehavior::NotCompletedOnceAt(
        WorkloadTeardownStep::WithdrawPublication,
    ));
    let driver = driver(store, &pending, provider.clone());

    driver
        .resume(pending.key())
        .await
        .expect("not-completed inspection should retry once");

    let calls = provider.calls();
    assert_eq!(calls[0].mode, WorkloadTeardownCommandMode::Inspect);
    assert_eq!(calls[1].mode, WorkloadTeardownCommandMode::Execute);
    assert_eq!(calls[0].step, calls[1].step);
    assert_eq!(
        original_claim.dispatch_epoch().checked_next(),
        Some(nimbus_workloads::WorkloadTeardownDispatchEpoch::new(1))
    );
}

#[tokio::test]
async fn teardown_claim_ambiguity_requires_one_fresh_read() {
    let initial = withdrawal_record("teardown-claim-ambiguous");
    let store = DurableTeardownStore::with_record_and_fault(
        initial.clone(),
        CasFault::AmbiguousBeforeApply,
    );
    let provider = RecordingTeardownProvider::new(TeardownProviderBehavior::Succeed);
    let driver = driver(store.clone(), &initial, provider.clone());

    let run = driver
        .resume(initial.key())
        .await
        .expect("unresolved claim ambiguity returns waiting");

    assert_eq!(run.disposition(), WorkloadTeardownRunDisposition::Waiting);
    assert_eq!(store.counts(), (2, 1));
    assert!(provider.calls().is_empty());
}

#[tokio::test]
async fn teardown_result_ambiguity_requires_fresh_read_before_progress() {
    let initial = withdrawal_record("teardown-claim-applied-ambiguous");
    let store =
        DurableTeardownStore::with_record_and_fault(initial.clone(), CasFault::AmbiguousAfterApply);
    let provider = RecordingTeardownProvider::new(TeardownProviderBehavior::Succeed);
    let driver = driver(store.clone(), &initial, provider.clone());

    driver
        .resume(initial.key())
        .await
        .expect("confirmed claim ambiguity should inspect and converge");

    assert_eq!(store.counts().0, 2);
    assert_eq!(
        provider.calls()[0].mode,
        WorkloadTeardownCommandMode::Inspect
    );
}

#[tokio::test]
async fn teardown_claim_contenders_produce_one_execute_call() {
    let initial = withdrawal_record("teardown-contenders");
    let store = DurableTeardownStore::with_record(initial.clone());
    let provider = RecordingTeardownProvider::new(TeardownProviderBehavior::Succeed);
    let first = driver(store.clone(), &initial, provider.clone());
    let second = driver(store, &initial, provider.clone());
    let key = initial.key().clone();
    let other_key = key.clone();

    let (first, second) = tokio::join!(first.resume(&key), second.resume(&other_key));
    first.expect("first contender should converge");
    second.expect("second contender should converge");

    let execute_withdrawals = provider
        .calls()
        .into_iter()
        .filter(|call| {
            call.step == WorkloadTeardownStep::WithdrawPublication
                && call.mode == WorkloadTeardownCommandMode::Execute
        })
        .count();
    assert_eq!(execute_withdrawals, 1);
}

#[tokio::test]
async fn teardown_claim_conflict_reloads_durable_truth() {
    let initial = withdrawal_record("teardown-conflict-reload");
    let store = DurableTeardownStore::with_record(initial.clone());
    let provider = RecordingTeardownProvider::new(TeardownProviderBehavior::Succeed);
    let first = driver(store.clone(), &initial, provider.clone());
    let second = driver(store, &initial, provider.clone());
    let key = initial.key().clone();
    let other_key = key.clone();
    let (first, second) = tokio::join!(first.resume(&key), second.resume(&other_key));
    assert!(first.is_ok());
    assert!(second.is_ok());
    assert_eq!(
        provider
            .calls()
            .into_iter()
            .filter(|call| {
                call.step == WorkloadTeardownStep::WithdrawPublication
                    && call.mode == WorkloadTeardownCommandMode::Execute
            })
            .count(),
        1
    );
}

#[tokio::test]
async fn resource_free_teardown_makes_zero_capability_calls() {
    let initial = no_reference_teardown_record(
        "teardown-resource-free",
        WorkloadSagaPhase::WithdrawalCommitted,
    );
    let store = DurableTeardownStore::with_record(initial.clone());
    let provider = RecordingTeardownProvider::new(TeardownProviderBehavior::Succeed);
    let driver = driver(store, &initial, provider.clone());

    let run = driver
        .resume(initial.key())
        .await
        .expect("resource-free teardown should complete");

    assert_eq!(run.record().phase(), WorkloadSagaPhase::Recorded);
    assert!(provider.calls().is_empty());
}

#[tokio::test]
async fn in_progress_and_ambiguous_inspection_return_bounded_waiting() {
    for (label, behavior) in [
        (
            "in-progress",
            TeardownProviderBehavior::InProgressAt(WorkloadTeardownStep::WithdrawPublication),
        ),
        (
            "ambiguous",
            TeardownProviderBehavior::AmbiguousAt(WorkloadTeardownStep::WithdrawPublication),
        ),
    ] {
        let initial = withdrawal_record(&format!("teardown-wait-{label}"));
        let store = DurableTeardownStore::with_record(initial.clone());
        let provider = RecordingTeardownProvider::new(behavior);
        let driver = driver(store, &initial, provider.clone());
        let run = driver
            .resume(initial.key())
            .await
            .expect("uncertain inspection returns a bounded wait");
        assert_eq!(run.disposition(), WorkloadTeardownRunDisposition::Waiting);
        assert_eq!(provider.calls().len(), 2);
    }

    let initial = withdrawal_record("teardown-repeating-conflict");
    let store = DurableTeardownStore::with_record_and_repeating_conflict(initial.clone());
    let provider = RecordingTeardownProvider::new(TeardownProviderBehavior::Succeed);
    let driver = driver(store.clone(), &initial, provider.clone());
    assert!(matches!(
        driver.resume(initial.key()).await,
        Err(WorkloadTeardownRunError::ProgressLimit)
    ));
    assert_eq!(store.counts(), (65, 64));
    assert!(provider.calls().is_empty());
}

#[tokio::test]
async fn restart_settlement_and_cleanup_pending_make_zero_teardown_calls() {
    for (record, expected) in [
        (
            restart_settlement_pending_record("teardown-restart-settlement-pending"),
            WorkloadTeardownRunDisposition::RestartSettlementPending,
        ),
        (
            cleanup_pending_record("teardown-cleanup-pending"),
            WorkloadTeardownRunDisposition::CleanupPending,
        ),
    ] {
        let store = DurableTeardownStore::with_record(record.clone());
        let provider = RecordingTeardownProvider::new(TeardownProviderBehavior::Succeed);
        let driver = driver(store, &record, provider.clone());

        let run = driver
            .resume(record.key())
            .await
            .expect("later-owned teardown handoff should be typed");

        assert_eq!(run.disposition(), expected);
        assert_eq!(run.record(), &record);
        assert!(provider.calls().is_empty());
    }
}

#[tokio::test]
async fn definite_failure_enters_cleanup_without_a_second_effect() {
    let initial = withdrawal_record("teardown-definite-failure");
    let store = DurableTeardownStore::with_record(initial.clone());
    let provider = RecordingTeardownProvider::new(TeardownProviderBehavior::DefiniteFailureAt(
        WorkloadTeardownStep::WithdrawPublication,
    ));
    let driver = driver(store, &initial, provider.clone());
    let run = driver
        .resume(initial.key())
        .await
        .expect("definite provider failure becomes a typed cleanup handoff");
    assert_eq!(
        run.disposition(),
        WorkloadTeardownRunDisposition::CleanupPending
    );
    assert_eq!(provider.calls().len(), 1);
}

#[tokio::test]
async fn resource_free_and_terminal_transitions_emit_no_command() {
    let initial = no_reference_teardown_record(
        "teardown-resource-free-terminal",
        WorkloadSagaPhase::WithdrawalCommitted,
    );
    let store = DurableTeardownStore::with_record(initial.clone());
    let provider = RecordingTeardownProvider::new(TeardownProviderBehavior::Succeed);
    let driver = driver(store, &initial, provider.clone());
    let run = driver
        .resume(initial.key())
        .await
        .expect("resource-free and terminal transitions should complete");
    assert_eq!(run.record().phase(), WorkloadSagaPhase::Recorded);
    assert!(provider.calls().is_empty());
}
