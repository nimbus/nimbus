use std::collections::BTreeMap;
use std::sync::{Mutex, RwLock};

use futures::executor::block_on;
use nimbus_core::{TenantId, WorkloadId};
use nimbus_network::{NetworkPlanDigest, NetworkPlanId, NetworkResourceGeneration};
use nimbus_tenant::TenantIsolationDecisionId;

use super::*;
use crate::{
    DesiredWorkloadKind, DesiredWorkloadState, NodeIdentity, TenantWorkloadUid,
    WorkloadActivationIntent, WorkloadAdmissionEvidence, WorkloadDesiredDigest,
    WorkloadEffectReferences, WorkloadGeneration, WorkloadNetworkIntent,
    WorkloadOwnerEvidenceDigest, WorkloadOwnerObservation, WorkloadPhaseDetail,
    WorkloadPublicationIntent, WorkloadSagaIntent, WorkloadSagaTransitionId,
};

fn require_object_safe_store(_: &dyn WorkloadSagaStore) {}

#[test]
fn store_port_is_object_safe_send_and_sync() {
    fn require_send_sync<T: Send + Sync>() {}

    let _ = require_object_safe_store;
    require_send_sync::<MutexMapStore>();
    require_send_sync::<RwLockAppendLogStore>();
}

#[derive(Default)]
struct MutexMapStore {
    state: Mutex<MapState>,
}

#[derive(Default)]
struct MapState {
    records: BTreeMap<WorkloadSagaKey, WorkloadSagaRecord>,
    transition_claims: BTreeMap<WorkloadSagaTransitionId, WorkloadSagaRecord>,
}

impl WorkloadSagaStore for MutexMapStore {
    fn load<'a>(
        &'a self,
        key: &'a WorkloadSagaKey,
    ) -> WorkloadSagaFuture<'a, Option<WorkloadSagaRecord>> {
        Box::pin(async move {
            let state = self.state.lock().expect("mutex map store lock");
            Ok(state.records.get(key).cloned())
        })
    }

    fn compare_and_swap<'a>(
        &'a self,
        expected: WorkloadSagaExpected,
        next: WorkloadSagaRecord,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaCommit> {
        Box::pin(async move {
            next.validate()?;
            let mut state = self.state.lock().expect("mutex map store lock");
            let current = state.records.get(next.key()).cloned();

            if current.as_ref() == Some(&next) {
                return Ok(WorkloadSagaCommit::Unchanged);
            }
            reject_divergent_transition_claim(
                state
                    .transition_claims
                    .get(next.last_transition().transition_id()),
                &next,
            )?;
            check_expected(expected, current.as_ref().map(WorkloadSagaRecord::revision))?;
            validate_store_successor(current.as_ref(), &next)?;

            state
                .transition_claims
                .insert(next.last_transition().transition_id().clone(), next.clone());
            state.records.insert(next.key().clone(), next);
            Ok(WorkloadSagaCommit::Applied)
        })
    }

    fn list_recoverable<'a>(
        &'a self,
        request: WorkloadSagaPageRequest,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaPage> {
        Box::pin(async move {
            let state = self.state.lock().expect("mutex map store lock");
            build_recovery_page(&request, state.records.values().cloned().collect())
        })
    }
}

#[derive(Default)]
struct RwLockAppendLogStore {
    state: RwLock<AppendLogState>,
}

#[derive(Default)]
struct AppendLogState {
    revisions: Vec<WorkloadSagaRecord>,
    transition_claims: Vec<(WorkloadSagaTransitionId, WorkloadSagaRecord)>,
}

impl AppendLogState {
    fn latest(&self, key: &WorkloadSagaKey) -> Option<&WorkloadSagaRecord> {
        self.revisions
            .iter()
            .rev()
            .find(|record| record.key() == key)
    }

    fn latest_records(&self) -> Vec<WorkloadSagaRecord> {
        let mut latest = Vec::<WorkloadSagaRecord>::new();
        for record in self.revisions.iter().rev() {
            if latest.iter().all(|existing| existing.key() != record.key()) {
                latest.push(record.clone());
            }
        }
        latest
    }
}

impl WorkloadSagaStore for RwLockAppendLogStore {
    fn load<'a>(
        &'a self,
        key: &'a WorkloadSagaKey,
    ) -> WorkloadSagaFuture<'a, Option<WorkloadSagaRecord>> {
        Box::pin(async move {
            let state = self.state.read().expect("append log store read lock");
            Ok(state.latest(key).cloned())
        })
    }

    fn compare_and_swap<'a>(
        &'a self,
        expected: WorkloadSagaExpected,
        next: WorkloadSagaRecord,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaCommit> {
        Box::pin(async move {
            next.validate()?;
            let mut state = self.state.write().expect("append log store write lock");
            let current = state.latest(next.key()).cloned();

            if current.as_ref() == Some(&next) {
                return Ok(WorkloadSagaCommit::Unchanged);
            }
            let claimed = state
                .transition_claims
                .iter()
                .rev()
                .find(|(id, _)| id == next.last_transition().transition_id())
                .map(|(_, record)| record);
            reject_divergent_transition_claim(claimed, &next)?;
            check_expected(expected, current.as_ref().map(WorkloadSagaRecord::revision))?;
            validate_store_successor(current.as_ref(), &next)?;

            state
                .transition_claims
                .push((next.last_transition().transition_id().clone(), next.clone()));
            state.revisions.push(next);
            Ok(WorkloadSagaCommit::Applied)
        })
    }

    fn list_recoverable<'a>(
        &'a self,
        request: WorkloadSagaPageRequest,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaPage> {
        Box::pin(async move {
            let state = self.state.read().expect("append log store read lock");
            build_recovery_page(&request, state.latest_records())
        })
    }
}

trait StoreConformance: WorkloadSagaStore + Default {
    fn inject_divergent_transition_claim(
        &self,
        transition_id: WorkloadSagaTransitionId,
        divergent_record: WorkloadSagaRecord,
    );
}

impl StoreConformance for MutexMapStore {
    fn inject_divergent_transition_claim(
        &self,
        transition_id: WorkloadSagaTransitionId,
        divergent_record: WorkloadSagaRecord,
    ) {
        self.state
            .lock()
            .expect("mutex map store lock")
            .transition_claims
            .insert(transition_id, divergent_record);
    }
}

impl StoreConformance for RwLockAppendLogStore {
    fn inject_divergent_transition_claim(
        &self,
        transition_id: WorkloadSagaTransitionId,
        divergent_record: WorkloadSagaRecord,
    ) {
        self.state
            .write()
            .expect("append log store write lock")
            .transition_claims
            .push((transition_id, divergent_record));
    }
}

fn reject_divergent_transition_claim(
    claimed: Option<&WorkloadSagaRecord>,
    next: &WorkloadSagaRecord,
) -> Result<(), WorkloadSagaStoreError> {
    if claimed.is_some_and(|record| record != next) {
        return Err(WorkloadSagaStoreError::InvalidTransition(
            WorkloadSagaError::InvalidTransition(
                "workload saga transition id already names divergent content",
            ),
        ));
    }
    Ok(())
}

fn check_expected(
    expected: WorkloadSagaExpected,
    observed: Option<WorkloadSagaRevision>,
) -> Result<(), WorkloadSagaStoreError> {
    let matches = match expected {
        WorkloadSagaExpected::Missing => observed.is_none(),
        WorkloadSagaExpected::Revision(revision) => observed == Some(revision),
    };
    if matches {
        Ok(())
    } else {
        Err(WorkloadSagaStoreError::Conflict { expected, observed })
    }
}

fn validate_store_successor(
    current: Option<&WorkloadSagaRecord>,
    next: &WorkloadSagaRecord,
) -> Result<(), WorkloadSagaStoreError> {
    if let Some(current) = current {
        current.validate_successor(next)?;
    } else if next.revision() != WorkloadSagaRevision::new(0)
        || next.last_transition().source_phase().is_some()
    {
        return Err(WorkloadSagaStoreError::InvalidTransition(
            WorkloadSagaError::InvalidTransition("a missing saga accepts only an initial revision"),
        ));
    }
    Ok(())
}

fn build_recovery_page(
    request: &WorkloadSagaPageRequest,
    mut records: Vec<WorkloadSagaRecord>,
) -> Result<WorkloadSagaPage, WorkloadSagaStoreError> {
    records.retain(WorkloadSagaRecord::requires_recovery);
    records.sort_by(|left, right| left.recovery_key().cmp(&right.recovery_key()));
    if let Some(after) = request.after() {
        records.retain(|record| {
            WorkloadSagaRecoveryCursor::for_record(record)
                .expect("recoverable record has a cursor")
                .order_key()
                > after.order_key()
        });
    }
    let has_more = records.len() > usize::from(request.limit());
    records.truncate(usize::from(request.limit()));
    WorkloadSagaPage::new(request, records, has_more)
}

#[test]
fn mutex_map_store_satisfies_shared_contract() {
    assert_store_contract::<MutexMapStore>();
}

#[test]
fn rwlock_append_log_store_satisfies_shared_contract() {
    assert_store_contract::<RwLockAppendLogStore>();
}

fn assert_store_contract<S: StoreConformance>() {
    let store = S::default();
    let initial = running_record("cas", WorkloadActivationIntent::ActivateWhenAttached);

    assert_eq!(block_on(store.load(initial.key())).unwrap(), None);
    assert_eq!(
        block_on(store.compare_and_swap(WorkloadSagaExpected::Missing, initial.clone())).unwrap(),
        WorkloadSagaCommit::Applied
    );
    assert_eq!(
        block_on(store.load(initial.key())).unwrap(),
        Some(initial.clone())
    );

    assert_eq!(
        block_on(store.compare_and_swap(
            WorkloadSagaExpected::Revision(WorkloadSagaRevision::new(91)),
            initial.clone(),
        ))
        .unwrap(),
        WorkloadSagaCommit::Unchanged,
        "exact replay must win before expectation-conflict handling"
    );

    let revision_one = advance_to(&initial, WorkloadSagaPhase::NetworkReserved);
    assert_eq!(
        block_on(store.compare_and_swap(
            WorkloadSagaExpected::Revision(initial.revision()),
            revision_one.clone(),
        ))
        .unwrap(),
        WorkloadSagaCommit::Applied
    );
    assert_eq!(
        block_on(store.load(initial.key())).unwrap(),
        Some(revision_one.clone())
    );

    let revision_two = advance_to(&revision_one, WorkloadSagaPhase::WorkloadPrepared);
    assert_conflict(
        block_on(store.compare_and_swap(WorkloadSagaExpected::Missing, revision_two.clone()))
            .unwrap_err(),
        WorkloadSagaExpected::Missing,
        Some(revision_one.revision()),
    );
    assert_conflict(
        block_on(store.compare_and_swap(
            WorkloadSagaExpected::Revision(initial.revision()),
            revision_two.clone(),
        ))
        .unwrap_err(),
        WorkloadSagaExpected::Revision(initial.revision()),
        Some(revision_one.revision()),
    );
    assert_eq!(
        block_on(store.load(initial.key())).unwrap(),
        Some(revision_one.clone()),
        "conflicts must not write"
    );

    let invalid = initial.clone();
    assert!(matches!(
        block_on(store.compare_and_swap(
            WorkloadSagaExpected::Revision(revision_one.revision()),
            invalid,
        )),
        Err(WorkloadSagaStoreError::InvalidTransition(_))
    ));
    assert_eq!(
        block_on(store.load(initial.key())).unwrap(),
        Some(revision_one.clone()),
        "an invalid successor must not write"
    );

    assert_eq!(
        block_on(store.compare_and_swap(
            WorkloadSagaExpected::Revision(revision_one.revision()),
            revision_two.clone(),
        ))
        .unwrap(),
        WorkloadSagaCommit::Applied
    );

    let collision_store = S::default();
    let collision = running_record(
        "transition-claim",
        WorkloadActivationIntent::ActivateWhenAttached,
    );
    collision_store.inject_divergent_transition_claim(
        collision.last_transition().transition_id().clone(),
        running_record(
            "different-content",
            WorkloadActivationIntent::ActivateWhenAttached,
        ),
    );
    assert!(matches!(
        block_on(
            collision_store.compare_and_swap(WorkloadSagaExpected::Missing, collision.clone(),)
        ),
        Err(WorkloadSagaStoreError::InvalidTransition(_))
    ));
    assert_eq!(
        block_on(collision_store.load(collision.key())).unwrap(),
        None,
        "a divergent transition-id claim must not write"
    );

    assert_recovery_paging_contract::<S>();
}

fn assert_conflict(
    error: WorkloadSagaStoreError,
    expected: WorkloadSagaExpected,
    observed: Option<WorkloadSagaRevision>,
) {
    assert_eq!(
        error,
        WorkloadSagaStoreError::Conflict { expected, observed }
    );
}

fn assert_recovery_paging_contract<S: StoreConformance>() {
    let store = S::default();
    let first = running_record("recover-a", WorkloadActivationIntent::ActivateWhenAttached);
    let second = running_record("recover-b", WorkloadActivationIntent::ActivateWhenAttached);
    let third = advance_to(
        &running_record("recover-c", WorkloadActivationIntent::ActivateWhenAttached),
        WorkloadSagaPhase::NetworkReserved,
    );
    let terminal = stopped_record("terminal");
    let prepared_only = prepare_only_attached_record("prepared-only");

    for record in [&first, &second, &terminal] {
        assert_eq!(
            block_on(store.compare_and_swap(WorkloadSagaExpected::Missing, record.clone()))
                .unwrap(),
            WorkloadSagaCommit::Applied
        );
    }
    persist_history(&store, &third);
    persist_history(&store, &prepared_only);

    let mut expected = [first, second, third];
    expected.sort_by(|left, right| left.recovery_key().cmp(&right.recovery_key()));

    let first_request = WorkloadSagaPageRequest::new(None, 2).unwrap();
    let first_page = block_on(store.list_recoverable(first_request)).unwrap();
    assert_eq!(first_page.records(), &expected[..2]);
    assert_eq!(
        first_page.next_cursor().unwrap(),
        &WorkloadSagaRecoveryCursor::for_record(&expected[1]).unwrap()
    );

    let second_request =
        WorkloadSagaPageRequest::new(first_page.next_cursor().cloned(), 2).unwrap();
    let second_page = block_on(store.list_recoverable(second_request)).unwrap();
    assert_eq!(second_page.records(), &expected[2..]);
    assert_eq!(second_page.next_cursor(), None);
    assert!(
        second_page
            .records()
            .iter()
            .all(WorkloadSagaRecord::requires_recovery),
        "terminal and prepare-only quiescent records must be omitted"
    );
}

fn persist_history<S: WorkloadSagaStore>(store: &S, latest: &WorkloadSagaRecord) {
    let initial = running_record_for_key(latest.key().clone(), latest.active_intent().activation());
    let reserved = advance_to(&initial, WorkloadSagaPhase::NetworkReserved);
    let prepared = advance_to(&reserved, WorkloadSagaPhase::WorkloadPrepared);
    let attached = advance_to(&prepared, WorkloadSagaPhase::NetworkAttached);

    let history = match latest.phase() {
        WorkloadSagaPhase::NetworkReserved => vec![initial, reserved],
        WorkloadSagaPhase::WorkloadPrepared => vec![initial, reserved, prepared],
        WorkloadSagaPhase::NetworkAttached => vec![initial, reserved, prepared, attached],
        phase => panic!("test history does not support {phase:?}"),
    };
    assert_eq!(history.last(), Some(latest));

    for (index, record) in history.into_iter().enumerate() {
        let expected = if index == 0 {
            WorkloadSagaExpected::Missing
        } else {
            WorkloadSagaExpected::Revision(WorkloadSagaRevision::new(
                u64::try_from(index - 1).unwrap(),
            ))
        };
        assert_eq!(
            block_on(store.compare_and_swap(expected, record)).unwrap(),
            WorkloadSagaCommit::Applied
        );
    }
}

#[test]
fn page_request_rejects_zero_limit() {
    assert!(matches!(
        WorkloadSagaPageRequest::new(None, 0),
        Err(WorkloadSagaStoreError::InvalidTransition(
            WorkloadSagaError::InvalidCounter(_)
        ))
    ));
}

#[test]
fn page_request_rejects_limit_above_256() {
    assert!(matches!(
        WorkloadSagaPageRequest::new(None, MAX_WORKLOAD_SAGA_PAGE_SIZE + 1),
        Err(WorkloadSagaStoreError::InvalidTransition(
            WorkloadSagaError::InvalidCounter(_)
        ))
    ));
}

#[test]
fn page_request_accepts_limits_one_and_256() {
    let minimum = WorkloadSagaPageRequest::new(None, 1).unwrap();
    let request = WorkloadSagaPageRequest::new(None, MAX_WORKLOAD_SAGA_PAGE_SIZE).unwrap();
    assert_eq!(minimum.limit(), 1);
    assert_eq!(request.limit(), MAX_WORKLOAD_SAGA_PAGE_SIZE);
}

#[test]
fn recovery_cursor_rejects_terminal_phase() {
    let terminal = stopped_record("cursor-terminal");
    assert!(matches!(
        WorkloadSagaRecoveryCursor::new(terminal.phase(), terminal.saga_id().clone()),
        Err(WorkloadSagaStoreError::InvalidTransition(_))
    ));
}

#[test]
fn recovery_cursor_rejects_prepare_only_quiescent_record() {
    let quiescent = prepare_only_attached_record("cursor-prepare-only");
    assert!(!quiescent.requires_recovery());
    assert!(matches!(
        WorkloadSagaRecoveryCursor::for_record(&quiescent),
        Err(WorkloadSagaStoreError::InvalidTransition(_))
    ));
}

#[test]
fn page_rejects_more_records_than_requested() {
    let request = WorkloadSagaPageRequest::new(None, 1).unwrap();
    let records = vec![
        running_record(
            "over-limit-a",
            WorkloadActivationIntent::ActivateWhenAttached,
        ),
        running_record(
            "over-limit-b",
            WorkloadActivationIntent::ActivateWhenAttached,
        ),
    ];
    assert!(matches!(
        WorkloadSagaPage::new(&request, records, false),
        Err(WorkloadSagaStoreError::InvalidTransition(
            WorkloadSagaError::InvalidEvidence(_)
        ))
    ));
}

#[test]
fn page_rejects_empty_result_that_claims_more() {
    let request = WorkloadSagaPageRequest::new(None, 1).unwrap();
    assert!(matches!(
        WorkloadSagaPage::new(&request, Vec::new(), true),
        Err(WorkloadSagaStoreError::InvalidTransition(
            WorkloadSagaError::InvalidEvidence(_)
        ))
    ));
}

#[test]
fn page_rejects_terminal_record() {
    let request = WorkloadSagaPageRequest::new(None, 1).unwrap();
    assert!(matches!(
        WorkloadSagaPage::new(&request, vec![stopped_record("page-terminal")], false),
        Err(WorkloadSagaStoreError::InvalidTransition(_))
    ));
}

#[test]
fn page_rejects_prepare_only_quiescent_record() {
    let request = WorkloadSagaPageRequest::new(None, 1).unwrap();
    assert!(matches!(
        WorkloadSagaPage::new(
            &request,
            vec![prepare_only_attached_record("page-prepare-only")],
            false,
        ),
        Err(WorkloadSagaStoreError::InvalidTransition(_))
    ));
}

#[test]
fn page_rejects_duplicate_records() {
    let request = WorkloadSagaPageRequest::new(None, 2).unwrap();
    let record = running_record("duplicate", WorkloadActivationIntent::ActivateWhenAttached);
    assert!(matches!(
        WorkloadSagaPage::new(&request, vec![record.clone(), record], false),
        Err(WorkloadSagaStoreError::InvalidTransition(_))
    ));
}

#[test]
fn page_rejects_unsorted_saga_ids_within_a_phase() {
    let request = WorkloadSagaPageRequest::new(None, 2).unwrap();
    let mut records = vec![
        running_record("unsorted-a", WorkloadActivationIntent::ActivateWhenAttached),
        running_record("unsorted-b", WorkloadActivationIntent::ActivateWhenAttached),
    ];
    records.sort_by(|left, right| left.recovery_key().cmp(&right.recovery_key()));
    records.reverse();
    assert!(matches!(
        WorkloadSagaPage::new(&request, records, false),
        Err(WorkloadSagaStoreError::InvalidTransition(_))
    ));
}

#[test]
fn page_rejects_unsorted_phases() {
    let initial = running_record(
        "unsorted-phase-initial",
        WorkloadActivationIntent::ActivateWhenAttached,
    );
    let reserved = advance_to(
        &running_record(
            "unsorted-phase-reserved",
            WorkloadActivationIntent::ActivateWhenAttached,
        ),
        WorkloadSagaPhase::NetworkReserved,
    );
    let request = WorkloadSagaPageRequest::new(None, 2).unwrap();
    assert!(matches!(
        WorkloadSagaPage::new(&request, vec![reserved, initial], false),
        Err(WorkloadSagaStoreError::InvalidTransition(_))
    ));
}

#[test]
fn page_advances_deterministically_by_phase_then_saga_id() {
    let mut intent_records = [
        running_record(
            "deterministic-b",
            WorkloadActivationIntent::ActivateWhenAttached,
        ),
        running_record(
            "deterministic-a",
            WorkloadActivationIntent::ActivateWhenAttached,
        ),
    ];
    intent_records.sort_by(|left, right| left.recovery_key().cmp(&right.recovery_key()));
    let reserved = advance_to(
        &running_record(
            "deterministic-reserved",
            WorkloadActivationIntent::ActivateWhenAttached,
        ),
        WorkloadSagaPhase::NetworkReserved,
    );
    let records = vec![
        intent_records[0].clone(),
        intent_records[1].clone(),
        reserved,
    ];
    let request = WorkloadSagaPageRequest::new(None, 3).unwrap();
    let page = WorkloadSagaPage::new(&request, records.clone(), true).unwrap();
    assert_eq!(page.records(), records);
    assert_eq!(
        page.next_cursor(),
        Some(&WorkloadSagaRecoveryCursor::for_record(&records[2]).unwrap())
    );
}

#[test]
fn page_rejects_cursor_regression() {
    let record = running_record(
        "cursor-regression",
        WorkloadActivationIntent::ActivateWhenAttached,
    );
    let after = WorkloadSagaRecoveryCursor::for_record(&record).unwrap();
    let request = WorkloadSagaPageRequest::new(Some(after), 1).unwrap();
    assert!(matches!(
        WorkloadSagaPage::new(&request, vec![record], false),
        Err(WorkloadSagaStoreError::InvalidTransition(_))
    ));
}

#[test]
fn page_next_cursor_matches_final_record_when_more_exists() {
    let request = WorkloadSagaPageRequest::new(None, 2).unwrap();
    let mut records = vec![
        running_record("cursor-a", WorkloadActivationIntent::ActivateWhenAttached),
        running_record("cursor-b", WorkloadActivationIntent::ActivateWhenAttached),
    ];
    records.sort_by(|left, right| left.recovery_key().cmp(&right.recovery_key()));
    let expected = WorkloadSagaRecoveryCursor::for_record(&records[1]).unwrap();
    let page = WorkloadSagaPage::new(&request, records, true).unwrap();
    assert_eq!(page.next_cursor(), Some(&expected));
}

#[test]
fn page_without_more_has_no_next_cursor() {
    let request = WorkloadSagaPageRequest::new(None, 1).unwrap();
    let page = WorkloadSagaPage::new(
        &request,
        vec![running_record(
            "no-next-cursor",
            WorkloadActivationIntent::ActivateWhenAttached,
        )],
        false,
    )
    .unwrap();
    assert_eq!(page.next_cursor(), None);
}

fn running_record(label: &str, activation: WorkloadActivationIntent) -> WorkloadSagaRecord {
    let key = workload_key(label);
    running_record_for_key(key, activation)
}

fn running_record_for_key(
    key: WorkloadSagaKey,
    activation: WorkloadActivationIntent,
) -> WorkloadSagaRecord {
    WorkloadSagaRecord::new(
        key.clone(),
        intent(&key, DesiredWorkloadState::Running, activation),
    )
    .unwrap()
}

fn stopped_record(label: &str) -> WorkloadSagaRecord {
    let key = workload_key(label);
    WorkloadSagaRecord::new(
        key.clone(),
        intent(
            &key,
            DesiredWorkloadState::Stopped,
            WorkloadActivationIntent::PrepareOnly,
        ),
    )
    .unwrap()
}

fn prepare_only_attached_record(label: &str) -> WorkloadSagaRecord {
    let initial = running_record(label, WorkloadActivationIntent::PrepareOnly);
    let reserved = advance_to(&initial, WorkloadSagaPhase::NetworkReserved);
    let prepared = advance_to(&reserved, WorkloadSagaPhase::WorkloadPrepared);
    advance_to(&prepared, WorkloadSagaPhase::NetworkAttached)
}

fn workload_key(label: &str) -> WorkloadSagaKey {
    WorkloadSagaKey::new(
        TenantId::new(format!("tenant-{label}")).unwrap(),
        WorkloadId::new(format!("workload-{label}")).unwrap(),
    )
}

fn intent(
    key: &WorkloadSagaKey,
    desired_state: DesiredWorkloadState,
    activation: WorkloadActivationIntent,
) -> WorkloadSagaIntent {
    let plan_id =
        NetworkPlanId::for_tenant_workload_plan(key.tenant_id(), key.workload_id().as_str());
    let publication = WorkloadPublicationIntent::Withheld;
    WorkloadSagaIntent::new(
        DesiredWorkloadKind::Service,
        desired_state,
        WorkloadGeneration::new(1),
        WorkloadDesiredDigest::sha256(key.workload_id().as_str()),
        WorkloadNetworkIntent::new(
            plan_id,
            NetworkResourceGeneration::new(1),
            NetworkPlanDigest::from_bytes([0x31; 32]),
        ),
        activation,
        publication,
        WorkloadAdmissionEvidence::new(
            TenantIsolationDecisionId::try_from(format!("tid_{}", "a".repeat(64))).unwrap(),
            TenantWorkloadUid::try_from(format!("twu_{}", "b".repeat(64))).unwrap(),
            Some(NodeIdentity::new("node-a").unwrap()),
        ),
    )
    .unwrap()
}

fn advance_to(record: &WorkloadSagaRecord, phase: WorkloadSagaPhase) -> WorkloadSagaRecord {
    let intent = record.active_intent();
    let references = WorkloadEffectReferences::provision(intent, None).unwrap();
    let network = references.network().unwrap().clone();
    let execution = references.execution().unwrap().clone();
    let mut observations = vec![WorkloadOwnerObservation::NetworkReserved {
        reference: network.clone(),
        evidence: WorkloadOwnerEvidenceDigest::sha256("network-reserved"),
    }];
    if matches!(
        phase,
        WorkloadSagaPhase::WorkloadPrepared | WorkloadSagaPhase::NetworkAttached
    ) {
        observations.push(WorkloadOwnerObservation::ExecutionPrepared {
            reference: execution,
            evidence: WorkloadOwnerEvidenceDigest::sha256("execution-prepared"),
        });
    }
    if phase == WorkloadSagaPhase::NetworkAttached {
        observations.push(WorkloadOwnerObservation::NetworkAttached {
            reference: network,
            evidence: WorkloadOwnerEvidenceDigest::sha256("network-attached"),
        });
    }
    let detail = WorkloadPhaseDetail::provision(phase, intent, references, observations).unwrap();
    record.advance(phase, detail, None).unwrap()
}
