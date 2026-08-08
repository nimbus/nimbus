use std::collections::{BTreeSet, VecDeque};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use nimbus_core::TenantId;
use nimbus_workloads::{
    WorkloadRestartCandidatePageRequest, WorkloadSagaCommit, WorkloadSagaExpected,
    WorkloadSagaFuture, WorkloadSagaKey, WorkloadSagaPage, WorkloadSagaPageRequest,
    WorkloadSagaStore, WorkloadSagaTenantPage, WorkloadSagaTenantPageRequest,
};

use super::*;
use crate::workload_saga::test_support;

struct FakeClock {
    now: AtomicU64,
    waits: Mutex<Vec<u64>>,
    wake: Notify,
}

impl FakeClock {
    fn new(now: u64) -> Arc<Self> {
        Arc::new(Self {
            now: AtomicU64::new(now),
            waits: Mutex::new(Vec::new()),
            wake: Notify::new(),
        })
    }

    fn advance_to(&self, now: u64) {
        self.now.store(now, Ordering::Release);
        self.wake.notify_waiters();
    }

    fn waits(&self) -> Vec<u64> {
        self.waits
            .lock()
            .expect("fake clock wait log should be healthy")
            .clone()
    }
}

impl RestartClock for FakeClock {
    fn now_unix_millis(&self) -> WorkloadRestartNotBeforeUnixMillis {
        WorkloadRestartNotBeforeUnixMillis::new(self.now.load(Ordering::Acquire))
    }

    fn wait_until(
        &self,
        deadline: WorkloadRestartNotBeforeUnixMillis,
        cancellation: &WorkloadRestartCancellationToken,
    ) -> RestartWaitFuture<'_> {
        self.waits
            .lock()
            .expect("fake clock wait log should be healthy")
            .push(deadline.as_u64());
        let mut cancellation = cancellation.subscribe();
        Box::pin(async move {
            loop {
                if *cancellation.borrow() {
                    return RestartWait::Cancelled;
                }
                if self.now.load(Ordering::Acquire) >= deadline.as_u64() {
                    return RestartWait::DeadlineReached;
                }
                tokio::select! {
                    changed = cancellation.changed() => {
                        if changed.is_err() || *cancellation.borrow() {
                            return RestartWait::Cancelled;
                        }
                    }
                    () = self.wake.notified() => {}
                }
            }
        })
    }
}

#[derive(Clone)]
struct PageSpec {
    records: Vec<WorkloadSagaRecord>,
    has_more: bool,
}

struct WatchStore {
    pages: Mutex<VecDeque<Result<PageSpec, WorkloadSagaStoreError>>>,
    page_calls: AtomicUsize,
    load_calls: AtomicUsize,
    cas_calls: AtomicUsize,
    limits: Mutex<Vec<u16>>,
    cursors: Mutex<Vec<Option<nimbus_workloads::WorkloadSagaId>>>,
}

impl WatchStore {
    fn repeating(spec: PageSpec) -> Arc<Self> {
        Self::from_pages([spec])
    }

    fn from_pages(pages: impl IntoIterator<Item = PageSpec>) -> Arc<Self> {
        Arc::new(Self {
            pages: Mutex::new(pages.into_iter().map(Ok).collect()),
            page_calls: AtomicUsize::new(0),
            load_calls: AtomicUsize::new(0),
            cas_calls: AtomicUsize::new(0),
            limits: Mutex::new(Vec::new()),
            cursors: Mutex::new(Vec::new()),
        })
    }

    fn next_page(&self) -> Result<PageSpec, WorkloadSagaStoreError> {
        let mut pages = self
            .pages
            .lock()
            .expect("watch page queue should be healthy");
        if pages.len() > 1 {
            pages.pop_front().expect("watch page queue is non-empty")
        } else {
            pages
                .front()
                .expect("watch page queue is non-empty")
                .clone()
        }
    }
}

impl WorkloadSagaStore for WatchStore {
    fn load<'a>(
        &'a self,
        _key: &'a WorkloadSagaKey,
    ) -> WorkloadSagaFuture<'a, Option<WorkloadSagaRecord>> {
        Box::pin(async move {
            self.load_calls.fetch_add(1, Ordering::AcqRel);
            Err(WorkloadSagaStoreError::Unavailable)
        })
    }

    fn compare_and_swap<'a>(
        &'a self,
        _expected: WorkloadSagaExpected,
        _next: WorkloadSagaRecord,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaCommit> {
        Box::pin(async move {
            self.cas_calls.fetch_add(1, Ordering::AcqRel);
            Err(WorkloadSagaStoreError::Unavailable)
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
        Box::pin(async move {
            self.page_calls.fetch_add(1, Ordering::AcqRel);
            self.limits
                .lock()
                .expect("watch limit log should be healthy")
                .push(request.limit());
            self.cursors
                .lock()
                .expect("watch cursor log should be healthy")
                .push(request.after().map(|cursor| cursor.saga_id().clone()));
            let spec = self.next_page()?;
            WorkloadRestartCandidatePage::new(&request, spec.records, spec.has_more)
        })
    }

    fn list_for_tenant<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        request: WorkloadSagaTenantPageRequest,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaTenantPage> {
        Box::pin(async move { WorkloadSagaTenantPage::new(tenant_id, &request, Vec::new(), false) })
    }
}

#[derive(Default)]
struct KeyedSupervisor {
    calls: AtomicUsize,
    started: AtomicUsize,
    keys: Mutex<BTreeSet<String>>,
}

impl RestartSupervisor for KeyedSupervisor {
    fn track(&self, record: WorkloadSagaRecord) -> Result<RestartTrack, String> {
        self.calls.fetch_add(1, Ordering::AcqRel);
        let epoch = record
            .restart_state()
            .active()
            .map_or(0, |active| active.admission().restart_epoch().as_u64());
        let key = format!("{}:{epoch}", record.saga_id());
        if self
            .keys
            .lock()
            .expect("supervisor key set should be healthy")
            .insert(key)
        {
            self.started.fetch_add(1, Ordering::AcqRel);
            Ok(RestartTrack::Started)
        } else {
            Ok(RestartTrack::Joined)
        }
    }
}

fn watch(
    store: Arc<WatchStore>,
    supervisor: Arc<KeyedSupervisor>,
    clock: Arc<FakeClock>,
    cancellation: WorkloadRestartCancellationToken,
    page_size: usize,
) -> Arc<DurableRestartWatch> {
    Arc::new(
        DurableRestartWatch::new(
            NonZeroUsize::new(page_size).expect("test page size is nonzero"),
            NonZeroU64::new(1_000).expect("test rescan interval is nonzero"),
            clock,
            cancellation,
            Arc::new(WorkloadSagaCoordinator::new(store)),
            supervisor,
        )
        .expect("test restart watch should validate"),
    )
}

async fn wait_for_wait(clock: &FakeClock) {
    for _ in 0..100 {
        if !clock.waits().is_empty() {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("watch did not enter its injected clock wait");
}

#[tokio::test]
async fn automatic_watch_loads_one_bounded_durable_page() {
    let store = WatchStore::repeating(PageSpec {
        records: Vec::new(),
        has_more: false,
    });
    let supervisor = Arc::new(KeyedSupervisor::default());
    let watch = watch(
        store.clone(),
        supervisor.clone(),
        FakeClock::new(0),
        WorkloadRestartCancellationToken::new(),
        7,
    );

    let page = watch
        .load_durable_restart_page(None)
        .await
        .expect("one bounded page should load");

    assert!(page.records().is_empty());
    assert_eq!(store.page_calls.load(Ordering::Acquire), 1);
    assert_eq!(*store.limits.lock().unwrap(), vec![7]);
    assert_eq!(*store.cursors.lock().unwrap(), vec![None]);
    assert_eq!(store.load_calls.load(Ordering::Acquire), 0);
    assert_eq!(store.cas_calls.load(Ordering::Acquire), 0);
    assert_eq!(supervisor.calls.load(Ordering::Acquire), 0);
}

#[tokio::test]
async fn automatic_watch_does_not_busy_spin_before_deadline() {
    let candidate = test_support::scheduled_restart_record("watch-deadline", 500);
    let store = WatchStore::repeating(PageSpec {
        records: vec![candidate],
        has_more: false,
    });
    let supervisor = Arc::new(KeyedSupervisor::default());
    let clock = FakeClock::new(499);
    let cancellation = WorkloadRestartCancellationToken::new();
    let watch = watch(
        store.clone(),
        supervisor.clone(),
        clock.clone(),
        cancellation.clone(),
        8,
    );
    let task = tokio::spawn({
        let watch = watch.clone();
        async move { watch.bounded_restart_watch().await }
    });
    wait_for_wait(&clock).await;

    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
    assert_eq!(clock.waits(), vec![500]);
    assert_eq!(store.page_calls.load(Ordering::Acquire), 1);
    assert_eq!(supervisor.calls.load(Ordering::Acquire), 0);

    cancellation.cancel();
    assert_eq!(
        task.await.unwrap().unwrap(),
        RestartWait::Cancelled,
        "cancellation should wake the clock wait"
    );
}

#[tokio::test]
async fn automatic_watch_dispatches_each_due_epoch_once() {
    let candidate = test_support::scheduled_restart_record("watch-once", 0);
    let store = WatchStore::repeating(PageSpec {
        records: vec![candidate],
        has_more: false,
    });
    let supervisor = Arc::new(KeyedSupervisor::default());
    let watch = watch(
        store,
        supervisor.clone(),
        FakeClock::new(0),
        WorkloadRestartCancellationToken::new(),
        8,
    );

    watch.dispatch_each_due_epoch_once().await.unwrap();
    watch.dispatch_each_due_epoch_once().await.unwrap();

    assert_eq!(supervisor.calls.load(Ordering::Acquire), 2);
    assert_eq!(supervisor.started.load(Ordering::Acquire), 1);
}

#[tokio::test]
async fn automatic_watch_caps_each_sweep_and_rotates_cursor() {
    let mut records = (0..=MAX_RESTART_PAGES_PER_SWEEP)
        .map(|index| test_support::scheduled_restart_record(&format!("watch-budget-{index:03}"), 0))
        .collect::<Vec<_>>();
    records.sort_by(|left, right| left.saga_id().cmp(right.saga_id()));
    let pages = records
        .into_iter()
        .enumerate()
        .map(|(index, record)| PageSpec {
            records: vec![record],
            has_more: index < MAX_RESTART_PAGES_PER_SWEEP,
        })
        .collect::<Vec<_>>();
    let expected_resume_after = pages[MAX_RESTART_PAGES_PER_SWEEP - 1].records[0]
        .saga_id()
        .clone();
    let store = WatchStore::from_pages(pages);
    let supervisor = Arc::new(KeyedSupervisor::default());
    let watch = watch(
        store.clone(),
        supervisor.clone(),
        FakeClock::new(0),
        WorkloadRestartCancellationToken::new(),
        1,
    );

    let first = watch.dispatch_each_due_epoch_once().await.unwrap();
    assert_eq!(first.pages, MAX_RESTART_PAGES_PER_SWEEP);
    assert_eq!(first.candidates, MAX_RESTART_PAGES_PER_SWEEP);
    assert_eq!(
        store.page_calls.load(Ordering::Acquire),
        MAX_RESTART_PAGES_PER_SWEEP
    );
    assert_eq!(
        supervisor.started.load(Ordering::Acquire),
        MAX_RESTART_PAGES_PER_SWEEP
    );

    let second = watch.dispatch_each_due_epoch_once().await.unwrap();
    assert_eq!(second.pages, 1);
    assert_eq!(second.candidates, 1);
    assert_eq!(
        store.page_calls.load(Ordering::Acquire),
        MAX_RESTART_PAGES_PER_SWEEP + 1
    );
    assert_eq!(
        supervisor.started.load(Ordering::Acquire),
        MAX_RESTART_PAGES_PER_SWEEP + 1
    );
    assert_eq!(
        store.cursors.lock().unwrap().last(),
        Some(&Some(expected_resume_after)),
        "the second sweep must continue after the exact retained page cursor"
    );
}

#[test]
fn read_only_exit_hint_cannot_submit_or_execute_restart() {
    let store = WatchStore::repeating(PageSpec {
        records: Vec::new(),
        has_more: false,
    });
    let supervisor = Arc::new(KeyedSupervisor::default());
    let watch = watch(
        store.clone(),
        supervisor.clone(),
        FakeClock::new(0),
        WorkloadRestartCancellationToken::new(),
        8,
    );

    watch.hint_handle().notify(read_only_exit_hint());

    assert_eq!(store.page_calls.load(Ordering::Acquire), 0);
    assert_eq!(store.load_calls.load(Ordering::Acquire), 0);
    assert_eq!(store.cas_calls.load(Ordering::Acquire), 0);
    assert_eq!(supervisor.calls.load(Ordering::Acquire), 0);
}

#[tokio::test]
async fn watch_cancellation_cancels_waiter_not_durable_work() {
    let candidate = test_support::scheduled_restart_record("watch-cancel", 0);
    let durable = candidate.clone();
    let store = WatchStore::repeating(PageSpec {
        records: vec![candidate],
        has_more: false,
    });
    let supervisor = Arc::new(KeyedSupervisor::default());
    let clock = FakeClock::new(0);
    let cancellation = WorkloadRestartCancellationToken::new();
    let watch = watch(
        store.clone(),
        supervisor.clone(),
        clock.clone(),
        cancellation.clone(),
        8,
    );
    let task = tokio::spawn({
        let watch = watch.clone();
        async move { watch.bounded_restart_watch().await }
    });
    wait_for_wait(&clock).await;
    assert_eq!(supervisor.started.load(Ordering::Acquire), 1);

    cancellation.cancel();
    assert_eq!(task.await.unwrap().unwrap(), RestartWait::Cancelled);
    assert_eq!(store.cas_calls.load(Ordering::Acquire), 0);
    assert!(durable.restart_state().active().is_some());
}

#[test]
fn get_and_name_resolution_make_zero_restart_effects() {
    let store = WatchStore::repeating(PageSpec {
        records: Vec::new(),
        has_more: false,
    });
    let supervisor = Arc::new(KeyedSupervisor::default());
    let _watch = watch(
        store.clone(),
        supervisor.clone(),
        FakeClock::new(0),
        WorkloadRestartCancellationToken::new(),
        8,
    );

    let get_snapshot = || "read-only-workload-snapshot";
    let resolve_logical_name = || "read-only-logical-binding";
    assert_eq!(get_snapshot(), "read-only-workload-snapshot");
    assert_eq!(resolve_logical_name(), "read-only-logical-binding");
    assert_eq!(store.page_calls.load(Ordering::Acquire), 0);
    assert_eq!(store.load_calls.load(Ordering::Acquire), 0);
    assert_eq!(store.cas_calls.load(Ordering::Acquire), 0);
    assert_eq!(supervisor.calls.load(Ordering::Acquire), 0);
}

#[tokio::test]
async fn clock_rollback_delays_existing_deadline() {
    let clock = FakeClock::new(499);
    let cancellation = WorkloadRestartCancellationToken::new();
    let wait = tokio::spawn({
        let clock = clock.clone();
        let cancellation = cancellation.clone();
        async move {
            clock
                .wait_until(WorkloadRestartNotBeforeUnixMillis::new(500), &cancellation)
                .await
        }
    });
    wait_for_wait(&clock).await;
    clock.advance_to(400);
    tokio::task::yield_now().await;
    assert!(!wait.is_finished());
    clock.advance_to(500);
    assert_eq!(wait.await.unwrap(), RestartWait::DeadlineReached);
}
