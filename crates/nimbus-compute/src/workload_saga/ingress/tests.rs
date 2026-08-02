use std::collections::VecDeque;
use std::future::pending;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nimbus_core::{TenantId, WorkloadId};
use nimbus_network::{
    NetworkAttachmentCapabilitySet, NetworkCapabilityRequirements, NetworkControlPlaneLocality,
    NetworkEndpointCapabilitySet, NetworkForwardingCapabilitySet, NetworkIngressCapabilitySet,
    NetworkLifecycleCapabilitySet, NetworkManagementMode, NetworkResourceGeneration,
    NetworkSovereigntyRequirements,
};
use nimbus_workloads::{
    CompiledWorkloadNetworkPlan, DesiredWorkloadKind, DesiredWorkloadState, NodeIdentity,
    WorkloadActivationIntent, WorkloadAdmissionEvidence, WorkloadDesiredDigest,
    WorkloadEffectReferences, WorkloadGeneration, WorkloadNetworkIntent,
    WorkloadNetworkPlanContent, WorkloadNetworkPlanIdentity, WorkloadOwnerEvidenceDigest,
    WorkloadOwnerObservation, WorkloadPhaseDetail, WorkloadPublicationIntent, WorkloadSagaCommit,
    WorkloadSagaError, WorkloadSagaExpected, WorkloadSagaFuture, WorkloadSagaIntent,
    WorkloadSagaIntentUpdate, WorkloadSagaKey, WorkloadSagaPage, WorkloadSagaPageRequest,
    WorkloadSagaPhase, WorkloadSagaRecord, WorkloadSagaStore, WorkloadSagaStoreError,
    WorkloadSagaTenantPage, WorkloadSagaTenantPageRequest,
};
use tokio::sync::Notify;

use super::{ConfirmedWorkloadSagaIntent, WorkloadSagaCoordinator, WorkloadSagaIngressDisposition};
use crate::workload_saga::WorkloadSagaAction;

#[derive(Debug, Clone, PartialEq, Eq)]
enum StoreCall {
    Load(WorkloadSagaKey),
    CompareAndSwap(WorkloadSagaExpected, Box<WorkloadSagaRecord>),
}

fn compare_and_swap_call(expected: WorkloadSagaExpected, next: WorkloadSagaRecord) -> StoreCall {
    StoreCall::CompareAndSwap(expected, Box::new(next))
}

struct ScriptedStore {
    loads: Mutex<VecDeque<Result<Option<WorkloadSagaRecord>, WorkloadSagaStoreError>>>,
    commits: Mutex<VecDeque<Result<WorkloadSagaCommit, WorkloadSagaStoreError>>>,
    calls: Mutex<Vec<StoreCall>>,
}

impl ScriptedStore {
    fn new(
        loads: impl IntoIterator<Item = Result<Option<WorkloadSagaRecord>, WorkloadSagaStoreError>>,
        commits: impl IntoIterator<Item = Result<WorkloadSagaCommit, WorkloadSagaStoreError>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            loads: Mutex::new(loads.into_iter().collect()),
            commits: Mutex::new(commits.into_iter().collect()),
            calls: Mutex::new(Vec::new()),
        })
    }

    fn calls(&self) -> Vec<StoreCall> {
        self.calls.lock().expect("call log lock is healthy").clone()
    }
}

impl WorkloadSagaStore for ScriptedStore {
    fn load<'a>(
        &'a self,
        key: &'a WorkloadSagaKey,
    ) -> WorkloadSagaFuture<'a, Option<WorkloadSagaRecord>> {
        Box::pin(async move {
            self.calls
                .lock()
                .expect("call log lock is healthy")
                .push(StoreCall::Load(key.clone()));
            self.loads
                .lock()
                .expect("load queue lock is healthy")
                .pop_front()
                .expect("test must script every load")
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
                .expect("call log lock is healthy")
                .push(compare_and_swap_call(expected, next));
            self.commits
                .lock()
                .expect("commit queue lock is healthy")
                .pop_front()
                .expect("test must script every commit")
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

fn tenant(label: &str) -> TenantId {
    TenantId::new(format!("tenant-{label}")).expect("fixture tenant is valid")
}

fn key(label: &str) -> WorkloadSagaKey {
    WorkloadSagaKey::new(
        tenant(label),
        WorkloadId::new(format!("workload-{label}")).expect("fixture workload is valid"),
    )
}

fn compiled_plan(
    tenant_id: &TenantId,
    label: &str,
    generation: u64,
    activation: WorkloadActivationIntent,
) -> CompiledWorkloadNetworkPlan {
    let identity = WorkloadNetworkPlanIdentity::new(
        tenant_id.clone(),
        format!("fixture-{label}"),
        NetworkResourceGeneration::new(generation),
    )
    .expect("fixture network identity is valid");
    let requirements = NetworkCapabilityRequirements::new(
        NetworkAttachmentCapabilitySet::new(NetworkManagementMode::NimbusHostManaged, [], []),
        NetworkEndpointCapabilitySet::new([], [], [], [], []),
        NetworkIngressCapabilitySet::new([]),
        NetworkForwardingCapabilitySet::new([]),
        NetworkLifecycleCapabilitySet::new([]),
        NetworkSovereigntyRequirements::new(NetworkControlPlaneLocality::LocalOnly, [], true),
    );
    let content = WorkloadNetworkPlanContent::new(
        identity,
        requirements,
        None,
        None,
        [],
        [],
        [],
        activation,
        WorkloadPublicationIntent::Withheld,
    )
    .expect("fixture network content is valid");
    CompiledWorkloadNetworkPlan::from_content(content)
        .expect("fixture compiled network plan is valid")
}

fn intent(
    label: &str,
    generation: u64,
    desired_state: DesiredWorkloadState,
    seed: u8,
) -> WorkloadSagaIntent {
    let tenant_id = tenant(label);
    let activation = if desired_state == DesiredWorkloadState::Running {
        WorkloadActivationIntent::ActivateWhenAttached
    } else {
        WorkloadActivationIntent::PrepareOnly
    };
    WorkloadSagaIntent::new(
        DesiredWorkloadKind::Sandbox,
        desired_state,
        WorkloadGeneration::new(generation),
        WorkloadDesiredDigest::sha256([seed, 1]),
        WorkloadNetworkIntent::new(compiled_plan(&tenant_id, label, generation, activation)),
        activation,
        WorkloadPublicationIntent::Withheld,
        WorkloadAdmissionEvidence::new(
            format!("tid_{}", format!("{seed:02x}").repeat(32))
                .try_into()
                .expect("fixture decision id is valid"),
            format!("twu_{}", format!("{:02x}", seed.wrapping_add(1)).repeat(32))
                .try_into()
                .expect("fixture workload uid is valid"),
            Some(
                NodeIdentity::new(format!("node-{label}-{generation}"))
                    .expect("fixture node is valid"),
            ),
        ),
    )
    .expect("fixture intent is valid")
}

fn running_intent(label: &str, generation: u64, seed: u8) -> WorkloadSagaIntent {
    intent(label, generation, DesiredWorkloadState::Running, seed)
}

fn stopped_intent(label: &str, generation: u64, seed: u8) -> WorkloadSagaIntent {
    intent(label, generation, DesiredWorkloadState::Stopped, seed)
}

fn evidence(label: &str) -> WorkloadOwnerEvidenceDigest {
    WorkloadOwnerEvidenceDigest::sha256(label)
}

fn provision_observations(
    phase: WorkloadSagaPhase,
    references: &WorkloadEffectReferences,
) -> Vec<WorkloadOwnerObservation> {
    let network = references.network().expect("network is retained").clone();
    let execution = references
        .execution()
        .expect("execution is retained")
        .clone();
    let rank = match phase {
        WorkloadSagaPhase::NetworkReserved => 1,
        WorkloadSagaPhase::WorkloadPrepared => 2,
        WorkloadSagaPhase::NetworkAttached => 3,
        WorkloadSagaPhase::WorkloadActivated => 4,
        WorkloadSagaPhase::Ready | WorkloadSagaPhase::Observed => 5,
        _ => panic!("phase has no provision observations"),
    };
    let mut observations = Vec::new();
    if rank >= 1 {
        observations.push(WorkloadOwnerObservation::NetworkReserved {
            reference: network.clone(),
            evidence: evidence("network-reserved"),
        });
    }
    if rank >= 2 {
        observations.push(WorkloadOwnerObservation::ExecutionPrepared {
            reference: execution.clone(),
            evidence: evidence("execution-prepared"),
        });
    }
    if rank >= 3 {
        observations.push(WorkloadOwnerObservation::NetworkAttached {
            reference: network.clone(),
            evidence: evidence("network-attached"),
        });
    }
    if rank >= 4 {
        observations.push(WorkloadOwnerObservation::ExecutionActivated {
            reference: execution.clone(),
            evidence: evidence("execution-activated"),
        });
    }
    if rank >= 5 {
        observations.push(WorkloadOwnerObservation::Ready {
            network,
            execution,
            evidence: evidence("ready"),
        });
    }
    observations
}

fn advance_provision(record: &WorkloadSagaRecord, phase: WorkloadSagaPhase) -> WorkloadSagaRecord {
    let references = WorkloadEffectReferences::provision(record.active_intent(), None)
        .expect("fixture provision references are valid");
    let observations = provision_observations(phase, &references);
    let detail =
        WorkloadPhaseDetail::provision(phase, record.active_intent(), references, observations)
            .expect("fixture phase detail is valid");
    record
        .advance(phase, detail, None)
        .expect("fixture provision transition is valid")
}

fn provision_record(label: &str, target: WorkloadSagaPhase) -> WorkloadSagaRecord {
    let mut record = WorkloadSagaRecord::new(key(label), running_intent(label, 1, 11))
        .expect("fixture record is valid");
    if target == WorkloadSagaPhase::IntentCommitted {
        return record;
    }
    for phase in [
        WorkloadSagaPhase::NetworkReserved,
        WorkloadSagaPhase::WorkloadPrepared,
        WorkloadSagaPhase::NetworkAttached,
        WorkloadSagaPhase::WorkloadActivated,
        WorkloadSagaPhase::Ready,
        WorkloadSagaPhase::Observed,
    ] {
        record = advance_provision(&record, phase);
        if phase == target {
            return record;
        }
    }
    panic!("unsupported provision target {target:?}")
}

fn transition_for(
    current: &WorkloadSagaRecord,
    candidate: WorkloadSagaIntent,
) -> WorkloadSagaRecord {
    let WorkloadSagaIntentUpdate::Transition(next) = current
        .apply_intent(candidate)
        .expect("fixture intent transition is valid")
    else {
        panic!("fixture candidate must transition")
    };
    *next
}

fn assert_exact_result(result: &ConfirmedWorkloadSagaIntent, record: &WorkloadSagaRecord) {
    assert_eq!(result.record(), record);
    assert_eq!(
        result.decision(),
        &crate::workload_saga::WorkloadSagaDecision::for_record(record)
            .expect("fixture record has a deterministic decision")
    );
    assert_eq!(result.decision().key(), record.key());
    assert_eq!(result.decision().saga_id(), record.saga_id());
    assert_eq!(result.decision().revision(), record.revision());
    assert_eq!(
        result.decision().active_generation(),
        record.active_intent().generation()
    );
}

#[tokio::test]
async fn missing_intent_is_confirmed_before_decision() {
    let key = key("missing");
    let intent = running_intent("missing", 1, 11);
    let expected = WorkloadSagaRecord::new(key.clone(), intent.clone()).unwrap();
    let store = ScriptedStore::new([Ok(None)], [Ok(WorkloadSagaCommit::Applied)]);
    let coordinator = WorkloadSagaCoordinator::new(store.clone());

    let result = coordinator
        .submit_intent(key.clone(), intent)
        .await
        .expect("missing intent should be durably confirmed");

    assert_exact_result(&result, &expected);
    assert_eq!(
        result.disposition(),
        WorkloadSagaIngressDisposition::Applied
    );
    assert!(matches!(
        result.decision().action(),
        WorkloadSagaAction::ReserveNetwork { plan, .. }
            if plan == expected.active_intent().network().compiled_plan()
    ));
    assert_eq!(
        store.calls(),
        vec![
            StoreCall::Load(key),
            compare_and_swap_call(WorkloadSagaExpected::Missing, expected),
        ]
    );
}

#[tokio::test]
async fn exact_replay_performs_zero_cas() {
    let current = provision_record("replay", WorkloadSagaPhase::Ready);
    let store = ScriptedStore::new([Ok(Some(current.clone()))], []);
    let coordinator = WorkloadSagaCoordinator::new(store.clone());

    let result = coordinator
        .submit_intent(current.key().clone(), current.active_intent().clone())
        .await
        .expect("exact replay should be confirmed without a write");

    assert_exact_result(&result, &current);
    assert_eq!(
        result.disposition(),
        WorkloadSagaIngressDisposition::ConfirmedReplay
    );
    assert_eq!(store.calls(), vec![StoreCall::Load(current.key().clone())]);
}

#[tokio::test]
async fn successor_withdraws_before_reservation() {
    for phase in [
        WorkloadSagaPhase::IntentCommitted,
        WorkloadSagaPhase::NetworkReserved,
        WorkloadSagaPhase::WorkloadPrepared,
        WorkloadSagaPhase::NetworkAttached,
        WorkloadSagaPhase::WorkloadActivated,
        WorkloadSagaPhase::Ready,
        WorkloadSagaPhase::Observed,
    ] {
        let label = format!("successor-{phase:?}").to_ascii_lowercase();
        let current = provision_record(&label, phase);
        let successor = stopped_intent(&label, 2, 42);
        let expected = transition_for(&current, successor.clone());
        let store = ScriptedStore::new(
            [Ok(Some(current.clone()))],
            [Ok(WorkloadSagaCommit::Applied)],
        );
        let coordinator = WorkloadSagaCoordinator::new(store.clone());

        let result = coordinator
            .submit_intent(current.key().clone(), successor.clone())
            .await
            .unwrap_or_else(|error| panic!("{phase:?} successor failed: {error}"));

        assert_exact_result(&result, &expected);
        assert_eq!(
            result.record().phase(),
            WorkloadSagaPhase::WithdrawalCommitted
        );
        assert_eq!(result.record().active_intent(), current.active_intent());
        assert_eq!(result.record().successor_intent(), Some(&successor));
        assert!(!matches!(
            result.decision().action(),
            WorkloadSagaAction::ReserveNetwork { .. }
        ));
        assert_eq!(
            store.calls(),
            vec![
                StoreCall::Load(current.key().clone()),
                compare_and_swap_call(WorkloadSagaExpected::Revision(current.revision()), expected,),
            ]
        );
    }
}

#[tokio::test]
async fn conflict_is_not_retried() {
    let current = provision_record("conflict", WorkloadSagaPhase::Ready);
    let successor = stopped_intent("conflict", 2, 42);
    let expected = transition_for(&current, successor.clone());
    let conflict = WorkloadSagaStoreError::Conflict {
        expected: WorkloadSagaExpected::Revision(current.revision()),
        observed: Some(expected.revision()),
    };
    let store = ScriptedStore::new([Ok(Some(current.clone()))], [Err(conflict.clone())]);
    let coordinator = WorkloadSagaCoordinator::new(store.clone());

    assert_eq!(
        coordinator
            .submit_intent(current.key().clone(), successor)
            .await,
        Err(conflict)
    );
    assert_eq!(
        store.calls(),
        vec![
            StoreCall::Load(current.key().clone()),
            compare_and_swap_call(WorkloadSagaExpected::Revision(current.revision()), expected,),
        ]
    );
}

#[tokio::test]
async fn ambiguous_exact_next_uses_one_fresh_read() {
    let current = provision_record("ambiguous", WorkloadSagaPhase::Ready);
    let successor = stopped_intent("ambiguous", 2, 42);
    let next = transition_for(&current, successor.clone());
    let store = ScriptedStore::new(
        [Ok(Some(current.clone())), Ok(Some(next.clone()))],
        [Err(WorkloadSagaStoreError::Ambiguous)],
    );
    let coordinator = WorkloadSagaCoordinator::new(store.clone());

    let result = coordinator
        .submit_intent(current.key().clone(), successor)
        .await
        .expect("fresh exact truth should confirm the transition");

    assert_exact_result(&result, &next);
    assert_eq!(
        result.disposition(),
        WorkloadSagaIngressDisposition::Applied
    );
    assert_eq!(
        store.calls(),
        vec![
            StoreCall::Load(current.key().clone()),
            compare_and_swap_call(
                WorkloadSagaExpected::Revision(current.revision()),
                next.clone(),
            ),
            StoreCall::Load(next.key().clone()),
        ]
    );
}

#[tokio::test]
async fn ambiguous_nonconfirming_outcomes_use_one_fresh_read() {
    let current = provision_record("ambiguous-matrix", WorkloadSagaPhase::Ready);
    let successor = stopped_intent("ambiguous-matrix", 2, 42);
    let next = transition_for(&current, successor.clone());
    let expected = WorkloadSagaExpected::Revision(current.revision());
    let expected_calls = |fresh_key: &WorkloadSagaKey| {
        vec![
            StoreCall::Load(current.key().clone()),
            compare_and_swap_call(expected, next.clone()),
            StoreCall::Load(fresh_key.clone()),
        ]
    };

    for observed in [None, Some(current.clone())] {
        let store = ScriptedStore::new(
            [Ok(Some(current.clone())), Ok(observed)],
            [Err(WorkloadSagaStoreError::Ambiguous)],
        );
        let coordinator = WorkloadSagaCoordinator::new(store.clone());

        assert_eq!(
            coordinator
                .submit_intent(current.key().clone(), successor.clone())
                .await,
            Err(WorkloadSagaStoreError::Ambiguous)
        );
        assert_eq!(store.calls(), expected_calls(next.key()));
    }

    let competing = transition_for(&current, running_intent("ambiguous-matrix", 2, 99));
    let store = ScriptedStore::new(
        [Ok(Some(current.clone())), Ok(Some(competing.clone()))],
        [Err(WorkloadSagaStoreError::Ambiguous)],
    );
    let coordinator = WorkloadSagaCoordinator::new(store.clone());
    assert_eq!(
        coordinator
            .submit_intent(current.key().clone(), successor.clone())
            .await,
        Err(WorkloadSagaStoreError::Conflict {
            expected,
            observed: Some(competing.revision()),
        })
    );
    assert_eq!(store.calls(), expected_calls(next.key()));

    for error in [
        WorkloadSagaStoreError::Unavailable,
        WorkloadSagaStoreError::Corrupt,
    ] {
        let store = ScriptedStore::new(
            [Ok(Some(current.clone())), Err(error.clone())],
            [Err(WorkloadSagaStoreError::Ambiguous)],
        );
        let coordinator = WorkloadSagaCoordinator::new(store.clone());

        assert_eq!(
            coordinator
                .submit_intent(current.key().clone(), successor.clone())
                .await,
            Err(error)
        );
        assert_eq!(store.calls(), expected_calls(next.key()));
    }
}

#[tokio::test]
async fn crossed_loaded_key_is_corrupt() {
    let requested = key("requested");
    let crossed = provision_record("crossed", WorkloadSagaPhase::Ready);
    let store = ScriptedStore::new([Ok(Some(crossed))], []);
    let coordinator = WorkloadSagaCoordinator::new(store.clone());

    assert_eq!(
        coordinator
            .submit_intent(requested.clone(), running_intent("requested", 1, 11))
            .await,
        Err(WorkloadSagaStoreError::Corrupt)
    );
    assert_eq!(store.calls(), vec![StoreCall::Load(requested)]);

    let current = provision_record("crossed-fresh", WorkloadSagaPhase::Ready);
    let successor = stopped_intent("crossed-fresh", 2, 42);
    let next = transition_for(&current, successor.clone());
    let crossed_fresh = provision_record("other-fresh", WorkloadSagaPhase::Ready);
    let store = ScriptedStore::new(
        [Ok(Some(current.clone())), Ok(Some(crossed_fresh))],
        [Err(WorkloadSagaStoreError::Ambiguous)],
    );
    let coordinator = WorkloadSagaCoordinator::new(store.clone());

    assert_eq!(
        coordinator
            .submit_intent(current.key().clone(), successor)
            .await,
        Err(WorkloadSagaStoreError::Corrupt)
    );
    assert_eq!(
        store.calls(),
        vec![
            StoreCall::Load(current.key().clone()),
            compare_and_swap_call(
                WorkloadSagaExpected::Revision(current.revision()),
                next.clone(),
            ),
            StoreCall::Load(next.key().clone()),
        ]
    );
}

#[tokio::test]
async fn negative_outcomes_expose_no_confirmed_decision() {
    let current = provision_record("negative", WorkloadSagaPhase::Ready);
    for (candidate, expected) in [
        (
            running_intent("negative", 0, 7),
            WorkloadSagaStoreError::InvalidTransition(WorkloadSagaError::StaleGeneration {
                current: WorkloadGeneration::new(1),
                candidate: WorkloadGeneration::new(0),
            }),
        ),
        (
            running_intent("negative", 1, 99),
            WorkloadSagaStoreError::InvalidTransition(WorkloadSagaError::EqualGenerationConflict(
                WorkloadGeneration::new(1),
            )),
        ),
    ] {
        let store = ScriptedStore::new([Ok(Some(current.clone()))], []);
        let coordinator = WorkloadSagaCoordinator::new(store.clone());
        assert_eq!(
            coordinator
                .submit_intent(current.key().clone(), candidate)
                .await,
            Err(expected)
        );
        assert_eq!(store.calls(), vec![StoreCall::Load(current.key().clone())]);
    }

    for error in [
        WorkloadSagaStoreError::Unavailable,
        WorkloadSagaStoreError::Corrupt,
    ] {
        let store = ScriptedStore::new([Err(error.clone())], []);
        let coordinator = WorkloadSagaCoordinator::new(store.clone());
        assert_eq!(
            coordinator
                .submit_intent(current.key().clone(), current.active_intent().clone())
                .await,
            Err(error)
        );
        assert_eq!(store.calls(), vec![StoreCall::Load(current.key().clone())]);
    }

    let requested = key("invalid-intent");
    let store = ScriptedStore::new([Ok(None)], []);
    let coordinator = WorkloadSagaCoordinator::new(store.clone());
    assert_eq!(
        coordinator
            .submit_intent(requested.clone(), running_intent("other-tenant", 1, 11))
            .await,
        Err(WorkloadSagaStoreError::InvalidTransition(
            WorkloadSagaError::InvalidIntent(
                "active network plan tenant must match workload saga tenant",
            ),
        ))
    );
    assert_eq!(store.calls(), vec![StoreCall::Load(requested)]);
}

#[tokio::test]
async fn stopped_and_terminal_successor_semantics_are_preserved() {
    let stopped = stopped_intent("terminal", 1, 41);
    let stopped_record = WorkloadSagaRecord::new(key("terminal"), stopped.clone()).unwrap();
    let store = ScriptedStore::new([Ok(None)], [Ok(WorkloadSagaCommit::Applied)]);
    let coordinator = WorkloadSagaCoordinator::new(store);
    let result = coordinator
        .submit_intent(key("terminal"), stopped)
        .await
        .expect("missing stopped intent should be durable");
    assert_exact_result(&result, &stopped_record);
    assert_eq!(result.record().phase(), WorkloadSagaPhase::Recorded);
    assert!(matches!(
        result.decision().action(),
        WorkloadSagaAction::Quiescent
    ));

    let successor = running_intent("terminal", 2, 12);
    let expected = transition_for(&stopped_record, successor.clone());
    let store = ScriptedStore::new(
        [Ok(Some(stopped_record.clone()))],
        [Ok(WorkloadSagaCommit::Applied)],
    );
    let coordinator = WorkloadSagaCoordinator::new(store);
    let result = coordinator
        .submit_intent(stopped_record.key().clone(), successor)
        .await
        .expect("terminal higher generation should promote exactly");
    assert_exact_result(&result, &expected);
    assert_eq!(result.record().phase(), WorkloadSagaPhase::IntentCommitted);
}

struct PendingCasStore {
    cas_started: Notify,
    cas_calls: AtomicUsize,
}

impl PendingCasStore {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            cas_started: Notify::new(),
            cas_calls: AtomicUsize::new(0),
        })
    }
}

impl WorkloadSagaStore for PendingCasStore {
    fn load<'a>(
        &'a self,
        _key: &'a WorkloadSagaKey,
    ) -> WorkloadSagaFuture<'a, Option<WorkloadSagaRecord>> {
        Box::pin(async { Ok(None) })
    }

    fn compare_and_swap<'a>(
        &'a self,
        _expected: WorkloadSagaExpected,
        _next: WorkloadSagaRecord,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaCommit> {
        Box::pin(async move {
            self.cas_calls.fetch_add(1, Ordering::SeqCst);
            self.cas_started.notify_one();
            pending().await
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

#[tokio::test]
async fn cancellation_before_commit_exposes_no_decision() {
    let store = PendingCasStore::new();
    let coordinator = Arc::new(WorkloadSagaCoordinator::new(store.clone()));
    let task = tokio::spawn({
        let coordinator = coordinator.clone();
        async move {
            coordinator
                .submit_intent(key("cancel"), running_intent("cancel", 1, 11))
                .await
        }
    });

    tokio::time::timeout(Duration::from_secs(2), store.cas_started.notified())
        .await
        .expect("submission must reach the pending CAS within the bound");
    task.abort();
    assert!(
        task.await
            .expect_err("aborted submission must not return a decision")
            .is_cancelled()
    );
    assert_eq!(store.cas_calls.load(Ordering::SeqCst), 1);
}
