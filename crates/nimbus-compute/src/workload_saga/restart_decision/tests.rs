use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use nimbus_workloads::{
    WorkloadInspectionVersion, WorkloadRestartCandidatePage, WorkloadRestartCandidatePageRequest,
    WorkloadRestartNotBeforeUnixMillis, WorkloadRestartPolicy, WorkloadSagaCommit,
    WorkloadSagaExpected, WorkloadSagaFuture, WorkloadSagaKey, WorkloadSagaPage,
    WorkloadSagaPageRequest, WorkloadSagaRecord, WorkloadSagaStore, WorkloadSagaStoreError,
    WorkloadSagaTenantPage, WorkloadSagaTenantPageRequest,
};

use super::{
    WorkloadRestartAdmissionDecision, WorkloadRestartAdmissionError,
    WorkloadRestartAdmissionRequest, WorkloadRestartCancellationToken, WorkloadRestartDecision,
    decide_restart_admission, decide_restart_progress,
};
use crate::workload_saga::{
    WorkloadDesireAdmissionError, WorkloadDesireAdmissionFuture, WorkloadDesireAdmissionGuard,
    WorkloadDesireAdmissionPermit, WorkloadDesireAdmissionRequest, WorkloadSagaCoordinator,
    test_support,
};
use tokio::sync::Barrier;

#[derive(Default)]
struct RestartStoreState {
    record: Option<WorkloadSagaRecord>,
    loads: usize,
    compare_and_swaps: usize,
}

struct RestartStore {
    state: Mutex<RestartStoreState>,
    admission_held: Option<Arc<AtomicBool>>,
}

impl RestartStore {
    fn new(record: WorkloadSagaRecord) -> Self {
        Self {
            state: Mutex::new(RestartStoreState {
                record: Some(record),
                ..RestartStoreState::default()
            }),
            admission_held: None,
        }
    }

    fn guarded(record: WorkloadSagaRecord, admission_held: Arc<AtomicBool>) -> Self {
        Self {
            state: Mutex::new(RestartStoreState {
                record: Some(record),
                ..RestartStoreState::default()
            }),
            admission_held: Some(admission_held),
        }
    }

    fn calls(&self) -> (usize, usize) {
        let state = self.state.lock().expect("restart store lock is healthy");
        (state.loads, state.compare_and_swaps)
    }
}

impl WorkloadSagaStore for RestartStore {
    fn load<'a>(
        &'a self,
        key: &'a WorkloadSagaKey,
    ) -> WorkloadSagaFuture<'a, Option<WorkloadSagaRecord>> {
        Box::pin(async move {
            let mut state = self.state.lock().expect("restart store lock is healthy");
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
            if let Some(held) = &self.admission_held {
                assert!(
                    held.load(Ordering::SeqCst),
                    "restart Engine CAS must execute while the provider permit is held"
                );
            }
            let mut state = self.state.lock().expect("restart store lock is healthy");
            state.compare_and_swaps += 1;
            let observed = state.record.as_ref().map(WorkloadSagaRecord::revision);
            if observed
                != match expected {
                    WorkloadSagaExpected::Missing => None,
                    WorkloadSagaExpected::Revision(revision) => Some(revision),
                }
            {
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

struct HeldRestartAdmissionPermit {
    held: Arc<AtomicBool>,
}

impl Drop for HeldRestartAdmissionPermit {
    fn drop(&mut self) {
        assert!(
            self.held.swap(false, Ordering::SeqCst),
            "restart admission permit must be held until its owner drops it"
        );
    }
}

impl WorkloadDesireAdmissionPermit for HeldRestartAdmissionPermit {}

struct RestartAdmissionGuard {
    held: Arc<AtomicBool>,
    requests: Mutex<Vec<WorkloadDesireAdmissionRequest>>,
    rejection: Option<WorkloadDesireAdmissionError>,
}

impl RestartAdmissionGuard {
    fn allowing(held: Arc<AtomicBool>) -> Arc<Self> {
        Arc::new(Self {
            held,
            requests: Mutex::new(Vec::new()),
            rejection: None,
        })
    }

    fn rejecting(error: WorkloadDesireAdmissionError) -> Arc<Self> {
        Arc::new(Self {
            held: Arc::new(AtomicBool::new(false)),
            requests: Mutex::new(Vec::new()),
            rejection: Some(error),
        })
    }

    fn requests(&self) -> Vec<WorkloadDesireAdmissionRequest> {
        self.requests
            .lock()
            .expect("restart admission request lock is healthy")
            .clone()
    }
}

impl WorkloadDesireAdmissionGuard for RestartAdmissionGuard {
    fn acquire<'a>(
        &'a self,
        request: &'a WorkloadDesireAdmissionRequest,
    ) -> WorkloadDesireAdmissionFuture<'a> {
        Box::pin(async move {
            self.requests
                .lock()
                .expect("restart admission request lock is healthy")
                .push(request.clone());
            if let Some(error) = self.rejection {
                return Err(error);
            }
            assert!(
                !self.held.swap(true, Ordering::SeqCst),
                "test guard cannot issue overlapping restart permits"
            );
            Ok(Box::new(HeldRestartAdmissionPermit {
                held: self.held.clone(),
            }) as Box<dyn WorkloadDesireAdmissionPermit>)
        })
    }
}

struct ContendedRestartStore {
    state: Mutex<RestartStoreState>,
    initial_loads: AtomicUsize,
    same_revision: Barrier,
}

impl ContendedRestartStore {
    fn new(record: WorkloadSagaRecord) -> Self {
        Self {
            state: Mutex::new(RestartStoreState {
                record: Some(record),
                ..RestartStoreState::default()
            }),
            initial_loads: AtomicUsize::new(0),
            same_revision: Barrier::new(2),
        }
    }

    fn calls(&self) -> (usize, usize) {
        let state = self.state.lock().expect("restart store lock is healthy");
        (state.loads, state.compare_and_swaps)
    }
}

impl WorkloadSagaStore for ContendedRestartStore {
    fn load<'a>(
        &'a self,
        key: &'a WorkloadSagaKey,
    ) -> WorkloadSagaFuture<'a, Option<WorkloadSagaRecord>> {
        Box::pin(async move {
            let record = {
                let mut state = self.state.lock().expect("restart store lock is healthy");
                state.loads += 1;
                state.record.clone()
            };
            if record.as_ref().is_some_and(|record| record.key() != key) {
                return Err(WorkloadSagaStoreError::Corrupt);
            }
            if self.initial_loads.fetch_add(1, Ordering::AcqRel) < 2 {
                self.same_revision.wait().await;
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
            let mut state = self.state.lock().expect("restart store lock is healthy");
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

fn automatic_request(record: &WorkloadSagaRecord, byte: u8) -> WorkloadRestartAdmissionRequest {
    WorkloadRestartAdmissionRequest::for_automatic(
        record,
        17,
        WorkloadInspectionVersion::from_bytes([byte; 32]),
        WorkloadRestartNotBeforeUnixMillis::new(500),
    )
}

#[test]
fn automatic_and_explicit_restart_use_same_reducer() {
    let automatic_record = test_support::restart_observed_record(
        "normalized-auto",
        WorkloadRestartPolicy::Always { max_restarts: 2 },
    );
    let explicit_record =
        test_support::restart_observed_record("normalized-explicit", WorkloadRestartPolicy::Never);
    let automatic = automatic_request(&automatic_record, 0x31);
    let explicit = WorkloadRestartAdmissionRequest::for_explicit(
        &explicit_record,
        "explicit-normalized",
        WorkloadRestartNotBeforeUnixMillis::new(0),
    )
    .expect("explicit request should validate");

    let WorkloadRestartAdmissionDecision::Transition(automatic) =
        decide_restart_admission(&automatic_record, &automatic).expect("automatic admission")
    else {
        panic!("automatic request should produce a transition");
    };
    let WorkloadRestartAdmissionDecision::Transition(explicit) =
        decide_restart_admission(&explicit_record, &explicit).expect("explicit admission")
    else {
        panic!("explicit request should produce a transition");
    };

    assert_eq!(
        automatic
            .restart_state()
            .active()
            .unwrap()
            .admission()
            .trigger(),
        nimbus_workloads::WorkloadRestartTrigger::Automatic { exit_code: 17 }
    );
    assert_eq!(
        explicit
            .restart_state()
            .active()
            .unwrap()
            .admission()
            .trigger(),
        nimbus_workloads::WorkloadRestartTrigger::Explicit
    );
}

#[tokio::test]
async fn machine_restart_admission_holds_guard_through_engine_cas() {
    let record = test_support::restart_observed_record(
        "guarded-restart",
        WorkloadRestartPolicy::Always { max_restarts: 2 },
    );
    let request = automatic_request(&record, 0x41);
    let active = record.active_intent();
    let expected_request = WorkloadDesireAdmissionRequest::new(
        record.key().clone(),
        active.source().execution_provider_id().clone(),
        active.generation(),
        active.desired_digest(),
        active.source().source_digest(),
    );
    let held = Arc::new(AtomicBool::new(false));
    let guard = RestartAdmissionGuard::allowing(held.clone());
    let store = Arc::new(RestartStore::guarded(record, held.clone()));
    let coordinator =
        WorkloadSagaCoordinator::with_desire_admission_guard(store.clone(), guard.clone());

    coordinator
        .compare_and_swap_restart_admission(&request, &WorkloadRestartCancellationToken::new())
        .await
        .expect("barrier-free restart admission should commit");

    assert_eq!(guard.requests(), vec![expected_request]);
    assert_eq!(store.calls(), (1, 1));
    assert!(
        !held.load(Ordering::SeqCst),
        "restart permit must release after the exact CAS result"
    );
}

#[tokio::test]
async fn machine_restart_admission_fence_rejects_before_engine_cas() {
    let record = test_support::restart_observed_record(
        "fenced-restart",
        WorkloadRestartPolicy::Always { max_restarts: 2 },
    );
    let request = automatic_request(&record, 0x42);
    let active = record.active_intent();
    let expected_request = WorkloadDesireAdmissionRequest::new(
        record.key().clone(),
        active.source().execution_provider_id().clone(),
        active.generation(),
        active.desired_digest(),
        active.source().source_digest(),
    );
    let guard = RestartAdmissionGuard::rejecting(WorkloadDesireAdmissionError::Fenced);
    let store = Arc::new(RestartStore::new(record));
    let coordinator =
        WorkloadSagaCoordinator::with_desire_admission_guard(store.clone(), guard.clone());

    assert!(matches!(
        coordinator
            .compare_and_swap_restart_admission(&request, &WorkloadRestartCancellationToken::new(),)
            .await,
        Err(WorkloadRestartAdmissionError::Admission(
            WorkloadDesireAdmissionError::Fenced
        ))
    ));
    assert_eq!(guard.requests(), vec![expected_request]);
    assert_eq!(store.calls(), (1, 0));
}

#[tokio::test]
async fn concurrent_triggers_force_same_revision_before_competing_cas() {
    let record = test_support::restart_observed_record(
        "concurrent",
        WorkloadRestartPolicy::Always { max_restarts: 2 },
    );
    let request = automatic_request(&record, 0x32);
    let store = Arc::new(ContendedRestartStore::new(record));
    let coordinator = Arc::new(WorkloadSagaCoordinator::new(store.clone()));
    let cancellation = WorkloadRestartCancellationToken::new();

    let (first, second) = tokio::join!(
        coordinator.compare_and_swap_restart_admission(&request, &cancellation),
        coordinator.compare_and_swap_restart_admission(&request, &cancellation),
    );

    let dispositions = [first.unwrap().disposition(), second.unwrap().disposition()];
    assert!(dispositions.contains(&super::WorkloadRestartAdmissionDisposition::Applied));
    assert!(dispositions.contains(&super::WorkloadRestartAdmissionDisposition::ConfirmedReplay));
    assert_eq!(store.calls(), (3, 2));
}

#[test]
fn crossed_admission_fences_fail_before_cas() {
    let record = test_support::restart_observed_record(
        "fence-a",
        WorkloadRestartPolicy::Always { max_restarts: 2 },
    );
    let crossed = test_support::restart_observed_record(
        "fence-b",
        WorkloadRestartPolicy::Always { max_restarts: 2 },
    );
    let request = automatic_request(&crossed, 0x33);

    assert!(decide_restart_admission(&record, &request).is_err());
}

#[tokio::test]
async fn withdrawal_winning_before_admission_vetoes_cas() {
    let record = test_support::restart_observed_record(
        "withdrawal",
        WorkloadRestartPolicy::Always { max_restarts: 2 },
    );
    let request = automatic_request(&record, 0x34);
    let withdrawn = test_support::withdrawn_record(&record);
    let store = Arc::new(RestartStore::new(withdrawn));
    let coordinator = WorkloadSagaCoordinator::new(store.clone());

    let result = coordinator
        .compare_and_swap_restart_admission(&request, &WorkloadRestartCancellationToken::new())
        .await;

    assert!(matches!(
        result,
        Err(WorkloadRestartAdmissionError::Saga(
            WorkloadSagaStoreError::InvalidTransition(_)
        ))
    ));
    assert_eq!(store.calls().1, 0);
}

#[tokio::test]
async fn successor_winning_before_admission_vetoes_cas() {
    let record = test_support::restart_observed_record(
        "successor",
        WorkloadRestartPolicy::Always { max_restarts: 2 },
    );
    let request = automatic_request(&record, 0x35);
    let successor = test_support::record_with_successor(&record, "successor-next");
    let store = Arc::new(RestartStore::new(successor));
    let coordinator = WorkloadSagaCoordinator::new(store.clone());

    let result = coordinator
        .compare_and_swap_restart_admission(&request, &WorkloadRestartCancellationToken::new())
        .await;

    assert!(matches!(
        result,
        Err(WorkloadRestartAdmissionError::Saga(
            WorkloadSagaStoreError::InvalidTransition(_)
        ))
    ));
    assert_eq!(store.calls().1, 0);
}

#[test]
fn explicit_restart_does_not_increment_automatic_count() {
    let record =
        test_support::restart_observed_record("explicit-count", WorkloadRestartPolicy::Never);
    let request = WorkloadRestartAdmissionRequest::for_explicit(
        &record,
        "explicit-count",
        WorkloadRestartNotBeforeUnixMillis::new(0),
    )
    .expect("explicit request should validate");
    let WorkloadRestartAdmissionDecision::Transition(candidate) =
        decide_restart_admission(&record, &request).expect("explicit restart should admit")
    else {
        panic!("explicit restart should transition");
    };

    assert_eq!(
        candidate
            .restart_state()
            .completed_automatic_restart_count(),
        0
    );
}

#[test]
fn deadline_not_due_returns_wait_without_effect() {
    let record = test_support::scheduled_restart_record("not-due", 500);

    assert_eq!(
        decide_restart_progress(&record, WorkloadRestartNotBeforeUnixMillis::new(499))
            .expect("scheduled restart should reduce"),
        WorkloadRestartDecision::WaitUntil(WorkloadRestartNotBeforeUnixMillis::new(500))
    );
}

#[tokio::test]
async fn cancellation_before_submission_makes_zero_store_and_provider_calls() {
    let record = test_support::restart_observed_record(
        "cancelled",
        WorkloadRestartPolicy::Always { max_restarts: 2 },
    );
    let request = automatic_request(&record, 0x36);
    let store = Arc::new(RestartStore::new(record));
    let coordinator = WorkloadSagaCoordinator::new(store.clone());
    let cancellation = WorkloadRestartCancellationToken::new();
    cancellation.cancel();

    let result = coordinator
        .compare_and_swap_restart_admission(&request, &cancellation)
        .await;

    assert!(matches!(
        result,
        Err(WorkloadRestartAdmissionError::Cancelled)
    ));
    assert_eq!(store.calls(), (0, 0));
}
