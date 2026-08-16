use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use nimbus_workloads::{
    WorkloadActivationIntent, WorkloadProvisionDisposition, WorkloadProvisionEffectResult,
    WorkloadProvisionStep, WorkloadPublicationIntent, WorkloadSagaIntentUpdate, WorkloadSagaPhase,
    WorkloadSagaRecord, WorkloadSagaStore,
};

use super::super::*;
use super::RecoveryStore;
use crate::workload_saga::WorkloadProvisionDecision;

type EventLog = Arc<Mutex<Vec<&'static str>>>;

struct ScriptedProvisionOwner {
    outcomes: Mutex<VecDeque<Result<StartupProvisionResult, String>>>,
    events: EventLog,
}

impl ScriptedProvisionOwner {
    fn new(
        events: EventLog,
        outcomes: impl IntoIterator<Item = Result<StartupProvisionResult, String>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            outcomes: Mutex::new(outcomes.into_iter().collect()),
            events,
        })
    }
}

impl WorkloadStartupProvisionOwner for ScriptedProvisionOwner {
    fn resume(
        &self,
        _key: WorkloadSagaKey,
        owner_reopened_publication: bool,
    ) -> WorkloadStartupOwnerFuture<'_, StartupProvisionResult> {
        Box::pin(async move {
            self.events
                .lock()
                .expect("route event log remains healthy")
                .push(if owner_reopened_publication {
                    "publication-reconcile"
                } else {
                    "provision"
                });
            self.outcomes
                .lock()
                .expect("provision script remains healthy")
                .pop_front()
                .expect("provision route supplies one outcome")
        })
    }
}

struct ScriptedRestartOwner {
    outcomes: Mutex<VecDeque<Result<(), String>>>,
    events: EventLog,
}

impl ScriptedRestartOwner {
    fn new(events: EventLog, outcomes: impl IntoIterator<Item = Result<(), String>>) -> Arc<Self> {
        Arc::new(Self {
            outcomes: Mutex::new(outcomes.into_iter().collect()),
            events,
        })
    }
}

impl WorkloadStartupRestartOwner for ScriptedRestartOwner {
    fn activate_watch(&self) -> Result<(), String> {
        self.events
            .lock()
            .expect("route event log remains healthy")
            .push("watch");
        Ok(())
    }

    fn recover(&self, _record: WorkloadSagaRecord) -> WorkloadStartupOwnerFuture<'_, ()> {
        Box::pin(async move {
            self.events
                .lock()
                .expect("route event log remains healthy")
                .push("restart");
            self.outcomes
                .lock()
                .expect("restart script remains healthy")
                .pop_front()
                .expect("restart route supplies one outcome")
        })
    }
}

struct ScriptedTeardownOwner {
    outcomes: Mutex<VecDeque<Result<StartupTeardownResult, String>>>,
    events: EventLog,
}

impl ScriptedTeardownOwner {
    fn new(
        events: EventLog,
        outcomes: impl IntoIterator<Item = Result<StartupTeardownResult, String>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            outcomes: Mutex::new(outcomes.into_iter().collect()),
            events,
        })
    }
}

impl WorkloadStartupTeardownOwner for ScriptedTeardownOwner {
    fn submit(
        &self,
        _key: WorkloadSagaKey,
    ) -> WorkloadStartupOwnerFuture<'_, StartupTeardownResult> {
        Box::pin(async move {
            self.events
                .lock()
                .expect("route event log remains healthy")
                .push("teardown");
            self.outcomes
                .lock()
                .expect("teardown script remains healthy")
                .pop_front()
                .expect("teardown route supplies one outcome")
        })
    }
}

fn recovery_with_owners(
    record: WorkloadSagaRecord,
    provision: Option<Arc<dyn WorkloadStartupProvisionOwner>>,
    restart: Arc<dyn WorkloadStartupRestartOwner>,
    teardown: Option<Arc<dyn WorkloadStartupTeardownOwner>>,
) -> WorkloadStartupRecovery {
    let store = RecoveryStore::new(vec![record], 1);
    let store: Arc<dyn WorkloadSagaStore> = store;
    WorkloadStartupRecovery::with_owners(
        Arc::new(WorkloadSagaCoordinator::new(store)),
        provision,
        restart,
        teardown,
    )
}

fn provision_result(
    record: WorkloadSagaRecord,
    disposition: WorkloadProvisionRunDisposition,
    compensation: WorkloadProvisionCompensationState,
) -> StartupProvisionResult {
    StartupProvisionResult {
        record,
        disposition,
        compensation,
    }
}

fn teardown_result(
    record: WorkloadSagaRecord,
    disposition: WorkloadTeardownRunDisposition,
) -> StartupTeardownResult {
    StartupTeardownResult {
        record,
        disposition,
    }
}

fn provision_settlement_record(label: &str) -> WorkloadSagaRecord {
    let initial = crate::workload_saga::recovery::tests::provision_record(
        label,
        WorkloadSagaPhase::IntentCommitted,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::Withheld,
    );
    let mut pending = initial;
    for _ in 0..4 {
        if pending
            .provision_disposition()
            .and_then(WorkloadProvisionDisposition::claim)
            .is_some()
        {
            break;
        }
        let WorkloadProvisionDecision::Proposed(proposed) =
            WorkloadProvisionDecision::plan(&pending)
                .expect("pending fixture should plan one exact provision step")
        else {
            panic!("pending fixture should propose durable dispatch truth");
        };
        pending = proposed.into_candidate();
    }
    let claim = pending
        .provision_disposition()
        .and_then(WorkloadProvisionDisposition::claim)
        .expect("pending fixture should retain its provision claim")
        .clone();
    let WorkloadSagaIntentUpdate::Transition(fenced) = pending
        .apply_intent(crate::workload_saga::recovery::tests::stopped_intent(
            label, 2,
        ))
        .expect("stopped successor should fence the pending provision")
    else {
        panic!("stopped successor should change durable truth");
    };
    let WorkloadProvisionDecision::Proposed(settled) = WorkloadProvisionDecision::reduce(
        &fenced,
        WorkloadProvisionEffectResult::Succeeded {
            attempt_id: claim.attempt().attempt_id().clone(),
            evidence: crate::workload_saga::test_support::success_for(claim.attempt()),
        },
    )
    .expect("inspected provider success should reduce") else {
        panic!("inspected provider success should persist exact settled truth");
    };
    let settled = settled.into_candidate();
    settled
        .commit_queued_successor_teardown()
        .expect("fixture should be ready for exact settlement handoff");
    settled
}

async fn route_once(
    recovery: &WorkloadStartupRecovery,
    record: WorkloadSagaRecord,
) -> Result<WorkloadStartupRecoveryOutcome, WorkloadStartupRecoveryError> {
    let decision = WorkloadSagaDecision::for_record(&record)
        .expect("route fixture record should produce one pure decision");
    recovery.route(&decision, record).await
}

#[tokio::test]
async fn provision_and_failed_provision_routes_map_every_owner_outcome() {
    let cases = [
        (
            "observed",
            WorkloadProvisionRunDisposition::Observed,
            WorkloadProvisionCompensationState::NotRequired,
            WorkloadStartupDisposition::ProvisionObserved,
        ),
        (
            "waiting",
            WorkloadProvisionRunDisposition::Waiting,
            WorkloadProvisionCompensationState::NotRequired,
            WorkloadStartupDisposition::ProvisionWaiting,
        ),
        (
            "compensated",
            WorkloadProvisionRunDisposition::DefiniteFailure,
            WorkloadProvisionCompensationState::Completed,
            WorkloadStartupDisposition::ProvisionCompensated,
        ),
        (
            "compensation-waiting",
            WorkloadProvisionRunDisposition::DefiniteFailure,
            WorkloadProvisionCompensationState::Waiting,
            WorkloadStartupDisposition::ProvisionCompensationWaiting,
        ),
        (
            "cleanup-retained",
            WorkloadProvisionRunDisposition::DefiniteFailure,
            WorkloadProvisionCompensationState::CleanupPending,
            WorkloadStartupDisposition::CleanupRetained,
        ),
    ];

    for (label, disposition, compensation, expected) in cases {
        let record = if disposition == WorkloadProvisionRunDisposition::DefiniteFailure {
            crate::workload_saga::test_support::failed_provision_record(
                &format!("startup-route-{label}"),
                WorkloadProvisionStep::PrepareWorkload,
            )
        } else {
            crate::workload_saga::recovery::tests::provision_record(
                &format!("startup-route-{label}"),
                WorkloadSagaPhase::IntentCommitted,
                WorkloadActivationIntent::ActivateWhenAttached,
                WorkloadPublicationIntent::Withheld,
            )
        };
        let events = Arc::new(Mutex::new(Vec::new()));
        let provision = ScriptedProvisionOwner::new(
            Arc::clone(&events),
            [Ok(provision_result(
                record.clone(),
                disposition,
                compensation,
            ))],
        );
        let restart = ScriptedRestartOwner::new(Arc::clone(&events), []);
        let recovery = recovery_with_owners(record.clone(), Some(provision), restart, None);

        let outcome = route_once(&recovery, record)
            .await
            .expect("scripted provision route should return durable truth");

        assert_eq!(outcome.disposition(), expected, "case {label}");
        assert_eq!(
            *events.lock().expect("route event log remains healthy"),
            ["provision"],
            "case {label}"
        );
    }
}

#[tokio::test]
async fn teardown_routes_map_completed_waiting_and_cleanup_truth() {
    for (label, disposition, expected) in [
        (
            "completed",
            WorkloadTeardownRunDisposition::Completed,
            WorkloadStartupDisposition::TeardownCompleted,
        ),
        (
            "waiting",
            WorkloadTeardownRunDisposition::Waiting,
            WorkloadStartupDisposition::TeardownWaiting,
        ),
        (
            "cleanup",
            WorkloadTeardownRunDisposition::CleanupPending,
            WorkloadStartupDisposition::CleanupRetained,
        ),
    ] {
        let record = crate::workload_saga::recovery::tests::teardown_record(
            &format!("startup-teardown-{label}"),
            WorkloadSagaPhase::NetworkDetached,
        );
        let events = Arc::new(Mutex::new(Vec::new()));
        let teardown = ScriptedTeardownOwner::new(
            Arc::clone(&events),
            [Ok(teardown_result(record.clone(), disposition))],
        );
        let restart = ScriptedRestartOwner::new(Arc::clone(&events), []);
        let recovery = recovery_with_owners(record.clone(), None, restart, Some(teardown));

        let outcome = route_once(&recovery, record)
            .await
            .expect("scripted teardown route should return durable truth");

        assert_eq!(outcome.disposition(), expected, "case {label}");
        assert_eq!(
            *events.lock().expect("route event log remains healthy"),
            ["teardown"],
            "case {label}"
        );
    }
}

#[tokio::test]
async fn active_restart_preempts_the_phase_route_and_reports_waiting() {
    let record =
        crate::workload_saga::test_support::scheduled_restart_record("startup-active-restart", 0);
    let events = Arc::new(Mutex::new(Vec::new()));
    let provision = ScriptedProvisionOwner::new(Arc::clone(&events), []);
    let restart = ScriptedRestartOwner::new(Arc::clone(&events), [Ok(())]);
    let teardown = ScriptedTeardownOwner::new(Arc::clone(&events), []);
    let recovery = recovery_with_owners(record.clone(), Some(provision), restart, Some(teardown));

    let outcome = route_once(&recovery, record)
        .await
        .expect("active restart should route through its retained owner");

    assert_eq!(
        outcome.disposition(),
        WorkloadStartupDisposition::RestartWaiting
    );
    assert_eq!(
        *events.lock().expect("route event log remains healthy"),
        ["restart"]
    );
}

#[tokio::test]
async fn stopped_and_running_successors_use_promotion_then_the_exact_owner() {
    let stopped = crate::workload_saga::recovery::tests::recorded_with_successor(
        "startup-stopped-successor",
        crate::workload_saga::recovery::tests::stopped_intent("startup-stopped-successor", 2),
    );
    let stopped_events = Arc::new(Mutex::new(Vec::new()));
    let stopped_restart = ScriptedRestartOwner::new(Arc::clone(&stopped_events), []);
    let stopped_recovery = recovery_with_owners(stopped.clone(), None, stopped_restart, None);
    let stopped_outcome = route_once(&stopped_recovery, stopped)
        .await
        .expect("stopped successor should promote without an effect owner");
    assert_eq!(
        stopped_outcome.disposition(),
        WorkloadStartupDisposition::SuccessorStopped
    );
    assert!(
        stopped_events
            .lock()
            .expect("event log remains healthy")
            .is_empty()
    );

    let running = crate::workload_saga::recovery::tests::recorded_with_successor(
        "startup-running-successor",
        crate::workload_saga::recovery::tests::running_intent(
            "startup-running-successor",
            2,
            WorkloadActivationIntent::ActivateWhenAttached,
            WorkloadPublicationIntent::Withheld,
        ),
    );
    let promoted = running
        .promote_successor()
        .expect("running successor fixture should promote");
    let running_events = Arc::new(Mutex::new(Vec::new()));
    let provision = ScriptedProvisionOwner::new(
        Arc::clone(&running_events),
        [Ok(provision_result(
            promoted,
            WorkloadProvisionRunDisposition::Waiting,
            WorkloadProvisionCompensationState::NotRequired,
        ))],
    );
    let running_restart = ScriptedRestartOwner::new(Arc::clone(&running_events), []);
    let running_recovery =
        recovery_with_owners(running.clone(), Some(provision), running_restart, None);
    let running_outcome = route_once(&running_recovery, running)
        .await
        .expect("running successor should promote before provision recovery");
    assert_eq!(
        running_outcome.disposition(),
        WorkloadStartupDisposition::SuccessorRunningWaiting
    );
    assert_eq!(
        *running_events
            .lock()
            .expect("route event log remains healthy"),
        ["provision"]
    );
}

#[tokio::test]
async fn provision_settlement_commits_withdrawal_before_teardown_owner() {
    let settled = provision_settlement_record("startup-provision-settlement");
    let withdrawal = settled
        .commit_queued_successor_teardown()
        .expect("settled fixture should produce exact withdrawal truth");
    let events = Arc::new(Mutex::new(Vec::new()));
    let provision = ScriptedProvisionOwner::new(
        Arc::clone(&events),
        [Ok(provision_result(
            settled.clone(),
            WorkloadProvisionRunDisposition::SuccessorSettlementReady,
            WorkloadProvisionCompensationState::NotRequired,
        ))],
    );
    let restart = ScriptedRestartOwner::new(Arc::clone(&events), []);
    let teardown = ScriptedTeardownOwner::new(
        Arc::clone(&events),
        [Ok(teardown_result(
            withdrawal,
            WorkloadTeardownRunDisposition::Completed,
        ))],
    );
    let recovery = recovery_with_owners(settled.clone(), Some(provision), restart, Some(teardown));

    let outcome = route_once(&recovery, settled)
        .await
        .expect("settled provision must hand off to teardown once");

    assert_eq!(
        outcome.disposition(),
        WorkloadStartupDisposition::SuccessorSettlementTeardownCompleted
    );
    assert_eq!(
        *events.lock().expect("route event log remains healthy"),
        ["provision", "teardown"]
    );
}

#[tokio::test]
async fn owner_errors_remain_typed_and_stop_the_route() {
    let provision_record = crate::workload_saga::recovery::tests::provision_record(
        "startup-provision-error",
        WorkloadSagaPhase::IntentCommitted,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::Withheld,
    );
    let events = Arc::new(Mutex::new(Vec::new()));
    let provision = ScriptedProvisionOwner::new(
        Arc::clone(&events),
        [Err("ambiguous provision truth".to_owned())],
    );
    let restart = ScriptedRestartOwner::new(Arc::clone(&events), []);
    let recovery = recovery_with_owners(provision_record.clone(), Some(provision), restart, None);
    assert!(matches!(
        route_once(&recovery, provision_record).await,
        Err(WorkloadStartupRecoveryError::Provision { ref message, .. })
            if message == "ambiguous provision truth"
    ));

    let teardown_record = crate::workload_saga::recovery::tests::teardown_record(
        "startup-teardown-error",
        WorkloadSagaPhase::NetworkDetached,
    );
    let events = Arc::new(Mutex::new(Vec::new()));
    let restart = ScriptedRestartOwner::new(Arc::clone(&events), []);
    let teardown = ScriptedTeardownOwner::new(
        Arc::clone(&events),
        [Err("ambiguous teardown truth".to_owned())],
    );
    let recovery = recovery_with_owners(teardown_record.clone(), None, restart, Some(teardown));
    assert!(matches!(
        route_once(&recovery, teardown_record).await,
        Err(WorkloadStartupRecoveryError::Teardown { ref message, .. })
            if message == "ambiguous teardown truth"
    ));

    let restart_record =
        crate::workload_saga::test_support::scheduled_restart_record("startup-restart-error", 0);
    let events = Arc::new(Mutex::new(Vec::new()));
    let restart = ScriptedRestartOwner::new(
        Arc::clone(&events),
        [Err("ambiguous restart truth".to_owned())],
    );
    let recovery = recovery_with_owners(restart_record.clone(), None, restart, None);
    assert!(matches!(
        route_once(&recovery, restart_record).await,
        Err(WorkloadStartupRecoveryError::Restart { ref message, .. })
            if message == "ambiguous restart truth"
    ));
}

#[tokio::test]
async fn quiescent_record_calls_no_lifecycle_owner() {
    let recorded = crate::workload_saga::recovery::tests::teardown_record(
        "startup-quiescent",
        WorkloadSagaPhase::Recorded,
    );
    let quiescent = recorded
        .promote_successor()
        .expect("stopped successor should become quiescent active truth");
    let events = Arc::new(Mutex::new(Vec::new()));
    let restart = ScriptedRestartOwner::new(Arc::clone(&events), []);
    let recovery = recovery_with_owners(quiescent.clone(), None, restart, None);

    let outcome = route_once(&recovery, quiescent)
        .await
        .expect("quiescent truth should need no lifecycle owner");

    assert_eq!(outcome.disposition(), WorkloadStartupDisposition::Quiescent);
    assert!(events.lock().expect("event log remains healthy").is_empty());
}

#[tokio::test]
async fn fresh_owner_reconciles_observed_publication_before_reporting_quiescent() {
    let observed = crate::workload_saga::recovery::tests::provision_record(
        "startup-owner-reopened-publication",
        WorkloadSagaPhase::Observed,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
    );
    let events = Arc::new(Mutex::new(Vec::new()));
    let provision = ScriptedProvisionOwner::new(
        Arc::clone(&events),
        [Ok(provision_result(
            observed.clone(),
            WorkloadProvisionRunDisposition::Observed,
            WorkloadProvisionCompensationState::NotRequired,
        ))],
    );
    let restart = ScriptedRestartOwner::new(Arc::clone(&events), []);
    let recovery = recovery_with_owners(observed.clone(), Some(provision), restart, None);

    let outcome = route_once(&recovery, observed)
        .await
        .expect("fresh owner should reconcile its process-bound publication");

    assert_eq!(
        outcome.disposition(),
        WorkloadStartupDisposition::ProvisionObserved
    );
    assert_eq!(
        *events.lock().expect("route event log remains healthy"),
        ["publication-reconcile"]
    );
}
