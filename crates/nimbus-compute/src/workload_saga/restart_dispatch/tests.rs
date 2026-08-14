use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use nimbus_core::TenantId;
use nimbus_workloads::{
    WorkloadRestartCandidatePage, WorkloadRestartCandidatePageRequest, WorkloadRestartDisposition,
    WorkloadRestartEffectResult, WorkloadRestartNotBeforeUnixMillis, WorkloadRestartPhase,
    WorkloadRestartPolicy, WorkloadSagaCommit, WorkloadSagaExpected, WorkloadSagaFuture,
    WorkloadSagaPage, WorkloadSagaPageRequest, WorkloadSagaPhase, WorkloadSagaStore,
    WorkloadSagaTenantPage, WorkloadSagaTenantPageRequest,
};

use super::*;
use crate::workload_saga::recovery::tests::provision_record;
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

fn succeed_restart_command(record: WorkloadSagaRecord, label: &str) -> WorkloadSagaRecord {
    let active = record
        .restart_state()
        .active()
        .expect("restart should remain active");
    let request_id = active.admission().request_id().clone();
    let pending = record
        .claim_restart_command(&request_id)
        .expect("restart step should claim");
    let claim = pending
        .restart_state()
        .active()
        .and_then(|active| active.disposition().claim())
        .expect("restart step should retain its claim")
        .clone();
    pending
        .apply_restart_effect_result(
            &claim,
            WorkloadRestartEffectResult::Succeeded {
                evidence: WorkloadRestartEvidenceDigest::sha256(label),
            },
        )
        .expect("restart step should succeed")
}

fn published_observation_inspection(label: &str) -> WorkloadSagaRecord {
    let observed = provision_record(
        label,
        WorkloadSagaPhase::Observed,
        nimbus_workloads::WorkloadActivationIntent::ActivateWhenAttached,
        nimbus_workloads::WorkloadPublicationIntent::PublishWhenReady,
    );
    let request = super::super::WorkloadRestartAdmissionRequest::for_explicit(
        &observed,
        label,
        WorkloadRestartNotBeforeUnixMillis::new(0),
    )
    .expect("published restart request should validate");
    let super::super::WorkloadRestartAdmissionDecision::Transition(admitted) =
        decide_restart_admission(&observed, &request).expect("published restart should admit")
    else {
        panic!("published restart should transition");
    };
    let request_id = admitted
        .restart_state()
        .active()
        .expect("published restart should remain active")
        .admission()
        .request_id()
        .clone();
    let mut record = admitted
        .advance_restart_without_effect(&request_id)
        .expect("published restart should withdraw publication first");
    for step in ["withdrawn", "quiesced"] {
        record = succeed_restart_command(record, step);
    }
    let due = record
        .restart_state()
        .active()
        .expect("published restart should remain active")
        .admission()
        .not_before_unix_millis();
    record = record
        .advance_scheduled_restart(&request_id, due)
        .expect("published restart should become due");
    for step in [
        "prepared",
        "attached",
        "prerequisites",
        "activated",
        "ready",
        "published",
    ] {
        record = succeed_restart_command(record, step);
    }
    let pending = record
        .claim_restart_command(&request_id)
        .expect("publication observation should claim");
    let claim = pending
        .restart_state()
        .active()
        .and_then(|active| active.disposition().claim())
        .expect("publication observation should retain its claim")
        .clone();
    assert_eq!(claim.step(), WorkloadRestartStep::ObservePublication);
    pending
        .restart_dispatch_to_inspection(&claim)
        .expect("fresh-process recovery should persist observation inspection")
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
    assert_eq!(command.successor_veto_generation(), None);
    assert_eq!(command.step(), command.claim().step());
    assert_eq!(command.mode(), WorkloadRestartCommandMode::Execute);
    assert_eq!(command.executable(), record.active_intent().executable());
    assert_eq!(
        command.compiled_network_plan(),
        record.active_intent().network().compiled_plan()
    );
    assert!(command.publication_reference().is_none());
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
async fn fresh_process_observation_absence_republishes_once_before_observing_again() {
    let inspection = published_observation_inspection("fresh-observation-absence");
    let recovery_store = TestStore::sequenced(
        vec![Ok(Some(inspection.clone()))],
        vec![Ok(WorkloadSagaCommit::Applied)],
    );
    let recovery_coordinator = WorkloadSagaCoordinator::new(recovery_store.clone());
    let recovered = recovery_coordinator
        .inspect_confirmed_restart(inspection.key())
        .await
        .expect("fresh process should recover exact observation inspection");
    let (record, observe) = confirmed_parts(&recovered);
    assert_eq!(observe.step(), WorkloadRestartStep::ObservePublication);
    assert_eq!(observe.mode(), WorkloadRestartCommandMode::Inspect);
    let publication = observe
        .publication_reference()
        .expect("published restart command retains exact target publication identity");
    assert_eq!(publication.execution(), observe.execution());
    assert_eq!(
        publication.endpoints(),
        observe
            .compiled_network_plan()
            .content()
            .listeners()
            .iter()
            .map(|listener| listener.endpoint_id().clone())
            .collect::<Vec<_>>()
    );
    let observe_epoch = observe.dispatch_epoch();
    let attempt_id = observe.attempt_id().clone();
    let result = WorkloadRestartCommandResult::for_command(
        observe,
        WorkloadRestartCommandOutcome::AuthenticatedAbsent {
            evidence: WorkloadRestartEvidenceDigest::sha256("fresh-observation-absence"),
        },
    );
    let WorkloadRestartDecision::Proposed(republish) =
        apply_restart_result(record, observe, result).expect("absence should require republish")
    else {
        panic!("observation absence should propose one republish");
    };
    assert_eq!(
        republish.action_after_confirmation(),
        Some(WorkloadRestartSymbolicAction::StartExactAttempt)
    );
    assert_eq!(
        republish
            .candidate()
            .restart_state()
            .active()
            .expect("republish should remain active")
            .phase(),
        WorkloadRestartPhase::PublicationPending
    );

    let convergence_store =
        TestStore::sequenced(vec![Ok(None)], vec![Ok(WorkloadSagaCommit::Applied); 5]);
    let convergence_coordinator = WorkloadSagaCoordinator::new(convergence_store.clone());
    let confirmed = convergence_coordinator
        .compare_and_swap_restart_result(record, &republish)
        .await
        .expect("republish should confirm");
    let publish = confirmed
        .command()
        .expect("republish should issue a command");
    assert_eq!(publish.step(), WorkloadRestartStep::Publish);
    assert_eq!(publish.mode(), WorkloadRestartCommandMode::Execute);
    assert_eq!(publish.attempt_id(), &attempt_id);
    assert_eq!(
        publish.dispatch_epoch(),
        observe_epoch.checked_next().unwrap()
    );
    let publish_result = WorkloadRestartCommandResult::for_command(
        publish,
        WorkloadRestartCommandOutcome::Succeeded {
            evidence: WorkloadRestartEvidenceDigest::sha256("republished"),
        },
    );
    let WorkloadRestartDecision::Proposed(published) = apply_restart_result(
        confirmed.confirmed_record().unwrap(),
        publish,
        publish_result,
    )
    .expect("republish should produce observation state") else {
        panic!("republish success should produce a durable candidate");
    };
    let published = convergence_coordinator
        .compare_and_swap_restart_result(confirmed.confirmed_record().unwrap(), &published)
        .await
        .expect("republish success should confirm");
    let published_record = published
        .confirmed_record()
        .expect("republish success should expose durable truth");
    let WorkloadRestartDecision::Proposed(observation) =
        decide_restart_progress(published_record, WorkloadRestartNotBeforeUnixMillis::new(0))
            .expect("republished endpoint should require observation")
    else {
        panic!("republished endpoint should claim observation");
    };
    let observed = convergence_coordinator
        .claim_restart_command(published_record, &observation)
        .await
        .expect("publication observation should confirm as inspection");
    let observation_command = observed
        .command()
        .expect("publication observation should issue a command");
    assert_eq!(
        observation_command.step(),
        WorkloadRestartStep::ObservePublication
    );
    assert_eq!(
        observation_command.mode(),
        WorkloadRestartCommandMode::Inspect
    );
    let observation_result = WorkloadRestartCommandResult::for_command(
        observation_command,
        WorkloadRestartCommandOutcome::Succeeded {
            evidence: WorkloadRestartEvidenceDigest::sha256("republish-observed"),
        },
    );
    let WorkloadRestartDecision::Proposed(completed) = apply_restart_result(
        observed.confirmed_record().unwrap(),
        observation_command,
        observation_result,
    )
    .expect("publication observation should complete the restart") else {
        panic!("publication observation should produce one completion candidate");
    };
    let completed = convergence_coordinator
        .compare_and_swap_restart_result(observed.confirmed_record().unwrap(), &completed)
        .await
        .expect("publication observation completion should confirm");
    assert!(
        completed
            .confirmed_record()
            .expect("restart completion should expose durable truth")
            .restart_state()
            .active()
            .is_none()
    );
    assert_eq!(convergence_store.counts(), (0, 5));

    let replay_store = TestStore::sequenced(
        vec![Ok(None)],
        vec![
            Ok(WorkloadSagaCommit::Unchanged),
            Ok(WorkloadSagaCommit::Applied),
        ],
    );
    let replay_coordinator = WorkloadSagaCoordinator::new(replay_store.clone());
    let replay = replay_coordinator
        .compare_and_swap_restart_result(record, &republish)
        .await
        .expect("republish replay should inspect");
    let replay_command = replay.command().expect("replay should issue inspection");
    assert_eq!(replay_command.step(), WorkloadRestartStep::Publish);
    assert_eq!(replay_command.mode(), WorkloadRestartCommandMode::Inspect);
    assert_eq!(replay_command.dispatch_epoch(), publish.dispatch_epoch());
    assert_eq!(recovery_store.counts(), (1, 0));
    assert_eq!(replay_store.counts(), (0, 2));
}

#[tokio::test]
async fn execute_absence_with_successor_requires_exact_inspection_before_terminal_veto() {
    let (confirmed, _) = directly_confirmed("execute-absence-successor").await;
    let (record, execute) = confirmed_parts(&confirmed);
    assert_eq!(execute.mode(), WorkloadRestartCommandMode::Execute);
    assert_eq!(execute.successor_veto_generation(), None);
    let successor = test_support::record_with_successor(record, "execute-absence-successor-next");
    assert!(matches!(
        successor
            .restart_state()
            .active()
            .expect("successor should retain issued restart evidence")
            .disposition(),
        WorkloadRestartDisposition::InspectionRequired { claim }
            if claim == execute.claim()
    ));
    assert_eq!(
        decide_restart_progress(&successor, WorkloadRestartNotBeforeUnixMillis::new(0)).unwrap(),
        WorkloadRestartDecision::InspectExact(Box::new(execute.claim().clone()))
    );
    let execute_absence = WorkloadRestartCommandResult::for_command(
        execute,
        WorkloadRestartCommandOutcome::AuthenticatedAbsent {
            evidence: WorkloadRestartEvidenceDigest::sha256("execute-time-absence"),
        },
    );
    let WorkloadRestartDecision::Proposed(inspection) =
        apply_restart_result(record, execute, execute_absence)
            .expect("execute-time absence should remain ambiguous")
    else {
        panic!("execute-time absence should persist inspection state");
    };
    assert_eq!(
        inspection.action_after_confirmation(),
        Some(WorkloadRestartSymbolicAction::InspectExactAttempt)
    );
    assert!(matches!(
        inspection
            .candidate()
            .restart_state()
            .active()
            .expect("inspection should retain active restart")
            .disposition(),
        WorkloadRestartDisposition::InspectionRequired { claim }
            if claim == execute.claim()
    ));
    assert!(
        apply_restart_result(
            &successor,
            execute,
            WorkloadRestartCommandResult::for_command(
                execute,
                WorkloadRestartCommandOutcome::AuthenticatedAbsent {
                    evidence: WorkloadRestartEvidenceDigest::sha256("stale-execute-absence"),
                },
            ),
        )
        .is_err()
    );

    let store = TestStore::sequenced(
        vec![Ok(Some(successor.clone()))],
        vec![Ok(WorkloadSagaCommit::Applied)],
    );
    let coordinator = WorkloadSagaCoordinator::new(store.clone());
    let recovered = coordinator
        .inspect_confirmed_restart(successor.key())
        .await
        .expect("successor should retain exact inspection authority");
    let (record, inspect) = confirmed_parts(&recovered);
    assert_eq!(inspect.mode(), WorkloadRestartCommandMode::Inspect);
    assert_eq!(inspect.dispatch_epoch(), execute.dispatch_epoch());
    assert_eq!(
        inspect.successor_veto_generation(),
        successor
            .successor_intent()
            .map(|intent| intent.generation())
    );
    let inspected_absence = WorkloadRestartCommandResult::for_command(
        inspect,
        WorkloadRestartCommandOutcome::AuthenticatedAbsent {
            evidence: WorkloadRestartEvidenceDigest::sha256("inspected-absence"),
        },
    );
    let WorkloadRestartDecision::Proposed(terminal) =
        apply_restart_result(record, inspect, inspected_absence)
            .expect("exact inspection absence should complete the successor fence")
    else {
        panic!("inspected absence should produce one terminal candidate");
    };
    assert!(terminal.action_after_confirmation().is_none());
    assert!(matches!(
        terminal
            .candidate()
            .restart_state()
            .active()
            .expect("terminal veto should retain issued evidence")
            .disposition(),
        WorkloadRestartDisposition::SuccessorVetoed {
            claim,
            result: WorkloadRestartEffectResult::AuthenticatedAbsent { .. },
        } if claim == inspect.claim()
    ));
    assert_eq!(store.counts(), (1, 0));
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
            .expect("exact result candidate should confirm")
            .confirmation(),
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

#[tokio::test]
async fn provider_journal_claim_binds_the_restart_attempt_chain_and_ordinal() {
    let (confirmed, _) = directly_confirmed("provider-journal-claim").await;
    let (_, command) = confirmed_parts(&confirmed);
    let claim = crate::workload_saga::restart_provider_command::claim_for_command(command)
        .expect("the exact confirmed restart command should produce one provider claim");

    assert_eq!(
        claim.source_attempt_id(),
        Some(command.source_attempt_id().as_str())
    );
    assert_eq!(claim.attempt_id(), command.attempt_id().as_str());
    assert_eq!(claim.restart_ordinal(), command.restart_epoch().as_u64());
    assert_eq!(
        claim.operation(),
        nimbus_sandbox::ProviderCommandOperation::ResetWorkloadForRestart
    );
}

#[tokio::test]
async fn provider_journal_exact_replay_never_executes_the_effect_twice() {
    let (confirmed, _) = directly_confirmed("provider-journal-replay").await;
    let (_, command) = confirmed_parts(&confirmed);
    let root = tempfile::tempdir().expect("temporary provider journal root should exist");
    let journal =
        nimbus_sandbox::ProviderCommandAttemptJournal::open(root.path(), "compute-restart-replay")
            .expect("provider restart journal should open");
    let adapter =
        crate::workload_saga::restart_provider_command::ProviderRestartPhaseAdapter::new(journal);

    let first = adapter.execute(command, || {
        crate::workload_saga::restart_provider_command::ProviderRestartEffectObservation::Succeeded {
            evidence: b"one exact provider effect".to_vec(),
        }
    });
    assert!(matches!(
        first.into_outcome(),
        WorkloadRestartCommandOutcome::Succeeded { .. }
    ));

    let replay = adapter.execute(command, || {
        panic!("an exact durable provider replay must not execute twice")
    });
    assert!(matches!(
        replay.into_outcome(),
        WorkloadRestartCommandOutcome::Succeeded { .. }
    ));
}

#[tokio::test]
async fn provider_journal_adopted_claimed_restart_inspects_exact_absence() {
    let (confirmed, _) = inspection_confirmed("provider-journal-claimed").await;
    let (_, command) = confirmed_parts(&confirmed);
    let root = tempfile::tempdir().expect("temporary provider journal root should exist");
    let journal =
        nimbus_sandbox::ProviderCommandAttemptJournal::open(root.path(), "restart-claimed")
            .expect("provider restart journal should open");
    let claim = crate::workload_saga::restart_provider_command::claim_for_command(command)
        .expect("confirmed restart command should produce one claim");
    let execution = match journal
        .claim_dispatch_epoch(&claim)
        .expect("initial claim should succeed")
    {
        nimbus_sandbox::ProviderCommandClaimDecision::ExecuteClaimed(execution) => execution,
        nimbus_sandbox::ProviderCommandClaimDecision::AdoptExactAttempt(_) => {
            panic!("first claim should grant exact execution authority")
        }
    };
    let delayed_journal = journal.clone();
    let adapter =
        crate::workload_saga::restart_provider_command::ProviderRestartPhaseAdapter::new(journal);

    let observed = adapter.inspect(command, || {
        crate::workload_saga::restart_provider_command::ProviderRestartEffectObservation::Absent {
            evidence: b"recovery proves the claimed restart effect absent".to_vec(),
        }
    });
    assert!(matches!(
        observed.into_outcome(),
        WorkloadRestartCommandOutcome::AuthenticatedAbsent { .. }
    ));
    let effects = std::cell::Cell::new(0_u64);
    assert!(
        delayed_journal
            .execute_current_claim(execution, |_| {
                effects.set(effects.get() + 1);
                (
                    (),
                    nimbus_sandbox::ProviderCommandObservationKind::Succeeded,
                    None,
                    b"delayed restart effect must not run".to_vec(),
                )
            })
            .is_err()
    );
    assert_eq!(effects.get(), 0);
}

#[tokio::test]
async fn provider_journal_adopted_restart_ambiguity_is_inspected_once() {
    let (confirmed, _) = inspection_confirmed("provider-journal-ambiguous").await;
    let (_, command) = confirmed_parts(&confirmed);
    let root = tempfile::tempdir().expect("temporary provider journal root should exist");
    let journal =
        nimbus_sandbox::ProviderCommandAttemptJournal::open(root.path(), "restart-ambiguous")
            .expect("provider restart journal should open");
    let adapter =
        crate::workload_saga::restart_provider_command::ProviderRestartPhaseAdapter::new(journal);

    let ambiguous = adapter.inspect(command, || {
        crate::workload_saga::restart_provider_command::ProviderRestartEffectObservation::Ambiguous {
            evidence: b"provider inspection was interrupted".to_vec(),
        }
    });
    assert!(matches!(
        ambiguous.into_outcome(),
        WorkloadRestartCommandOutcome::Ambiguous
    ));
    let absent = adapter.inspect(command, || {
        crate::workload_saga::restart_provider_command::ProviderRestartEffectObservation::Absent {
            evidence: b"exact inspection proves restart effect absent".to_vec(),
        }
    });
    assert!(matches!(
        absent.into_outcome(),
        WorkloadRestartCommandOutcome::AuthenticatedAbsent { .. }
    ));
    let replay = adapter.inspect(command, || {
        panic!("terminal restart absence must replay without another provider inspection")
    });
    assert!(matches!(
        replay.into_outcome(),
        WorkloadRestartCommandOutcome::AuthenticatedAbsent { .. }
    ));
}
