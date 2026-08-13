use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use nimbus_core::TenantId;
use nimbus_network::NetworkCapabilityRegistry;
use nimbus_workloads::{
    WorkloadRestartCandidatePage, WorkloadRestartCandidatePageRequest, WorkloadSagaCommit,
    WorkloadSagaExpected, WorkloadSagaFuture, WorkloadSagaKey, WorkloadSagaPage,
    WorkloadSagaPageRequest, WorkloadSagaRecord, WorkloadSagaStore, WorkloadSagaStoreError,
    WorkloadSagaTenantPage, WorkloadSagaTenantPageRequest,
};

use super::*;
use crate::workload_saga::recovery::tests::{cleanup_pending_record, provision_record};
use crate::workload_saga::restart_resolution::NoopWorkloadRestartResolutionFence;
use crate::workload_saga::{
    WorkloadProvisionCapabilityRegistry, WorkloadProvisionSourceAuthority,
    WorkloadProvisionSourceAuthorityError, WorkloadProvisionSourceFuture,
    WorkloadRestartCapabilityRegistry, WorkloadTeardownCapabilityRegistry,
};

struct EffectForbiddenSource {
    calls: AtomicUsize,
}

impl EffectForbiddenSource {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            calls: AtomicUsize::new(0),
        })
    }
}

impl WorkloadProvisionSourceAuthority for EffectForbiddenSource {
    fn current_source<'a>(
        &'a self,
        _key: &'a WorkloadSagaKey,
        _identity: &'a nimbus_workloads::WorkloadProvisionSourceIdentity,
    ) -> WorkloadProvisionSourceFuture<'a> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::AcqRel);
            Err(WorkloadProvisionSourceAuthorityError::Unavailable)
        })
    }
}

struct RecoveryStore {
    listed: Vec<WorkloadSagaRecord>,
    current: Mutex<Vec<WorkloadSagaRecord>>,
    page_width: usize,
    recovery_error: Mutex<Option<WorkloadSagaStoreError>>,
    recovery_reads: AtomicUsize,
    loads: AtomicUsize,
    restart_reads: AtomicUsize,
    recovery_completed: AtomicBool,
}

impl RecoveryStore {
    fn new(records: Vec<WorkloadSagaRecord>, page_width: usize) -> Arc<Self> {
        let mut listed = records;
        listed.sort_by(|left, right| left.saga_id().cmp(right.saga_id()));
        Arc::new(Self {
            current: Mutex::new(listed.clone()),
            listed,
            page_width,
            recovery_error: Mutex::new(None),
            recovery_reads: AtomicUsize::new(0),
            loads: AtomicUsize::new(0),
            restart_reads: AtomicUsize::new(0),
            recovery_completed: AtomicBool::new(false),
        })
    }

    fn with_crossed_current(listed: WorkloadSagaRecord, current: WorkloadSagaRecord) -> Arc<Self> {
        Arc::new(Self {
            listed: vec![listed],
            current: Mutex::new(vec![current]),
            page_width: 1,
            recovery_error: Mutex::new(None),
            recovery_reads: AtomicUsize::new(0),
            loads: AtomicUsize::new(0),
            restart_reads: AtomicUsize::new(0),
            recovery_completed: AtomicBool::new(false),
        })
    }

    fn with_recovery_error(error: WorkloadSagaStoreError) -> Arc<Self> {
        let store = Self::new(Vec::new(), 1);
        *store
            .recovery_error
            .lock()
            .expect("startup store remains healthy") = Some(error);
        store
    }
}

impl WorkloadSagaStore for RecoveryStore {
    fn load<'a>(
        &'a self,
        key: &'a WorkloadSagaKey,
    ) -> WorkloadSagaFuture<'a, Option<WorkloadSagaRecord>> {
        Box::pin(async move {
            self.loads.fetch_add(1, Ordering::AcqRel);
            Ok(self
                .current
                .lock()
                .expect("startup store remains healthy")
                .iter()
                .find(|record| record.key() == key)
                .cloned())
        })
    }

    fn compare_and_swap<'a>(
        &'a self,
        expected: WorkloadSagaExpected,
        next: WorkloadSagaRecord,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaCommit> {
        Box::pin(async move {
            let mut records = self.current.lock().expect("startup store remains healthy");
            let position = records.iter().position(|record| record.key() == next.key());
            if position
                .and_then(|index| records.get(index))
                .is_some_and(|record| record == &next)
            {
                return Ok(WorkloadSagaCommit::Unchanged);
            }
            let observed = position.and_then(|index| records.get(index));
            let matches = match (&expected, observed) {
                (WorkloadSagaExpected::Missing, None) => true,
                (WorkloadSagaExpected::Revision(expected), Some(record)) => {
                    *expected == record.revision()
                }
                _ => false,
            };
            if !matches {
                return Err(WorkloadSagaStoreError::Conflict {
                    expected,
                    observed: observed.map(WorkloadSagaRecord::revision),
                });
            }
            match position {
                Some(index) => records[index] = next,
                None => records.push(next),
            }
            Ok(WorkloadSagaCommit::Applied)
        })
    }

    fn list_recoverable<'a>(
        &'a self,
        request: WorkloadSagaPageRequest,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaPage> {
        Box::pin(async move {
            self.recovery_reads.fetch_add(1, Ordering::AcqRel);
            if let Some(error) = self
                .recovery_error
                .lock()
                .expect("startup store remains healthy")
                .clone()
            {
                return Err(error);
            }
            let records = self
                .listed
                .iter()
                .filter(|record| {
                    request
                        .after()
                        .is_none_or(|cursor| record.saga_id() > cursor.saga_id())
                })
                .take(self.page_width.min(usize::from(request.limit())))
                .cloned()
                .collect::<Vec<_>>();
            let has_more = records.last().is_some_and(|last| {
                self.listed
                    .iter()
                    .any(|candidate| candidate.saga_id() > last.saga_id())
            });
            let page = WorkloadSagaPage::new(&request, records, has_more)?;
            if page.next_cursor().is_none() {
                self.recovery_completed.store(true, Ordering::Release);
            }
            Ok(page)
        })
    }

    fn list_restart_candidates<'a>(
        &'a self,
        request: WorkloadRestartCandidatePageRequest,
    ) -> WorkloadSagaFuture<'a, WorkloadRestartCandidatePage> {
        Box::pin(async move {
            assert!(
                self.recovery_completed.load(Ordering::Acquire),
                "restart discovery must not begin before all-phase recovery"
            );
            self.restart_reads.fetch_add(1, Ordering::AcqRel);
            WorkloadRestartCandidatePage::new(&request, Vec::new(), false)
        })
    }

    fn list_for_tenant<'a>(
        &'a self,
        _tenant_id: &'a TenantId,
        _request: WorkloadSagaTenantPageRequest,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaTenantPage> {
        Box::pin(async { Err(WorkloadSagaStoreError::Unavailable) })
    }
}

fn recovery(store: Arc<RecoveryStore>) -> (WorkloadStartupRecovery, Arc<EffectForbiddenSource>) {
    let coordinator = Arc::new(WorkloadSagaCoordinator::new(store));
    let source = EffectForbiddenSource::new();
    let source_authority: Arc<dyn WorkloadProvisionSourceAuthority> = source.clone();
    let reports = NetworkCapabilityRegistry::new([]).expect("empty reports are valid");
    let restart_runtime = Arc::new(
        WorkloadRestartRuntime::compose(
            Arc::clone(&coordinator),
            Arc::clone(&source_authority),
            reports.clone(),
            Arc::new(
                WorkloadProvisionCapabilityRegistry::new([], [], [])
                    .expect("empty provision registry is valid"),
            ),
            Arc::new(
                WorkloadRestartCapabilityRegistry::new([])
                    .expect("empty restart registry is valid"),
            ),
            Arc::new(NoopWorkloadRestartResolutionFence),
        )
        .expect("restart runtime should compose without starting its watch"),
    );
    let teardown = Arc::new(WorkloadTeardownRuntime::new(
        Arc::clone(&coordinator),
        source_authority,
        reports,
        Arc::new(
            WorkloadTeardownCapabilityRegistry::new([], [], [])
                .expect("empty teardown registry is valid"),
        ),
    ));
    (
        WorkloadStartupRecovery::new(coordinator, None, restart_runtime, Some(teardown)),
        source,
    )
}

#[tokio::test]
async fn bounded_pages_return_exact_cleanup_retention_aggregate() {
    let store = RecoveryStore::new(
        vec![
            cleanup_pending_record("startup-cleanup-a"),
            cleanup_pending_record("startup-cleanup-b"),
            cleanup_pending_record("startup-cleanup-c"),
        ],
        1,
    );
    let (recovery, source) = recovery(Arc::clone(&store));

    let report = recovery
        .recover_once()
        .await
        .expect("cleanup retention is successful startup truth");

    assert_eq!(report.pages(), 3);
    assert_eq!(report.outcomes().len(), 3);
    assert_eq!(report.cleanup_retained_count(), 3);
    assert_eq!(report.waiting_count(), 0);
    assert!(report.outcomes().iter().all(|outcome| {
        outcome.disposition() == WorkloadStartupDisposition::CleanupRetained
            && outcome.record().phase() == WorkloadSagaPhase::CleanupPending
    }));
    assert_eq!(store.recovery_reads.load(Ordering::Acquire), 3);
    assert_eq!(
        store.loads.load(Ordering::Acquire),
        6,
        "startup authenticates each page record and the teardown owner reloads each exact key"
    );
    assert_eq!(source.calls.load(Ordering::Acquire), 0);
}

#[tokio::test]
async fn stale_page_fails_before_any_lifecycle_owner() {
    let listed = provision_record(
        "startup-stale",
        WorkloadSagaPhase::IntentCommitted,
        nimbus_workloads::WorkloadActivationIntent::ActivateWhenAttached,
        nimbus_workloads::WorkloadPublicationIntent::Withheld,
    );
    let current = crate::workload_saga::test_support::first_proposed_candidate(&listed);
    let store = RecoveryStore::with_crossed_current(listed.clone(), current);
    let (recovery, source) = recovery(Arc::clone(&store));

    let error = recovery
        .recover_once()
        .await
        .expect_err("stale page truth must fail closed");

    assert!(matches!(
        error,
        WorkloadStartupRecoveryError::Crossed { ref key } if key == listed.key()
    ));
    assert_eq!(store.loads.load(Ordering::Acquire), 1);
    assert_eq!(source.calls.load(Ordering::Acquire), 0);
}

#[tokio::test]
async fn crossed_successor_at_the_same_revision_fails_before_lifecycle_route() {
    let base = crate::workload_saga::test_support::restart_observed_record(
        "startup-crossed-successor",
        nimbus_workloads::WorkloadRestartPolicy::Never,
    );
    let listed = crate::workload_saga::test_support::record_with_successor(&base, "listed");
    let current = crate::workload_saga::test_support::record_with_successor(&base, "current");
    assert_eq!(listed.revision(), current.revision());
    assert_eq!(
        listed.active_intent().generation(),
        current.active_intent().generation()
    );
    assert_ne!(listed.successor_intent(), current.successor_intent());
    let store = RecoveryStore::with_crossed_current(listed.clone(), current);
    let (recovery, source) = recovery(Arc::clone(&store));

    let error = recovery
        .recover_once()
        .await
        .expect_err("crossed successor truth must fail closed before lifecycle routing");

    assert!(matches!(
        error,
        WorkloadStartupRecoveryError::Crossed { ref key } if key == listed.key()
    ));
    assert_eq!(store.loads.load(Ordering::Acquire), 1);
    assert_eq!(source.calls.load(Ordering::Acquire), 0);
}

#[tokio::test]
async fn unavailable_corrupt_and_ambiguous_store_fail_closed_before_restart_discovery() {
    for expected in [
        WorkloadSagaStoreError::Unavailable,
        WorkloadSagaStoreError::Corrupt,
        WorkloadSagaStoreError::Ambiguous,
    ] {
        let store = RecoveryStore::with_recovery_error(expected.clone());
        let (recovery, source) = recovery(Arc::clone(&store));

        let error = recovery
            .recover_and_activate()
            .await
            .expect_err("untrusted all-phase truth must fail startup");

        assert!(matches!(
            error,
            WorkloadStartupRecoveryError::Store(ref observed) if observed == &expected
        ));
        assert_eq!(store.recovery_reads.load(Ordering::Acquire), 1);
        assert_eq!(store.restart_reads.load(Ordering::Acquire), 0);
        assert_eq!(source.calls.load(Ordering::Acquire), 0);
    }
}

#[tokio::test]
async fn page_budget_fails_closed_at_the_exact_bound() {
    let records = (0..=MAX_STARTUP_RECOVERY_PAGES)
        .map(|index| cleanup_pending_record(&format!("startup-bound-{index:03}")))
        .collect();
    let store = RecoveryStore::new(records, 1);
    let (recovery, source) = recovery(Arc::clone(&store));

    let error = recovery
        .recover_once()
        .await
        .expect_err("the next page beyond the hard bound must fail closed");

    assert!(matches!(error, WorkloadStartupRecoveryError::PageLimit));
    assert_eq!(
        store.recovery_reads.load(Ordering::Acquire),
        MAX_STARTUP_RECOVERY_PAGES
    );
    assert_eq!(source.calls.load(Ordering::Acquire), 0);
}

#[tokio::test]
async fn restart_watch_activates_only_after_the_all_phase_pass() {
    let store = RecoveryStore::new(Vec::new(), 1);
    let (recovery, source) = recovery(Arc::clone(&store));

    let report = recovery
        .recover_and_activate()
        .await
        .expect("empty durable startup truth should activate discovery");

    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        while store.restart_reads.load(Ordering::Acquire) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("activated restart discovery should complete its first bounded read");

    assert_eq!(report.pages(), 1);
    assert!(report.outcomes().is_empty());
    assert_eq!(store.recovery_reads.load(Ordering::Acquire), 1);
    assert!(store.restart_reads.load(Ordering::Acquire) >= 1);
    assert_eq!(source.calls.load(Ordering::Acquire), 0);
}

#[path = "tests/routes.rs"]
mod routes;
