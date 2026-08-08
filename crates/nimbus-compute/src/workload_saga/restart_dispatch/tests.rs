use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use nimbus_core::TenantId;
use nimbus_workloads::{
    WorkloadRestartCandidatePage, WorkloadRestartCandidatePageRequest, WorkloadRestartDisposition,
    WorkloadRestartNotBeforeUnixMillis, WorkloadRestartPolicy, WorkloadSagaCommit,
    WorkloadSagaExpected, WorkloadSagaFuture, WorkloadSagaPage, WorkloadSagaPageRequest,
    WorkloadSagaStore, WorkloadSagaTenantPage, WorkloadSagaTenantPageRequest,
};

use super::*;
use crate::workload_saga::{decide_restart_admission, decide_restart_progress, test_support};

#[derive(Debug, Default)]
struct Calls {
    loads: usize,
    compare_and_swaps: usize,
}

struct TestStore {
    load_results: Mutex<VecDeque<Result<Option<WorkloadSagaRecord>, WorkloadSagaStoreError>>>,
    cas_results: Mutex<VecDeque<Result<WorkloadSagaCommit, WorkloadSagaStoreError>>>,
    calls: Mutex<Calls>,
}

impl TestStore {
    fn sequenced(
        load_results: Vec<Result<Option<WorkloadSagaRecord>, WorkloadSagaStoreError>>,
        cas_results: Vec<Result<WorkloadSagaCommit, WorkloadSagaStoreError>>,
    ) -> Arc<Self> {
        assert!(!load_results.is_empty());
        assert!(!cas_results.is_empty());
        Arc::new(Self {
            load_results: Mutex::new(load_results.into()),
            cas_results: Mutex::new(cas_results.into()),
            calls: Mutex::new(Calls::default()),
        })
    }

    fn counts(&self) -> (usize, usize) {
        let calls = self.calls.lock().expect("test call lock is healthy");
        (calls.loads, calls.compare_and_swaps)
    }

    fn next_repeating<T: Clone>(results: &Mutex<VecDeque<T>>, label: &str) -> T {
        let mut results = results.lock().expect("test result lock is healthy");
        if results.len() > 1 {
            results.pop_front().expect(label)
        } else {
            results.front().expect(label).clone()
        }
    }
}

impl WorkloadSagaStore for TestStore {
    fn load<'a>(
        &'a self,
        _key: &'a WorkloadSagaKey,
    ) -> WorkloadSagaFuture<'a, Option<WorkloadSagaRecord>> {
        Box::pin(async move {
            self.calls.lock().expect("test call lock is healthy").loads += 1;
            Self::next_repeating(&self.load_results, "load sequence is non-empty")
        })
    }

    fn compare_and_swap<'a>(
        &'a self,
        _expected: WorkloadSagaExpected,
        _next: WorkloadSagaRecord,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaCommit> {
        Box::pin(async move {
            self.calls
                .lock()
                .expect("test call lock is healthy")
                .compare_and_swaps += 1;
            Self::next_repeating(&self.cas_results, "CAS sequence is non-empty")
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
        tenant_id: &'a TenantId,
        request: WorkloadSagaTenantPageRequest,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaTenantPage> {
        Box::pin(async move { WorkloadSagaTenantPage::new(tenant_id, &request, Vec::new(), false) })
    }
}

fn pending_command(label: &str) -> (WorkloadSagaRecord, ProposedWorkloadRestartTransition) {
    let observed = test_support::restart_observed_record(label, WorkloadRestartPolicy::Never);
    let request = super::super::WorkloadRestartAdmissionRequest::for_explicit(
        &observed,
        label,
        WorkloadRestartNotBeforeUnixMillis::new(0),
    )
    .expect("explicit restart request should validate");
    let super::super::WorkloadRestartAdmissionDecision::Transition(admitted) =
        decide_restart_admission(&observed, &request).expect("restart should admit")
    else {
        panic!("new restart should transition");
    };
    let WorkloadRestartDecision::Proposed(withdrawal) =
        decide_restart_progress(&admitted, WorkloadRestartNotBeforeUnixMillis::new(0))
            .expect("requested restart should reduce")
    else {
        panic!("requested restart should enter withdrawal");
    };
    assert!(withdrawal.action_after_confirmation().is_none());
    let withdrawal = withdrawal.into_candidate();
    let WorkloadRestartDecision::Proposed(pending) =
        decide_restart_progress(&withdrawal, WorkloadRestartNotBeforeUnixMillis::new(0))
            .expect("withdrawal should claim a command")
    else {
        panic!("withdrawal should produce a command claim");
    };
    assert_eq!(
        pending.action_after_confirmation(),
        Some(WorkloadRestartSymbolicAction::StartExactAttempt)
    );
    (withdrawal, pending)
}

async fn directly_confirmed(label: &str) -> (ConfirmedWorkloadRestartTransition, Arc<TestStore>) {
    let (loaded, proposed) = pending_command(label);
    let store = TestStore::sequenced(vec![Ok(None)], vec![Ok(WorkloadSagaCommit::Applied)]);
    let coordinator = WorkloadSagaCoordinator::new(store.clone());
    let confirmed = coordinator
        .claim_restart_command(&loaded, &proposed)
        .await
        .expect("direct claim should confirm");
    (confirmed, store)
}

async fn inspection_confirmed(label: &str) -> (ConfirmedWorkloadRestartTransition, Arc<TestStore>) {
    let (loaded, proposed) = pending_command(label);
    let store = TestStore::sequenced(
        vec![Ok(None)],
        vec![
            Ok(WorkloadSagaCommit::Unchanged),
            Ok(WorkloadSagaCommit::Applied),
        ],
    );
    let coordinator = WorkloadSagaCoordinator::new(store.clone());
    let confirmed = coordinator
        .claim_restart_command(&loaded, &proposed)
        .await
        .expect("replay should enter inspection");
    (confirmed, store)
}

fn confirmed_parts(
    confirmed: &ConfirmedWorkloadRestartTransition,
) -> (&WorkloadSagaRecord, &ConfirmedWorkloadRestartCommand) {
    (
        confirmed
            .confirmed_record()
            .expect("confirmation should expose durable truth"),
        confirmed
            .command()
            .expect("confirmation should expose a provider command"),
    )
}

#[tokio::test]
async fn confirmed_restart_command_is_private_and_complete() {
    let (confirmed, store) = directly_confirmed("complete-command").await;
    assert_eq!(
        confirmed.confirmation(),
        WorkloadSagaConfirmation::AppliedByThisCall
    );
    let (record, command) = confirmed_parts(&confirmed);
    let active = record.restart_state().active().unwrap();
    let admission = active.admission();

    assert_eq!(command.command_id(), command.claim().command_id());
    assert_eq!(command.key(), record.key());
    assert_eq!(command.saga_id(), record.saga_id());
    assert_eq!(
        command.transition_id(),
        record.last_transition().transition_id()
    );
    assert_eq!(command.generation(), admission.generation());
    assert_eq!(command.desired_digest(), admission.desired_digest());
    assert_eq!(command.source(), admission.source());
    assert_eq!(command.source_attempt_id(), admission.source_attempt_id());
    assert_eq!(command.attempt_id(), admission.attempt_id());
    assert_eq!(command.restart_epoch(), admission.restart_epoch());
    assert_eq!(command.dispatch_epoch(), command.claim().dispatch_epoch());
    assert_eq!(command.request_id(), admission.request_id());
    assert_eq!(
        command.issuing_revision(),
        command.claim().issuing_revision()
    );
    assert_eq!(command.confirmed_revision(), record.revision());
    assert_eq!(command.inspection_version(), None);
    assert_eq!(command.provider_selection(), admission.provider_selection());
    assert_eq!(command.step(), command.claim().step());
    assert_eq!(command.mode(), WorkloadRestartCommandMode::Execute);
    assert_eq!(command.executable(), record.active_intent().executable());
    assert_eq!(
        command.compiled_network_plan(),
        record.active_intent().network().compiled_plan()
    );
    assert_eq!(store.counts(), (0, 1));
}

#[tokio::test]
async fn direct_claim_cas_winner_alone_executes() {
    let (loaded, proposed) = pending_command("one-executor");
    let store = TestStore::sequenced(
        vec![Ok(None)],
        vec![
            Ok(WorkloadSagaCommit::Applied),
            Ok(WorkloadSagaCommit::Unchanged),
            Ok(WorkloadSagaCommit::Applied),
        ],
    );
    let coordinator = WorkloadSagaCoordinator::new(store.clone());
    let winner = coordinator
        .claim_restart_command(&loaded, &proposed)
        .await
        .expect("winner should confirm");
    let loser = coordinator
        .claim_restart_command(&loaded, &proposed)
        .await
        .expect("loser should adopt through inspection");

    assert_eq!(
        winner.command().unwrap().mode(),
        WorkloadRestartCommandMode::Execute
    );
    assert_eq!(
        loser.command().unwrap().mode(),
        WorkloadRestartCommandMode::Inspect
    );
    assert_eq!(store.counts(), (0, 3));
}

#[tokio::test]
async fn confirmed_replay_does_not_execute() {
    let (confirmed, store) = inspection_confirmed("replay-inspects").await;
    assert_eq!(
        confirmed.command().unwrap().mode(),
        WorkloadRestartCommandMode::Inspect
    );
    assert_eq!(store.counts(), (0, 2));
}

#[tokio::test]
async fn ambiguous_claim_cas_fresh_reads_before_effect() {
    let (loaded, proposed) = pending_command("ambiguous-read");
    let candidate = proposed.candidate().clone();
    let store = TestStore::sequenced(
        vec![Ok(Some(candidate))],
        vec![
            Err(WorkloadSagaStoreError::Ambiguous),
            Ok(WorkloadSagaCommit::Applied),
        ],
    );
    let coordinator = WorkloadSagaCoordinator::new(store.clone());
    let confirmed = coordinator
        .claim_restart_command(&loaded, &proposed)
        .await
        .expect("fresh read should confirm before inspection");

    assert_eq!(
        confirmed.command().unwrap().mode(),
        WorkloadRestartCommandMode::Inspect
    );
    assert_eq!(store.counts(), (1, 2));
}

#[tokio::test]
async fn crash_after_restart_effect_inspects_before_retry() {
    let (_, proposed) = pending_command("crash-inspects");
    let pending = proposed.into_candidate();
    let store = TestStore::sequenced(
        vec![Ok(Some(pending.clone()))],
        vec![Ok(WorkloadSagaCommit::Applied)],
    );
    let coordinator = WorkloadSagaCoordinator::new(store.clone());
    let recovered = coordinator
        .inspect_confirmed_restart(pending.key())
        .await
        .expect("fresh recovery should durably inspect");

    assert!(matches!(
        recovered
            .confirmed_record()
            .unwrap()
            .restart_state()
            .active()
            .unwrap()
            .disposition(),
        WorkloadRestartDisposition::InspectionRequired { .. }
    ));
    assert_eq!(
        recovered.command().unwrap().mode(),
        WorkloadRestartCommandMode::Inspect
    );
    assert_eq!(store.counts(), (1, 1));
}

#[tokio::test]
async fn authenticated_absence_retries_same_attempt_at_next_dispatch_epoch() {
    let (confirmed, _) = inspection_confirmed("absence-retry").await;
    let (record, command) = confirmed_parts(&confirmed);
    let result = WorkloadRestartCommandResult::for_command(
        command,
        WorkloadRestartCommandOutcome::AuthenticatedAbsent {
            evidence: WorkloadRestartEvidenceDigest::sha256("exact-absence"),
        },
    );
    let WorkloadRestartDecision::Proposed(retry) =
        apply_restart_result(record, command, result).expect("absence should authorize retry")
    else {
        panic!("absence should produce one retry proposal");
    };
    let retry_claim = retry
        .candidate()
        .restart_state()
        .active()
        .and_then(|active| active.disposition().claim())
        .expect("retry should retain a claim");

    assert_eq!(retry_claim.attempt_id(), command.attempt_id());
    assert_eq!(retry_claim.restart_epoch(), command.restart_epoch());
    assert_eq!(
        retry_claim.dispatch_epoch(),
        command.dispatch_epoch().checked_next().unwrap()
    );
}

#[tokio::test]
async fn in_progress_never_retries() {
    let (confirmed, _) = inspection_confirmed("in-progress").await;
    let (record, command) = confirmed_parts(&confirmed);
    for result in [
        WorkloadRestartCommandResult::for_command(
            command,
            WorkloadRestartCommandOutcome::InProgress {
                evidence: WorkloadRestartEvidenceDigest::sha256("still-running"),
            },
        ),
        WorkloadRestartCommandResult::for_command(
            command,
            WorkloadRestartCommandOutcome::Ambiguous,
        ),
    ] {
        assert!(matches!(
            result.outcome(),
            WorkloadRestartCommandOutcome::InProgress { .. }
                | WorkloadRestartCommandOutcome::Ambiguous
        ));
        assert_eq!(
            apply_restart_result(record, command, result)
                .expect("nonterminal inspection should remain inspect-only"),
            WorkloadRestartDecision::InspectExact(Box::new(command.claim().clone()))
        );
    }
}

#[tokio::test]
async fn definite_failure_stops_later_commands() {
    let (confirmed, store) = directly_confirmed("terminal-failure").await;
    let (record, command) = confirmed_parts(&confirmed);
    let result = WorkloadRestartCommandResult::for_command(
        command,
        WorkloadRestartCommandOutcome::DefiniteFailure {
            evidence: WorkloadRestartEvidenceDigest::sha256("definite-failure"),
        },
    );
    let WorkloadRestartDecision::Proposed(failed) =
        apply_restart_result(record, command, result).expect("failure should persist")
    else {
        panic!("failure should create one durable candidate");
    };
    let coordinator = WorkloadSagaCoordinator::new(store);
    assert_eq!(
        coordinator
            .compare_and_swap_restart_result(record, &failed)
            .await
            .expect("exact result candidate should confirm"),
        WorkloadSagaConfirmation::AppliedByThisCall
    );

    assert!(matches!(
        decide_restart_progress(
            failed.candidate(),
            WorkloadRestartNotBeforeUnixMillis::new(0)
        )
        .expect("terminal state should reduce"),
        WorkloadRestartDecision::DefiniteFailure
    ));
}

#[tokio::test]
async fn crossed_restart_result_is_rejected() {
    let (confirmed, _) = directly_confirmed("crossed-result").await;
    let (record, command) = confirmed_parts(&confirmed);
    let mut result = WorkloadRestartCommandResult::for_command(
        command,
        WorkloadRestartCommandOutcome::Succeeded {
            evidence: WorkloadRestartEvidenceDigest::sha256("crossed-success"),
            observed_detail: None,
        },
    );
    result.attempt_id = command.source_attempt_id().clone();

    assert!(apply_restart_result(record, command, result).is_err());
}

#[tokio::test]
async fn reused_skipped_and_crossed_dispatch_epochs_fail_closed() {
    let (confirmed, _) = directly_confirmed("epoch-fences").await;
    let (record, command) = confirmed_parts(&confirmed);
    let success = || {
        WorkloadRestartCommandResult::for_command(
            command,
            WorkloadRestartCommandOutcome::Succeeded {
                evidence: WorkloadRestartEvidenceDigest::sha256("epoch-success"),
                observed_detail: None,
            },
        )
    };

    let WorkloadRestartDecision::Proposed(completed) =
        apply_restart_result(record, command, success()).expect("exact epoch should succeed")
    else {
        panic!("exact result should create one candidate");
    };
    assert!(apply_restart_result(completed.candidate(), command, success()).is_err());

    let mut skipped = success();
    skipped.dispatch_epoch = WorkloadRestartDispatchEpoch::new(
        command.dispatch_epoch().as_u64().checked_add(2).unwrap(),
    );
    assert!(apply_restart_result(record, command, skipped).is_err());

    let mut crossed = success();
    crossed.attempt_id = command.source_attempt_id().clone();
    assert!(apply_restart_result(record, command, crossed).is_err());
}
