use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use nimbus_core::TenantId;
use nimbus_workloads::{
    WorkloadActivationIntent, WorkloadFailureEvidence, WorkloadProvisionDisposition,
    WorkloadPublicationIntent, WorkloadSagaFuture, WorkloadSagaPage, WorkloadSagaPageRequest,
    WorkloadSagaPhase, WorkloadSagaStore, WorkloadSagaTenantPage, WorkloadSagaTenantPageRequest,
};

use super::*;
use crate::workload_saga::recovery::tests::provision_record;

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
    fn new(
        load_result: Result<Option<WorkloadSagaRecord>, WorkloadSagaStoreError>,
        cas_result: Result<WorkloadSagaCommit, WorkloadSagaStoreError>,
    ) -> Arc<Self> {
        Self::sequenced(vec![load_result], vec![cas_result])
    }

    fn sequenced(
        load_results: Vec<Result<Option<WorkloadSagaRecord>, WorkloadSagaStoreError>>,
        cas_results: Vec<Result<WorkloadSagaCommit, WorkloadSagaStoreError>>,
    ) -> Arc<Self> {
        assert!(!load_results.is_empty(), "load sequence must not be empty");
        assert!(!cas_results.is_empty(), "CAS sequence must not be empty");
        Arc::new(Self {
            load_results: Mutex::new(load_results.into()),
            cas_results: Mutex::new(cas_results.into()),
            calls: Mutex::new(Calls::default()),
        })
    }

    fn counts(&self) -> (usize, usize) {
        let calls = self.calls.lock().expect("test store lock is healthy");
        (calls.loads, calls.compare_and_swaps)
    }

    fn next_repeating<T: Clone>(results: &Mutex<VecDeque<T>>, label: &str) -> T {
        let mut results = results.lock().expect("test store result lock is healthy");
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
            self.calls.lock().expect("test store lock is healthy").loads += 1;
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
                .expect("test store lock is healthy")
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

    fn list_for_tenant<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        request: WorkloadSagaTenantPageRequest,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaTenantPage> {
        Box::pin(async move { WorkloadSagaTenantPage::new(tenant_id, &request, Vec::new(), false) })
    }
}

fn current(label: &str, phase: WorkloadSagaPhase) -> WorkloadSagaRecord {
    provision_record(
        label,
        phase,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
    )
}

fn proposal(record: &WorkloadSagaRecord) -> ProposedWorkloadProvisionTransition {
    let WorkloadProvisionDecision::Proposed(proposal) =
        WorkloadProvisionDecision::plan(record).expect("phase should produce a proposal")
    else {
        panic!("phase should produce one provision proposal");
    };
    proposal
}

fn confirmed_record(confirmed: &ConfirmedWorkloadProvisionTransition) -> &WorkloadSagaRecord {
    confirmed
        .confirmed_record()
        .expect("fixture confirmation should expose durable candidate truth")
}

async fn confirm(
    record: &WorkloadSagaRecord,
    proposal: &ProposedWorkloadProvisionTransition,
    load_result: Result<Option<WorkloadSagaRecord>, WorkloadSagaStoreError>,
    cas_result: Result<WorkloadSagaCommit, WorkloadSagaStoreError>,
) -> (ConfirmedWorkloadProvisionTransition, Arc<TestStore>) {
    let store = TestStore::new(load_result, cas_result);
    let coordinator = WorkloadSagaCoordinator::new(store.clone());
    let confirmed = coordinator
        .confirm_provision_transition(record, proposal)
        .await
        .expect("confirmation should produce a closed disposition");
    (confirmed, store)
}

fn ambiguous(command: &ConfirmedWorkloadProvisionCommand) -> WorkloadProvisionInspectionResult {
    WorkloadProvisionInspectionResult::Ambiguous {
        attempt_id: command.attempt_id().clone(),
        dispatch_epoch: command.dispatch_epoch(),
        provider_target: command.provider_target().clone(),
    }
}

#[tokio::test]
async fn direct_cas_winner_executes_exact_attempt_once() {
    let record = current("direct-winner", WorkloadSagaPhase::IntentCommitted);
    let proposal = proposal(&record);
    let (confirmed, store) = confirm(
        &record,
        &proposal,
        Ok(None),
        Ok(WorkloadSagaCommit::Applied),
    )
    .await;

    assert_eq!(
        confirmed.confirmation(),
        WorkloadSagaConfirmation::AppliedByThisCall
    );
    assert_eq!(confirmed.confirmed_record(), Some(proposal.candidate()));
    let command = confirmed.command().expect("direct winner should dispatch");
    assert_eq!(command.mode(), WorkloadProvisionCommandMode::Execute);
    assert_eq!(command.attempt_id(), command.claim().attempt().attempt_id());
    assert_eq!(command.executable(), record.active_intent().executable());
    assert_eq!(command.source(), record.active_intent().source());
    assert_eq!(
        command.compiled_network_plan(),
        record.active_intent().network().compiled_plan()
    );
    assert_eq!(store.counts(), (0, 1));
}

#[tokio::test]
async fn confirmed_replay_inspects_without_execute() {
    let record = current("replay", WorkloadSagaPhase::NetworkReserved);
    let proposal = proposal(&record);
    let (confirmed, store) = confirm(
        &record,
        &proposal,
        Ok(None),
        Ok(WorkloadSagaCommit::Unchanged),
    )
    .await;

    assert_eq!(
        confirmed.confirmation(),
        WorkloadSagaConfirmation::ConfirmedReplay
    );
    assert_eq!(
        confirmed.command().expect("replay should inspect").mode(),
        WorkloadProvisionCommandMode::Inspect
    );
    assert_eq!(store.counts(), (0, 2));
}

#[tokio::test]
async fn ambiguous_cas_confirmation_inspects_without_execute() {
    let record = current("ambiguous-confirmed", WorkloadSagaPhase::WorkloadPrepared);
    let proposal = proposal(&record);
    let candidate = proposal.candidate().clone();
    let store = TestStore::sequenced(
        vec![Ok(Some(candidate))],
        vec![
            Err(WorkloadSagaStoreError::Ambiguous),
            Ok(WorkloadSagaCommit::Applied),
        ],
    );
    let coordinator = WorkloadSagaCoordinator::new(store.clone());
    let confirmed = coordinator
        .confirm_provision_transition(&record, &proposal)
        .await
        .expect("confirmed ambiguous claim should durably enter inspection");

    assert_eq!(
        confirmed.confirmation(),
        WorkloadSagaConfirmation::AppliedByThisCall
    );
    assert_eq!(
        confirmed
            .command()
            .expect("confirmed candidate should inspect")
            .mode(),
        WorkloadProvisionCommandMode::Inspect
    );
    assert_eq!(store.counts(), (1, 2));
}

#[tokio::test]
async fn unresolved_cas_ambiguity_emits_no_command() {
    let record = current("ambiguous-unresolved", WorkloadSagaPhase::WorkloadPrepared);
    let proposal = proposal(&record);
    let (confirmed, store) = confirm(
        &record,
        &proposal,
        Ok(Some(record.clone())),
        Err(WorkloadSagaStoreError::Ambiguous),
    )
    .await;

    assert_eq!(
        confirmed.confirmation(),
        WorkloadSagaConfirmation::UnresolvedAmbiguity
    );
    assert!(confirmed.command().is_none());
    assert!(confirmed.confirmed_record().is_none());
    assert_eq!(store.counts(), (1, 1));
}

#[tokio::test]
async fn conflict_exposes_no_candidate_record_or_command() {
    let record = current("conflict-no-candidate", WorkloadSagaPhase::WorkloadPrepared);
    let proposal = proposal(&record);
    let conflict = WorkloadSagaStoreError::Conflict {
        expected: WorkloadSagaExpected::Revision(record.revision()),
        observed: Some(
            record
                .revision()
                .checked_next()
                .expect("fixture revision should advance"),
        ),
    };
    let (confirmed, store) = confirm(&record, &proposal, Ok(None), Err(conflict)).await;

    assert!(matches!(
        confirmed.confirmation(),
        WorkloadSagaConfirmation::Conflict { .. }
    ));
    assert!(confirmed.confirmed_record().is_none());
    assert!(confirmed.command().is_none());
    assert_eq!(store.counts(), (0, 1));
}

#[tokio::test]
async fn one_fresh_read_after_ambiguous_command_cas() {
    let record = current("one-fresh-read", WorkloadSagaPhase::WorkloadPrepared);
    let proposal = proposal(&record);
    let candidate = proposal.candidate().clone();
    let store = TestStore::sequenced(
        vec![Ok(Some(candidate))],
        vec![
            Err(WorkloadSagaStoreError::Ambiguous),
            Ok(WorkloadSagaCommit::Applied),
        ],
    );
    let coordinator = WorkloadSagaCoordinator::new(store.clone());
    let confirmed = coordinator
        .confirm_provision_transition(&record, &proposal)
        .await
        .expect("confirmed ambiguous claim should durably enter inspection");
    assert_eq!(
        confirmed.confirmation(),
        WorkloadSagaConfirmation::AppliedByThisCall
    );
    assert_eq!(store.counts(), (1, 2));
}

#[tokio::test]
async fn ambiguous_successor_cas_reads_before_later_decision() {
    let record = current("ambiguous-before-later", WorkloadSagaPhase::NetworkReserved);
    let proposal = proposal(&record);
    let (confirmed, store) = confirm(
        &record,
        &proposal,
        Ok(Some(record.clone())),
        Err(WorkloadSagaStoreError::Ambiguous),
    )
    .await;
    assert_eq!(store.counts(), (1, 1));
    assert!(confirmed.command().is_none());
    assert_eq!(
        WorkloadProvisionDecision::plan(&record).expect("old record remains decidable"),
        WorkloadProvisionDecision::Proposed(proposal)
    );
}

#[test]
fn unconfirmed_candidate_cannot_form_provider_command() {
    let record = current("unconfirmed", WorkloadSagaPhase::IntentCommitted);
    let proposal = proposal(&record);
    assert_eq!(
        proposal.action_after_confirmation(),
        Some(WorkloadProvisionSymbolicAction::StartExactAttempt)
    );
    assert!(matches!(
        proposal.candidate().provision_disposition(),
        Some(WorkloadProvisionDisposition::DispatchPending(_))
    ));
}

#[tokio::test]
async fn inspection_absence_authorizes_same_attempt_next_epoch() {
    let record = current("absence-retry", WorkloadSagaPhase::NetworkReserved);
    let proposal = proposal(&record);
    let (confirmed, _) = confirm(
        &record,
        &proposal,
        Ok(None),
        Ok(WorkloadSagaCommit::Unchanged),
    )
    .await;
    let command = confirmed.command().expect("replay should inspect");
    let absence = command.absence_evidence(WorkloadOwnerEvidenceDigest::sha256("absent"));
    let result = WorkloadProvisionCommandResult::for_command(
        command,
        WorkloadProvisionInspectionResult::Absent { evidence: absence },
    )
    .expect("exact inspection absence should correlate");
    let WorkloadProvisionDecision::Proposed(retry) =
        reduce_command_result(confirmed_record(&confirmed), command, result)
            .expect("exact absence should authorize a retry")
    else {
        panic!("absence should produce one retry proposal");
    };
    let retry_claim = retry
        .candidate()
        .provision_disposition()
        .and_then(WorkloadProvisionDisposition::claim)
        .expect("retry should retain one claim");

    assert_eq!(retry_claim.attempt().attempt_id(), command.attempt_id());
    assert_eq!(
        retry_claim.dispatch_epoch(),
        command
            .dispatch_epoch()
            .checked_next()
            .expect("fixture epoch should advance")
    );
}

#[tokio::test]
async fn absence_retry_increments_dispatch_epoch_exactly_once() {
    let record = current("epoch-once", WorkloadSagaPhase::NetworkReserved);
    let proposal = proposal(&record);
    let (confirmed, _) = confirm(
        &record,
        &proposal,
        Ok(None),
        Ok(WorkloadSagaCommit::Unchanged),
    )
    .await;
    let command = confirmed.command().expect("replay should inspect");
    let result = WorkloadProvisionCommandResult::for_command(
        command,
        WorkloadProvisionInspectionResult::Absent {
            evidence: command
                .absence_evidence(WorkloadOwnerEvidenceDigest::sha256("epoch-once-absent")),
        },
    )
    .expect("absence should correlate");
    let WorkloadProvisionDecision::Proposed(retry) =
        reduce_command_result(confirmed_record(&confirmed), command, result)
            .expect("absence should reduce to retry")
    else {
        panic!("absence should propose retry");
    };
    let retry_claim = retry
        .candidate()
        .provision_disposition()
        .and_then(WorkloadProvisionDisposition::claim)
        .expect("retry claim should exist");
    assert_eq!(
        retry_claim.dispatch_epoch().as_u64(),
        command.dispatch_epoch().as_u64() + 1
    );
}

#[tokio::test]
async fn retry_without_absence_evidence_is_rejected() {
    let record = current("no-absence", WorkloadSagaPhase::NetworkReserved);
    let proposal = proposal(&record);
    let (confirmed, _) = confirm(
        &record,
        &proposal,
        Ok(None),
        Ok(WorkloadSagaCommit::Unchanged),
    )
    .await;
    let command = confirmed.command().expect("replay should inspect");
    let result = WorkloadProvisionCommandResult::for_command(command, ambiguous(command))
        .expect("exact ambiguous inspection should correlate");

    assert_eq!(
        reduce_command_result(confirmed_record(&confirmed), command, result)
            .expect("ambiguity should remain inspect-only"),
        WorkloadProvisionDecision::InspectExact(Box::new(command.claim().clone()))
    );
}

#[tokio::test]
async fn in_progress_and_ambiguous_inspection_never_execute_or_retry() {
    let record = current("inspect-nonterminal", WorkloadSagaPhase::NetworkReserved);
    let proposal = proposal(&record);
    let (confirmed, _) = confirm(
        &record,
        &proposal,
        Ok(None),
        Ok(WorkloadSagaCommit::Unchanged),
    )
    .await;
    let command = confirmed.command().expect("replay should inspect");
    let in_progress = WorkloadProvisionCommandResult::for_command(
        command,
        WorkloadProvisionInspectionResult::InProgress {
            attempt_id: command.attempt_id().clone(),
            dispatch_epoch: command.dispatch_epoch(),
            provider_target: command.provider_target().clone(),
            evidence: WorkloadOwnerEvidenceDigest::sha256("still-in-progress"),
        },
    )
    .expect("in-progress observation should correlate");
    let ambiguous = WorkloadProvisionCommandResult::for_command(command, ambiguous(command))
        .expect("ambiguous observation should correlate");

    for result in [in_progress, ambiguous] {
        assert_eq!(
            reduce_command_result(confirmed_record(&confirmed), command, result)
                .expect("nonterminal observation should remain inspect-only"),
            WorkloadProvisionDecision::InspectExact(Box::new(command.claim().clone()))
        );
    }
}

#[tokio::test]
async fn absence_from_execute_mode_is_rejected() {
    let record = current("execute-absence", WorkloadSagaPhase::NetworkReserved);
    let proposal = proposal(&record);
    let (confirmed, _) = confirm(
        &record,
        &proposal,
        Ok(None),
        Ok(WorkloadSagaCommit::Applied),
    )
    .await;
    let command = confirmed.command().expect("direct winner should execute");
    assert_eq!(command.mode(), WorkloadProvisionCommandMode::Execute);
    assert!(
        WorkloadProvisionCommandResult::for_command(
            command,
            WorkloadProvisionInspectionResult::Absent {
                evidence: command
                    .absence_evidence(WorkloadOwnerEvidenceDigest::sha256("invalid-absence")),
            },
        )
        .is_err()
    );
}

#[tokio::test]
async fn absence_for_inspection_only_step_is_rejected() {
    let record = current(
        "inspection-step-absence",
        WorkloadSagaPhase::NetworkAttached,
    );
    let proposal = proposal(&record);
    let (confirmed, _) = confirm(
        &record,
        &proposal,
        Ok(None),
        Ok(WorkloadSagaCommit::Unchanged),
    )
    .await;
    let command = confirmed.command().expect("replay should inspect");
    assert_eq!(
        command.step(),
        WorkloadProvisionStep::InspectActivationPrerequisites
    );
    assert!(
        WorkloadProvisionCommandResult::for_command(
            command,
            WorkloadProvisionInspectionResult::Absent {
                evidence: command
                    .absence_evidence(WorkloadOwnerEvidenceDigest::sha256("invalid-absence")),
            },
        )
        .is_err()
    );
}

#[tokio::test]
async fn fresh_recovery_of_pending_or_inspection_state_inspects_only() {
    let record = current("fresh-recovery", WorkloadSagaPhase::WorkloadPrepared);
    let pending = proposal(&record).into_candidate();
    let store = TestStore::new(Ok(Some(pending.clone())), Ok(WorkloadSagaCommit::Applied));
    let coordinator = WorkloadSagaCoordinator::new(store.clone());
    let pending_confirmation = coordinator
        .inspect_confirmed_provision(pending.key())
        .await
        .expect("fresh recovery should inspect pending state");
    let pending_command = pending_confirmation
        .command()
        .expect("durable pending state should create an inspection command");
    assert_eq!(
        pending_command.mode(),
        WorkloadProvisionCommandMode::Inspect
    );
    assert_eq!(store.counts(), (1, 1));

    let inspection = pending
        .dispatch_to_inspection()
        .expect("fixture ambiguity should persist inspection state");
    let store = TestStore::new(
        Ok(Some(inspection.clone())),
        Ok(WorkloadSagaCommit::Applied),
    );
    let coordinator = WorkloadSagaCoordinator::new(store.clone());
    let inspection_confirmation = coordinator
        .inspect_confirmed_provision(inspection.key())
        .await
        .expect("fresh recovery should inspect inspection state");
    let inspection_command = inspection_confirmation
        .command()
        .expect("durable inspection state should create an inspection command");
    assert_eq!(
        inspection_command.mode(),
        WorkloadProvisionCommandMode::Inspect
    );
    assert_eq!(store.counts(), (1, 0));
}

#[tokio::test]
async fn unconfirmed_recovery_candidate_cannot_form_provider_command() {
    let record = current("unconfirmed-recovery", WorkloadSagaPhase::WorkloadPrepared);
    let candidate = proposal(&record).into_candidate();
    assert!(matches!(
        candidate.provision_disposition(),
        Some(WorkloadProvisionDisposition::DispatchPending(_))
    ));

    let store = TestStore::new(Ok(Some(record.clone())), Ok(WorkloadSagaCommit::Applied));
    let coordinator = WorkloadSagaCoordinator::new(store.clone());
    assert!(
        coordinator
            .inspect_confirmed_provision(candidate.key())
            .await
            .is_err(),
        "recovery must load durable truth instead of trusting an in-memory candidate"
    );
    assert_eq!(store.counts(), (1, 0));
}

#[tokio::test]
async fn every_phase_mode_and_command_result_is_exhaustive() {
    for phase in [
        WorkloadSagaPhase::IntentCommitted,
        WorkloadSagaPhase::NetworkReserved,
        WorkloadSagaPhase::WorkloadPrepared,
        WorkloadSagaPhase::NetworkAttached,
        WorkloadSagaPhase::WorkloadActivated,
        WorkloadSagaPhase::Ready,
        WorkloadSagaPhase::Published,
    ] {
        let record = current(&format!("phase-{phase:?}"), phase);
        let proposal = proposal(&record);
        let (confirmed, _) = confirm(
            &record,
            &proposal,
            Ok(None),
            Ok(WorkloadSagaCommit::Applied),
        )
        .await;
        let command = confirmed.command().expect("effectful phase should command");
        let expected_mode = match command.step() {
            WorkloadProvisionStep::InspectActivationPrerequisites
            | WorkloadProvisionStep::InspectWorkloadReadiness
            | WorkloadProvisionStep::ObservePublication => WorkloadProvisionCommandMode::Inspect,
            _ => WorkloadProvisionCommandMode::Execute,
        };
        assert_eq!(command.mode(), expected_mode);

        let ambiguous = WorkloadProvisionCommandResult::for_command(command, ambiguous(command))
            .expect("every phase accepts exact ambiguity");
        let reduced = reduce_command_result(confirmed_record(&confirmed), command, ambiguous)
            .expect("ambiguity should reduce");
        if expected_mode == WorkloadProvisionCommandMode::Inspect {
            assert_eq!(
                reduced,
                WorkloadProvisionDecision::InspectExact(Box::new(command.claim().clone()))
            );
        } else {
            assert!(matches!(reduced, WorkloadProvisionDecision::Proposed(_)));
        }
    }
}

#[tokio::test]
async fn crossed_command_result_rejects_before_successor_cas() {
    let first = current("crossed-first", WorkloadSagaPhase::NetworkReserved);
    let second = current("crossed-second", WorkloadSagaPhase::NetworkReserved);
    let first_proposal = proposal(&first);
    let second_proposal = proposal(&second);
    let (first_confirmed, _) = confirm(
        &first,
        &first_proposal,
        Ok(None),
        Ok(WorkloadSagaCommit::Applied),
    )
    .await;
    let (second_confirmed, _) = confirm(
        &second,
        &second_proposal,
        Ok(None),
        Ok(WorkloadSagaCommit::Applied),
    )
    .await;
    let first_command = first_confirmed
        .command()
        .expect("first command should exist");
    let second_command = second_confirmed
        .command()
        .expect("second command should exist");
    assert!(
        WorkloadProvisionCommandResult::for_command(first_command, ambiguous(second_command))
            .is_err()
    );
}

#[tokio::test]
async fn inspection_definite_failure_is_correlated_and_terminal() {
    let record = current("inspection-failure", WorkloadSagaPhase::NetworkReserved);
    let proposal = proposal(&record);
    let (confirmed, _) = confirm(
        &record,
        &proposal,
        Ok(None),
        Ok(WorkloadSagaCommit::Unchanged),
    )
    .await;
    let command = confirmed
        .command()
        .expect("inspection command should exist");
    let failure = WorkloadFailureEvidence::new(
        "provider_failed",
        WorkloadOwnerEvidenceDigest::sha256("provider-failed"),
    )
    .expect("failure evidence should validate");
    let result = WorkloadProvisionCommandResult::for_command(
        command,
        WorkloadProvisionInspectionResult::DefiniteFailure {
            attempt_id: command.attempt_id().clone(),
            dispatch_epoch: command.dispatch_epoch(),
            provider_target: command.provider_target().clone(),
            failure,
        },
    )
    .expect("failure should correlate");
    assert!(matches!(
        reduce_command_result(confirmed_record(&confirmed), command, result)
            .expect("failure should reduce"),
        WorkloadProvisionDecision::Proposed(_)
    ));
}
