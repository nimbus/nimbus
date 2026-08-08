use std::sync::{Arc, Mutex};

use nimbus_workloads::{
    WorkloadRestartCandidatePage, WorkloadRestartCandidatePageRequest, WorkloadRestartPolicy,
    WorkloadSagaCommit, WorkloadSagaExpected, WorkloadSagaFuture, WorkloadSagaPage,
    WorkloadSagaPageRequest, WorkloadSagaRecord, WorkloadSagaStore, WorkloadSagaStoreError,
    WorkloadSagaTenantPage, WorkloadSagaTenantPageRequest,
};

use super::*;
use crate::workload_saga::restart_supervisor::{
    RestartCandidateCoordinator, RestartCandidateFuture,
};
use crate::workload_saga::test_support;

#[derive(Default)]
struct StoreState {
    record: Option<WorkloadSagaRecord>,
    loads: usize,
    compare_and_swaps: usize,
}

struct RecordingStore {
    state: Mutex<StoreState>,
}

impl RecordingStore {
    fn new(record: WorkloadSagaRecord) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(StoreState {
                record: Some(record),
                ..StoreState::default()
            }),
        })
    }

    fn calls(&self) -> (usize, usize) {
        let state = self.state.lock().expect("restart store remains healthy");
        (state.loads, state.compare_and_swaps)
    }
}

impl WorkloadSagaStore for RecordingStore {
    fn load<'a>(
        &'a self,
        key: &'a WorkloadSagaKey,
    ) -> WorkloadSagaFuture<'a, Option<WorkloadSagaRecord>> {
        Box::pin(async move {
            let mut state = self.state.lock().expect("restart store remains healthy");
            state.loads += 1;
            let record = state.record.clone();
            if record.as_ref().is_some_and(|record| record.key() != key) {
                return Err(WorkloadSagaStoreError::Corrupt);
            }
            Ok(record)
        })
    }

    fn compare_and_swap<'a>(
        &'a self,
        expected: WorkloadSagaExpected,
        next: WorkloadSagaRecord,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaCommit> {
        Box::pin(async move {
            let mut state = self.state.lock().expect("restart store remains healthy");
            state.compare_and_swaps += 1;
            let observed = state.record.as_ref().map(WorkloadSagaRecord::revision);
            let expected_revision = match expected {
                WorkloadSagaExpected::Missing => None,
                WorkloadSagaExpected::Revision(revision) => Some(revision),
            };
            if observed != expected_revision {
                return Err(WorkloadSagaStoreError::Conflict { expected, observed });
            }
            if state.record.as_ref() == Some(&next) {
                return Ok(WorkloadSagaCommit::Unchanged);
            }
            state.record = Some(next);
            Ok(WorkloadSagaCommit::Applied)
        })
    }

    fn list_recoverable<'a>(
        &'a self,
        request: WorkloadSagaPageRequest,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaPage> {
        Box::pin(async move { WorkloadSagaPage::new(&request, Vec::new(), false) })
    }

    fn list_restart_candidates<'a>(
        &'a self,
        request: WorkloadRestartCandidatePageRequest,
    ) -> WorkloadSagaFuture<'a, WorkloadRestartCandidatePage> {
        Box::pin(async move { WorkloadRestartCandidatePage::new(&request, Vec::new(), false) })
    }

    fn list_for_tenant<'a>(
        &'a self,
        tenant_id: &'a nimbus_core::TenantId,
        request: WorkloadSagaTenantPageRequest,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaTenantPage> {
        Box::pin(async move { WorkloadSagaTenantPage::new(tenant_id, &request, Vec::new(), false) })
    }
}

struct PendingCandidateCoordinator;

impl RestartCandidateCoordinator for PendingCandidateCoordinator {
    fn coordinate(&self, _record: WorkloadSagaRecord) -> RestartCandidateFuture<'_> {
        Box::pin(std::future::pending())
    }
}

fn fixture(
    label: &str,
) -> (
    Arc<RecordingStore>,
    ExplicitWorkloadRestartSubmitter,
    ExplicitWorkloadRestartRequest,
) {
    let record = test_support::restart_observed_record(label, WorkloadRestartPolicy::Never);
    let request = ExplicitWorkloadRestartRequest::new(
        record.key().clone(),
        record.active_intent().source().source_identity().clone(),
        record.active_intent().source().source_generation(),
        "stable-explicit-request",
    );
    let store = RecordingStore::new(record);
    let coordinator = Arc::new(WorkloadSagaCoordinator::new(store.clone()));
    let supervisor = Arc::new(RetainedRestartSupervisor::new(Arc::new(
        PendingCandidateCoordinator,
    )));
    (
        store,
        ExplicitWorkloadRestartSubmitter::new(coordinator, supervisor),
        request,
    )
}

#[tokio::test]
async fn duplicate_explicit_request_returns_same_restart_epoch() {
    let (store, submitter, request) = fixture("explicit-replay");
    let cancellation = WorkloadRestartCancellationToken::new();

    let first = submitter
        .submit(&request, &cancellation)
        .await
        .expect("first explicit request should be admitted");
    let replay = submitter
        .submit(&request, &cancellation)
        .await
        .expect("exact explicit replay should join durable work");

    assert_eq!(first.restart_epoch(), replay.restart_epoch());
    assert_eq!(
        first.disposition(),
        ExplicitWorkloadRestartDisposition::Applied
    );
    assert_eq!(
        replay.disposition(),
        ExplicitWorkloadRestartDisposition::Replayed
    );
    assert_eq!(store.calls(), (4, 1));
}

#[tokio::test]
async fn cancellation_before_submission_has_zero_store_or_provider_calls() {
    let (store, submitter, request) = fixture("explicit-cancelled");
    let cancellation = WorkloadRestartCancellationToken::new();
    cancellation.cancel();

    assert!(matches!(
        submitter.submit(&request, &cancellation).await,
        Err(ExplicitWorkloadRestartError::Cancelled)
    ));
    assert_eq!(store.calls(), (0, 0));
}

#[tokio::test]
async fn crossed_source_generation_fails_before_admission_cas() {
    let (store, submitter, request) = fixture("explicit-crossed-generation");
    let crossed = ExplicitWorkloadRestartRequest::new(
        request.key.clone(),
        request.source_identity.clone(),
        WorkloadProvisionSourceGeneration::new(request.source_generation.as_u64() + 1),
        request.idempotency_key.clone(),
    );

    assert!(matches!(
        submitter
            .submit(&crossed, &WorkloadRestartCancellationToken::new())
            .await,
        Err(ExplicitWorkloadRestartError::SourceGenerationMismatch)
    ));
    assert_eq!(store.calls(), (1, 0));
}
