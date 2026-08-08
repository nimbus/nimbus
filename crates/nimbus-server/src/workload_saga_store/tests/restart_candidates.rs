use nimbus_workloads::{
    WorkloadRestartCandidateCursor, WorkloadRestartCandidatePageRequest, WorkloadRestartPolicy,
    WorkloadSagaExpected, WorkloadSagaRecord, WorkloadSagaStore,
};

use super::super::EngineWorkloadSagaStore;
use super::engine;
use super::restart::{admit, explicit_input, observed_history, persist_history};

async fn persist_observed(
    store: &EngineWorkloadSagaStore,
    label: &str,
    policy: WorkloadRestartPolicy,
) -> WorkloadSagaRecord {
    let history = observed_history(label, policy);
    persist_history(store, &history).await;
    history.last().unwrap().clone()
}

async fn all_candidates(store: &EngineWorkloadSagaStore) -> Vec<WorkloadSagaRecord> {
    let mut cursor = None;
    let mut records = Vec::new();
    loop {
        let request = WorkloadRestartCandidatePageRequest::new(cursor, 2).unwrap();
        let page = store
            .list_restart_candidates(request)
            .await
            .expect("candidate page should load");
        records.extend_from_slice(page.records());
        let Some(next) = page.next_cursor().cloned() else {
            break;
        };
        cursor = Some(next);
    }
    records
}

#[tokio::test]
async fn global_query_includes_inactive_policy_and_active_never_candidates_only() {
    let root = tempfile::tempdir().expect("fixture root should build");
    let store = EngineWorkloadSagaStore::new(engine(&root));
    let inactive = persist_observed(
        &store,
        "restart-candidate-inactive",
        WorkloadRestartPolicy::Always { max_restarts: 2 },
    )
    .await;
    let never = persist_observed(
        &store,
        "restart-candidate-never",
        WorkloadRestartPolicy::Never,
    )
    .await;
    let active_source = persist_observed(
        &store,
        "restart-candidate-active-never",
        WorkloadRestartPolicy::Never,
    )
    .await;
    let active = admit(
        &active_source,
        explicit_input(&active_source, "active-never", u64::MAX),
    );
    store
        .compare_and_swap(
            WorkloadSagaExpected::Revision(active_source.revision()),
            active.clone(),
        )
        .await
        .expect("active restart should persist");

    let mut expected = vec![inactive, active];
    expected.sort_by(|left, right| left.saga_id().cmp(right.saga_id()));
    let records = all_candidates(&store).await;
    assert_eq!(records, expected);
    assert!(
        !records
            .iter()
            .any(|record| record.saga_id() == never.saga_id())
    );
    assert_ne!(
        records[0].key().tenant_id(),
        records[1].key().tenant_id(),
        "the restart watch must be global across tenant partitions"
    );
}

#[tokio::test]
async fn candidate_pages_are_bounded_complete_stable_and_survive_engine_reopen() {
    let root = tempfile::tempdir().expect("fixture root should build");
    let expected = {
        let store = EngineWorkloadSagaStore::new(engine(&root));
        let mut records = Vec::new();
        for label in [
            "restart-page-a",
            "restart-page-b",
            "restart-page-c",
            "restart-page-d",
        ] {
            records.push(
                persist_observed(
                    &store,
                    label,
                    WorkloadRestartPolicy::OnFailure { max_restarts: 3 },
                )
                .await,
            );
        }
        records.sort_by(|left, right| left.saga_id().cmp(right.saga_id()));
        assert_eq!(all_candidates(&store).await, records);
        records
    };

    let reopened = EngineWorkloadSagaStore::new(engine(&root));
    assert_eq!(all_candidates(&reopened).await, expected);
}

#[tokio::test]
async fn insertion_behind_a_cursor_is_found_by_the_next_complete_sweep() {
    let root = tempfile::tempdir().expect("fixture root should build");
    let store = EngineWorkloadSagaStore::new(engine(&root));
    let mut histories = [
        observed_history(
            "restart-cursor-a",
            WorkloadRestartPolicy::Always { max_restarts: 1 },
        ),
        observed_history(
            "restart-cursor-b",
            WorkloadRestartPolicy::Always { max_restarts: 1 },
        ),
        observed_history(
            "restart-cursor-c",
            WorkloadRestartPolicy::Always { max_restarts: 1 },
        ),
    ];
    histories.sort_by(|left, right| {
        left.last()
            .unwrap()
            .saga_id()
            .cmp(right.last().unwrap().saga_id())
    });
    let behind = histories[0].clone();
    let first = histories[1].clone();
    let last = histories[2].clone();
    persist_history(&store, &first).await;
    persist_history(&store, &last).await;

    let first_page = store
        .list_restart_candidates(WorkloadRestartCandidatePageRequest::new(None, 1).unwrap())
        .await
        .unwrap();
    assert_eq!(first_page.records(), &[first.last().unwrap().clone()]);
    let cursor = first_page.next_cursor().cloned().unwrap();

    persist_history(&store, &behind).await;
    let remainder = store
        .list_restart_candidates(WorkloadRestartCandidatePageRequest::new(Some(cursor), 2).unwrap())
        .await
        .unwrap();
    assert_eq!(remainder.records(), &[last.last().unwrap().clone()]);

    let complete_next_sweep = all_candidates(&store).await;
    assert_eq!(complete_next_sweep.len(), 3);
    assert_eq!(complete_next_sweep[0], behind.last().unwrap().clone());
}

#[test]
fn candidate_cursor_identity_is_stable_across_active_restart_revisions() {
    let observed = super::restart::observed_record(
        "restart-cursor-stable",
        WorkloadRestartPolicy::Always { max_restarts: 2 },
    );
    let active = admit(
        &observed,
        explicit_input(&observed, "cursor-stable", u64::MAX),
    );
    assert_eq!(
        WorkloadRestartCandidateCursor::for_record(&observed).unwrap(),
        WorkloadRestartCandidateCursor::for_record(&active).unwrap(),
    );
}
