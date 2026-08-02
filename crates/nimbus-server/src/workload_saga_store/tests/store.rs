use std::sync::Arc;

use nimbus_core::SequenceNumber;
use nimbus_workloads::{
    WorkloadSagaCommit, WorkloadSagaExpected, WorkloadSagaStore, WorkloadSagaStoreError,
};

use super::super::EngineWorkloadSagaStore;
use super::super::schema::workload_saga_tenant;
use super::{
    engine, initial_record, initial_record_with_seed, valid_competing_successor, valid_successor,
};

#[tokio::test]
async fn missing_and_current_cas_persist_and_exact_replay_is_unchanged() {
    let root = tempfile::tempdir().expect("fixture root should build");
    let store = EngineWorkloadSagaStore::new(engine(&root));
    let initial = initial_record("store-cas");

    assert_eq!(store.load(initial.key()).await, Ok(None));
    assert_eq!(
        store
            .compare_and_swap(WorkloadSagaExpected::Missing, initial.clone())
            .await,
        Ok(WorkloadSagaCommit::Applied)
    );
    assert_eq!(store.load(initial.key()).await, Ok(Some(initial.clone())));
    assert_eq!(
        store
            .compare_and_swap(
                WorkloadSagaExpected::Revision(nimbus_workloads::WorkloadSagaRevision::new(99)),
                initial.clone(),
            )
            .await,
        Ok(WorkloadSagaCommit::Unchanged)
    );

    let next = valid_successor(&initial);
    assert_eq!(
        store
            .compare_and_swap(
                WorkloadSagaExpected::Revision(initial.revision()),
                next.clone(),
            )
            .await,
        Ok(WorkloadSagaCommit::Applied)
    );
    assert_eq!(store.load(initial.key()).await, Ok(Some(next)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_and_current_contention_each_have_one_winner() {
    let root = tempfile::tempdir().expect("fixture root should build");
    let engine = engine(&root);
    let left = Arc::new(EngineWorkloadSagaStore::new(Arc::clone(&engine)));
    let right = Arc::new(EngineWorkloadSagaStore::new(engine));
    let initial = initial_record_with_seed("contention", "left");
    let divergent_initial = initial_record_with_seed("contention", "right");

    let (first, second) = tokio::join!(
        left.compare_and_swap(WorkloadSagaExpected::Missing, initial.clone()),
        right.compare_and_swap(WorkloadSagaExpected::Missing, divergent_initial)
    );
    let results = [first, second];
    assert_eq!(
        results
            .iter()
            .filter(|result| **result == Ok(WorkloadSagaCommit::Applied))
            .count(),
        1
    );
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(WorkloadSagaStoreError::Conflict { .. })))
            .count(),
        1,
        "divergent missing-record contention must fence the loser"
    );

    let current = left
        .load(initial.key())
        .await
        .expect("winner should load")
        .expect("winner should be durable");
    let next = valid_successor(&current);
    let competing = valid_competing_successor(&current);
    let expected = WorkloadSagaExpected::Revision(current.revision());
    let (first, second) = tokio::join!(
        left.compare_and_swap(expected, next.clone()),
        right.compare_and_swap(expected, competing)
    );
    assert_eq!(
        [first.clone(), second.clone()]
            .iter()
            .filter(|result| **result == Ok(WorkloadSagaCommit::Applied))
            .count(),
        1
    );
    assert_eq!(
        [first, second]
            .iter()
            .filter(|result| matches!(result, Err(WorkloadSagaStoreError::Conflict { .. })))
            .count(),
        1
    );
}

#[tokio::test]
async fn stale_expectation_and_illegal_successor_commit_nothing() {
    let root = tempfile::tempdir().expect("fixture root should build");
    let engine = engine(&root);
    let store = EngineWorkloadSagaStore::new(engine.clone());
    let current = initial_record_with_seed("rejection", "current");
    assert_eq!(
        store
            .compare_and_swap(WorkloadSagaExpected::Missing, current.clone())
            .await,
        Ok(WorkloadSagaCommit::Applied)
    );
    let before = engine
        .read_durable_journal_async(workload_saga_tenant().unwrap(), SequenceNumber(0))
        .await
        .expect("journal should read before rejected transitions");

    assert!(matches!(
        store
            .compare_and_swap(
                WorkloadSagaExpected::Revision(nimbus_workloads::WorkloadSagaRevision::new(99)),
                valid_successor(&current),
            )
            .await,
        Err(WorkloadSagaStoreError::Conflict { .. })
    ));

    let crossed_intent = valid_successor(&initial_record_with_seed("rejection", "crossed"));
    assert!(matches!(
        store
            .compare_and_swap(
                WorkloadSagaExpected::Revision(current.revision()),
                crossed_intent,
            )
            .await,
        Err(WorkloadSagaStoreError::InvalidTransition(_))
    ));
    assert_eq!(store.load(current.key()).await, Ok(Some(current)));
    assert_eq!(
        engine
            .read_durable_journal_async(workload_saga_tenant().unwrap(), SequenceNumber(0))
            .await
            .expect("journal should remain readable"),
        before,
        "stale and illegal transitions must not commit"
    );
}
