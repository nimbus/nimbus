use std::num::NonZeroU64;

use nimbus_core::{Error, WorkloadId};
use nimbus_engine::{Fault, commit_fault_labels};
use nimbus_workloads::{
    TenantRetirementCommit, TenantRetirementExpected, TenantRetirementPageRequest,
    TenantRetirementPhase, TenantRetirementRecord, TenantRetirementStore,
    TenantRetirementStoreError, TenantWorkloadMutationEpoch, WorkloadSagaCommit,
    WorkloadSagaExpected, WorkloadSagaStore,
};

use super::super::EngineWorkloadSagaStore;
use super::{engine, initial_record, valid_successor};

fn retirement(label: &str, incarnation: u64) -> TenantRetirementRecord {
    TenantRetirementRecord::new(
        nimbus_core::TenantId::new(format!("tenant-{label}")).expect("fixture tenant is valid"),
        NonZeroU64::new(incarnation).expect("fixture incarnation is nonzero"),
        Vec::new(),
    )
    .expect("fixture retirement is valid")
}

#[tokio::test]
async fn workload_saga_cas_advances_exact_tenant_epoch_once_per_applied_transition() {
    let root = tempfile::tempdir().expect("fixture root should build");
    let store = EngineWorkloadSagaStore::new(engine(&root));
    let initial = initial_record("tenant-epoch");
    assert_eq!(
        store
            .load_workload_mutation_epoch(initial.key().tenant_id())
            .await,
        Ok(TenantWorkloadMutationEpoch::new(0))
    );

    assert_eq!(
        store
            .compare_and_swap(WorkloadSagaExpected::Missing, initial.clone())
            .await,
        Ok(WorkloadSagaCommit::Applied)
    );
    assert_eq!(
        store
            .load_workload_mutation_epoch(initial.key().tenant_id())
            .await,
        Ok(TenantWorkloadMutationEpoch::new(1))
    );

    assert_eq!(
        store
            .compare_and_swap(WorkloadSagaExpected::Missing, initial.clone())
            .await,
        Ok(WorkloadSagaCommit::Unchanged)
    );
    assert_eq!(
        store
            .load_workload_mutation_epoch(initial.key().tenant_id())
            .await,
        Ok(TenantWorkloadMutationEpoch::new(1)),
        "an exact replay must not create a false inventory mutation"
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
    assert_eq!(
        store
            .load_workload_mutation_epoch(initial.key().tenant_id())
            .await,
        Ok(TenantWorkloadMutationEpoch::new(2))
    );
    assert_eq!(store.load(initial.key()).await, Ok(Some(next)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn different_workloads_retry_shared_tenant_epoch_contention_without_false_saga_conflict() {
    let root = tempfile::tempdir().expect("fixture root should build");
    let engine = engine(&root);
    let store = std::sync::Arc::new(EngineWorkloadSagaStore::new(engine.clone()));
    store
        .prepare()
        .await
        .expect("fixture schema should prepare");
    let first = initial_record("shared-epoch");
    let second_key = nimbus_workloads::WorkloadSagaKey::new(
        first.key().tenant_id().clone(),
        WorkloadId::new("workload-shared-epoch-second").expect("second fixture workload is valid"),
    );
    let second =
        nimbus_workloads::WorkloadSagaRecord::new(second_key, first.active_intent().clone())
            .expect("second fixture saga is valid");
    let faults = engine.commit_fault_handle_for_testing();
    faults.arm(commit_fault_labels::PRE_ASSIGN);

    let first_commit = tokio::spawn({
        let store = store.clone();
        let first = first.clone();
        async move {
            store
                .compare_and_swap(WorkloadSagaExpected::Missing, first)
                .await
        }
    });
    let second_commit = tokio::spawn({
        let store = store.clone();
        let second = second.clone();
        async move {
            store
                .compare_and_swap(WorkloadSagaExpected::Missing, second)
                .await
        }
    });
    let both_staged = tokio::task::spawn_blocking({
        let faults = faults.clone();
        move || {
            faults.wait_until_hits(
                commit_fault_labels::PRE_ASSIGN,
                2,
                std::time::Duration::from_secs(5),
            )
        }
    })
    .await
    .expect("commit-fault wait should join");
    assert!(
        both_staged,
        "both transactions must stage the same old epoch"
    );
    faults.release(commit_fault_labels::PRE_ASSIGN);

    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_secs(5), first_commit)
            .await
            .expect("first commit should finish")
            .expect("first commit task should join"),
        Ok(WorkloadSagaCommit::Applied)
    );
    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_secs(5), second_commit)
            .await
            .expect("second commit should finish")
            .expect("second commit task should join"),
        Ok(WorkloadSagaCommit::Applied)
    );
    assert_eq!(store.load(first.key()).await, Ok(Some(first.clone())));
    assert_eq!(store.load(second.key()).await, Ok(Some(second)));
    assert_eq!(
        store
            .load_workload_mutation_epoch(first.key().tenant_id())
            .await,
        Ok(TenantWorkloadMutationEpoch::new(2))
    );
}

#[tokio::test]
async fn tenant_retirement_cas_page_terminal_delete_and_reopen_are_exact() {
    let root = tempfile::tempdir().expect("fixture root should build");
    let writer_engine = engine(&root);
    let store = EngineWorkloadSagaStore::new(writer_engine.clone());
    let mut current = retirement("durable-retirement", 3);

    assert_eq!(store.load_retirement(current.tenant_id()).await, Ok(None));
    assert_eq!(
        store
            .compare_and_swap_retirement(TenantRetirementExpected::Missing, current.clone())
            .await,
        Ok(TenantRetirementCommit::Applied)
    );
    assert_eq!(
        store
            .compare_and_swap_retirement(TenantRetirementExpected::Missing, current.clone())
            .await,
        Ok(TenantRetirementCommit::Unchanged)
    );

    let active = store
        .list_active_retirements(TenantRetirementPageRequest::new(None, 1).unwrap())
        .await
        .expect("active retirement page should load");
    assert_eq!(active.records(), &[current.clone()]);

    for phase in [
        TenantRetirementPhase::ChildrenRecorded,
        TenantRetirementPhase::SourcesFinalized,
        TenantRetirementPhase::EngineDeleted,
        TenantRetirementPhase::Recorded,
    ] {
        let next = current.advance(phase).unwrap();
        assert_eq!(
            store
                .compare_and_swap_retirement(
                    TenantRetirementExpected::Revision(current.revision()),
                    next.clone(),
                )
                .await,
            Ok(TenantRetirementCommit::Applied)
        );
        current = next;
    }
    assert!(
        store
            .list_active_retirements(TenantRetirementPageRequest::new(None, 1).unwrap())
            .await
            .unwrap()
            .records()
            .is_empty()
    );

    drop(store);
    drop(writer_engine);
    let reopened = EngineWorkloadSagaStore::new(engine(&root));
    assert_eq!(
        reopened.load_retirement(current.tenant_id()).await,
        Ok(Some(current.clone()))
    );
    assert_eq!(
        reopened.delete_retirement(current.clone()).await,
        Ok(TenantRetirementCommit::Applied)
    );
    assert_eq!(
        reopened.delete_retirement(current.clone()).await,
        Ok(TenantRetirementCommit::Unchanged)
    );
    assert_eq!(
        reopened.load_retirement(current.tenant_id()).await,
        Ok(None)
    );
}

#[tokio::test]
async fn active_retirement_pages_are_bounded_ordered_and_cursor_complete() {
    let root = tempfile::tempdir().expect("fixture root should build");
    let store = EngineWorkloadSagaStore::new(engine(&root));
    let mut active = vec![
        retirement("page-charlie", 3),
        retirement("page-alpha", 1),
        retirement("page-bravo", 2),
    ];
    for record in &active {
        assert_eq!(
            store
                .compare_and_swap_retirement(TenantRetirementExpected::Missing, record.clone())
                .await,
            Ok(TenantRetirementCommit::Applied)
        );
    }
    let mut terminal = retirement("page-terminal", 4);
    assert_eq!(
        store
            .compare_and_swap_retirement(TenantRetirementExpected::Missing, terminal.clone())
            .await,
        Ok(TenantRetirementCommit::Applied)
    );
    for phase in [
        TenantRetirementPhase::ChildrenRecorded,
        TenantRetirementPhase::SourcesFinalized,
        TenantRetirementPhase::EngineDeleted,
        TenantRetirementPhase::Recorded,
    ] {
        let next = terminal.advance(phase).unwrap();
        assert_eq!(
            store
                .compare_and_swap_retirement(
                    TenantRetirementExpected::Revision(terminal.revision()),
                    next.clone(),
                )
                .await,
            Ok(TenantRetirementCommit::Applied)
        );
        terminal = next;
    }

    active.sort_by(|left, right| left.retirement_id().cmp(right.retirement_id()));
    let mut observed = Vec::new();
    let mut cursor = None;
    loop {
        let page = store
            .list_active_retirements(TenantRetirementPageRequest::new(cursor.clone(), 1).unwrap())
            .await
            .expect("bounded active-retirement page should load");
        assert!(page.records().len() <= 1);
        observed.extend_from_slice(page.records());
        match page.next_cursor().cloned() {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }

    assert_eq!(observed, active);
    assert!(observed.iter().all(|record| !record.phase().is_terminal()));
    assert!(observed.iter().all(|record| record != &terminal));

    let mut retained = active.clone();
    retained.push(terminal.clone());
    retained.sort_by(|left, right| left.retirement_id().cmp(right.retirement_id()));
    let mut observed_retained = Vec::new();
    let mut retained_cursor = None;
    loop {
        let page = store
            .list_retirements(TenantRetirementPageRequest::new(retained_cursor.clone(), 1).unwrap())
            .await
            .expect("bounded retained-retirement page should load");
        observed_retained.extend_from_slice(page.records());
        match page.next_cursor().cloned() {
            Some(next) => retained_cursor = Some(next),
            None => break,
        }
    }
    assert_eq!(observed_retained, retained);
    assert!(observed_retained.iter().any(|record| record == &terminal));
}

#[tokio::test]
async fn active_retirement_delete_and_crossed_successor_fail_closed() {
    let root = tempfile::tempdir().expect("fixture root should build");
    let store = EngineWorkloadSagaStore::new(engine(&root));
    let current = retirement("retirement-rejection", 7);
    assert_eq!(
        store
            .compare_and_swap_retirement(TenantRetirementExpected::Missing, current.clone())
            .await,
        Ok(TenantRetirementCommit::Applied)
    );
    assert_eq!(
        store.delete_retirement(current.clone()).await,
        Err(TenantRetirementStoreError::Corrupt)
    );

    let crossed = retirement("retirement-rejection", 8)
        .advance(TenantRetirementPhase::ChildrenRecorded)
        .unwrap();
    assert_eq!(
        store
            .compare_and_swap_retirement(
                TenantRetirementExpected::Revision(current.revision()),
                crossed,
            )
            .await,
        Err(TenantRetirementStoreError::Corrupt)
    );
    assert_eq!(
        store.load_retirement(current.tenant_id()).await,
        Ok(Some(current))
    );
}

#[tokio::test]
async fn retirement_commit_inspects_exact_truth_after_ambiguous_engine_outcome() {
    let root = tempfile::tempdir().expect("fixture root should build");
    let engine = engine(&root);
    let store = EngineWorkloadSagaStore::new(engine.clone());
    let current = retirement("retirement-ambiguity", 4);
    assert_eq!(
        store
            .compare_and_swap_retirement(TenantRetirementExpected::Missing, current.clone())
            .await,
        Ok(TenantRetirementCommit::Applied)
    );
    let next = current
        .advance(TenantRetirementPhase::ChildrenRecorded)
        .unwrap();
    engine.commit_fault_handle_for_testing().inject(
        commit_fault_labels::DURABLE_BEFORE_PUBLISH,
        Fault::Error(Error::Internal(
            "injected durable tenant-retirement outcome".to_owned(),
        )),
    );

    assert_eq!(
        store
            .compare_and_swap_retirement(
                TenantRetirementExpected::Revision(current.revision()),
                next.clone(),
            )
            .await,
        Ok(TenantRetirementCommit::Applied),
        "exact readback must resolve a durable ambiguous outcome"
    );
    assert_eq!(
        store.load_retirement(next.tenant_id()).await,
        Ok(Some(next))
    );
}
