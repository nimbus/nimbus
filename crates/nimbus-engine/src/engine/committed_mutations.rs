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
    projection_token: ProjectionToken,
    armed: bool,
}

impl CatchUpOwnership {
    fn new(
        runtime: Arc<TenantRuntime>,
        first_sequence: SequenceNumber,
        requested_through: SequenceNumber,
        projection_token: ProjectionToken,
    ) -> Self {
        Self {
            runtime,
            first_sequence,
            requested_through,
            projection_token,
            armed: true,
        }
    }

    fn take_request(&mut self) -> Option<(SequenceNumber, SequenceNumber, ProjectionToken)> {
        let request = self
            .runtime
            .take_committed_mutation_observer_catch_up_request()?;
        self.first_sequence = request.0;
        self.requested_through = request.1;
        self.projection_token = request.2;
        Some(request)
    }

    fn complete(&mut self) -> bool {
        let has_more = self.runtime.complete_committed_mutation_observer_catch_up();
        if !has_more {
            self.armed = false;
        }
        has_more
    }

    fn abandon(
        &mut self,
        first_sequence: SequenceNumber,
        requested_through: SequenceNumber,
        projection_token: ProjectionToken,
    ) {
        self.runtime.abandon_committed_mutation_observer_catch_up(
            first_sequence,
            requested_through,
            projection_token,
        );
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
            self.projection_token,
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
    /// Tables whose state is represented by this event. This includes schema
    /// and lifecycle scopes from zero-write durable records during provider
    /// catch-up, not only tables with document writes.
    pub affected_tables: Vec<TableName>,
    pub projection_token: ProjectionToken,
}

/// Durable source order represented by a system-table projection sample.
///
/// Incarnation is the durable tenant-lifecycle generation and is ordered first
/// so same-id recreation dominates every sample from the deleted tenant.
/// Within an incarnation, epoch zero is reserved for embedded process-local
/// sequence authority. Provider epochs are the durable committer-lease epochs
/// that authorized the source state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct ProjectionToken {
    /// Durable tenant generation. Production runtimes always use a positive
    /// value; zero is reserved for synthetic/default test samples.
    pub tenant_incarnation: u64,
    /// Durable committer-lease epoch, or zero for embedded sequence authority.
    pub lease_epoch: u64,
    /// Applied journal frontier covered by the sampled table state.
    pub durable_sequence: SequenceNumber,
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

/// One tenant runtime generation becoming reachable through an engine.
#[derive(Clone)]
pub struct TenantRuntimeLoadedEvent {
    pub tenant_id: TenantId,
    pub runtime_identity: TenantRuntimeObserverIdentity,
}

/// Observer for tenant-runtime load and replacement.
///
/// The engine schedules the returned future on its owned background executor.
/// Implementations therefore receive the same lifecycle and cancellation
/// boundary for embedded loads, provider loads, and observers installed after
/// runtimes were already present.
pub trait TenantRuntimeObserver: Send + Sync {
    fn tenant_runtime_loaded(
        &self,
        event: TenantRuntimeLoadedEvent,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'static>>;
}

#[derive(Clone)]
pub struct ProjectionReconciliationSnapshot {
    pub tenant_id: TenantId,
    pub runtime_identity: TenantRuntimeObserverIdentity,
    pub active_tables: Vec<TableName>,
    pub projection_token: ProjectionToken,
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
    pub dirty_scope_count: usize,
    pub token_lag_scope_count: usize,
    pub stale_no_op_count: u64,
    pub delayed_retry_count: u64,
    pub consecutive_failure_count: u32,
    pub current_retry_backoff_millis: u64,
    pub reconciliation_retry_count: u64,
    pub current_reconciliation_backoff_millis: u64,
    pub poisoned: bool,
}

/// A table schema or collection metadata change applied to a tenant.
#[derive(Clone, Debug)]
pub struct TableSchemaChangeEvent {
    pub tenant_id: TenantId,
    pub table: TableName,
    pub projection_token: ProjectionToken,
}

/// Observer for table schema or collection metadata changes.
pub trait TableSchemaChangeObserver: Send + Sync {
    fn table_schema_changed(&self, event: TableSchemaChangeEvent);
}

pub(super) type CommittedMutationObserverRegistry =
    HashMap<&'static str, Arc<dyn CommittedMutationObserver>>;
pub(super) type TableSchemaChangeObserverRegistry =
    HashMap<&'static str, Arc<dyn TableSchemaChangeObserver>>;
pub(super) type TenantRuntimeObserverRegistry =
    HashMap<&'static str, Arc<dyn TenantRuntimeObserver>>;

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
    /// Schedules work produced by an installed observer on the engine-owned
    /// runtime, regardless of which thread invoked the synchronous callback.
    ///
    /// The work participates in engine quiescence and is cancelled when
    /// shutdown begins. Dropping the future on cancellation or spawn rejection
    /// lets observer-owned RAII guards restore any work they represented.
    #[doc(hidden)]
    pub fn try_spawn_observer_work<F>(
        &self,
        future: F,
    ) -> std::result::Result<tokio::task::JoinHandle<()>, Error>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let shutdown = self.engine_executor.shutdown_token();
        let cancellable = async move {
            tokio::select! {
                () = shutdown.cancelled() => {}
                () = future => {}
            }
        };
        self.try_spawn_background("installed_observer_work", cancellable)
            .map_err(|(error, rejected)| {
                drop(rejected);
                error
            })
    }

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
    /// Live and catch-up delivery is de-duplicated within one loaded tenant
    /// runtime. Provider processes may still observe the same durable source
    /// record independently; observers that publish durable effects must use
    /// the event's ordered [`ProjectionToken`] as their idempotency fence.
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
        let Some(last_sequence) = applied.last().map(|commit| commit.sequence) else {
            return;
        };
        let projection_token = match runtime.projection_token() {
            Ok(token) => token,
            Err(error) => {
                tracing::error!(
                    tenant = %runtime.tenant_id(),
                    %error,
                    "committed mutation observer provenance was unavailable"
                );
                return;
            }
        };
        let claimed_through = runtime.claim_committed_mutation_observer_through(last_sequence);
        let events = applied
            .iter()
            .filter(|commit| commit.sequence > claimed_through && !commit.writes.is_empty())
            .cloned()
            .map(|commit| {
                let mut affected_tables = commit.affected_tables().into_iter().collect::<Vec<_>>();
                affected_tables.sort_by(|left, right| left.as_str().cmp(right.as_str()));
                CommittedMutationEvent {
                    tenant_id: runtime.tenant_id().clone(),
                    affected_tables,
                    commit,
                    projection_token,
                }
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
        applied: &[TenantEventRecord],
        projection_token: ProjectionToken,
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
            .find(|record| {
                !TenantEventRecord::as_commit_entry(record).writes.is_empty()
                    || !record.schema_epoch_tables().is_empty()
            })
            .map(|record| record.sequence)?;
        let requested_through = applied.last()?.sequence;
        if !runtime.request_committed_mutation_observer_catch_up(
            first_sequence,
            requested_through,
            projection_token,
        ) {
            return None;
        }
        let ownership = CatchUpOwnership::new(
            runtime.clone(),
            first_sequence,
            requested_through,
            projection_token,
        );
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
                    aggregate.dirty_scope_count = aggregate
                        .dirty_scope_count
                        .saturating_add(observer.dirty_scope_count);
                    aggregate.token_lag_scope_count = aggregate
                        .token_lag_scope_count
                        .saturating_add(observer.token_lag_scope_count);
                    aggregate.stale_no_op_count = aggregate
                        .stale_no_op_count
                        .saturating_add(observer.stale_no_op_count);
                    aggregate.delayed_retry_count = aggregate
                        .delayed_retry_count
                        .saturating_add(observer.delayed_retry_count);
                    aggregate.consecutive_failure_count = aggregate
                        .consecutive_failure_count
                        .max(observer.consecutive_failure_count);
                    aggregate.current_retry_backoff_millis = aggregate
                        .current_retry_backoff_millis
                        .max(observer.current_retry_backoff_millis);
                    aggregate.reconciliation_retry_count = aggregate
                        .reconciliation_retry_count
                        .saturating_add(observer.reconciliation_retry_count);
                    aggregate.current_reconciliation_backoff_millis = aggregate
                        .current_reconciliation_backoff_millis
                        .max(observer.current_reconciliation_backoff_millis);
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
        stats.observer_spawned_work_dirty_scope_count = aggregate.dirty_scope_count;
        stats.observer_spawned_work_token_lag_scope_count = aggregate.token_lag_scope_count;
        stats.observer_spawned_work_stale_no_op_count = aggregate.stale_no_op_count;
        stats.observer_spawned_work_delayed_retry_count = aggregate.delayed_retry_count;
        stats.observer_spawned_work_consecutive_failure_count = aggregate.consecutive_failure_count;
        stats.observer_spawned_work_current_retry_backoff_millis =
            aggregate.current_retry_backoff_millis;
        stats.observer_spawned_work_reconciliation_retry_count =
            aggregate.reconciliation_retry_count;
        stats.observer_spawned_work_current_reconciliation_backoff_millis =
            aggregate.current_reconciliation_backoff_millis;
        stats.observer_spawned_work_poisoned = aggregate.poisoned;
    }

    /// Returns an opaque identity for the loaded tenant runtime currently
    /// responsible for observer work.
    ///
    /// This accessor is deliberately registry-only: observer callbacks and
    /// catch-up drains run synchronously, so they must never open an embedded
    /// tenant or cross an external-provider lifecycle boundary. Runtime-load
    /// observers reconcile replacement runtimes through the async lifecycle.
    #[doc(hidden)]
    pub fn committed_mutation_observer_runtime_identity(
        &self,
        tenant_id: &TenantId,
    ) -> nimbus_core::Result<TenantRuntimeObserverIdentity> {
        let runtime = self
            .tenants
            .read()
            .expect("tenant registry lock should not be poisoned")
            .get(tenant_id)
            .cloned()
            .ok_or_else(|| Error::TenantNotFound(tenant_id.clone()))?;
        Ok(runtime.observer_identity())
    }

    /// Installs a named runtime-load observer and reconciles runtimes that
    /// were registered before installation through the same callback.
    pub fn install_tenant_runtime_observer(
        &self,
        name: &'static str,
        observer: Arc<dyn TenantRuntimeObserver>,
    ) {
        let installed = match self
            .tenant_runtime_observers
            .write()
            .expect("tenant runtime observer registry lock should not be poisoned")
            .entry(name)
        {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(observer.clone());
                true
            }
            std::collections::hash_map::Entry::Occupied(_) => false,
        };
        if !installed {
            return;
        }
        let runtimes = self
            .tenants
            .read()
            .expect("tenant registry lock should not be poisoned")
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for runtime in runtimes {
            self.schedule_tenant_runtime_observer(observer.clone(), &runtime);
        }
    }

    pub(crate) fn notify_tenant_runtime_loaded(&self, runtime: &Arc<TenantRuntime>) {
        let observers = self
            .tenant_runtime_observers
            .read()
            .expect("tenant runtime observer registry lock should not be poisoned")
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for observer in observers {
            self.schedule_tenant_runtime_observer(observer, runtime);
        }
    }

    fn schedule_tenant_runtime_observer(
        &self,
        observer: Arc<dyn TenantRuntimeObserver>,
        runtime: &Arc<TenantRuntime>,
    ) {
        let tenant_id = runtime.tenant_id().clone();
        let event = TenantRuntimeLoadedEvent {
            tenant_id: tenant_id.clone(),
            runtime_identity: runtime.observer_identity(),
        };
        if let Err((error, _future)) = self.try_spawn_background(
            "tenant_runtime_observer",
            observer.tenant_runtime_loaded(event),
        ) {
            tracing::debug!(
                tenant = %tenant_id,
                error = %error,
                "tenant runtime observer was not scheduled during engine shutdown"
            );
        }
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
        projection_token: ProjectionToken,
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
            projection_token,
        };
        for observer in observers {
            observer.table_schema_changed(event.clone());
        }
    }

    /// Resolves the durable provider order represented by the runtime's
    /// reconciled serving state. The lease row is read only after journal
    /// application; its durable sequence must already be covered by the local
    /// applied frontier before its epoch may label projection work.
    pub(crate) async fn provider_projection_token(
        &self,
        runtime: &Arc<TenantRuntime>,
    ) -> nimbus_core::Result<ProjectionToken> {
        if !runtime.store().requires_committer_lease() {
            return Ok(ProjectionToken {
                tenant_incarnation: runtime.tenant_incarnation(),
                lease_epoch: 0,
                durable_sequence: runtime.applied_head(),
            });
        }
        let lease = runtime
            .read_storage
            .execute(|store| store.read_committer_lease())
            .await?
            .ok_or_else(|| {
                Error::Internal(
                    "provider projection provenance requires a durable committer lease".to_string(),
                )
            })?;
        // Sample the monotonic applied frontier after the provider read. This
        // still proves coverage of the exact lease row observed, while avoiding
        // a false stale result when local catch-up advances during that read.
        let applied = runtime.applied_head();
        provider_projection_token_for_lease(runtime.tenant_incarnation(), applied, &lease)
    }

    #[doc(hidden)]
    pub async fn projection_token_for_tenant_async(
        self: &Arc<Self>,
        tenant_id: &TenantId,
    ) -> nimbus_core::Result<ProjectionToken> {
        let runtime = self.get_existing_tenant_async(tenant_id).await?;
        self.provider_projection_token(&runtime).await
    }

    /// Returns the O(table-scope) source snapshot used to republish derived
    /// table state after runtime load or replacement.
    #[doc(hidden)]
    pub async fn projection_reconciliation_snapshot_async(
        self: &Arc<Self>,
        tenant_id: &TenantId,
    ) -> nimbus_core::Result<ProjectionReconciliationSnapshot> {
        let runtime = self.get_existing_tenant_async(tenant_id).await?;
        let requires_lease = runtime.store().requires_committer_lease();
        let (lease, applied, mut active_tables) = runtime
            .read_storage
            .execute(|store| {
                // Observe the lease before the snapshot. The snapshot's applied
                // frontier must cover that exact row before its epoch may label
                // the active identities returned alongside it.
                let lease = store.read_committer_lease()?;
                let snapshot = store.read_snapshot()?;
                let applied = snapshot.applied_sequence()?;
                let active_tables = snapshot
                    .table_identities()?
                    .into_iter()
                    .filter(nimbus_storage::TableIdentitySnapshotEntry::is_active)
                    .map(|identity| identity.table)
                    .collect::<Vec<_>>();
                Ok((lease, applied, active_tables))
            })
            .await?;
        active_tables.sort();
        active_tables.dedup();
        let projection_token = match lease {
            Some(lease) => {
                provider_projection_token_for_lease(runtime.tenant_incarnation(), applied, &lease)?
            }
            None if !requires_lease
                || (applied == SequenceNumber(0) && active_tables.is_empty()) =>
            {
                ProjectionToken {
                    tenant_incarnation: runtime.tenant_incarnation(),
                    lease_epoch: 0,
                    durable_sequence: applied,
                }
            }
            None => {
                return Err(Error::Internal(
                    "provider projection reconciliation found durable table state without a committer lease"
                        .to_string(),
                ));
            }
        };
        Ok(ProjectionReconciliationSnapshot {
            tenant_id: tenant_id.clone(),
            runtime_identity: runtime.observer_identity(),
            active_tables,
            projection_token,
        })
    }
}

fn provider_projection_token_for_lease(
    tenant_incarnation: u64,
    applied: SequenceNumber,
    lease: &nimbus_storage::CommitterLease,
) -> nimbus_core::Result<ProjectionToken> {
    if applied < lease.durable_sequence {
        return Err(Error::Internal(format!(
            "provider projection applied frontier {applied} does not cover lease durable sequence {}",
            lease.durable_sequence
        )));
    }
    Ok(ProjectionToken {
        tenant_incarnation,
        lease_epoch: lease.epoch,
        durable_sequence: applied,
    })
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
        let Some((requested_first, requested_through, projection_token)) = ownership.take_request()
        else {
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
            projection_token,
        )
        .await
        {
            ownership.abandon(first_sequence, requested_through, projection_token);
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
    projection_token: ProjectionToken,
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
        pending.extend(records.into_iter().filter_map(|record| {
            let commit = TenantEventRecord::as_commit_entry(&record);
            let mut affected_tables = commit.affected_tables();
            affected_tables.extend(record.schema_epoch_tables());
            (!affected_tables.is_empty()).then(|| {
                let mut affected_tables = affected_tables.into_iter().collect::<Vec<_>>();
                affected_tables.sort_by(|left, right| left.as_str().cmp(right.as_str()));
                CommittedMutationEvent {
                    tenant_id: runtime.tenant_id().clone(),
                    commit,
                    affected_tables,
                    projection_token,
                }
            })
        }));
        let reached_frontier = last_sequence >= requested_through;
        let mut dispatched = 0;
        while pending.len() - dispatched >= chunk_size
            || (reached_frontier && dispatched < pending.len())
        {
            let end = pending.len().min(dispatched + chunk_size);
            let candidate = &pending[dispatched..end];
            let through = candidate
                .last()
                .expect("non-empty catch-up dispatch must have a final event")
                .commit
                .sequence;
            let claimed_through = runtime.claim_committed_mutation_observer_through(through);
            let events = candidate
                .iter()
                .filter(|event| event.commit.sequence > claimed_through)
                .cloned()
                .collect::<Vec<_>>();
            if !events.is_empty() {
                runtime
                    .enqueue_committed_mutation_observer_catch_up_dispatch(
                        CommittedMutationObserverDispatch {
                            observers: observers.to_vec(),
                            events,
                            completion: None,
                        },
                    )
                    .await?;
            }
            dispatched = end;
        }
        pending.drain(..dispatched);
        if reached_frontier {
            runtime.claim_committed_mutation_observer_through(requested_through);
            return Ok(());
        }
        cursor = last_sequence;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projection_provider_epoch_rejects_unreconciled_frontier() {
        let lease = nimbus_storage::CommitterLease {
            owner_id: "owner".to_string(),
            epoch: 9,
            expires_at: nimbus_core::Timestamp(1_000),
            durable_sequence: SequenceNumber(12),
        };
        let error = provider_projection_token_for_lease(1, SequenceNumber(11), &lease)
            .expect_err("a stale runtime must not publish the provider's newer epoch");
        assert!(
            error
                .to_string()
                .contains("does not cover lease durable sequence 12")
        );

        assert_eq!(
            provider_projection_token_for_lease(1, SequenceNumber(12), &lease)
                .expect("the reconciled frontier may publish the observed epoch"),
            ProjectionToken {
                tenant_incarnation: 1,
                lease_epoch: 9,
                durable_sequence: SequenceNumber(12),
            }
        );
    }
}
