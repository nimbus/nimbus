use std::sync::{Arc, Mutex};

use nimbus_core::{TenantId, WorkloadId};
use nimbus_network::{NetworkPlanDigest, NetworkPlanId, NetworkResourceGeneration};
use nimbus_workloads::{
    DesiredWorkloadKind, DesiredWorkloadState, NodeIdentity, WorkloadActivationIntent,
    WorkloadAdmissionEvidence, WorkloadDesiredDigest, WorkloadEffectReferences,
    WorkloadNetworkIntent, WorkloadOwnerEvidenceDigest, WorkloadOwnerObservation,
    WorkloadPhaseDetail, WorkloadPublicationIntent, WorkloadSagaCommit, WorkloadSagaExpected,
    WorkloadSagaFuture, WorkloadSagaKey, WorkloadSagaPage, WorkloadSagaPageRequest,
    WorkloadSagaRecord, WorkloadSagaStore, WorkloadSagaStoreError,
};

use super::WorkloadSagaCoordinator;

#[derive(Debug, Default)]
struct RecordedCalls {
    loads: Vec<WorkloadSagaKey>,
    compare_and_swaps: Vec<(WorkloadSagaExpected, WorkloadSagaRecord)>,
    recovery_reads: Vec<WorkloadSagaPageRequest>,
}

struct RecordingStore {
    load_result: Result<Option<WorkloadSagaRecord>, WorkloadSagaStoreError>,
    compare_and_swap_result: Result<WorkloadSagaCommit, WorkloadSagaStoreError>,
    recovery_result: Result<WorkloadSagaPage, WorkloadSagaStoreError>,
    calls: Mutex<RecordedCalls>,
}

impl RecordingStore {
    fn new(
        load_result: Result<Option<WorkloadSagaRecord>, WorkloadSagaStoreError>,
        compare_and_swap_result: Result<WorkloadSagaCommit, WorkloadSagaStoreError>,
        recovery_result: Result<WorkloadSagaPage, WorkloadSagaStoreError>,
    ) -> Self {
        Self {
            load_result,
            compare_and_swap_result,
            recovery_result,
            calls: Mutex::new(RecordedCalls::default()),
        }
    }

    fn calls(&self) -> std::sync::MutexGuard<'_, RecordedCalls> {
        self.calls.lock().expect("recording store lock is healthy")
    }
}

impl WorkloadSagaStore for RecordingStore {
    fn load<'a>(
        &'a self,
        key: &'a WorkloadSagaKey,
    ) -> WorkloadSagaFuture<'a, Option<WorkloadSagaRecord>> {
        Box::pin(async move {
            self.calls
                .lock()
                .expect("recording store lock is healthy")
                .loads
                .push(key.clone());
            self.load_result.clone()
        })
    }

    fn compare_and_swap<'a>(
        &'a self,
        expected: WorkloadSagaExpected,
        next: WorkloadSagaRecord,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaCommit> {
        Box::pin(async move {
            self.calls
                .lock()
                .expect("recording store lock is healthy")
                .compare_and_swaps
                .push((expected, next));
            self.compare_and_swap_result.clone()
        })
    }

    fn list_recoverable<'a>(
        &'a self,
        request: WorkloadSagaPageRequest,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaPage> {
        Box::pin(async move {
            self.calls
                .lock()
                .expect("recording store lock is healthy")
                .recovery_reads
                .push(request);
            self.recovery_result.clone()
        })
    }
}

fn initial_record(label: &str) -> WorkloadSagaRecord {
    let tenant_id = TenantId::new(format!("tenant-{label}")).expect("fixture tenant is valid");
    let key = WorkloadSagaKey::new(
        tenant_id.clone(),
        WorkloadId::new(format!("workload-{label}")).expect("fixture workload is valid"),
    );
    let intent = nimbus_workloads::WorkloadSagaIntent::new(
        DesiredWorkloadKind::Sandbox,
        DesiredWorkloadState::Running,
        nimbus_workloads::WorkloadGeneration::new(1),
        WorkloadDesiredDigest::sha256(format!("desired-{label}")),
        WorkloadNetworkIntent::new(
            NetworkPlanId::for_tenant_workload_plan(&tenant_id, label),
            NetworkResourceGeneration::new(1),
            NetworkPlanDigest::from_bytes([0x31; 32]),
        ),
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::Withheld,
        WorkloadAdmissionEvidence::new(
            format!("tid_{}", "1".repeat(64))
                .try_into()
                .expect("fixture decision id is valid"),
            format!("twu_{}", "2".repeat(64))
                .try_into()
                .expect("fixture workload uid is valid"),
            Some(NodeIdentity::new(format!("node-{label}")).expect("fixture node is valid")),
        ),
    )
    .expect("fixture intent is valid");
    WorkloadSagaRecord::new(key, intent).expect("initial record is valid")
}

fn valid_successor(current: &WorkloadSagaRecord) -> WorkloadSagaRecord {
    let references = WorkloadEffectReferences::provision(current.active_intent(), None)
        .expect("fixture references are valid");
    let observation = WorkloadOwnerObservation::NetworkReserved {
        reference: references
            .network()
            .expect("provision references contain network authority")
            .clone(),
        evidence: WorkloadOwnerEvidenceDigest::sha256("network-reserved"),
    };
    let detail = WorkloadPhaseDetail::provision(
        nimbus_workloads::WorkloadSagaPhase::NetworkReserved,
        current.active_intent(),
        references,
        vec![observation],
    )
    .expect("fixture phase detail is valid");
    current
        .advance(
            nimbus_workloads::WorkloadSagaPhase::NetworkReserved,
            detail,
            None,
        )
        .expect("fixture successor is valid")
}

fn empty_page() -> WorkloadSagaPage {
    let request = WorkloadSagaPageRequest::new(None, 1).expect("fixture request is valid");
    WorkloadSagaPage::new(&request, Vec::new(), false).expect("empty terminal page is valid")
}

fn store_with_cas(
    result: Result<WorkloadSagaCommit, WorkloadSagaStoreError>,
) -> Arc<RecordingStore> {
    Arc::new(RecordingStore::new(Ok(None), result, Ok(empty_page())))
}

fn ambiguous_store(
    load_result: Result<Option<WorkloadSagaRecord>, WorkloadSagaStoreError>,
) -> Arc<RecordingStore> {
    Arc::new(RecordingStore::new(
        load_result,
        Err(WorkloadSagaStoreError::Ambiguous),
        Ok(empty_page()),
    ))
}

fn valid_competing_successor(current: &WorkloadSagaRecord) -> WorkloadSagaRecord {
    let references = current.phase_detail().references();
    let detail = WorkloadPhaseDetail::teardown(
        nimbus_workloads::WorkloadSagaPhase::WithdrawalCommitted,
        current.active_intent(),
        current.phase(),
        references,
        Vec::new(),
    )
    .expect("fixture teardown detail is valid");
    current
        .advance(
            nimbus_workloads::WorkloadSagaPhase::WithdrawalCommitted,
            detail,
            None,
        )
        .expect("fixture competing successor is valid")
}

#[test]
fn coordinator_requires_a_dyn_store() {
    let constructor: fn(Arc<dyn WorkloadSagaStore>) -> WorkloadSagaCoordinator =
        WorkloadSagaCoordinator::new;
    let store: Arc<dyn WorkloadSagaStore> = store_with_cas(Ok(WorkloadSagaCommit::Applied));

    let _coordinator = constructor(store);
}

#[tokio::test]
async fn valid_creation_issues_one_missing_cas() {
    let initial = initial_record("missing-cas");
    let store = store_with_cas(Ok(WorkloadSagaCommit::Applied));
    let coordinator = WorkloadSagaCoordinator::new(store.clone());

    let result = coordinator.commit_loaded(None, initial.clone()).await;

    assert_eq!(result, Ok(WorkloadSagaCommit::Applied));
    assert_eq!(
        store.calls().compare_and_swaps,
        vec![(WorkloadSagaExpected::Missing, initial)]
    );
}

#[tokio::test]
async fn valid_transition_issues_one_revision_cas() {
    let current = initial_record("valid-cas");
    let next = valid_successor(&current);
    let store = store_with_cas(Ok(WorkloadSagaCommit::Applied));
    let coordinator = WorkloadSagaCoordinator::new(store.clone());

    let result = coordinator
        .commit_loaded(Some(&current), next.clone())
        .await;

    assert_eq!(result, Ok(WorkloadSagaCommit::Applied));
    let calls = store.calls();
    assert_eq!(
        calls.compare_and_swaps,
        vec![(WorkloadSagaExpected::Revision(current.revision()), next)]
    );
    assert!(calls.loads.is_empty());
    assert!(calls.recovery_reads.is_empty());
}

#[tokio::test]
async fn idempotent_unchanged_result_is_preserved() {
    let current = initial_record("unchanged");
    let next = valid_successor(&current);
    let store = store_with_cas(Ok(WorkloadSagaCommit::Unchanged));
    let coordinator = WorkloadSagaCoordinator::new(store.clone());

    let result = coordinator
        .commit_loaded(Some(&current), next.clone())
        .await;

    assert_eq!(result, Ok(WorkloadSagaCommit::Unchanged));
    assert_eq!(
        store.calls().compare_and_swaps,
        vec![(WorkloadSagaExpected::Revision(current.revision()), next)]
    );
}

#[tokio::test]
async fn invalid_successor_is_rejected_before_cas() {
    let current = initial_record("loaded");
    let crossed = initial_record("crossed");
    let store = store_with_cas(Ok(WorkloadSagaCommit::Applied));
    let coordinator = WorkloadSagaCoordinator::new(store.clone());

    let result = coordinator.commit_loaded(Some(&current), crossed).await;

    assert!(matches!(
        result,
        Err(WorkloadSagaStoreError::InvalidTransition(_))
    ));
    assert!(store.calls().compare_and_swaps.is_empty());
}

#[tokio::test]
async fn conflict_is_preserved_after_one_cas_without_retry() {
    let current = initial_record("conflict");
    let next = valid_successor(&current);
    let conflict = WorkloadSagaStoreError::Conflict {
        expected: WorkloadSagaExpected::Revision(current.revision()),
        observed: Some(next.revision()),
    };
    let store = store_with_cas(Err(conflict.clone()));
    let coordinator = WorkloadSagaCoordinator::new(store.clone());

    let result = coordinator.commit_loaded(Some(&current), next).await;

    assert_eq!(result, Err(conflict));
    assert_eq!(store.calls().compare_and_swaps.len(), 1);
}

#[tokio::test]
async fn ambiguous_commit_confirmed_by_exact_next_is_applied_without_retry() {
    let current = initial_record("ambiguous-next");
    let next = valid_successor(&current);
    let store = ambiguous_store(Ok(Some(next.clone())));
    let coordinator = WorkloadSagaCoordinator::new(store.clone());

    let result = coordinator
        .commit_loaded(Some(&current), next.clone())
        .await;

    assert_eq!(result, Ok(WorkloadSagaCommit::Applied));
    let calls = store.calls();
    assert_eq!(calls.loads, vec![next.key().clone()]);
    assert_eq!(calls.compare_and_swaps.len(), 1);
}

#[tokio::test]
async fn ambiguous_commit_with_exact_old_record_remains_ambiguous_without_retry() {
    let current = initial_record("ambiguous-old");
    let next = valid_successor(&current);
    let store = ambiguous_store(Ok(Some(current.clone())));
    let coordinator = WorkloadSagaCoordinator::new(store.clone());

    let result = coordinator
        .commit_loaded(Some(&current), next.clone())
        .await;

    assert_eq!(result, Err(WorkloadSagaStoreError::Ambiguous));
    let calls = store.calls();
    assert_eq!(calls.loads, vec![next.key().clone()]);
    assert_eq!(calls.compare_and_swaps.len(), 1);
}

#[tokio::test]
async fn ambiguous_commit_with_missing_record_remains_ambiguous_without_retry() {
    let current = initial_record("ambiguous-missing");
    let next = valid_successor(&current);
    let store = ambiguous_store(Ok(None));
    let coordinator = WorkloadSagaCoordinator::new(store.clone());

    let result = coordinator
        .commit_loaded(Some(&current), next.clone())
        .await;

    assert_eq!(result, Err(WorkloadSagaStoreError::Ambiguous));
    let calls = store.calls();
    assert_eq!(calls.loads, vec![next.key().clone()]);
    assert_eq!(calls.compare_and_swaps.len(), 1);
}

#[tokio::test]
async fn ambiguous_commit_with_competing_record_becomes_typed_conflict_without_retry() {
    let current = initial_record("ambiguous-competing");
    let next = valid_successor(&current);
    let competing = valid_competing_successor(&current);
    let store = ambiguous_store(Ok(Some(competing.clone())));
    let coordinator = WorkloadSagaCoordinator::new(store.clone());

    let result = coordinator
        .commit_loaded(Some(&current), next.clone())
        .await;

    assert_eq!(
        result,
        Err(WorkloadSagaStoreError::Conflict {
            expected: WorkloadSagaExpected::Revision(current.revision()),
            observed: Some(competing.revision()),
        })
    );
    let calls = store.calls();
    assert_eq!(calls.loads, vec![next.key().clone()]);
    assert_eq!(calls.compare_and_swaps.len(), 1);
}

#[tokio::test]
async fn ambiguous_commit_fails_closed_when_fresh_truth_cannot_be_loaded() {
    let current = initial_record("ambiguous-load-error");
    let next = valid_successor(&current);

    for expected in [
        WorkloadSagaStoreError::Corrupt,
        WorkloadSagaStoreError::Unavailable,
    ] {
        let store = ambiguous_store(Err(expected.clone()));
        let coordinator = WorkloadSagaCoordinator::new(store.clone());

        let result = coordinator
            .commit_loaded(Some(&current), next.clone())
            .await;

        assert_eq!(result, Err(expected));
        let calls = store.calls();
        assert_eq!(calls.loads, vec![next.key().clone()]);
        assert_eq!(calls.compare_and_swaps.len(), 1);
    }
}

#[tokio::test]
async fn bounded_recovery_delegates_exactly_one_page_read() {
    let record = initial_record("recovery-page");
    let request = WorkloadSagaPageRequest::new(None, 7).expect("fixture request is valid");
    let page = WorkloadSagaPage::new(&request, vec![record], false)
        .expect("fixture recovery page is valid");
    let store = Arc::new(RecordingStore::new(
        Ok(None),
        Ok(WorkloadSagaCommit::Applied),
        Ok(page.clone()),
    ));
    let coordinator = WorkloadSagaCoordinator::new(store.clone());

    let result = coordinator.list_recoverable(request.clone()).await;

    assert_eq!(result, Ok(page));
    assert_eq!(store.calls().recovery_reads, vec![request]);
}

#[tokio::test]
async fn load_preserves_corrupt_and_unavailable_errors() {
    let key = initial_record("load-errors").key().clone();
    for expected in [
        WorkloadSagaStoreError::Corrupt,
        WorkloadSagaStoreError::Unavailable,
    ] {
        let store = Arc::new(RecordingStore::new(
            Err(expected.clone()),
            Ok(WorkloadSagaCommit::Applied),
            Ok(empty_page()),
        ));
        let coordinator = WorkloadSagaCoordinator::new(store.clone());

        assert_eq!(coordinator.load(&key).await, Err(expected));
        assert_eq!(store.calls().loads, vec![key.clone()]);
    }
}

#[tokio::test]
async fn recovery_preserves_corrupt_and_unavailable_errors() {
    for expected in [
        WorkloadSagaStoreError::Corrupt,
        WorkloadSagaStoreError::Unavailable,
    ] {
        let request = WorkloadSagaPageRequest::new(None, 13).expect("fixture request is valid");
        let store = Arc::new(RecordingStore::new(
            Ok(None),
            Ok(WorkloadSagaCommit::Applied),
            Err(expected.clone()),
        ));
        let coordinator = WorkloadSagaCoordinator::new(store.clone());

        assert_eq!(
            coordinator.list_recoverable(request.clone()).await,
            Err(expected)
        );
        assert_eq!(store.calls().recovery_reads, vec![request]);
    }
}
