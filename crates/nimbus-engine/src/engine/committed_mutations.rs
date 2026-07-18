use std::collections::HashMap;
use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::sync::{Arc, Weak};

use nimbus_core::{CommitEntry, Error, SequenceNumber, TableName, TenantEventRecord, TenantId};
use nimbus_storage::MAX_DURABLE_JOURNAL_STREAM_LIMIT;
use tokio::sync::mpsc;
#[cfg(any(test, feature = "test-hooks"))]
use tokio::sync::oneshot;

use crate::{Engine, tenant::TenantRuntime};

thread_local! {
    static COMMITTED_MUTATION_OBSERVER_DISPATCH_ACTIVE: std::cell::Cell<bool> = const {
        std::cell::Cell::new(false)
    };
}

#[cfg(test)]
static PANIC_PROVIDER_CATCH_UP_FOR_TESTING: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashSet<TenantId>>,
> = std::sync::OnceLock::new();

#[cfg(test)]
fn panic_provider_catch_up_for_testing(tenant_id: &TenantId) {
    if PANIC_PROVIDER_CATCH_UP_FOR_TESTING
        .get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()))
        .lock()
        .expect("provider catch-up panic test-hook lock should not be poisoned")
        .remove(tenant_id)
    {
        panic!("injected provider catch-up task panic");
    }
}

#[cfg(not(test))]
fn panic_provider_catch_up_for_testing(_tenant_id: &TenantId) {}

/// Number of catch-up journal pages a tenant may still read before the next
/// one fails. One-shot: the injected failure is consumed when it fires, so a
/// successor task exercises the real recovery path.
#[cfg(test)]
static FAIL_PROVIDER_CATCH_UP_PAGE_FOR_TESTING: std::sync::OnceLock<
    std::sync::Mutex<HashMap<TenantId, usize>>,
> = std::sync::OnceLock::new();

#[cfg(test)]
fn fail_provider_catch_up_page_for_testing(tenant_id: &TenantId) -> nimbus_core::Result<()> {
    let mut injected = FAIL_PROVIDER_CATCH_UP_PAGE_FOR_TESTING
        .get_or_init(|| std::sync::Mutex::new(HashMap::new()))
        .lock()
        .expect("provider catch-up page-failure test-hook lock should not be poisoned");
    let Some(remaining) = injected.get_mut(tenant_id) else {
        return Ok(());
    };
    if *remaining == 0 {
        injected.remove(tenant_id);
        return Err(Error::Internal(
            "injected provider catch-up page read failure".to_string(),
        ));
    }
    *remaining -= 1;
    Ok(())
}

#[cfg(not(test))]
fn fail_provider_catch_up_page_for_testing(_tenant_id: &TenantId) -> nimbus_core::Result<()> {
    Ok(())
}

/// Record counts returned by each journal page a provider catch-up read, so
/// tests can prove the tail is paged rather than materialised whole.
#[cfg(test)]
static PROVIDER_CATCH_UP_PAGE_READS_FOR_TESTING: std::sync::OnceLock<
    std::sync::Mutex<HashMap<TenantId, Vec<usize>>>,
> = std::sync::OnceLock::new();

#[cfg(test)]
fn provider_catch_up_page_reads_lock_for_testing()
-> std::sync::MutexGuard<'static, HashMap<TenantId, Vec<usize>>> {
    PROVIDER_CATCH_UP_PAGE_READS_FOR_TESTING
        .get_or_init(|| std::sync::Mutex::new(HashMap::new()))
        .lock()
        .expect("provider catch-up page-read test-hook lock should not be poisoned")
}

#[cfg(test)]
fn record_provider_catch_up_page_for_testing(tenant_id: &TenantId, record_count: usize) {
    provider_catch_up_page_reads_lock_for_testing()
        .entry(tenant_id.clone())
        .or_default()
        .push(record_count);
}

#[cfg(not(test))]
fn record_provider_catch_up_page_for_testing(_tenant_id: &TenantId, _record_count: usize) {}

#[cfg(test)]
pub(crate) fn provider_catch_up_page_reads_for_testing(tenant_id: &TenantId) -> Vec<usize> {
    provider_catch_up_page_reads_lock_for_testing()
        .get(tenant_id)
        .cloned()
        .unwrap_or_default()
}

struct CatchUpOwnership {
    runtime: Arc<TenantRuntime>,
    first_sequence: SequenceNumber,
    requested_through: SequenceNumber,
    armed: bool,
}

impl CatchUpOwnership {
    fn new(
        runtime: Arc<TenantRuntime>,
        first_sequence: SequenceNumber,
        requested_through: SequenceNumber,
    ) -> Self {
        Self {
            runtime,
            first_sequence,
            requested_through,
            armed: true,
        }
    }

    fn take_request(&mut self) -> Option<(SequenceNumber, SequenceNumber)> {
        let request = self
            .runtime
            .take_committed_mutation_observer_catch_up_request()?;
        self.first_sequence = request.0;
        self.requested_through = request.1;
        Some(request)
    }

    fn complete(&mut self) -> bool {
        let has_more = self.runtime.complete_committed_mutation_observer_catch_up();
        if !has_more {
            self.armed = false;
        }
        has_more
    }

    fn abandon(&mut self, first_sequence: SequenceNumber, requested_through: SequenceNumber) {
        self.runtime
            .abandon_committed_mutation_observer_catch_up(first_sequence, requested_through);
        self.armed = false;
    }
}

impl Drop for CatchUpOwnership {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        self.runtime.abandon_committed_mutation_observer_catch_up(
            self.first_sequence,
            self.requested_through,
        );
        self.runtime
            .record_committed_mutation_observer_catch_up_enqueue_failure();
        tracing::error!(
            tenant = %self.runtime.tenant_id(),
            first_sequence = %self.first_sequence,
            requested_through = %self.requested_through,
            "provider catch-up observer task unwound before releasing ownership; request was republished"
        );
    }
}

#[cfg(test)]
struct CatchUpTaskCountGuard(Arc<TenantRuntime>);

#[cfg(test)]
impl Drop for CatchUpTaskCountGuard {
    fn drop(&mut self) {
        self.0
            .record_committed_mutation_observer_catch_up_task_finished();
    }
}

/// A durable mutation that has been applied to the tenant's serving state.
#[derive(Clone, Debug)]
pub struct CommittedMutationEvent {
    pub tenant_id: TenantId,
    pub commit: CommitEntry,
}

/// Opaque identity for one loaded tenant-runtime generation.
///
/// Observers that retain per-runtime state can use this token to distinguish a
/// replayed replacement from the evicted runtime that preceded it without
/// retaining the runtime itself.
#[doc(hidden)]
#[derive(Clone)]
pub struct TenantRuntimeObserverIdentity {
    lifetime: Weak<()>,
}

impl TenantRuntimeObserverIdentity {
    pub(crate) fn new(lifetime: &Arc<()>) -> Self {
        Self {
            lifetime: Arc::downgrade(lifetime),
        }
    }

    pub fn same_runtime(&self, other: &Self) -> bool {
        Weak::ptr_eq(&self.lifetime, &other.lifetime)
    }

    pub fn is_live(&self) -> bool {
        self.lifetime.strong_count() != 0
    }
}

/// Observer for committed mutation events.
pub trait CommittedMutationObserver: Send + Sync {
    fn committed_mutation_applied(&self, event: CommittedMutationEvent);

    /// Returns work spawned after this observer accepted callbacks.
    ///
    /// The default describes an observer that finishes before returning.
    #[doc(hidden)]
    fn spawned_work_stats(&self, _tenant_id: &TenantId) -> CommittedMutationObserverWorkStats {
        CommittedMutationObserverWorkStats::default()
    }

    /// Waits for work that the callback spawned after accepting an event.
    ///
    /// The default is appropriate for callbacks that finish their work before
    /// returning. This hidden hook only extends the test flush seam; production
    /// dispatch never waits on it.
    #[doc(hidden)]
    fn flush_spawned_work_for_testing(
        &self,
        _tenant_id: &TenantId,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async {})
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CommittedMutationObserverWorkStats {
    pub depth: usize,
    pub capacity: usize,
    pub high_watermark: usize,
    pub high_water_warning_count: u64,
    pub cap_breach_count: u64,
    pub dropped_event_count: u64,
    pub poisoned: bool,
}

/// A table schema or collection metadata change applied to a tenant.
#[derive(Clone, Debug)]
pub struct TableSchemaChangeEvent {
    pub tenant_id: TenantId,
    pub table: TableName,
}

/// Observer for table schema or collection metadata changes.
pub trait TableSchemaChangeObserver: Send + Sync {
    fn table_schema_changed(&self, event: TableSchemaChangeEvent);
}

pub(super) type CommittedMutationObserverRegistry =
    HashMap<&'static str, Arc<dyn CommittedMutationObserver>>;
pub(super) type TableSchemaChangeObserverRegistry =
    HashMap<&'static str, Arc<dyn TableSchemaChangeObserver>>;

pub(crate) struct CommittedMutationObserverDispatch {
    observers: Vec<Arc<dyn CommittedMutationObserver>>,
    events: Vec<CommittedMutationEvent>,
    completion: Option<DispatchCompletion>,
}

pub(crate) enum CommittedMutationObserverMessage {
    Dispatch(CommittedMutationObserverDispatch),
    #[cfg(any(test, feature = "test-hooks"))]
    Fence(oneshot::Sender<()>),
    Close,
}

impl CommittedMutationObserverDispatch {
    pub(crate) fn event_count(&self) -> usize {
        self.events.len()
    }

    fn run(self) {
        for event in self.events {
            for observer in &self.observers {
                observer.committed_mutation_applied(event.clone());
            }
        }
    }

    pub(crate) fn arm_completion(&mut self, runtime: std::sync::Weak<TenantRuntime>) {
        debug_assert!(
            self.completion.is_none(),
            "observer dispatch completion must be armed exactly once"
        );
        self.completion = Some(DispatchCompletion {
            runtime,
            event_count: self.event_count(),
        });
    }

    pub(crate) fn disarm_completion(&mut self) {
        self.completion = None;
    }
}

struct DispatchCompletion {
    runtime: std::sync::Weak<TenantRuntime>,
    event_count: usize,
}

impl Drop for DispatchCompletion {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.upgrade() {
            runtime.complete_committed_mutation_observer_dispatch(self.event_count);
        }
    }
}

fn run_dispatch(dispatch: CommittedMutationObserverDispatch) -> bool {
    COMMITTED_MUTATION_OBSERVER_DISPATCH_ACTIVE.with(|active| {
        let previous = active.replace(true);
        debug_assert!(!previous, "observer dispatch callbacks must not nest");
        struct Reset<'a> {
            active: &'a std::cell::Cell<bool>,
            previous: bool,
        }
        impl Drop for Reset<'_> {
            fn drop(&mut self) {
                self.active.set(self.previous);
            }
        }
        let _reset = Reset { active, previous };
        catch_unwind(AssertUnwindSafe(|| dispatch.run())).is_ok()
    })
}

pub(crate) fn on_committed_mutation_observer_dispatcher() -> bool {
    COMMITTED_MUTATION_OBSERVER_DISPATCH_ACTIVE.with(std::cell::Cell::get)
}

pub(crate) async fn run_committed_mutation_observer_dispatcher(
    mut receiver: mpsc::UnboundedReceiver<CommittedMutationObserverMessage>,
    runtime: std::sync::Weak<TenantRuntime>,
) {
    while let Some(message) = receiver.recv().await {
        match message {
            CommittedMutationObserverMessage::Dispatch(dispatch) => {
                if !run_dispatch(dispatch) {
                    if let Some(runtime) = runtime.upgrade() {
                        runtime.poison_committed_mutation_observers(
                            "committed mutation observer callback panicked",
                        );
                    }
                    receiver.close();
                    while let Some(message) = receiver.recv().await {
                        drop(message);
                    }
                    break;
                }
            }
            #[cfg(any(test, feature = "test-hooks"))]
            CommittedMutationObserverMessage::Fence(completed) => {
                let _ = completed.send(());
            }
            CommittedMutationObserverMessage::Close => {
                receiver.close();
                while let Some(message) = receiver.recv().await {
                    match message {
                        CommittedMutationObserverMessage::Dispatch(dispatch) => {
                            if !run_dispatch(dispatch) {
                                if let Some(runtime) = runtime.upgrade() {
                                    runtime.poison_committed_mutation_observers(
                                        "committed mutation observer callback panicked while draining",
                                    );
                                }
                                receiver.close();
                                while let Some(message) = receiver.recv().await {
                                    drop(message);
                                }
                                break;
                            }
                        }
                        #[cfg(any(test, feature = "test-hooks"))]
                        CommittedMutationObserverMessage::Fence(completed) => {
                            let _ = completed.send(());
                        }
                        CommittedMutationObserverMessage::Close => {}
                    }
                }
                break;
            }
        }
    }
    if let Some(runtime) = runtime.upgrade() {
        runtime.mark_committed_mutation_observers_drained();
    }
}

impl Engine {
    /// Installs a named committed-mutation observer.
    ///
    /// Calling this more than once with the same name is idempotent. The first
    /// observer wins so repeated router construction does not duplicate
    /// projection work for the same engine instance.
    ///
    /// Callbacks run serially on a per-tenant dispatcher. They must not call
    /// the engine's blocking mutation APIs synchronously; those calls return
    /// [`Error::InvalidInput`] to prevent dispatcher self-deadlock. Spawn
    /// asynchronous mutation work instead. Publishing never blocks on observer
    /// backlog. Installations must budget callback throughput so the queue
    /// stays below `NIMBUS_COMMITTED_OBSERVER_QUEUE_HIGH_WATERMARK` (3,072
    /// events by default) and its hard
    /// `NIMBUS_COMMITTED_OBSERVER_QUEUE_CAPACITY` (4,096 by default). A
    /// high-water crossing emits one warning until the queue recovers. A hard
    /// cap breach cannot safely block the publisher, so it loudly refuses the
    /// breaching dispatch, poisons and closes that tenant's dispatcher after
    /// accepted work drains, and exposes the failure in `MutationJournalStats`.
    /// Treat a poisoned dispatcher as a fatal health condition requiring
    /// operator intervention; already-durable observer events are not retried.
    pub fn install_committed_mutation_observer(
        &self,
        name: &'static str,
        observer: Arc<dyn CommittedMutationObserver>,
    ) {
        self.committed_mutation_observers
            .write()
            .expect("committed mutation observer registry lock should not be poisoned")
            .entry(name)
            .or_insert(observer);
    }

    /// Waits until every observer dispatch already accepted for `tenant_id`
    /// has completed, including work spawned by observers that implement the
    /// drain hook. This is a test seam for isolating process-wide commit faults
    /// from ordered observer work left by fixture setup.
    #[cfg(any(test, feature = "test-hooks"))]
    pub async fn flush_committed_mutation_observers_for_testing(
        &self,
        tenant_id: &TenantId,
    ) -> nimbus_core::Result<()> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        runtime
            .flush_committed_mutation_observers_for_testing()
            .await?;
        let observers = self
            .committed_mutation_observers
            .read()
            .expect("committed mutation observer registry lock should not be poisoned")
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for observer in observers {
            observer.flush_spawned_work_for_testing(tenant_id).await;
        }
        Ok(())
    }

    pub(crate) fn enqueue_applied_commit_batch_observers(
        &self,
        runtime: Arc<TenantRuntime>,
        applied: &[CommitEntry],
    ) {
        let observers = self
            .committed_mutation_observers
            .read()
            .expect("committed mutation observer registry lock should not be poisoned")
            .values()
            .cloned()
            .collect::<Vec<_>>();
        if observers.is_empty() {
            return;
        }
        let events = applied
            .iter()
            .filter(|commit| !commit.writes.is_empty())
            .cloned()
            .map(|commit| CommittedMutationEvent {
                tenant_id: runtime.tenant_id().clone(),
                commit,
            })
            .collect::<Vec<_>>();
        if events.is_empty() {
            return;
        }
        if let Err(error) = runtime.enqueue_committed_mutation_observer_dispatch(
            CommittedMutationObserverDispatch {
                observers,
                events,
                completion: None,
            },
        ) {
            tracing::error!(
                tenant = %runtime.tenant_id(),
                %error,
                "committed mutation observer dispatch was refused"
            );
        }
    }

    pub(crate) fn enqueue_provider_catch_up_commit_observers(
        &self,
        runtime: Arc<TenantRuntime>,
        applied: &[CommitEntry],
    ) -> Option<tokio::sync::oneshot::Receiver<nimbus_core::Result<()>>> {
        let observers = self
            .committed_mutation_observers
            .read()
            .expect("committed mutation observer registry lock should not be poisoned")
            .values()
            .cloned()
            .collect::<Vec<_>>();
        if observers.is_empty() {
            return None;
        }
        let first_sequence = applied
            .iter()
            .filter(|commit| !commit.writes.is_empty())
            .map(|commit| commit.sequence)
            .next()?;
        let requested_through = applied.last()?.sequence;
        if !runtime.request_committed_mutation_observer_catch_up(first_sequence, requested_through)
        {
            return None;
        }
        let ownership = CatchUpOwnership::new(runtime.clone(), first_sequence, requested_through);
        let (completed, completion) = tokio::sync::oneshot::channel();
        let runtime_on_spawn_failure = runtime.clone();
        let catch_up = async move {
            #[cfg(test)]
            runtime.record_committed_mutation_observer_catch_up_task_started();
            #[cfg(test)]
            let _task_count = CatchUpTaskCountGuard(runtime.clone());
            let result =
                run_provider_catch_up_observers(runtime.clone(), observers, ownership).await;
            if let Err(error) = &result {
                runtime.record_committed_mutation_observer_catch_up_enqueue_failure();
                tracing::error!(
                    tenant = %runtime.tenant_id(),
                    %error,
                    "provider catch-up observer enqueue failed; other tenants and scheduler work remain active"
                );
            }
            let _ = completed.send(result);
        };
        match self.try_spawn_background("provider_catch_up_observers", catch_up) {
            Ok(_) => Some(completion),
            Err((error, catch_up)) => {
                drop(catch_up);
                tracing::warn!(
                    tenant = %runtime_on_spawn_failure.tenant_id(),
                    error = %error,
                    "engine quiesce rejected provider catch-up observer task"
                );
                let (failed, failure) = tokio::sync::oneshot::channel();
                let _ = failed.send(Err(error));
                Some(failure)
            }
        }
    }

    /// Lets the next provider catch-up for `tenant_id` read `pages` journal
    /// pages before its following page read fails once.
    #[cfg(test)]
    pub(crate) fn fail_provider_catch_up_after_pages_for_testing(
        &self,
        tenant_id: TenantId,
        pages: usize,
    ) {
        FAIL_PROVIDER_CATCH_UP_PAGE_FOR_TESTING
            .get_or_init(|| std::sync::Mutex::new(HashMap::new()))
            .lock()
            .expect("provider catch-up page-failure test-hook lock should not be poisoned")
            .insert(tenant_id, pages);
    }

    #[cfg(test)]
    pub(crate) fn panic_next_provider_catch_up_for_testing(&self, tenant_id: TenantId) {
        PANIC_PROVIDER_CATCH_UP_FOR_TESTING
            .get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()))
            .lock()
            .expect("provider catch-up panic test-hook lock should not be poisoned")
            .insert(tenant_id);
    }

    pub(crate) fn apply_committed_mutation_observer_work_stats(
        &self,
        tenant_id: &TenantId,
        stats: &mut crate::tenant::MutationJournalStats,
    ) {
        let aggregate = self
            .committed_mutation_observers
            .read()
            .expect("committed mutation observer registry lock should not be poisoned")
            .values()
            .map(|observer| observer.spawned_work_stats(tenant_id))
            .fold(
                CommittedMutationObserverWorkStats::default(),
                |mut aggregate, observer| {
                    aggregate.depth = aggregate.depth.saturating_add(observer.depth);
                    aggregate.capacity = aggregate.capacity.saturating_add(observer.capacity);
                    aggregate.high_watermark = aggregate
                        .high_watermark
                        .saturating_add(observer.high_watermark);
                    aggregate.high_water_warning_count = aggregate
                        .high_water_warning_count
                        .saturating_add(observer.high_water_warning_count);
                    aggregate.cap_breach_count = aggregate
                        .cap_breach_count
                        .saturating_add(observer.cap_breach_count);
                    aggregate.dropped_event_count = aggregate
                        .dropped_event_count
                        .saturating_add(observer.dropped_event_count);
                    aggregate.poisoned |= observer.poisoned;
                    aggregate
                },
            );
        stats.observer_spawned_work_depth = aggregate.depth;
        stats.observer_spawned_work_capacity = aggregate.capacity;
        stats.observer_spawned_work_high_watermark = aggregate.high_watermark;
        stats.observer_spawned_work_high_water_warning_count = aggregate.high_water_warning_count;
        stats.observer_spawned_work_cap_breach_count = aggregate.cap_breach_count;
        stats.observer_spawned_work_dropped_event_count = aggregate.dropped_event_count;
        stats.observer_spawned_work_poisoned = aggregate.poisoned;
    }

    /// Returns an opaque identity for the tenant runtime currently responsible
    /// for observer work.
    #[doc(hidden)]
    pub fn committed_mutation_observer_runtime_identity(
        &self,
        tenant_id: &TenantId,
    ) -> nimbus_core::Result<TenantRuntimeObserverIdentity> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        Ok(runtime.observer_identity())
    }

    /// Installs a named table-schema observer.
    ///
    /// Calling this more than once with the same name is idempotent. The first
    /// observer wins so repeated router construction does not duplicate
    /// projection work for the same engine instance.
    pub fn install_table_schema_change_observer(
        &self,
        name: &'static str,
        observer: Arc<dyn TableSchemaChangeObserver>,
    ) {
        self.table_schema_change_observers
            .write()
            .expect("table schema change observer registry lock should not be poisoned")
            .entry(name)
            .or_insert(observer);
    }

    pub(crate) fn notify_table_schema_change_observers(
        &self,
        tenant_id: &TenantId,
        table: &TableName,
    ) {
        let observers = self
            .table_schema_change_observers
            .read()
            .expect("table schema change observer registry lock should not be poisoned")
            .values()
            .cloned()
            .collect::<Vec<_>>();
        if observers.is_empty() {
            return;
        }

        let event = TableSchemaChangeEvent {
            tenant_id: tenant_id.clone(),
            table: table.clone(),
        };
        for observer in observers {
            observer.table_schema_changed(event.clone());
        }
    }
}

async fn run_provider_catch_up_observers(
    runtime: Arc<TenantRuntime>,
    observers: Vec<Arc<dyn CommittedMutationObserver>>,
    mut ownership: CatchUpOwnership,
) -> nimbus_core::Result<()> {
    panic_provider_catch_up_for_testing(runtime.tenant_id());
    let chunk_size = runtime.committed_mutation_observer_catch_up_chunk_size();
    let mut delivered_through = None::<SequenceNumber>;
    loop {
        let Some((requested_first, requested_through)) = ownership.take_request() else {
            if ownership.complete() {
                continue;
            }
            return Ok(());
        };
        if delivered_through.is_some_and(|delivered| requested_through <= delivered) {
            continue;
        }
        let first_sequence = delivered_through
            .and_then(|delivered| delivered.0.checked_add(1).map(SequenceNumber))
            .map_or(requested_first, |next| next.max(requested_first));
        if let Err(error) = deliver_provider_catch_up_tail(
            &runtime,
            &observers,
            first_sequence,
            requested_through,
            chunk_size,
        )
        .await
        {
            ownership.abandon(first_sequence, requested_through);
            return Err(error);
        }
        delivered_through = Some(requested_through);
    }
}

/// Streams `first_sequence..=requested_through` to the observer queue one
/// bounded page at a time.
///
/// Reading the tail in pages is what keeps catch-up memory proportional to the
/// dispatch chunk budget instead of the tenant's whole journal history: a
/// tenant that missed a large tail would otherwise materialise every record of
/// it before dispatching the first event, defeating the bounded observer queue
/// this path is built around.
///
/// Page boundaries stay invisible to observers. Zero-write records are dropped
/// after the read, so a page yields fewer events than it read; carrying the
/// remainder into the next page keeps dispatches the same full `chunk_size`
/// batches an unpaged read produced, while holding at most one chunk plus one
/// page in memory.
async fn deliver_provider_catch_up_tail(
    runtime: &Arc<TenantRuntime>,
    observers: &[Arc<dyn CommittedMutationObserver>],
    first_sequence: SequenceNumber,
    requested_through: SequenceNumber,
    chunk_size: usize,
) -> nimbus_core::Result<()> {
    let page_limit = chunk_size.min(MAX_DURABLE_JOURNAL_STREAM_LIMIT);
    let mut cursor = SequenceNumber(first_sequence.0.saturating_sub(1));
    let mut pending = Vec::<CommittedMutationEvent>::new();
    loop {
        fail_provider_catch_up_page_for_testing(runtime.tenant_id())?;
        let page = runtime
            .store
            .stream_durable_journal_async(&runtime.read_storage, cursor, page_limit)
            .await?;
        record_provider_catch_up_page_for_testing(runtime.tenant_id(), page.records.len());
        // The durable head is the cheapest proof that the requested frontier is
        // reachable at all, so a journal that never got there still fails
        // before any event is handed to an observer.
        if page.latest_sequence < requested_through {
            return Err(Error::Internal(format!(
                "provider catch-up journal re-read did not reach requested sequence {requested_through} from {first_sequence}"
            )));
        }
        let page_len = page.records.len();
        let records = page
            .records
            .into_iter()
            .take_while(|record| record.sequence <= requested_through)
            .collect::<Vec<_>>();
        let Some(last_sequence) = records.last().map(|record| record.sequence) else {
            return Err(Error::Internal(format!(
                "provider catch-up journal re-read made no progress toward sequence {requested_through} from {cursor}"
            )));
        };
        if records.len() < page_len && last_sequence != requested_through {
            return Err(Error::Internal(format!(
                "provider catch-up journal re-read did not reach requested sequence {requested_through} from {first_sequence}"
            )));
        }
        pending.extend(
            records
                .into_iter()
                .map(|record| TenantEventRecord::as_commit_entry(&record))
                .filter(|commit| !commit.writes.is_empty())
                .map(|commit| CommittedMutationEvent {
                    tenant_id: runtime.tenant_id().clone(),
                    commit,
                }),
        );
        let reached_frontier = last_sequence >= requested_through;
        let mut dispatched = 0;
        while pending.len() - dispatched >= chunk_size
            || (reached_frontier && dispatched < pending.len())
        {
            let end = pending.len().min(dispatched + chunk_size);
            runtime
                .enqueue_committed_mutation_observer_catch_up_dispatch(
                    CommittedMutationObserverDispatch {
                        observers: observers.to_vec(),
                        events: pending[dispatched..end].to_vec(),
                        completion: None,
                    },
                )
                .await?;
            dispatched = end;
        }
        pending.drain(..dispatched);
        if reached_frontier {
            return Ok(());
        }
        cursor = last_sequence;
    }
}
