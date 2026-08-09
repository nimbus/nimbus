use std::num::{NonZeroU64, NonZeroUsize};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nimbus_workloads::{
    WorkloadRestartAdmissionInput, WorkloadRestartAdmissionUpdate, WorkloadRestartCandidatePage,
    WorkloadRestartCandidatePageRequest, WorkloadRestartEffectResult,
    WorkloadRestartEvidenceDigest, WorkloadRestartPolicy, WorkloadRestartTrigger,
    WorkloadSagaCommit, WorkloadSagaExpected, WorkloadSagaFuture, WorkloadSagaPage,
    WorkloadSagaPageRequest, WorkloadSagaRecord, WorkloadSagaStore, WorkloadSagaStoreError,
    WorkloadSagaTenantPage, WorkloadSagaTenantPageRequest,
};

use super::*;
use crate::workload_saga::restart_supervisor::{
    RestartCandidateCoordinator, RestartCandidateFuture,
};
use crate::workload_saga::restart_watch::{
    DurableRestartWatch, RestartClock, RestartWait, RestartWaitFuture, read_only_exit_hint,
};
use crate::workload_saga::test_support;
use tokio::sync::{Notify, Semaphore};

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

struct ImmediateFailureCoordinator;

impl RestartCandidateCoordinator for ImmediateFailureCoordinator {
    fn coordinate(&self, _record: WorkloadSagaRecord) -> RestartCandidateFuture<'_> {
        Box::pin(async { Err("transient restart coordination failure".to_owned()) })
    }
}

struct CrashCutStore {
    state: Mutex<StoreState>,
    page_calls: AtomicUsize,
    committed: Notify,
    page_completed: Semaphore,
}

impl CrashCutStore {
    fn new(record: WorkloadSagaRecord) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(StoreState {
                record: Some(record),
                ..StoreState::default()
            }),
            page_calls: AtomicUsize::new(0),
            committed: Notify::new(),
            page_completed: Semaphore::new(0),
        })
    }

    async fn wait_until_commit_is_durable(&self) {
        self.committed.notified().await;
    }

    fn record(&self) -> WorkloadSagaRecord {
        self.state
            .lock()
            .expect("crash-cut store remains healthy")
            .record
            .clone()
            .expect("crash-cut store retains the workload saga")
    }

    async fn wait_for_page_calls(&self, expected: usize) {
        self.page_completed
            .acquire_many(u32::try_from(expected).expect("page expectation fits u32"))
            .await
            .expect("crash-cut page completion signal remains open")
            .forget();
    }
}

impl WorkloadSagaStore for CrashCutStore {
    fn load<'a>(
        &'a self,
        key: &'a WorkloadSagaKey,
    ) -> WorkloadSagaFuture<'a, Option<WorkloadSagaRecord>> {
        Box::pin(async move {
            let mut state = self.state.lock().expect("crash-cut store remains healthy");
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
            {
                let mut state = self.state.lock().expect("crash-cut store remains healthy");
                state.compare_and_swaps += 1;
                let observed = state.record.as_ref().map(WorkloadSagaRecord::revision);
                let expected_revision = match expected {
                    WorkloadSagaExpected::Missing => None,
                    WorkloadSagaExpected::Revision(revision) => Some(revision),
                };
                if observed != expected_revision {
                    return Err(WorkloadSagaStoreError::Conflict { expected, observed });
                }
                state.record = Some(next);
            }

            // The durable write is visible before its acknowledgment. Dropping
            // this future models a caller process dying in that exact window.
            self.committed.notify_one();
            std::future::pending::<()>().await;
            unreachable!("the crash-cut write acknowledgment stays unavailable")
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
        Box::pin(async move {
            self.page_calls.fetch_add(1, Ordering::AcqRel);
            let page = WorkloadRestartCandidatePage::new(&request, vec![self.record()], false);
            self.page_completed.add_permits(1);
            page
        })
    }

    fn list_for_tenant<'a>(
        &'a self,
        tenant_id: &'a nimbus_core::TenantId,
        request: WorkloadSagaTenantPageRequest,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaTenantPage> {
        Box::pin(async move { WorkloadSagaTenantPage::new(tenant_id, &request, Vec::new(), false) })
    }
}

#[derive(Default)]
struct BlockingRecoveryCoordinator {
    calls: AtomicUsize,
    started: Notify,
    release: Notify,
}

impl BlockingRecoveryCoordinator {
    async fn wait_until_started(&self) {
        self.started.notified().await;
    }
}

impl RestartCandidateCoordinator for BlockingRecoveryCoordinator {
    fn coordinate(&self, _record: WorkloadSagaRecord) -> RestartCandidateFuture<'_> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::AcqRel);
            self.started.notify_one();
            self.release.notified().await;
            Ok(())
        })
    }
}

struct CancellationClock;

impl RestartClock for CancellationClock {
    fn now_unix_millis(&self) -> WorkloadRestartNotBeforeUnixMillis {
        WorkloadRestartNotBeforeUnixMillis::new(0)
    }

    fn wait_until(
        &self,
        _deadline: WorkloadRestartNotBeforeUnixMillis,
        cancellation: &WorkloadRestartCancellationToken,
    ) -> RestartWaitFuture<'_> {
        let mut cancellation = cancellation.subscribe();
        Box::pin(async move {
            loop {
                if *cancellation.borrow() {
                    return RestartWait::Cancelled;
                }
                if cancellation.changed().await.is_err() {
                    return RestartWait::Cancelled;
                }
            }
        })
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

fn complete_explicit_restart(
    record: &WorkloadSagaRecord,
    idempotency_key: &str,
) -> WorkloadSagaRecord {
    let request_id = WorkloadRestartRequestId::for_explicit(
        record.saga_id(),
        record.active_intent().source().source_generation(),
        idempotency_key,
    )
    .expect("explicit request ID");
    let WorkloadRestartAdmissionUpdate::Transition(admitted) = record
        .admit_restart(WorkloadRestartAdmissionInput {
            expected_revision: record.revision(),
            trigger: WorkloadRestartTrigger::Explicit,
            inspection_version: None,
            request_id: request_id.clone(),
            not_before_unix_millis: WorkloadRestartNotBeforeUnixMillis::new(0),
        })
        .expect("explicit restart admission")
    else {
        panic!("new explicit request must transition");
    };
    let mut current = admitted
        .advance_restart_without_effect(&request_id)
        .expect("requested restart advances");
    for label in [
        "quiesced",
        "prepared",
        "attached",
        "prerequisites",
        "activated",
        "ready",
    ] {
        if current.restart_state().active().is_some_and(|active| {
            active.phase() == nimbus_workloads::WorkloadRestartPhase::Scheduled
        }) {
            current = current
                .advance_scheduled_restart(&request_id, WorkloadRestartNotBeforeUnixMillis::new(0))
                .expect("scheduled restart becomes due");
        }
        let claimed = current
            .claim_restart_command(&request_id)
            .expect("restart command claim");
        let claim = claimed
            .restart_state()
            .active()
            .and_then(|active| active.disposition().claim())
            .expect("durable restart claim")
            .clone();
        current = claimed
            .apply_restart_effect_result(
                &claim,
                WorkloadRestartEffectResult::Succeeded {
                    evidence: WorkloadRestartEvidenceDigest::sha256(label),
                },
            )
            .expect("restart command success");
    }
    current = current
        .advance_restart_without_effect(&request_id)
        .expect("withheld publication advances");
    current
        .advance_restart_without_effect(&request_id)
        .expect("withheld observation completes")
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
async fn explicit_replay_reports_a_retained_candidate_failure() {
    let record = test_support::restart_observed_record(
        "explicit-retained-failure",
        WorkloadRestartPolicy::Never,
    );
    let request = ExplicitWorkloadRestartRequest::new(
        record.key().clone(),
        record.active_intent().source().source_identity().clone(),
        record.active_intent().source().source_generation(),
        "retained-failure",
    );
    let store = RecordingStore::new(record);
    let supervisor = Arc::new(RetainedRestartSupervisor::new(Arc::new(
        ImmediateFailureCoordinator,
    )));
    let submitter = ExplicitWorkloadRestartSubmitter::new(
        Arc::new(WorkloadSagaCoordinator::new(store)),
        supervisor.clone(),
    );

    submitter
        .submit(&request, &WorkloadRestartCancellationToken::new())
        .await
        .expect("first explicit submission owns the retained task");
    supervisor
        .wait_until_quiescent()
        .await
        .expect("failed retained task becomes observable");

    let error = submitter
        .submit(&request, &WorkloadRestartCancellationToken::new())
        .await
        .expect_err("explicit replay must report the retained candidate failure");
    assert!(matches!(
        error,
        ExplicitWorkloadRestartError::Supervision(message)
            if message.contains("transient restart coordination failure")
    ));
}

#[tokio::test]
async fn nonadjacent_explicit_replay_makes_zero_new_cas_or_provider_calls() {
    let initial = test_support::restart_observed_record(
        "explicit-history-replay",
        WorkloadRestartPolicy::Never,
    );
    let first = complete_explicit_restart(&initial, "first-completed");
    let completed = complete_explicit_restart(&first, "second-completed");
    let request = ExplicitWorkloadRestartRequest::new(
        completed.key().clone(),
        completed.active_intent().source().source_identity().clone(),
        completed.active_intent().source().source_generation(),
        "first-completed",
    );
    let store = RecordingStore::new(completed);
    let submitter = ExplicitWorkloadRestartSubmitter::new(
        Arc::new(WorkloadSagaCoordinator::new(store.clone())),
        Arc::new(RetainedRestartSupervisor::new(Arc::new(
            PendingCandidateCoordinator,
        ))),
    );

    let replay = submitter
        .submit(&request, &WorkloadRestartCancellationToken::new())
        .await
        .expect("nonadjacent completed request must replay");

    assert_eq!(replay.restart_epoch(), WorkloadRestartEpoch::new(1));
    assert_eq!(
        replay.disposition(),
        ExplicitWorkloadRestartDisposition::Replayed
    );
    assert_eq!(store.calls(), (2, 0));
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

#[tokio::test]
async fn cancellation_after_submission_preserves_durable_work() {
    let record =
        test_support::restart_observed_record("explicit-crash-cut", WorkloadRestartPolicy::Never);
    let request = ExplicitWorkloadRestartRequest::new(
        record.key().clone(),
        record.active_intent().source().source_identity().clone(),
        record.active_intent().source().source_generation(),
        "durable-before-ack",
    );
    let store = CrashCutStore::new(record);
    let submitter = ExplicitWorkloadRestartSubmitter::new(
        Arc::new(WorkloadSagaCoordinator::new(store.clone())),
        Arc::new(RetainedRestartSupervisor::new(Arc::new(
            PendingCandidateCoordinator,
        ))),
    );
    let submit_task = tokio::spawn(async move {
        submitter
            .submit(&request, &WorkloadRestartCancellationToken::new())
            .await
    });

    tokio::time::timeout(Duration::from_secs(5), store.wait_until_commit_is_durable())
        .await
        .expect("durable crash-cut commit should become visible");
    assert!(store.record().restart_state().active().is_some());
    submit_task.abort();
    assert!(
        submit_task
            .await
            .expect_err("submitter should be dropped")
            .is_cancelled()
    );

    let recovery = Arc::new(BlockingRecoveryCoordinator::default());
    let supervisor = Arc::new(RetainedRestartSupervisor::new(recovery.clone()));
    let watch_cancellation = WorkloadRestartCancellationToken::new();
    let watch = Arc::new(
        DurableRestartWatch::new(
            NonZeroUsize::new(8).unwrap(),
            NonZeroU64::new(1_000).unwrap(),
            Arc::new(CancellationClock),
            watch_cancellation.clone(),
            Arc::new(WorkloadSagaCoordinator::new(store.clone())),
            supervisor.clone(),
        )
        .expect("fresh durable watch should validate"),
    );
    let watch_task = tokio::spawn({
        let watch = watch.clone();
        async move { watch.bounded_restart_watch().await }
    });

    tokio::time::timeout(Duration::from_secs(5), recovery.wait_until_started())
        .await
        .expect("fresh durable watch should recover the submitted epoch");
    assert_eq!(recovery.calls.load(Ordering::Acquire), 1);
    watch.hint_handle().notify(read_only_exit_hint());
    store.wait_for_page_calls(2).await;
    assert_eq!(
        recovery.calls.load(Ordering::Acquire),
        1,
        "a duplicate durable sweep must join the retained exact epoch"
    );
    assert!(store.record().restart_state().active().is_some());

    watch_cancellation.cancel();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), watch_task)
            .await
            .expect("watch cancellation should be bounded")
            .unwrap()
            .unwrap(),
        RestartWait::Cancelled
    );
    recovery.release.notify_one();
    tokio::time::timeout(Duration::from_secs(5), supervisor.wait_until_quiescent())
        .await
        .expect("retained recovery should quiesce")
        .unwrap();
}
