use nimbus_core::Error;
use nimbus_engine::{Fault, commit_fault_labels};
use nimbus_workloads::{
    WorkloadSagaCommit, WorkloadSagaExpected, WorkloadSagaStore, WorkloadSagaStoreError,
};

use super::super::EngineWorkloadSagaStore;
use super::{engine, initial_record, valid_successor};

#[tokio::test]
async fn pre_persist_commit_error_is_ambiguous_and_fresh_truth_remains_old() {
    let root = tempfile::tempdir().expect("fixture root should build");
    let engine = engine(&root);
    let store = EngineWorkloadSagaStore::new(engine.clone());
    let initial = initial_record("ambiguity-pre-persist");
    assert_eq!(
        store
            .compare_and_swap(WorkloadSagaExpected::Missing, initial.clone())
            .await,
        Ok(WorkloadSagaCommit::Applied)
    );
    let next = valid_successor(&initial);

    engine.commit_fault_handle_for_testing().inject(
        commit_fault_labels::PRE_PERSIST,
        Fault::Error(Error::Internal(
            "injected workload-saga pre-persist failure".to_owned(),
        )),
    );

    assert_eq!(
        store
            .compare_and_swap(WorkloadSagaExpected::Revision(initial.revision()), next,)
            .await,
        Err(WorkloadSagaStoreError::Ambiguous)
    );
    assert_eq!(
        store.load(initial.key()).await,
        Ok(Some(initial)),
        "a fresh truth read must preserve the old record when persistence never began"
    );
}

#[tokio::test]
async fn durable_before_publish_error_is_ambiguous_and_fresh_truth_is_exact_next() {
    let root = tempfile::tempdir().expect("fixture root should build");
    let engine = engine(&root);
    let store = EngineWorkloadSagaStore::new(engine.clone());
    let initial = initial_record("ambiguity-durable-before-publish");
    assert_eq!(
        store
            .compare_and_swap(WorkloadSagaExpected::Missing, initial.clone())
            .await,
        Ok(WorkloadSagaCommit::Applied)
    );
    let next = valid_successor(&initial);

    engine.commit_fault_handle_for_testing().inject(
        commit_fault_labels::DURABLE_BEFORE_PUBLISH,
        Fault::Error(Error::Internal(
            "injected workload-saga durable-before-publish failure".to_owned(),
        )),
    );

    assert_eq!(
        store
            .compare_and_swap(
                WorkloadSagaExpected::Revision(initial.revision()),
                next.clone(),
            )
            .await,
        Err(WorkloadSagaStoreError::Ambiguous)
    );
    assert_eq!(
        store.load(initial.key()).await,
        Ok(Some(next)),
        "a fresh truth read must observe the exact durable transition"
    );
}

#[tokio::test]
async fn post_publish_pre_fanout_error_is_ambiguous_and_fresh_truth_is_exact_next() {
    let root = tempfile::tempdir().expect("fixture root should build");
    let engine = engine(&root);
    let store = EngineWorkloadSagaStore::new(engine.clone());
    let initial = initial_record("ambiguity-post-publish-pre-fanout");
    assert_eq!(
        store
            .compare_and_swap(WorkloadSagaExpected::Missing, initial.clone())
            .await,
        Ok(WorkloadSagaCommit::Applied)
    );
    let next = valid_successor(&initial);

    engine.commit_fault_handle_for_testing().inject(
        commit_fault_labels::POST_PUBLISH_PRE_FANOUT,
        Fault::Error(Error::Internal(
            "injected workload-saga post-publish-pre-fanout failure".to_owned(),
        )),
    );

    assert_eq!(
        store
            .compare_and_swap(
                WorkloadSagaExpected::Revision(initial.revision()),
                next.clone(),
            )
            .await,
        Err(WorkloadSagaStoreError::Ambiguous)
    );
    assert_eq!(
        store.load(initial.key()).await,
        Ok(Some(next)),
        "a fresh truth read must observe the exact published transition"
    );
}

#[tokio::test]
async fn commit_task_panic_after_durability_is_ambiguous_and_truth_is_exact_next() {
    let root = tempfile::tempdir().expect("fixture root should build");
    let engine = engine(&root);
    let store = EngineWorkloadSagaStore::new(engine.clone());
    let initial = initial_record("ambiguity-commit-task-panic");
    assert_eq!(
        store
            .compare_and_swap(WorkloadSagaExpected::Missing, initial.clone())
            .await,
        Ok(WorkloadSagaCommit::Applied)
    );
    let next = valid_successor(&initial);
    engine
        .commit_fault_handle_for_testing()
        .inject_panic_on_nth_hit(commit_fault_labels::DURABLE_BEFORE_PUBLISH, 1);

    assert_eq!(
        store
            .compare_and_swap(
                WorkloadSagaExpected::Revision(initial.revision()),
                next.clone(),
            )
            .await,
        Err(WorkloadSagaStoreError::Ambiguous)
    );
    assert_eq!(
        store.load(initial.key()).await,
        Ok(Some(next)),
        "a blocking-task panic after durability must not be misclassified as unavailable"
    );
}
