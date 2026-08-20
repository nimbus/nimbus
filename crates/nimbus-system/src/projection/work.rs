use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;
use std::{future::Future, pin::Pin};

use nimbus_core::{DocumentId, TableName, TenantId};
use nimbus_engine::{
    CommittedMutationEvent, CommittedMutationObserver, CommittedMutationObserverWorkStats, Engine,
    ProjectionToken, TableSchemaChangeEvent, TableSchemaChangeObserver, TenantRuntimeLoadedEvent,
    TenantRuntimeObserver, TenantRuntimeObserverIdentity,
};
use tracing::warn;

use super::config::{
    PROJECTION_RETRY_BASE_BACKOFF, PROJECTION_RETRY_MAX_BACKOFF, PROJECTION_TENANT_SWEEP_INTERVAL,
    ProjectionWorkLimits, TABLE_PROJECTION_OBSERVER,
};
use crate::projection::publication::{
    ProjectionPublicationOutcome, projection_fence_tables_for_tenant_async,
};
use crate::{is_reserved_tenant_id, record_table_state_for_generation_async};

pub(super) struct TableProjectionObserver {
    projection_work: Arc<ProjectionWork>,
}

/// Slice-A overload contract: projection spawning is bounded and loud. Both the
/// per-tenant and the aggregate cap drop the breaching event, warn once per
/// crossing, and expose breach/drop diagnostics until in-flight work drains and
/// capacity returns. A cap breach is backpressure, not a fault, so neither cap
/// may turn into permanent state: in-flight work drains on its own and the
/// tenant resumes projecting without replacing its runtime. Blocking either
/// path can deadlock nested observer writes, while unbounded spawning can exhaust process memory.
///
/// Dropping the event alone would still lose it: an already-accepted projection
/// may have sampled the source table before the dropped mutation committed, so
/// draining in-flight work does not incorporate what the drop skipped. Each drop
/// therefore leaves a coalesced dirty marker for its `(tenant, table)` scope, and
/// the first guard release that returns capacity re-projects one catch-up per
/// dirty scope. Markers coalesce, so overload costs one table name per scope
/// rather than one retained event per drop, and the catch-up re-samples the
/// source table instead of replaying the dropped commit. The marker retains the
/// maximum durable source token observed for that scope, so this coalescing
/// cannot let an older sample supersede a newer one.
///
/// A dirty marker and the in-flight work that replaces it are the two things
/// the observer flush seam can see, so a catch-up hands off from one to the
/// other without ever being neither: the drain reserves its in-flight slot
/// before it claims the marker. That makes a returning flush mean what it says
/// — no projection for this tenant is pending or in flight — for catch-up work
/// on the same terms as ordinary projections.
struct ProjectionWork {
    engine: Weak<Engine>,
    epoch: Arc<str>,
    capacity: usize,
    high_watermark: usize,
    aggregate_capacity: usize,
    aggregate_high_watermark: usize,
    aggregate_in_flight: AtomicUsize,
    aggregate_high_water_warning_active: AtomicBool,
    aggregate_high_water_warning_count: AtomicU64,
    aggregate_cap_warning_active: AtomicBool,
    aggregate_cap_breach_count: AtomicU64,
    /// Tenants holding at least one dirty marker. Read lock-free so an ordinary
    /// guard release never locks the tenant registry to look for catch-up work.
    dirty_tenants: AtomicUsize,
    catch_up_drain_running: AtomicBool,
    catch_up_next_run: Mutex<Option<tokio::time::Instant>>,
    catch_up_drain_wake: tokio::sync::Notify,
    next_generation: AtomicU64,
    tenants: Mutex<ProjectionTenantRegistry>,
    #[cfg(test)]
    registered: tokio::sync::Notify,
    #[cfg(test)]
    sweep_count: AtomicU64,
    #[cfg(test)]
    drain_scan_count: AtomicU64,
    /// Catch-up table projections that must fail before the next one is
    /// allowed to run for real.
    #[cfg(test)]
    projection_failures_to_inject: AtomicU32,
    #[cfg(test)]
    lease_contentions_to_inject: AtomicU32,
    #[cfg(test)]
    cancel_next_projection: AtomicBool,
}

#[derive(Default)]
struct ProjectionTenantRegistry {
    tenants: HashMap<TenantId, Arc<TenantProjectionWork>>,
    registrations_since_sweep: usize,
}

struct TenantProjectionWork {
    runtime_identity: Mutex<TenantRuntimeObserverIdentity>,
    generation: AtomicU64,
    in_flight: AtomicUsize,
    projection_lock: Arc<tokio::sync::Mutex<()>>,
    high_water_warning_active: AtomicBool,
    high_water_warning_count: AtomicU64,
    cap_warning_active: AtomicBool,
    cap_breach_count: AtomicU64,
    dropped_event_count: AtomicU64,
    /// Tables whose committed projection was dropped and still owes a catch-up.
    /// Repeated drops for one table collapse into a single entry, which is what
    /// keeps overload bounded where retaining the dropped events would not.
    dirty_tables: Mutex<BTreeMap<TableName, ProjectionToken>>,
    dirty_table_count: AtomicUsize,
    token_frontiers: Mutex<BTreeMap<TableName, ProjectionTokenFrontier>>,
    token_lag_scope_count: AtomicUsize,
    stale_no_op_count: AtomicU64,
    catch_up_projection_count: AtomicU64,
    consecutive_failure_count: AtomicU32,
    delayed_retry_count: AtomicU64,
    current_retry_backoff_millis: AtomicU64,
    reconciliation_retry_count: AtomicU64,
    current_reconciliation_backoff_millis: AtomicU64,
    retry_not_before: Mutex<Option<tokio::time::Instant>>,
    idle: tokio::sync::Notify,
    #[cfg(test)]
    flush_waiting: AtomicBool,
    #[cfg(test)]
    flush_waiting_notify: tokio::sync::Notify,
}

#[derive(Clone, Copy)]
struct ProjectionTokenFrontier {
    observed: ProjectionToken,
    published: Option<ProjectionToken>,
}

/// Projection work diagnostics.
///
/// This is a superset of [`CommittedMutationObserverWorkStats`]: the engine
/// diagnostics consume the bounded work, dirty-scope, and retry fields, while
/// the catch-up-attempt count remains an internal scheduling diagnostic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ProjectionWorkStats {
    depth: usize,
    capacity: usize,
    high_watermark: usize,
    high_water_warning_count: u64,
    cap_breach_count: u64,
    dropped_event_count: u64,
    dirty_projection_scope_count: usize,
    token_lag_scope_count: usize,
    stale_no_op_count: u64,
    catch_up_projection_count: u64,
    delayed_retry_count: u64,
    consecutive_failure_count: u32,
    current_retry_backoff_millis: u64,
    reconciliation_retry_count: u64,
    current_reconciliation_backoff_millis: u64,
    poisoned: bool,
}

impl From<ProjectionWorkStats> for CommittedMutationObserverWorkStats {
    fn from(stats: ProjectionWorkStats) -> Self {
        Self {
            depth: stats.depth,
            capacity: stats.capacity,
            high_watermark: stats.high_watermark,
            high_water_warning_count: stats.high_water_warning_count,
            cap_breach_count: stats.cap_breach_count,
            dropped_event_count: stats.dropped_event_count,
            dirty_scope_count: stats.dirty_projection_scope_count,
            token_lag_scope_count: stats.token_lag_scope_count,
            stale_no_op_count: stats.stale_no_op_count,
            delayed_retry_count: stats.delayed_retry_count,
            consecutive_failure_count: stats.consecutive_failure_count,
            current_retry_backoff_millis: stats.current_retry_backoff_millis,
            reconciliation_retry_count: stats.reconciliation_retry_count,
            current_reconciliation_backoff_millis: stats.current_reconciliation_backoff_millis,
            poisoned: stats.poisoned,
        }
    }
}

impl TenantProjectionWork {
    /// Advances the per-scope source frontier before admission. This map is
    /// bounded by table scopes, not event count, and makes lag observable even
    /// while work is in flight rather than only after it becomes dirty.
    fn observe_scopes(&self, scopes: &[(TableName, ProjectionToken)]) {
        let mut frontiers = self
            .token_frontiers
            .lock()
            .expect("projection token-frontier lock should not be poisoned");
        for (table, token) in scopes {
            let Some(frontier) = frontiers.get_mut(table) else {
                frontiers.insert(
                    table.clone(),
                    ProjectionTokenFrontier {
                        observed: *token,
                        published: None,
                    },
                );
                self.adjust_token_lag_scope_count(true);
                continue;
            };
            let was_lagging = frontier
                .published
                .is_none_or(|published| published < frontier.observed);
            frontier.observed = frontier.observed.max(*token);
            let is_lagging = frontier
                .published
                .is_none_or(|published| published < frontier.observed);
            if was_lagging != is_lagging {
                self.adjust_token_lag_scope_count(is_lagging);
            }
        }
    }

    fn mark_scope_published(
        &self,
        table: &TableName,
        token: ProjectionToken,
        outcome: ProjectionPublicationOutcome,
    ) {
        let mut frontiers = self
            .token_frontiers
            .lock()
            .expect("projection token-frontier lock should not be poisoned");
        let Some(frontier) = frontiers.get_mut(table) else {
            // Another registered task for the same scope can publish an equal
            // or newer token first and retire the diagnostic entry. The
            // durable fence still classifies this completion; there is no lag
            // state left for this task to update.
            if outcome == ProjectionPublicationOutcome::StaleNoOp {
                self.stale_no_op_count.fetch_add(1, Ordering::Relaxed);
            }
            return;
        };
        let was_lagging = frontier
            .published
            .is_none_or(|published| published < frontier.observed);
        frontier.observed = frontier.observed.max(token);
        frontier.published = Some(
            frontier
                .published
                .map_or(token, |current| current.max(token)),
        );
        let is_lagging = frontier
            .published
            .is_none_or(|published| published < frontier.observed);
        if was_lagging != is_lagging {
            self.adjust_token_lag_scope_count(is_lagging);
        }
        // The durable `_projection_fences` row owns replay ordering after a
        // scope catches up. Retaining an equal diagnostic frontier would keep
        // every historical table name alive for the runtime's lifetime.
        if !is_lagging {
            frontiers.remove(table);
        }
        if outcome == ProjectionPublicationOutcome::StaleNoOp {
            self.stale_no_op_count.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn adjust_token_lag_scope_count(&self, increment: bool) {
        if increment {
            self.token_lag_scope_count.fetch_add(1, Ordering::AcqRel);
        } else {
            let previous = self.token_lag_scope_count.fetch_sub(1, Ordering::AcqRel);
            debug_assert!(previous != 0, "projection token-lag count cannot underflow");
        }
    }

    /// Records a coalesced catch-up marker for every dropped table while
    /// retaining the maximum durable source order observed for that scope.
    fn mark_scopes_dirty(
        &self,
        scopes: &[(TableName, ProjectionToken)],
        dirty_tenants: &AtomicUsize,
    ) {
        if scopes.is_empty() {
            return;
        }
        let mut dirty = self
            .dirty_tables
            .lock()
            .expect("projection dirty-table lock should not be poisoned");
        let was_clean = dirty.is_empty();
        for (table, token) in scopes {
            dirty
                .entry(table.clone())
                .and_modify(|current| *current = (*current).max(*token))
                .or_insert(*token);
        }
        self.dirty_table_count.store(dirty.len(), Ordering::Release);
        if was_clean {
            dirty_tenants.fetch_add(1, Ordering::AcqRel);
        }
    }

    /// Claims this tenant's dirty scopes for exactly one catch-up projection.
    ///
    /// The caller must already hold a registered [`ProjectionWorkGuard`] for
    /// this tenant. Releasing a marker is what makes the tenant look clean, so
    /// the replacement in-flight work has to exist first; see
    /// [`ProjectionWork::drain_dirty_projections`].
    fn take_dirty_scopes(&self, dirty_tenants: &AtomicUsize) -> Vec<(TableName, ProjectionToken)> {
        let mut dirty = self
            .dirty_tables
            .lock()
            .expect("projection dirty-table lock should not be poisoned");
        if dirty.is_empty() {
            return Vec::new();
        }
        let scopes = std::mem::take(&mut *dirty).into_iter().collect::<Vec<_>>();
        self.dirty_table_count.store(0, Ordering::Release);
        dirty_tenants.fetch_sub(1, Ordering::AcqRel);
        scopes
    }

    fn is_dirty(&self) -> bool {
        self.dirty_table_count.load(Ordering::Acquire) != 0
    }
}

struct ProjectionWorkGuard {
    work: Arc<ProjectionWork>,
    tenant_work: Arc<TenantProjectionWork>,
    tenant_id: TenantId,
    generation: u64,
    /// Whether releasing this guard should look for catch-up work.
    ///
    /// Cleared only for a reservation the drain took but did not use: that
    /// release happens inside the drain loop, which is still walking its
    /// remaining candidates, so re-entering the drain from it would only
    /// recurse.
    drain_on_release: bool,
}

/// Owns every scope claimed by one projection task until that scope lands.
///
/// This guard is created before the task is spawned and owns the in-flight
/// reservation itself. Its `Drop` implementation restores remaining scopes;
/// only after that implementation returns can Rust drop the reservation field.
/// That ordering also holds when Tokio aborts the future before its first poll,
/// so cancellation cannot expose a no-marker/no-work window.
struct ProjectionAttempt {
    work: Arc<ProjectionWork>,
    tenant_work: Arc<TenantProjectionWork>,
    tenant_id: TenantId,
    remaining: BTreeMap<TableName, ProjectionToken>,
    _projection_work: ProjectionWorkGuard,
}

impl ProjectionAttempt {
    fn new(
        projection_work: ProjectionWorkGuard,
        tenant_id: TenantId,
        scopes: Vec<(TableName, ProjectionToken)>,
    ) -> Self {
        let work = projection_work.work.clone();
        let tenant_work = projection_work.tenant_work.clone();
        let mut remaining: BTreeMap<TableName, ProjectionToken> = BTreeMap::new();
        for (table, token) in scopes {
            remaining
                .entry(table)
                .and_modify(|current| *current = (*current).max(token))
                .or_insert(token);
        }
        Self {
            work,
            tenant_work,
            tenant_id,
            remaining,
            _projection_work: projection_work,
        }
    }

    fn complete(&mut self, table: &TableName) {
        self.remaining.remove(table);
    }
}

impl Drop for ProjectionAttempt {
    fn drop(&mut self) {
        if self.remaining.is_empty() {
            if !self.tenant_work.is_dirty() {
                self.tenant_work
                    .consecutive_failure_count
                    .store(0, Ordering::Release);
                self.tenant_work
                    .current_retry_backoff_millis
                    .store(0, Ordering::Release);
                *self
                    .tenant_work
                    .retry_not_before
                    .lock()
                    .expect("projection retry deadline lock should not be poisoned") = None;
            }
            return;
        }

        let scopes = self
            .remaining
            .iter()
            .map(|(table, token)| (table.clone(), *token))
            .collect::<Vec<_>>();
        self.tenant_work
            .mark_scopes_dirty(&scopes, &self.work.dirty_tenants);
        let failures = self
            .tenant_work
            .consecutive_failure_count
            .fetch_add(1, Ordering::AcqRel)
            .saturating_add(1);
        let multiplier = 1_u64 << failures.saturating_sub(1).min(16);
        let backoff_millis = (PROJECTION_RETRY_BASE_BACKOFF.as_millis() as u64)
            .saturating_mul(multiplier)
            .min(PROJECTION_RETRY_MAX_BACKOFF.as_millis() as u64);
        self.tenant_work
            .current_retry_backoff_millis
            .store(backoff_millis, Ordering::Release);
        // Release, and after every field above. This counter is the anchor a
        // diagnostics snapshot acquires, so a reader that sees the retry must
        // also see the dirty markers, failure count, and backoff it was
        // recorded for. See `ProjectionWork::stats`.
        self.tenant_work
            .delayed_retry_count
            .fetch_add(1, Ordering::Release);
        let deadline = tokio::time::Instant::now() + Duration::from_millis(backoff_millis);
        {
            let mut not_before = self
                .tenant_work
                .retry_not_before
                .lock()
                .expect("projection retry deadline lock should not be poisoned");
            *not_before = Some(not_before.map_or(deadline, |current| current.max(deadline)));
        }
        tracing::error!(
            tenant_id = %self.tenant_id,
            projection_consecutive_failures = failures,
            projection_retry_backoff_millis = backoff_millis,
            retained_scopes = scopes.len(),
            "system table projection attempt did not complete; scopes retained for bounded delayed retry"
        );
        self.work.schedule_catch_up_drain_at(deadline);
    }
}

impl ProjectionWork {
    #[cfg(test)]
    fn new(engine: &Arc<Engine>, capacity: usize, high_watermark: usize) -> Self {
        let aggregate_capacity = capacity.saturating_mul(8).max(capacity).max(1);
        let aggregate_high_watermark = high_watermark
            .saturating_mul(8)
            .max(high_watermark)
            .min(aggregate_capacity);
        Self::new_with_aggregate(
            engine,
            capacity,
            high_watermark,
            aggregate_capacity,
            aggregate_high_watermark,
        )
    }

    fn new_with_aggregate(
        engine: &Arc<Engine>,
        capacity: usize,
        high_watermark: usize,
        aggregate_capacity: usize,
        aggregate_high_watermark: usize,
    ) -> Self {
        let capacity = capacity.max(1);
        let aggregate_capacity = aggregate_capacity.max(1);
        Self {
            engine: Arc::downgrade(engine),
            epoch: DocumentId::new().to_string().into(),
            capacity,
            high_watermark: high_watermark.max(1).min(capacity),
            aggregate_capacity,
            aggregate_high_watermark: aggregate_high_watermark.max(1).min(aggregate_capacity),
            aggregate_in_flight: AtomicUsize::new(0),
            aggregate_high_water_warning_active: AtomicBool::new(false),
            aggregate_high_water_warning_count: AtomicU64::new(0),
            aggregate_cap_warning_active: AtomicBool::new(false),
            aggregate_cap_breach_count: AtomicU64::new(0),
            dirty_tenants: AtomicUsize::new(0),
            catch_up_drain_running: AtomicBool::new(false),
            catch_up_next_run: Mutex::new(None),
            catch_up_drain_wake: tokio::sync::Notify::new(),
            next_generation: AtomicU64::new(0),
            tenants: Mutex::new(ProjectionTenantRegistry::default()),
            #[cfg(test)]
            registered: tokio::sync::Notify::new(),
            #[cfg(test)]
            sweep_count: AtomicU64::new(0),
            #[cfg(test)]
            drain_scan_count: AtomicU64::new(0),
            #[cfg(test)]
            projection_failures_to_inject: AtomicU32::new(0),
            #[cfg(test)]
            lease_contentions_to_inject: AtomicU32::new(0),
            #[cfg(test)]
            cancel_next_projection: AtomicBool::new(false),
        }
    }

    /// Makes the next `count` catch-up table projections fail, so a test can
    /// exercise recovery from a catch-up that does not land on its first try.
    #[cfg(test)]
    fn fail_next_projections(&self, count: u32) {
        self.projection_failures_to_inject
            .store(count, Ordering::Release);
    }

    #[cfg(test)]
    fn cancel_next_projection(&self) {
        self.cancel_next_projection.store(true, Ordering::Release);
    }

    #[cfg(test)]
    fn contend_next_projections(&self, count: u32) {
        self.lease_contentions_to_inject
            .store(count, Ordering::Release);
    }

    #[cfg(test)]
    fn take_injected_lease_contention(&self) -> bool {
        self.lease_contentions_to_inject
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |remaining| {
                remaining.checked_sub(1)
            })
            .is_ok()
    }

    #[cfg(not(test))]
    fn take_injected_lease_contention(&self) -> bool {
        false
    }

    #[cfg(test)]
    fn take_injected_projection_failure(&self) -> bool {
        self.projection_failures_to_inject
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |remaining| {
                remaining.checked_sub(1)
            })
            .is_ok()
    }

    #[cfg(not(test))]
    fn take_injected_projection_failure(&self) -> bool {
        false
    }

    #[cfg(test)]
    fn tenant_work(
        &self,
        tenant_id: &TenantId,
        runtime_identity: TenantRuntimeObserverIdentity,
    ) -> Arc<TenantProjectionWork> {
        let mut registry = self
            .tenants
            .lock()
            .expect("projection tenant-work lock should not be poisoned");
        self.tenant_work_locked(&mut registry, tenant_id, runtime_identity)
    }

    /// Returns reconciliation diagnostics only while `runtime_identity` is
    /// still the registered generation. An older load callback must never
    /// replace a newer runtime's observer state merely to report its failure.
    fn reconciliation_work(
        &self,
        tenant_id: &TenantId,
        runtime_identity: &TenantRuntimeObserverIdentity,
    ) -> Option<Arc<TenantProjectionWork>> {
        let mut registry = self
            .tenants
            .lock()
            .expect("projection tenant-work lock should not be poisoned");
        match registry.tenants.get(tenant_id) {
            Some(work)
                if work
                    .runtime_identity
                    .lock()
                    .expect("projection runtime-identity lock should not be poisoned")
                    .same_runtime(runtime_identity) =>
            {
                Some(work.clone())
            }
            Some(_) => None,
            None => {
                Some(self.tenant_work_locked(&mut registry, tenant_id, runtime_identity.clone()))
            }
        }
    }

    fn record_reconciliation_retry(
        &self,
        tenant_id: &TenantId,
        runtime_identity: &TenantRuntimeObserverIdentity,
        backoff: Duration,
    ) -> bool {
        let Some(work) = self.reconciliation_work(tenant_id, runtime_identity) else {
            return false;
        };
        work.reconciliation_retry_count
            .fetch_add(1, Ordering::Relaxed);
        work.current_reconciliation_backoff_millis
            .store(backoff.as_millis() as u64, Ordering::Release);
        true
    }

    fn finish_reconciliation(
        &self,
        tenant_id: &TenantId,
        runtime_identity: &TenantRuntimeObserverIdentity,
    ) {
        if let Some(work) = self.reconciliation_work(tenant_id, runtime_identity) {
            work.current_reconciliation_backoff_millis
                .store(0, Ordering::Release);
        }
    }

    fn tenant_work_locked(
        &self,
        registry: &mut ProjectionTenantRegistry,
        tenant_id: &TenantId,
        runtime_identity: TenantRuntimeObserverIdentity,
    ) -> Arc<TenantProjectionWork> {
        if let Some(work) = registry.tenants.get(tenant_id) {
            let mut current_identity = work
                .runtime_identity
                .lock()
                .expect("projection runtime-identity lock should not be poisoned");
            if current_identity.same_runtime(&runtime_identity) {
                return work.clone();
            }
            *current_identity = runtime_identity;
            drop(current_identity);
            let generation = self.next_generation.fetch_add(1, Ordering::AcqRel) + 1;
            work.generation.store(generation, Ordering::Release);
            work.high_water_warning_active
                .store(false, Ordering::Release);
            work.high_water_warning_count.store(0, Ordering::Relaxed);
            work.cap_warning_active.store(false, Ordering::Release);
            work.cap_breach_count.store(0, Ordering::Relaxed);
            work.dropped_event_count.store(0, Ordering::Relaxed);
            work.catch_up_projection_count.store(0, Ordering::Relaxed);
            work.consecutive_failure_count.store(0, Ordering::Relaxed);
            work.current_retry_backoff_millis
                .store(0, Ordering::Relaxed);
            work.current_reconciliation_backoff_millis
                .store(0, Ordering::Relaxed);
            *work
                .retry_not_before
                .lock()
                .expect("projection retry deadline lock should not be poisoned") = None;
            // Dirty markers survive the reload on purpose. Clearing them would
            // reintroduce the permanent staleness they exist to close, and a
            // catch-up re-samples the table under the new generation anyway. A
            // marker for a runtime that never comes back is swept with its
            // tenant entry.
            return work.clone();
        }
        let generation = self.next_generation.fetch_add(1, Ordering::AcqRel) + 1;
        let work = Arc::new(TenantProjectionWork {
            runtime_identity: Mutex::new(runtime_identity),
            generation: AtomicU64::new(generation),
            in_flight: AtomicUsize::new(0),
            projection_lock: Arc::new(tokio::sync::Mutex::new(())),
            high_water_warning_active: AtomicBool::new(false),
            high_water_warning_count: AtomicU64::new(0),
            cap_warning_active: AtomicBool::new(false),
            cap_breach_count: AtomicU64::new(0),
            dropped_event_count: AtomicU64::new(0),
            dirty_tables: Mutex::new(BTreeMap::new()),
            dirty_table_count: AtomicUsize::new(0),
            token_frontiers: Mutex::new(BTreeMap::new()),
            token_lag_scope_count: AtomicUsize::new(0),
            stale_no_op_count: AtomicU64::new(0),
            catch_up_projection_count: AtomicU64::new(0),
            consecutive_failure_count: AtomicU32::new(0),
            delayed_retry_count: AtomicU64::new(0),
            current_retry_backoff_millis: AtomicU64::new(0),
            reconciliation_retry_count: AtomicU64::new(0),
            current_reconciliation_backoff_millis: AtomicU64::new(0),
            retry_not_before: Mutex::new(None),
            idle: tokio::sync::Notify::new(),
            #[cfg(test)]
            flush_waiting: AtomicBool::new(false),
            #[cfg(test)]
            flush_waiting_notify: tokio::sync::Notify::new(),
        });
        registry.tenants.insert(tenant_id.clone(), work.clone());
        work
    }

    fn maybe_sweep_dead_tenants_locked(&self, registry: &mut ProjectionTenantRegistry) {
        registry.registrations_since_sweep = registry.registrations_since_sweep.saturating_add(1);
        if registry.registrations_since_sweep < PROJECTION_TENANT_SWEEP_INTERVAL {
            return;
        }
        registry.registrations_since_sweep = 0;
        self.sweep_dead_tenants_locked(registry);
        #[cfg(test)]
        self.sweep_count.fetch_add(1, Ordering::Relaxed);
    }

    fn sweep_dead_tenants_locked(&self, registry: &mut ProjectionTenantRegistry) {
        registry.tenants.retain(|_, work| {
            let live = work.in_flight.load(Ordering::Acquire) != 0
                || work
                    .runtime_identity
                    .lock()
                    .expect("projection runtime-identity lock should not be poisoned")
                    .is_live();
            // A dead runtime cannot be projected into, so its markers go with
            // it; leaving the tenant counter behind would strand the drain.
            if !live && work.is_dirty() {
                self.dirty_tenants.fetch_sub(1, Ordering::AcqRel);
            }
            live
        });
    }

    fn existing_tenant_work(&self, tenant_id: &TenantId) -> Option<Arc<TenantProjectionWork>> {
        self.tenants
            .lock()
            .expect("projection tenant-work lock should not be poisoned")
            .tenants
            .get(tenant_id)
            .cloned()
    }

    fn register(
        self: &Arc<Self>,
        tenant_id: &TenantId,
        runtime_identity: TenantRuntimeObserverIdentity,
        scopes: &[(TableName, ProjectionToken)],
    ) -> Option<ProjectionWorkGuard> {
        let mut registry = self
            .tenants
            .lock()
            .expect("projection tenant-work lock should not be poisoned");
        self.maybe_sweep_dead_tenants_locked(&mut registry);
        let tenant_work = self.tenant_work_locked(&mut registry, tenant_id, runtime_identity);
        tenant_work.observe_scopes(scopes);
        let previous = match tenant_work.in_flight.fetch_update(
            Ordering::AcqRel,
            Ordering::Acquire,
            |depth| (depth < self.capacity).then_some(depth + 1),
        ) {
            Ok(previous) => previous,
            Err(depth) => {
                tenant_work.mark_scopes_dirty(scopes, &self.dirty_tenants);
                let dropped = tenant_work
                    .dropped_event_count
                    .fetch_add(1, Ordering::Relaxed)
                    + 1;
                let breaches = tenant_work.cap_breach_count.fetch_add(1, Ordering::Relaxed) + 1;
                if !tenant_work.cap_warning_active.swap(true, Ordering::AcqRel) {
                    warn!(
                        projection_work_depth = depth,
                        projection_work_capacity = self.capacity,
                        projection_work_cap_breach_count = breaches,
                        projection_work_dropped_event_count = dropped,
                        projection_dirty_scope_count =
                            tenant_work.dirty_table_count.load(Ordering::Acquire),
                        tenant = %tenant_id,
                        "system table projection per-tenant work cap breached; committed events dropped and marked for catch-up until this tenant's projection work drains"
                    );
                }
                drop(registry);
                self.schedule_catch_up_drain();
                return None;
            }
        };
        let aggregate_previous = match self.aggregate_in_flight.fetch_update(
            Ordering::AcqRel,
            Ordering::Acquire,
            |depth| (depth < self.aggregate_capacity).then_some(depth + 1),
        ) {
            Ok(previous) => previous,
            Err(depth) => {
                tenant_work.mark_scopes_dirty(scopes, &self.dirty_tenants);
                let rollback_previous = tenant_work.in_flight.fetch_sub(1, Ordering::AcqRel);
                debug_assert!(
                    rollback_previous != 0,
                    "projection cap rollback cannot underflow"
                );
                if rollback_previous == 1 {
                    tenant_work.idle.notify_waiters();
                }
                tenant_work
                    .dropped_event_count
                    .fetch_add(1, Ordering::Relaxed);
                tenant_work.cap_breach_count.fetch_add(1, Ordering::Relaxed);
                self.aggregate_cap_breach_count
                    .fetch_add(1, Ordering::Relaxed);
                if !self
                    .aggregate_cap_warning_active
                    .swap(true, Ordering::AcqRel)
                {
                    warn!(
                        projection_aggregate_work_depth = depth,
                        projection_aggregate_work_capacity = self.aggregate_capacity,
                        projection_dirty_tenant_count =
                            self.dirty_tenants.load(Ordering::Acquire),
                        tenant = %tenant_id,
                        "system table projection aggregate work cap breached; committed event dropped and marked for catch-up until aggregate work drains"
                    );
                }
                drop(registry);
                self.schedule_catch_up_drain();
                return None;
            }
        };
        let generation = tenant_work.generation.load(Ordering::Acquire);
        drop(registry);
        let depth = previous + 1;
        if depth >= self.high_watermark
            && !tenant_work
                .high_water_warning_active
                .swap(true, Ordering::AcqRel)
        {
            tenant_work
                .high_water_warning_count
                .fetch_add(1, Ordering::Relaxed);
            warn!(
                tenant = %tenant_id,
                projection_work_depth = depth,
                projection_work_high_watermark = self.high_watermark,
                projection_work_capacity = self.capacity,
                "system table projection work crossed its high-water mark"
            );
        }
        let aggregate_depth = aggregate_previous + 1;
        if aggregate_depth >= self.aggregate_high_watermark
            && !self
                .aggregate_high_water_warning_active
                .swap(true, Ordering::AcqRel)
        {
            self.aggregate_high_water_warning_count
                .fetch_add(1, Ordering::Relaxed);
            warn!(
                tenant = %tenant_id,
                projection_aggregate_work_depth = aggregate_depth,
                projection_aggregate_work_high_watermark = self.aggregate_high_watermark,
                projection_aggregate_work_capacity = self.aggregate_capacity,
                "system table projection aggregate work crossed its high-water mark"
            );
        }
        #[cfg(test)]
        self.registered.notify_waiters();
        Some(ProjectionWorkGuard {
            work: self.clone(),
            tenant_work,
            tenant_id: tenant_id.clone(),
            generation,
            drain_on_release: true,
        })
    }

    /// Projects `tables` for `tenant_id` on the engine-owned runtime.
    ///
    /// Never blocks: a rejected registration leaves a dirty marker behind and
    /// returns, so the commit path and the observer dispatcher keep moving.
    fn project_tables(
        self: &Arc<Self>,
        tenant_id: TenantId,
        tables: Vec<TableName>,
        projection_token: ProjectionToken,
    ) {
        let Some(engine) = self.engine.upgrade() else {
            return;
        };
        let runtime_identity = match engine.committed_mutation_observer_runtime_identity(&tenant_id)
        {
            Ok(identity) => identity,
            Err(error) => {
                warn!(
                    tenant_id = %tenant_id,
                    error = %error,
                    "skipping system table projection because its tenant runtime is unavailable"
                );
                return;
            }
        };
        let scopes = tables
            .into_iter()
            .map(|table| (table, projection_token))
            .collect();
        self.spawn_projection(&engine, tenant_id, runtime_identity, scopes);
    }

    fn spawn_projection(
        self: &Arc<Self>,
        engine: &Arc<Engine>,
        tenant_id: TenantId,
        runtime_identity: TenantRuntimeObserverIdentity,
        mut scopes: Vec<(TableName, ProjectionToken)>,
    ) {
        scopes.sort_by(|left, right| left.0.as_str().cmp(right.0.as_str()));
        // Register before spawning so a dispatcher fence followed immediately
        // by a test flush cannot overtake a task that has not been polled yet.
        let Some(projection_work) = self.register(&tenant_id, runtime_identity, &scopes) else {
            return;
        };
        self.spawn_registered_projection(engine, tenant_id, projection_work, scopes);
    }

    /// Spawns projection work whose in-flight slot is already registered.
    ///
    /// Splitting this out lets the catch-up drain reserve its slot before it
    /// claims the dirty markers that the slot stands in for.
    fn spawn_registered_projection(
        self: &Arc<Self>,
        engine: &Arc<Engine>,
        tenant_id: TenantId,
        projection_work: ProjectionWorkGuard,
        scopes: Vec<(TableName, ProjectionToken)>,
    ) {
        self.spawn_projection_task(engine, tenant_id, projection_work, scopes);
    }

    /// Spawns a catch-up projection for scopes whose markers this task owns.
    ///
    /// The markers were cleared to claim them, so this task is the only record
    /// that the work is still owed: whatever it fails to project has to go back
    /// on the dirty set before its guard releases, or the tenant reports idle
    /// while `_nimbus` stays stale.
    fn spawn_catch_up_projection(
        self: &Arc<Self>,
        engine: &Arc<Engine>,
        tenant_id: TenantId,
        projection_work: ProjectionWorkGuard,
        scopes: Vec<(TableName, ProjectionToken)>,
    ) {
        self.spawn_projection_task(engine, tenant_id, projection_work, scopes);
    }

    fn spawn_projection_task(
        self: &Arc<Self>,
        engine: &Arc<Engine>,
        tenant_id: TenantId,
        projection_work: ProjectionWorkGuard,
        scopes: Vec<(TableName, ProjectionToken)>,
    ) {
        let tenant_work = projection_work.tenant_work.clone();
        let work = projection_work.work.clone();
        let projection_epoch = self.epoch.clone();
        let projection_generation = projection_work.generation;
        let engine_for_task = engine.clone();
        let tenant_id_for_task = tenant_id.clone();
        // Construct ownership before spawning. Aborting a task before its
        // first poll still drops this captured guard and republishes scopes.
        let attempt = ProjectionAttempt::new(projection_work, tenant_id.clone(), scopes);
        let task = async move {
            let mut attempt = attempt;
            let _projection_guard = tenant_work.projection_lock.lock().await;
            let scopes = attempt
                .remaining
                .iter()
                .map(|(table, token)| (table.clone(), *token))
                .collect::<Vec<_>>();
            for (table, projection_token) in scopes {
                // A generation that goes stale parks the rest of this task's
                // work; the runtime that replaced it owes those scopes.
                if projection_generation < tenant_work.generation.load(Ordering::Acquire) {
                    break;
                }
                let projected = if work.take_injected_lease_contention() {
                    Err(nimbus_core::Error::storage(
                        nimbus_core::StorageErrorKind::Busy,
                        "injected system-tenant committer lease contention",
                    ))
                } else if work.take_injected_projection_failure() {
                    Err(nimbus_core::Error::Internal(
                        "injected projection failure".to_string(),
                    ))
                } else {
                    record_table_state_for_generation_async(
                        &engine_for_task,
                        &tenant_id_for_task,
                        &table,
                        projection_token,
                        &projection_epoch,
                        projection_generation,
                    )
                    .await
                };
                match projected {
                    Ok(outcome) => {
                        tenant_work.mark_scope_published(&table, projection_token, outcome);
                        attempt.complete(&table);
                    }
                    Err(error) => {
                        warn!(
                            tenant_id = %tenant_id_for_task,
                            table = %table,
                            error = %error,
                            "failed to project committed table state into _nimbus"
                        );
                    }
                }
            }
        };
        let task = match engine.try_spawn_observer_work(task) {
            Ok(task) => task,
            Err(error) => {
                // Dropping the unspawned attempt republishes every scope it
                // owns. During engine quiescence those markers remain honest
                // until the runtime itself is discarded.
                warn!(
                    tenant_id = %tenant_id,
                    error = %error,
                    "system table projection was rejected by the engine observer executor"
                );
                return;
            }
        };
        #[cfg(test)]
        if self.cancel_next_projection.swap(false, Ordering::AcqRel) {
            task.abort();
        }
        #[cfg(not(test))]
        drop(task);
    }

    /// Defers one catch-up drain onto the runtime after a drop marked a scope.
    ///
    /// The drop that recorded the marker may race a concurrent guard release
    /// that already looked for catch-up work, so the marking side always gets a
    /// second look. Only one drain is ever pending, which keeps a sustained
    /// overload from turning every dropped event into a spawned task.
    fn schedule_catch_up_drain(self: &Arc<Self>) {
        self.schedule_catch_up_drain_at(tokio::time::Instant::now());
    }

    fn schedule_catch_up_drain_at(self: &Arc<Self>, deadline: tokio::time::Instant) {
        {
            let mut next_run = self
                .catch_up_next_run
                .lock()
                .expect("projection drain deadline lock should not be poisoned");
            *next_run = Some(next_run.map_or(deadline, |current| current.min(deadline)));
        }
        self.catch_up_drain_wake.notify_one();
        if self.catch_up_drain_running.swap(true, Ordering::AcqRel) {
            return;
        }
        let Some(engine) = self.engine.upgrade() else {
            self.catch_up_drain_running.store(false, Ordering::Release);
            return;
        };
        let work = self.clone();
        if let Err(error) = engine.try_spawn_observer_work(async move {
            work.run_catch_up_drain_driver().await;
        }) {
            self.catch_up_drain_running.store(false, Ordering::Release);
            warn!(
                error = %error,
                "system table projection catch-up driver was rejected by the engine observer executor"
            );
        }
    }

    async fn run_catch_up_drain_driver(self: Arc<Self>) {
        loop {
            let notified = self.catch_up_drain_wake.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let deadline = *self
                .catch_up_next_run
                .lock()
                .expect("projection drain deadline lock should not be poisoned");
            let Some(deadline) = deadline else {
                self.catch_up_drain_running.store(false, Ordering::Release);
                let pending = self
                    .catch_up_next_run
                    .lock()
                    .expect("projection drain deadline lock should not be poisoned")
                    .is_some();
                if pending && !self.catch_up_drain_running.swap(true, Ordering::AcqRel) {
                    continue;
                }
                return;
            };
            tokio::select! {
                () = tokio::time::sleep_until(deadline) => {}
                () = notified.as_mut() => continue,
            }
            {
                let mut next_run = self
                    .catch_up_next_run
                    .lock()
                    .expect("projection drain deadline lock should not be poisoned");
                if next_run.is_some_and(|scheduled| scheduled <= tokio::time::Instant::now()) {
                    *next_run = None;
                }
            }
            self.drain_dirty_projections();
        }
    }

    /// Enqueues exactly one catch-up projection per dirty scope that now fits
    /// under both caps, and clears the markers it claims.
    ///
    /// Runs only in the coalesced catch-up driver: guard release performs an
    /// O(1) wake instead of scanning all tenants on the mutation hot path.
    fn drain_dirty_projections(self: &Arc<Self>) {
        if self.dirty_tenants.load(Ordering::Acquire) == 0 {
            return;
        }
        let Some(engine) = self.engine.upgrade() else {
            return;
        };
        for (tenant_id, tenant_work) in self.dirty_projection_candidates() {
            self.drain_dirty_candidate(&engine, tenant_id, tenant_work);
        }
    }

    fn dirty_projection_candidates(&self) -> Vec<(TenantId, Arc<TenantProjectionWork>)> {
        #[cfg(test)]
        self.drain_scan_count.fetch_add(1, Ordering::Relaxed);
        {
            let registry = self
                .tenants
                .lock()
                .expect("projection tenant-work lock should not be poisoned");
            registry
                .tenants
                .iter()
                .filter(|(_, work)| work.is_dirty())
                .map(|(tenant_id, work)| (tenant_id.clone(), work.clone()))
                .collect::<Vec<_>>()
        }
    }

    fn drain_dirty_candidate(
        self: &Arc<Self>,
        engine: &Arc<Engine>,
        tenant_id: TenantId,
        tenant_work: Arc<TenantProjectionWork>,
    ) {
        if tenant_work.in_flight.load(Ordering::Acquire) >= self.capacity
            || self.aggregate_in_flight.load(Ordering::Acquire) >= self.aggregate_capacity
        {
            return;
        }
        // Resolve the runtime before claiming the markers so an unavailable
        // tenant keeps them instead of losing its catch-up.
        let Ok(runtime_identity) = engine.committed_mutation_observer_runtime_identity(&tenant_id)
        else {
            return;
        };
        // Reserve the in-flight slot before claiming the markers, for the same
        // reason `spawn_projection` registers before it spawns: a dirty marker
        // and its replacement work are the two things the flush seam can see,
        // so they must never both be absent. Claiming first would leave a
        // window where the tenant reports no dirty scope and no in-flight work
        // while a catch-up is still owed.
        //
        // Registering with no tables is deliberate: the markers are already
        // recorded, so a cap breach here must not re-mark them.
        let Some(projection_work) = self.register(&tenant_id, runtime_identity, &[]) else {
            return;
        };
        // `register` may sweep a dead runtime generation represented by the
        // earlier candidate and install a replacement. Claim markers only from
        // the exact Arc owned by the returned reservation; using the stale
        // candidate can decrement `dirty_tenants` twice.
        let registered_tenant_work = projection_work.tenant_work.clone();
        let retry_not_before = *registered_tenant_work
            .retry_not_before
            .lock()
            .expect("projection retry deadline lock should not be poisoned");
        if retry_not_before.is_some_and(|deadline| deadline > tokio::time::Instant::now()) {
            let mut unused = projection_work;
            unused.drain_on_release = false;
            self.schedule_catch_up_drain_at(
                retry_not_before.expect("checked projection retry deadline should exist"),
            );
            return;
        }
        let mut scopes = registered_tenant_work.take_dirty_scopes(&self.dirty_tenants);
        if scopes.is_empty() {
            // A concurrent drain claimed this tenant first, or registration
            // replaced a stale candidate. Release without scheduling a second
            // registry scan; the coalesced driver owns the remaining work.
            let mut unused = projection_work;
            unused.drain_on_release = false;
            return;
        }
        registered_tenant_work
            .catch_up_projection_count
            .fetch_add(1, Ordering::Relaxed);
        *registered_tenant_work
            .retry_not_before
            .lock()
            .expect("projection retry deadline lock should not be poisoned") = None;
        scopes.sort_by(|left, right| left.0.as_str().cmp(right.0.as_str()));
        self.spawn_catch_up_projection(engine, tenant_id, projection_work, scopes);
    }

    async fn wait_for_idle(&self, tenant_id: &TenantId) {
        let Some(tenant_work) = self.existing_tenant_work(tenant_id) else {
            return;
        };
        loop {
            let notified = tenant_work.idle.notified();
            tokio::pin!(notified);
            // Register before reading the state. `notify_waiters` wakes only
            // waiters that are already registered and stores no permit, and a
            // `Notified` does not register until it is first polled, so a guard
            // release landing between the read and the await would otherwise be
            // a lost wakeup on a tenant that has already gone idle.
            notified.as_mut().enable();
            // A tenant that still owes a catch-up is not idle: its dropped
            // events have not reached `_nimbus` yet.
            if tenant_work.in_flight.load(Ordering::Acquire) == 0 && !tenant_work.is_dirty() {
                return;
            }
            #[cfg(test)]
            {
                tenant_work.flush_waiting.store(true, Ordering::Release);
                tenant_work.flush_waiting_notify.notify_waiters();
            }
            notified.await;
        }
    }

    fn stats(&self, tenant_id: &TenantId) -> ProjectionWorkStats {
        let tenant_work = self.existing_tenant_work(tenant_id);
        // Anchor the snapshot on the retry counter, read first and with
        // Acquire. `ProjectionAttempt::drop` records a delayed retry only after
        // it has published the dirty markers, failure count, and backoff that
        // the retry was recorded for, so acquiring the counter here orders
        // every field read below it against that publication. Reading it
        // relaxed, or later, lets a snapshot pair a recorded retry with the
        // state that preceded it -- a diagnostics report of a retry with
        // nothing outstanding to retry.
        let delayed_retry_count = tenant_work
            .as_ref()
            .map_or(0, |work| work.delayed_retry_count.load(Ordering::Acquire));
        ProjectionWorkStats {
            depth: tenant_work
                .as_ref()
                .map_or(0, |work| work.in_flight.load(Ordering::Acquire)),
            capacity: self.capacity,
            high_watermark: self.high_watermark,
            high_water_warning_count: tenant_work.as_ref().map_or(0, |work| {
                work.high_water_warning_count.load(Ordering::Relaxed)
            }),
            cap_breach_count: tenant_work
                .as_ref()
                .map_or(0, |work| work.cap_breach_count.load(Ordering::Relaxed)),
            dropped_event_count: tenant_work
                .as_ref()
                .map_or(0, |work| work.dropped_event_count.load(Ordering::Relaxed)),
            dirty_projection_scope_count: tenant_work
                .as_ref()
                .map_or(0, |work| work.dirty_table_count.load(Ordering::Acquire)),
            token_lag_scope_count: tenant_work
                .as_ref()
                .map_or(0, |work| work.token_lag_scope_count.load(Ordering::Acquire)),
            stale_no_op_count: tenant_work
                .as_ref()
                .map_or(0, |work| work.stale_no_op_count.load(Ordering::Relaxed)),
            catch_up_projection_count: tenant_work.as_ref().map_or(0, |work| {
                work.catch_up_projection_count.load(Ordering::Relaxed)
            }),
            delayed_retry_count,
            consecutive_failure_count: tenant_work.as_ref().map_or(0, |work| {
                work.consecutive_failure_count.load(Ordering::Relaxed)
            }),
            current_retry_backoff_millis: tenant_work.as_ref().map_or(0, |work| {
                work.current_retry_backoff_millis.load(Ordering::Relaxed)
            }),
            reconciliation_retry_count: tenant_work.as_ref().map_or(0, |work| {
                work.reconciliation_retry_count.load(Ordering::Relaxed)
            }),
            current_reconciliation_backoff_millis: tenant_work.as_ref().map_or(0, |work| {
                work.current_reconciliation_backoff_millis
                    .load(Ordering::Relaxed)
            }),
            // Projection overload is recoverable backpressure, so this observer
            // never reports poison; a fatal dispatcher poison is the engine's.
            poisoned: false,
        }
    }

    #[cfg(test)]
    async fn wait_until_registered(&self, tenant_id: &TenantId) {
        loop {
            let notified = self.registered.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self
                .existing_tenant_work(tenant_id)
                .is_some_and(|work| work.in_flight.load(Ordering::Acquire) != 0)
            {
                return;
            }
            notified.await;
        }
    }

    #[cfg(test)]
    async fn wait_until_flush_waits(&self, tenant_id: &TenantId) {
        let tenant_work = self
            .existing_tenant_work(tenant_id)
            .expect("tenant projection work should exist before a flush waits");
        loop {
            let notified = tenant_work.flush_waiting_notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if tenant_work.flush_waiting.load(Ordering::Acquire) {
                return;
            }
            notified.await;
        }
    }

    #[cfg(test)]
    fn tenant_count(&self) -> usize {
        let mut registry = self
            .tenants
            .lock()
            .expect("projection tenant-work lock should not be poisoned");
        self.sweep_dead_tenants_locked(&mut registry);
        registry.tenants.len()
    }

    #[cfg(test)]
    fn sweep_count(&self) -> u64 {
        self.sweep_count.load(Ordering::Relaxed)
    }

    #[cfg(test)]
    fn drain_scan_count(&self) -> u64 {
        self.drain_scan_count.load(Ordering::Relaxed)
    }
}

impl Drop for ProjectionWorkGuard {
    fn drop(&mut self) {
        let tenant_previous = self.tenant_work.in_flight.fetch_sub(1, Ordering::AcqRel);
        debug_assert!(
            tenant_previous != 0,
            "tenant projection work count cannot underflow for {}",
            self.tenant_id
        );
        if tenant_previous.saturating_sub(1) < self.work.high_watermark {
            self.tenant_work
                .high_water_warning_active
                .store(false, Ordering::Release);
        }
        if tenant_previous.saturating_sub(1) < self.work.capacity {
            self.tenant_work
                .cap_warning_active
                .store(false, Ordering::Release);
        }
        let aggregate_previous = self.work.aggregate_in_flight.fetch_sub(1, Ordering::AcqRel);
        debug_assert!(
            aggregate_previous != 0,
            "aggregate projection work count cannot underflow"
        );
        if aggregate_previous.saturating_sub(1) < self.work.aggregate_high_watermark {
            self.work
                .aggregate_high_water_warning_active
                .store(false, Ordering::Release);
        }
        if aggregate_previous.saturating_sub(1) < self.work.aggregate_capacity {
            self.work
                .aggregate_cap_warning_active
                .store(false, Ordering::Release);
        }
        // Capacity has returned, so wake the single coalesced driver that
        // spends it on scopes an overload dropped. This O(1) scheduling step
        // runs before waking idle waiters: a tenant that still owes a catch-up
        // must not look drained to the observer flush seam.
        if self.drain_on_release {
            self.work.schedule_catch_up_drain();
        }
        if tenant_previous == 1 {
            self.tenant_work.idle.notify_waiters();
        }
    }
}

impl CommittedMutationObserver for TableProjectionObserver {
    fn committed_mutation_applied(&self, event: CommittedMutationEvent) {
        if is_reserved_tenant_id(&event.tenant_id) {
            return;
        }
        let tables = event.affected_tables;
        if tables.is_empty() {
            return;
        }
        self.project_tables(event.tenant_id, tables, event.projection_token);
    }

    fn spawned_work_stats(&self, tenant_id: &TenantId) -> CommittedMutationObserverWorkStats {
        self.projection_work.stats(tenant_id).into()
    }

    fn flush_spawned_work_for_testing(
        &self,
        tenant_id: &TenantId,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        let tenant_id = tenant_id.clone();
        Box::pin(async move { self.projection_work.wait_for_idle(&tenant_id).await })
    }
}

impl TableSchemaChangeObserver for TableProjectionObserver {
    fn table_schema_changed(&self, event: TableSchemaChangeEvent) {
        if is_reserved_tenant_id(&event.tenant_id) {
            return;
        }
        self.project_tables(event.tenant_id, vec![event.table], event.projection_token);
    }
}

impl TenantRuntimeObserver for TableProjectionObserver {
    fn tenant_runtime_loaded(
        &self,
        event: TenantRuntimeLoadedEvent,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'static>> {
        let projection_work = self.projection_work.clone();
        Box::pin(async move {
            if is_reserved_tenant_id(&event.tenant_id) {
                return;
            }
            let mut backoff = PROJECTION_RETRY_BASE_BACKOFF;
            loop {
                if !event.runtime_identity.is_live() {
                    return;
                }
                let Some(engine) = projection_work.engine.upgrade() else {
                    return;
                };
                let reconciled = async {
                    let snapshot = engine
                        .projection_reconciliation_snapshot_async(&event.tenant_id)
                        .await?;
                    if !snapshot
                        .runtime_identity
                        .same_runtime(&event.runtime_identity)
                    {
                        return Ok(None);
                    }
                    let mut tables = snapshot.active_tables.into_iter().collect::<BTreeSet<_>>();
                    tables.extend(
                        projection_fence_tables_for_tenant_async(&engine, &event.tenant_id).await?,
                    );
                    Ok::<_, nimbus_core::Error>(Some((
                        tables.into_iter().collect::<Vec<_>>(),
                        snapshot.projection_token,
                    )))
                }
                .await;
                match reconciled {
                    Ok(Some((tables, token))) => {
                        projection_work
                            .finish_reconciliation(&event.tenant_id, &event.runtime_identity);
                        if !tables.is_empty() {
                            projection_work.project_tables(event.tenant_id.clone(), tables, token);
                        }
                        return;
                    }
                    Ok(None) => {
                        projection_work
                            .finish_reconciliation(&event.tenant_id, &event.runtime_identity);
                        return;
                    }
                    Err(error) => {
                        if !projection_work.record_reconciliation_retry(
                            &event.tenant_id,
                            &event.runtime_identity,
                            backoff,
                        ) {
                            return;
                        }
                        warn!(
                            tenant = %event.tenant_id,
                            error = %error,
                            retry_backoff_millis = backoff.as_millis(),
                            "tenant runtime projection reconciliation failed; retrying"
                        );
                        tokio::time::sleep(backoff).await;
                        backoff = backoff.saturating_mul(2).min(PROJECTION_RETRY_MAX_BACKOFF);
                    }
                }
            }
        })
    }
}

impl TableProjectionObserver {
    fn project_tables(
        &self,
        tenant_id: TenantId,
        tables: Vec<TableName>,
        projection_token: ProjectionToken,
    ) {
        self.projection_work
            .project_tables(tenant_id, tables, projection_token);
    }

    #[cfg(all(test, any(feature = "libsql", feature = "mysql", feature = "postgres")))]
    pub(super) fn project_tables_for_testing(
        &self,
        tenant_id: TenantId,
        tables: Vec<TableName>,
        projection_token: ProjectionToken,
    ) {
        self.project_tables(tenant_id, tables, projection_token);
    }

    #[cfg(all(test, any(feature = "libsql", feature = "mysql", feature = "postgres")))]
    pub(super) fn cancel_next_projection_for_testing(&self) {
        self.projection_work.cancel_next_projection();
    }
}

fn build_and_install_table_projection_observer(
    engine: &Arc<Engine>,
) -> Arc<TableProjectionObserver> {
    let limits = ProjectionWorkLimits::from_env();
    let observer = Arc::new(TableProjectionObserver {
        projection_work: Arc::new(ProjectionWork::new_with_aggregate(
            engine,
            limits.capacity,
            limits.high_watermark,
            limits.aggregate_capacity,
            limits.aggregate_high_watermark,
        )),
    });
    engine.install_committed_mutation_observer(TABLE_PROJECTION_OBSERVER, observer.clone());
    engine.install_table_schema_change_observer(TABLE_PROJECTION_OBSERVER, observer.clone());
    engine.install_tenant_runtime_observer(TABLE_PROJECTION_OBSERVER, observer.clone());
    observer
}

pub fn install_table_projection_observer(engine: &Arc<Engine>) {
    build_and_install_table_projection_observer(engine);
}

#[cfg(all(test, any(feature = "libsql", feature = "mysql", feature = "postgres")))]
pub(super) fn install_table_projection_observer_for_testing(
    engine: &Arc<Engine>,
) -> Arc<TableProjectionObserver> {
    build_and_install_table_projection_observer(engine)
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
