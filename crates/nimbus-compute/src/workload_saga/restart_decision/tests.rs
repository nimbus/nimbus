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
use crate::workload_saga::{WorkloadSagaCoordinator, test_support};

#[derive(Default)]
struct RestartStoreState {
    record: Option<WorkloadSagaRecord>,
    loads: usize,
    compare_and_swaps: usize,
}

struct RestartStore {
    state: Mutex<RestartStoreState>,
}

impl RestartStore {
    fn new(record: WorkloadSagaRecord) -> Self {
        Self {
            state: Mutex::new(RestartStoreState {
                record: Some(record),
                ..RestartStoreState::default()
            }),
        }
    }

    fn calls(&self) -> (usize, usize) {
        let state = self.state.lock().expect("restart store lock is healthy");
        (state.loads, state.compare_and_swaps)
    }

    fn record(&self) -> WorkloadSagaRecord {
        self.state
            .lock()
            .expect("restart store lock is healthy")
            .record
            .clone()
            .expect("restart store retains one record")
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
async fn concurrent_triggers_admit_one_restart_epoch() {
    let record = test_support::restart_observed_record(
        "concurrent",
        WorkloadRestartPolicy::Always { max_restarts: 2 },
    );
    let request = automatic_request(&record, 0x32);
    let store = Arc::new(RestartStore::new(record));
    let coordinator = Arc::new(WorkloadSagaCoordinator::new(store.clone()));
    let cancellation = WorkloadRestartCancellationToken::new();

    let (first, second) = tokio::join!(
        coordinator.compare_and_swap_restart_admission(&request, &cancellation),
        coordinator.compare_and_swap_restart_admission(&request, &cancellation),
    );

    let accepted = [first, second]
        .into_iter()
        .filter(|result| result.is_ok())
        .count();
    assert_eq!(
        accepted, 2,
        "the loser must adopt the exact idempotent request"
    );
    assert_eq!(
        store.record().restart_state().phase(),
        nimbus_workloads::WorkloadRestartPhase::Requested
    );
    assert_eq!(store.calls().1, 1, "only one writer needs a CAS");
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
