use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Mutex, RwLock};

use futures::executor::block_on;
use nimbus_core::{TenantId, WorkloadId};
use nimbus_network::{
    NetworkAttachmentCapabilitySet, NetworkCapabilityRequirements, NetworkControlPlaneLocality,
    NetworkEndpointCapabilitySet, NetworkForwardingCapabilitySet, NetworkIngressCapabilitySet,
    NetworkLifecycleCapabilitySet, NetworkManagementMode, NetworkResourceGeneration,
    NetworkSovereigntyRequirements, PublishedEndpointId,
};
use nimbus_tenant::TenantIsolationDecisionId;

use super::*;
use crate::{
    CompiledWorkloadNetworkPlan, DesiredWorkloadKind, DesiredWorkloadState, NodeIdentity,
    TenantWorkloadUid, WorkloadActivationIntent, WorkloadAdmissionEvidence,
    WorkloadEffectReferences, WorkloadExecutableEncoding, WorkloadExecutableIntent,
    WorkloadGeneration, WorkloadInspectionRequirement, WorkloadNetworkIntent,
    WorkloadNetworkPlanContent, WorkloadNetworkPlanIdentity, WorkloadOwnerEvidenceDigest,
    WorkloadOwnerObservation, WorkloadPhaseDetail, WorkloadPublicationIntent,
    WorkloadPublicationReference, WorkloadSagaIntent, WorkloadSagaPhase, WorkloadSagaTransitionId,
    WorkloadTerminalEvidenceDigest, WorkloadTerminalObservation,
};

fn require_object_safe_store(_: &dyn WorkloadSagaStore) {}

#[test]
fn store_port_is_object_safe_send_and_sync() {
    fn require_send_sync<T: Send + Sync>() {}

    let _ = require_object_safe_store;
    require_send_sync::<MutexMapStore>();
    require_send_sync::<RwLockAppendLogStore>();
}

#[test]
fn recovery_order_is_complete_unique_and_stable() {
    let expected = [
        WorkloadSagaPhase::IntentCommitted,
        WorkloadSagaPhase::NetworkReserved,
        WorkloadSagaPhase::WorkloadPrepared,
        WorkloadSagaPhase::NetworkAttached,
        WorkloadSagaPhase::WorkloadActivated,
        WorkloadSagaPhase::Ready,
        WorkloadSagaPhase::Published,
        WorkloadSagaPhase::Observed,
        WorkloadSagaPhase::WithdrawalCommitted,
        WorkloadSagaPhase::Withdrawn,
        WorkloadSagaPhase::Drained,
        WorkloadSagaPhase::WorkloadStopped,
        WorkloadSagaPhase::NetworkDetached,
        WorkloadSagaPhase::NetworkReleased,
        WorkloadSagaPhase::CleanupPending,
        WorkloadSagaPhase::Recorded,
    ];
    assert_eq!(crate::WORKLOAD_SAGA_RECOVERY_ORDER, expected);

    let unique = crate::WORKLOAD_SAGA_RECOVERY_ORDER
        .into_iter()
        .collect::<BTreeSet<_>>();
    assert_eq!(unique.len(), crate::WORKLOAD_SAGA_RECOVERY_ORDER.len());
    for (rank, phase) in crate::WORKLOAD_SAGA_RECOVERY_ORDER.into_iter().enumerate() {
        assert_eq!(usize::from(phase.recovery_order()), rank);
    }
    assert!(crate::WORKLOAD_SAGA_RECOVERY_ORDER.contains(&WorkloadSagaPhase::Recorded));
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

    fn list_for_tenant<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        request: WorkloadSagaTenantPageRequest,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaTenantPage> {
        Box::pin(async move {
            let state = self.state.lock().expect("mutex map store lock");
            build_tenant_page(
                tenant_id,
                &request,
                state.records.values().cloned().collect(),
            )
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

    fn list_for_tenant<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        request: WorkloadSagaTenantPageRequest,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaTenantPage> {
        Box::pin(async move {
            let state = self.state.read().expect("append log store read lock");
            build_tenant_page(tenant_id, &request, state.latest_records())
        })
    }
}

trait StoreConformance: WorkloadSagaStore + Default {
    fn inject_divergent_transition_claim(
        &self,
        transition_id: WorkloadSagaTransitionId,
        divergent_record: WorkloadSagaRecord,
    );

    fn inject_latest(&self, record: WorkloadSagaRecord);
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

    fn inject_latest(&self, record: WorkloadSagaRecord) {
        self.state
            .lock()
            .expect("mutex map store lock")
            .records
            .insert(record.key().clone(), record);
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

    fn inject_latest(&self, record: WorkloadSagaRecord) {
        self.state
            .write()
            .expect("append log store write lock")
            .revisions
            .push(record);
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
    records.sort_by(|left, right| left.saga_id().cmp(right.saga_id()));
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

fn build_tenant_page(
    tenant_id: &TenantId,
    request: &WorkloadSagaTenantPageRequest,
    mut records: Vec<WorkloadSagaRecord>,
) -> Result<WorkloadSagaTenantPage, WorkloadSagaStoreError> {
    request.validate_for_tenant(tenant_id)?;
    records.retain(|record| record.key().tenant_id() == tenant_id);
    records.sort_by(|left, right| left.key().cmp(right.key()));
    if let Some(after) = request.after() {
        records.retain(|record| record.key() > after.order_key());
    }
    records.truncate(usize::from(request.limit()).saturating_add(1));
    let has_more = records.len() > usize::from(request.limit());
    records.truncate(usize::from(request.limit()));
    WorkloadSagaTenantPage::new(tenant_id, request, records, has_more)
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
    assert_tenant_paging_contract::<S>();
}

fn assert_tenant_paging_contract<S: StoreConformance>() {
    let store = S::default();
    let tenant_id = TenantId::new("tenant-inventory").unwrap();
    let mut expected = crate::WORKLOAD_SAGA_RECOVERY_ORDER
        .into_iter()
        .map(|phase| tenant_phase_record(&tenant_id, phase))
        .collect::<Vec<_>>();
    let prepare_only = prepare_only_attached_record_for_key(workload_key_for_tenant(
        &tenant_id,
        "phase-03-prepare-only",
    ));
    assert!(!prepare_only.requires_recovery());
    expected.push(prepare_only);
    expected.sort_by(|left, right| left.key().cmp(right.key()));
    for record in &expected {
        store.inject_latest(record.clone());
    }
    store.inject_latest(tenant_phase_record(
        &TenantId::new("tenant-outsider").unwrap(),
        WorkloadSagaPhase::Observed,
    ));

    let crossed = WorkloadSagaTenantPageRequest::new(
        Some(WorkloadSagaTenantCursor::for_record(&tenant_phase_record(
            &TenantId::new("tenant-crossed-cursor").unwrap(),
            WorkloadSagaPhase::Observed,
        ))),
        3,
    )
    .unwrap();
    assert!(matches!(
        block_on(store.list_for_tenant(&tenant_id, crossed)),
        Err(WorkloadSagaStoreError::InvalidTransition(
            WorkloadSagaError::InvalidEvidence(_)
        ))
    ));

    let mut after = None;
    let mut observed = Vec::new();
    loop {
        let request = WorkloadSagaTenantPageRequest::new(after, 3).unwrap();
        let page = block_on(store.list_for_tenant(&tenant_id, request)).unwrap();
        assert_eq!(page.tenant_id(), &tenant_id);
        assert!(
            page.records()
                .iter()
                .all(|record| record.key().tenant_id() == &tenant_id)
        );
        observed.extend_from_slice(page.records());
        let Some(next) = page.next_cursor().cloned() else {
            break;
        };
        assert_eq!(
            next,
            WorkloadSagaTenantCursor::for_record(
                page.records()
                    .last()
                    .expect("non-terminal page is nonempty")
            )
        );
        after = Some(next);
    }

    assert_eq!(observed, expected);
    assert_eq!(
        observed
            .iter()
            .map(WorkloadSagaRecord::phase)
            .collect::<BTreeSet<_>>(),
        crate::WORKLOAD_SAGA_RECOVERY_ORDER
            .into_iter()
            .collect::<BTreeSet<_>>(),
        "tenant inventory must include every active, quiescent, cleanup, and terminal phase"
    );
    assert!(
        observed
            .windows(2)
            .all(|pair| pair[0].key() < pair[1].key()),
        "tenant paging must be strictly increasing without duplicates"
    );
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
    expected.sort_by(|left, right| left.saga_id().cmp(right.saga_id()));

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
fn tenant_page_request_rejects_zero_and_oversize_limits() {
    for limit in [0, MAX_WORKLOAD_SAGA_PAGE_SIZE + 1] {
        assert!(matches!(
            WorkloadSagaTenantPageRequest::new(None, limit),
            Err(WorkloadSagaStoreError::InvalidTransition(
                WorkloadSagaError::InvalidCounter(_)
            ))
        ));
    }
}

#[test]
fn tenant_page_request_rejects_cross_tenant_cursor() {
    let record = running_record_for_key(
        workload_key_for_tenant(&TenantId::new("tenant-a").unwrap(), "cursor"),
        WorkloadActivationIntent::ActivateWhenAttached,
    );
    let request =
        WorkloadSagaTenantPageRequest::new(Some(WorkloadSagaTenantCursor::for_record(&record)), 1)
            .unwrap();
    assert!(matches!(
        request.validate_for_tenant(&TenantId::new("tenant-b").unwrap()),
        Err(WorkloadSagaStoreError::InvalidTransition(
            WorkloadSagaError::InvalidEvidence(_)
        ))
    ));
}

#[test]
fn tenant_page_rejects_cross_tenant_record() {
    let tenant_id = TenantId::new("tenant-a").unwrap();
    let request = WorkloadSagaTenantPageRequest::new(None, 1).unwrap();
    let crossed = running_record_for_key(
        workload_key_for_tenant(&TenantId::new("tenant-b").unwrap(), "crossed"),
        WorkloadActivationIntent::ActivateWhenAttached,
    );
    assert!(matches!(
        WorkloadSagaTenantPage::new(&tenant_id, &request, vec![crossed], false),
        Err(WorkloadSagaStoreError::InvalidTransition(
            WorkloadSagaError::InvalidEvidence(_)
        ))
    ));
}

#[test]
fn tenant_page_rejects_duplicate_unsorted_and_regressing_records() {
    let tenant_id = TenantId::new("tenant-order").unwrap();
    let first = running_record_for_key(
        workload_key_for_tenant(&tenant_id, "a"),
        WorkloadActivationIntent::ActivateWhenAttached,
    );
    let second = running_record_for_key(
        workload_key_for_tenant(&tenant_id, "b"),
        WorkloadActivationIntent::ActivateWhenAttached,
    );
    let request = WorkloadSagaTenantPageRequest::new(None, 2).unwrap();
    for records in [
        vec![first.clone(), first.clone()],
        vec![second.clone(), first.clone()],
    ] {
        assert!(matches!(
            WorkloadSagaTenantPage::new(&tenant_id, &request, records, false),
            Err(WorkloadSagaStoreError::InvalidTransition(
                WorkloadSagaError::InvalidEvidence(_)
            ))
        ));
    }

    let after = WorkloadSagaTenantCursor::for_record(&first);
    let regressing = WorkloadSagaTenantPageRequest::new(Some(after), 1).unwrap();
    assert!(matches!(
        WorkloadSagaTenantPage::new(&tenant_id, &regressing, vec![first], false),
        Err(WorkloadSagaStoreError::InvalidTransition(
            WorkloadSagaError::InvalidEvidence(_)
        ))
    ));
}

#[test]
fn tenant_page_rejects_over_limit_and_empty_with_more() {
    let tenant_id = TenantId::new("tenant-shape").unwrap();
    let request = WorkloadSagaTenantPageRequest::new(None, 1).unwrap();
    let first = running_record_for_key(
        workload_key_for_tenant(&tenant_id, "a"),
        WorkloadActivationIntent::ActivateWhenAttached,
    );
    let second = running_record_for_key(
        workload_key_for_tenant(&tenant_id, "b"),
        WorkloadActivationIntent::ActivateWhenAttached,
    );
    assert!(matches!(
        WorkloadSagaTenantPage::new(&tenant_id, &request, vec![first, second], false),
        Err(WorkloadSagaStoreError::InvalidTransition(
            WorkloadSagaError::InvalidEvidence(_)
        ))
    ));
    assert!(matches!(
        WorkloadSagaTenantPage::new(&tenant_id, &request, Vec::new(), true),
        Err(WorkloadSagaStoreError::InvalidTransition(
            WorkloadSagaError::InvalidEvidence(_)
        ))
    ));

    let partial_request = WorkloadSagaTenantPageRequest::new(None, 2).unwrap();
    let partial = running_record_for_key(
        workload_key_for_tenant(&tenant_id, "partial"),
        WorkloadActivationIntent::ActivateWhenAttached,
    );
    assert!(matches!(
        WorkloadSagaTenantPage::new(&tenant_id, &partial_request, vec![partial], true),
        Err(WorkloadSagaStoreError::InvalidTransition(
            WorkloadSagaError::InvalidEvidence(_)
        ))
    ));
}

#[test]
fn tenant_cursor_is_stable_across_phase_changes() {
    let tenant_id = TenantId::new("tenant-stable").unwrap();
    let initial = running_record_for_key(
        workload_key_for_tenant(&tenant_id, "stable"),
        WorkloadActivationIntent::ActivateWhenAttached,
    );
    let reserved = advance_to(&initial, WorkloadSagaPhase::NetworkReserved);
    assert_eq!(
        WorkloadSagaTenantCursor::for_record(&initial),
        WorkloadSagaTenantCursor::for_record(&reserved)
    );
}

#[test]
fn recovery_cursor_constructor_names_only_immutable_saga_identity() {
    let terminal = stopped_record("cursor-terminal");
    let cursor = WorkloadSagaRecoveryCursor::new(terminal.saga_id().clone());
    assert_eq!(cursor.saga_id(), terminal.saga_id());
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
fn page_rejects_unsorted_saga_ids() {
    let request = WorkloadSagaPageRequest::new(None, 2).unwrap();
    let mut records = vec![
        running_record("unsorted-a", WorkloadActivationIntent::ActivateWhenAttached),
        running_record("unsorted-b", WorkloadActivationIntent::ActivateWhenAttached),
    ];
    records.sort_by(|left, right| left.saga_id().cmp(right.saga_id()));
    records.reverse();
    assert!(matches!(
        WorkloadSagaPage::new(&request, records, false),
        Err(WorkloadSagaStoreError::InvalidTransition(_))
    ));
}

#[test]
fn page_order_is_independent_of_mutable_phase() {
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
    let mut records = vec![reserved, initial];
    records.sort_by(|left, right| left.saga_id().cmp(right.saga_id()));
    let request = WorkloadSagaPageRequest::new(None, 2).unwrap();
    let page = WorkloadSagaPage::new(&request, records.clone(), false).unwrap();
    assert_eq!(page.records(), records);
}

#[test]
fn page_advances_deterministically_by_immutable_saga_id() {
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
    intent_records.sort_by(|left, right| left.saga_id().cmp(right.saga_id()));
    let reserved = advance_to(
        &running_record(
            "deterministic-reserved",
            WorkloadActivationIntent::ActivateWhenAttached,
        ),
        WorkloadSagaPhase::NetworkReserved,
    );
    let mut records = vec![
        intent_records[0].clone(),
        intent_records[1].clone(),
        reserved,
    ];
    records.sort_by(|left, right| left.saga_id().cmp(right.saga_id()));
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
    records.sort_by(|left, right| left.saga_id().cmp(right.saga_id()));
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

fn tenant_phase_record(tenant_id: &TenantId, phase: WorkloadSagaPhase) -> WorkloadSagaRecord {
    let label = format!("phase-{:02}", phase.recovery_order());
    let key = workload_key_for_tenant(tenant_id, &label);
    if phase.is_provision() {
        return provision_phase_record(key, phase);
    }

    let observed = provision_phase_record(key, WorkloadSagaPhase::Observed);
    if phase == WorkloadSagaPhase::CleanupPending {
        let references = observed.phase_detail().references();
        let detail = WorkloadPhaseDetail::cleanup_pending(
            observed.active_intent(),
            observed.phase(),
            references.clone(),
            cleanup_inspections(observed.phase(), &references),
        )
        .unwrap();
        return observed
            .advance(WorkloadSagaPhase::CleanupPending, detail, None)
            .unwrap();
    }

    let mut record = begin_teardown(&observed);
    if phase == WorkloadSagaPhase::WithdrawalCommitted {
        return record;
    }
    for target in [
        WorkloadSagaPhase::Withdrawn,
        WorkloadSagaPhase::Drained,
        WorkloadSagaPhase::WorkloadStopped,
        WorkloadSagaPhase::NetworkDetached,
        WorkloadSagaPhase::NetworkReleased,
    ] {
        record = advance_teardown(&record, target);
        if phase == target {
            return record;
        }
    }
    assert_eq!(phase, WorkloadSagaPhase::Recorded);
    let WorkloadPhaseDetail::Teardown(detail) = record.phase_detail() else {
        panic!("network-released fixture must carry teardown detail");
    };
    let terminal_digest =
        WorkloadTerminalEvidenceDigest::for_observations(detail.terminal_observations()).unwrap();
    record
        .advance(
            WorkloadSagaPhase::Recorded,
            WorkloadPhaseDetail::recorded(record.active_intent(), terminal_digest),
            None,
        )
        .unwrap()
}

fn provision_phase_record(key: WorkloadSagaKey, phase: WorkloadSagaPhase) -> WorkloadSagaRecord {
    let saga_intent = intent_with_publication(
        &key,
        DesiredWorkloadState::Running,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
    );
    let publication =
        WorkloadPublicationReference::new([PublishedEndpointId::generate()], &saga_intent).unwrap();
    let mut record = WorkloadSagaRecord::new(key, saga_intent).unwrap();
    if phase == WorkloadSagaPhase::IntentCommitted {
        return record;
    }
    for target in [
        WorkloadSagaPhase::NetworkReserved,
        WorkloadSagaPhase::WorkloadPrepared,
        WorkloadSagaPhase::NetworkAttached,
        WorkloadSagaPhase::WorkloadActivated,
        WorkloadSagaPhase::Ready,
        WorkloadSagaPhase::Published,
        WorkloadSagaPhase::Observed,
    ] {
        record = advance_provision(&record, target, &publication);
        if phase == target {
            return record;
        }
    }
    panic!("{phase:?} is not a provision fixture phase")
}

fn advance_provision(
    record: &WorkloadSagaRecord,
    phase: WorkloadSagaPhase,
    publication: &WorkloadPublicationReference,
) -> WorkloadSagaRecord {
    let publication = matches!(
        phase,
        WorkloadSagaPhase::Ready | WorkloadSagaPhase::Published | WorkloadSagaPhase::Observed
    )
    .then_some(publication.clone());
    let references =
        WorkloadEffectReferences::provision(record.active_intent(), publication).unwrap();
    let network = references.network().unwrap().clone();
    let execution = references.execution().unwrap().clone();
    let rank = match phase {
        WorkloadSagaPhase::NetworkReserved => 1,
        WorkloadSagaPhase::WorkloadPrepared => 2,
        WorkloadSagaPhase::NetworkAttached => 3,
        WorkloadSagaPhase::WorkloadActivated => 4,
        WorkloadSagaPhase::Ready => 5,
        WorkloadSagaPhase::Published | WorkloadSagaPhase::Observed => 6,
        _ => panic!("{phase:?} is not an advanced provision phase"),
    };
    let mut observations = Vec::new();
    if rank >= 1 {
        observations.push(WorkloadOwnerObservation::NetworkReserved {
            reference: network.clone(),
            evidence: WorkloadOwnerEvidenceDigest::sha256("network-reserved"),
        });
    }
    if rank >= 2 {
        observations.push(WorkloadOwnerObservation::ExecutionPrepared {
            reference: execution.clone(),
            evidence: WorkloadOwnerEvidenceDigest::sha256("execution-prepared"),
        });
    }
    if rank >= 3 {
        observations.push(WorkloadOwnerObservation::NetworkAttached {
            reference: network.clone(),
            evidence: WorkloadOwnerEvidenceDigest::sha256("network-attached"),
        });
    }
    if rank >= 4 {
        observations.push(WorkloadOwnerObservation::ExecutionActivated {
            reference: execution.clone(),
            evidence: WorkloadOwnerEvidenceDigest::sha256("execution-activated"),
        });
    }
    if rank >= 5 {
        observations.push(WorkloadOwnerObservation::Ready {
            network,
            execution,
            evidence: WorkloadOwnerEvidenceDigest::sha256("ready"),
        });
    }
    if rank >= 6 {
        observations.push(WorkloadOwnerObservation::PublicationPresent {
            reference: references.publication().unwrap().clone(),
            evidence: WorkloadOwnerEvidenceDigest::sha256("publication-present"),
        });
    }
    let detail =
        WorkloadPhaseDetail::provision(phase, record.active_intent(), references, observations)
            .unwrap();
    record.advance(phase, detail, None).unwrap()
}

fn begin_teardown(record: &WorkloadSagaRecord) -> WorkloadSagaRecord {
    let references = record.phase_detail().references();
    let detail = WorkloadPhaseDetail::teardown(
        WorkloadSagaPhase::WithdrawalCommitted,
        record.active_intent(),
        record.phase(),
        references,
        Vec::new(),
    )
    .unwrap();
    record
        .advance(WorkloadSagaPhase::WithdrawalCommitted, detail, None)
        .unwrap()
}

fn advance_teardown(record: &WorkloadSagaRecord, phase: WorkloadSagaPhase) -> WorkloadSagaRecord {
    let WorkloadPhaseDetail::Teardown(current) = record.phase_detail() else {
        panic!("teardown fixture must carry teardown detail");
    };
    let references = current.retained_references().clone();
    let rank = match phase {
        WorkloadSagaPhase::Withdrawn => 1,
        WorkloadSagaPhase::Drained => 2,
        WorkloadSagaPhase::WorkloadStopped => 3,
        WorkloadSagaPhase::NetworkDetached => 4,
        WorkloadSagaPhase::NetworkReleased => 5,
        _ => panic!("{phase:?} is not an advanced teardown phase"),
    };
    let mut observations = Vec::new();
    if rank >= 1 {
        observations.push(WorkloadTerminalObservation::PublicationAbsent {
            reference: references.publication().unwrap().clone(),
            evidence: WorkloadOwnerEvidenceDigest::sha256("publication-absent"),
        });
    }
    if rank >= 2 {
        observations.push(WorkloadTerminalObservation::ExecutionDrained {
            reference: references.execution().unwrap().clone(),
            evidence: WorkloadOwnerEvidenceDigest::sha256("execution-drained"),
        });
    }
    if rank >= 3 {
        observations.push(WorkloadTerminalObservation::ExecutionStopped {
            reference: references.execution().unwrap().clone(),
            evidence: WorkloadOwnerEvidenceDigest::sha256("execution-stopped"),
        });
    }
    if rank >= 4 {
        observations.push(WorkloadTerminalObservation::NetworkDetached {
            reference: references.network().unwrap().clone(),
            evidence: WorkloadOwnerEvidenceDigest::sha256("network-detached"),
        });
    }
    if rank >= 5 {
        observations.push(WorkloadTerminalObservation::NetworkReleased {
            reference: references.network().unwrap().clone(),
            evidence: WorkloadOwnerEvidenceDigest::sha256("network-released"),
        });
    }
    let detail = WorkloadPhaseDetail::teardown(
        phase,
        record.active_intent(),
        current.origin(),
        references,
        observations,
    )
    .unwrap();
    record.advance(phase, detail, None).unwrap()
}

fn cleanup_inspections(
    phase: WorkloadSagaPhase,
    references: &WorkloadEffectReferences,
) -> Vec<WorkloadInspectionRequirement> {
    let mut inspections = Vec::new();
    if let Some(reference) = references.network() {
        inspections.push(WorkloadInspectionRequirement::Network {
            reference: reference.clone(),
            expected_phase: phase,
        });
    }
    if let Some(reference) = references.execution() {
        inspections.push(WorkloadInspectionRequirement::Execution {
            reference: reference.clone(),
            expected_phase: phase,
        });
    }
    if let Some(reference) = references.publication() {
        inspections.push(WorkloadInspectionRequirement::Publication {
            reference: reference.clone(),
            expected_phase: phase,
        });
    }
    inspections
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
    prepare_only_attached_record_for_key(workload_key(label))
}

fn prepare_only_attached_record_for_key(key: WorkloadSagaKey) -> WorkloadSagaRecord {
    let initial = running_record_for_key(key, WorkloadActivationIntent::PrepareOnly);
    let reserved = advance_to(&initial, WorkloadSagaPhase::NetworkReserved);
    let prepared = advance_to(&reserved, WorkloadSagaPhase::WorkloadPrepared);
    advance_to(&prepared, WorkloadSagaPhase::NetworkAttached)
}

fn workload_key(label: &str) -> WorkloadSagaKey {
    workload_key_for_tenant(&TenantId::new(format!("tenant-{label}")).unwrap(), label)
}

fn workload_key_for_tenant(tenant_id: &TenantId, label: &str) -> WorkloadSagaKey {
    WorkloadSagaKey::new(
        tenant_id.clone(),
        WorkloadId::new(format!("workload-{label}")).unwrap(),
    )
}

fn intent(
    key: &WorkloadSagaKey,
    desired_state: DesiredWorkloadState,
    activation: WorkloadActivationIntent,
) -> WorkloadSagaIntent {
    intent_with_publication(
        key,
        desired_state,
        activation,
        WorkloadPublicationIntent::Withheld,
    )
}

fn intent_with_publication(
    key: &WorkloadSagaKey,
    desired_state: DesiredWorkloadState,
    activation: WorkloadActivationIntent,
    publication: WorkloadPublicationIntent,
) -> WorkloadSagaIntent {
    let identity = WorkloadNetworkPlanIdentity::new(
        key.tenant_id().clone(),
        key.workload_id().as_str(),
        NetworkResourceGeneration::new(1),
    )
    .unwrap();
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
        publication,
    )
    .unwrap();
    let compiled_plan = CompiledWorkloadNetworkPlan::from_content(content).unwrap();
    WorkloadSagaIntent::new(
        DesiredWorkloadKind::Service,
        desired_state,
        WorkloadGeneration::new(1),
        WorkloadExecutableIntent::new(
            WorkloadExecutableEncoding::SandboxSpecCanonicalJsonV1,
            format!(r#"{{"workload":"{}"}}"#, key.workload_id().as_str()),
        )
        .unwrap(),
        WorkloadNetworkIntent::new(compiled_plan),
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
