use std::collections::{BTreeSet, HashMap};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};

use nimbus_core::{DocumentId, TableName, TenantId};
use nimbus_engine::{
    CommittedMutationEvent, CommittedMutationObserver, CommittedMutationObserverWorkStats, Engine,
    TableSchemaChangeEvent, TableSchemaChangeObserver, TenantRuntimeObserverIdentity,
};
use tracing::warn;

use super::{is_reserved_tenant_id, record_table_state_for_generation_async};

const TABLE_PROJECTION_OBSERVER: &str = "nimbus-system-table-projection";
const DEFAULT_PROJECTION_WORK_CAPACITY: usize = 1_024;
const DEFAULT_PROJECTION_WORK_HIGH_WATERMARK: usize = 768;
const DEFAULT_PROJECTION_AGGREGATE_WORK_CAPACITY: usize = 8_192;
const DEFAULT_PROJECTION_AGGREGATE_WORK_HIGH_WATERMARK: usize = 6_144;
const PROJECTION_TENANT_SWEEP_INTERVAL: usize = 1_024;

struct TableProjectionObserver {
    projection_work: Arc<ProjectionWork>,
}

/// Slice-A overload contract: projection spawning is bounded and loud. Both the
/// per-tenant and the aggregate cap drop the breaching event, warn once per
/// crossing, and expose breach/drop diagnostics until in-flight work drains and
/// capacity returns. A cap breach is backpressure, not a fault, so neither cap
/// may turn into permanent state: in-flight work drains on its own and the
/// tenant resumes projecting without replacing its runtime. Blocking either
/// path can deadlock nested observer writes, while unbounded spawning can
/// exhaust process memory.
///
/// Dropping the event alone would still lose it: an already-accepted projection
/// may have sampled the source table before the dropped mutation committed, so
/// draining in-flight work does not incorporate what the drop skipped. Each drop
/// therefore leaves a coalesced dirty marker for its `(tenant, table)` scope, and
/// the first guard release that returns capacity re-projects one catch-up per
/// dirty scope. Markers coalesce, so overload costs one table name per scope
/// rather than one retained event per drop, and the catch-up re-samples the
/// source table instead of replaying the dropped commit. Lossless
/// commit-by-commit projection replay belongs to PPSC5-B durable-journal replay.
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
    catch_up_drain_scheduled: AtomicBool,
    next_generation: AtomicU64,
    tenants: Mutex<ProjectionTenantRegistry>,
    #[cfg(test)]
    registered: tokio::sync::Notify,
    #[cfg(test)]
    sweep_count: AtomicU64,
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
    dirty_tables: Mutex<BTreeSet<TableName>>,
    dirty_table_count: AtomicUsize,
    catch_up_projection_count: AtomicU64,
    idle: tokio::sync::Notify,
    #[cfg(test)]
    flush_waiting: AtomicBool,
    #[cfg(test)]
    flush_waiting_notify: tokio::sync::Notify,
}

/// Projection work diagnostics.
///
/// This is a superset of [`CommittedMutationObserverWorkStats`]: the dirty and
/// catch-up counters live here because the engine-owned stats struct cannot
/// carry them without a `nimbus-engine` change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ProjectionWorkStats {
    depth: usize,
    capacity: usize,
    high_watermark: usize,
    high_water_warning_count: u64,
    cap_breach_count: u64,
    dropped_event_count: u64,
    dirty_projection_scope_count: usize,
    catch_up_projection_count: u64,
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
            poisoned: stats.poisoned,
        }
    }
}

impl TenantProjectionWork {
    /// Records a coalesced catch-up marker for every dropped table.
    fn mark_tables_dirty(&self, tables: &[TableName], dirty_tenants: &AtomicUsize) {
        if tables.is_empty() {
            return;
        }
        let mut dirty = self
            .dirty_tables
            .lock()
            .expect("projection dirty-table lock should not be poisoned");
        let was_clean = dirty.is_empty();
        for table in tables {
            dirty.insert(table.clone());
        }
        self.dirty_table_count.store(dirty.len(), Ordering::Release);
        if was_clean {
            dirty_tenants.fetch_add(1, Ordering::AcqRel);
        }
    }

    /// Claims this tenant's dirty scopes for exactly one catch-up projection.
    fn take_dirty_tables(&self, dirty_tenants: &AtomicUsize) -> Vec<TableName> {
        let mut dirty = self
            .dirty_tables
            .lock()
            .expect("projection dirty-table lock should not be poisoned");
        if dirty.is_empty() {
            return Vec::new();
        }
        let tables = std::mem::take(&mut *dirty).into_iter().collect::<Vec<_>>();
        self.dirty_table_count.store(0, Ordering::Release);
        dirty_tenants.fetch_sub(1, Ordering::AcqRel);
        tables
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
}

impl ProjectionWork {
    fn from_env(engine: &Arc<Engine>) -> Self {
        Self::new_with_aggregate(
            engine,
            env_positive_usize(
                "NIMBUS_SYSTEM_PROJECTION_WORK_CAPACITY",
                DEFAULT_PROJECTION_WORK_CAPACITY,
            ),
            env_positive_usize(
                "NIMBUS_SYSTEM_PROJECTION_WORK_HIGH_WATERMARK",
                DEFAULT_PROJECTION_WORK_HIGH_WATERMARK,
            ),
            env_positive_usize(
                "NIMBUS_SYSTEM_PROJECTION_AGGREGATE_WORK_CAPACITY",
                DEFAULT_PROJECTION_AGGREGATE_WORK_CAPACITY,
            ),
            env_positive_usize(
                "NIMBUS_SYSTEM_PROJECTION_AGGREGATE_WORK_HIGH_WATERMARK",
                DEFAULT_PROJECTION_AGGREGATE_WORK_HIGH_WATERMARK,
            ),
        )
    }

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
            catch_up_drain_scheduled: AtomicBool::new(false),
            next_generation: AtomicU64::new(0),
            tenants: Mutex::new(ProjectionTenantRegistry::default()),
            #[cfg(test)]
            registered: tokio::sync::Notify::new(),
            #[cfg(test)]
            sweep_count: AtomicU64::new(0),
        }
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
            dirty_tables: Mutex::new(BTreeSet::new()),
            dirty_table_count: AtomicUsize::new(0),
            catch_up_projection_count: AtomicU64::new(0),
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
        tables: &[TableName],
    ) -> Option<ProjectionWorkGuard> {
        let mut registry = self
            .tenants
            .lock()
            .expect("projection tenant-work lock should not be poisoned");
        self.maybe_sweep_dead_tenants_locked(&mut registry);
        let tenant_work = self.tenant_work_locked(&mut registry, tenant_id, runtime_identity);
        let previous = match tenant_work.in_flight.fetch_update(
            Ordering::AcqRel,
            Ordering::Acquire,
            |depth| (depth < self.capacity).then_some(depth + 1),
        ) {
            Ok(previous) => previous,
            Err(depth) => {
                tenant_work.mark_tables_dirty(tables, &self.dirty_tenants);
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
                tenant_work.mark_tables_dirty(tables, &self.dirty_tenants);
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
        })
    }

    /// Projects `tables` for `tenant_id` on the current tokio runtime.
    ///
    /// Never blocks: a rejected registration leaves a dirty marker behind and
    /// returns, so the commit path and the observer dispatcher keep moving.
    fn project_tables(self: &Arc<Self>, tenant_id: TenantId, tables: Vec<TableName>) {
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
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            warn!(
                tenant_id = %tenant_id,
                "skipping system table projection because no tokio runtime is active"
            );
            return;
        };
        self.spawn_projection(&engine, &handle, tenant_id, runtime_identity, tables);
    }

    fn spawn_projection(
        self: &Arc<Self>,
        engine: &Arc<Engine>,
        handle: &tokio::runtime::Handle,
        tenant_id: TenantId,
        runtime_identity: TenantRuntimeObserverIdentity,
        mut tables: Vec<TableName>,
    ) {
        tables.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        // Register before spawning so a dispatcher fence followed immediately
        // by a test flush cannot overtake a task that has not been polled yet.
        let Some(projection_work) = self.register(&tenant_id, runtime_identity, &tables) else {
            return;
        };
        let tenant_work = projection_work.tenant_work.clone();
        let projection_epoch = self.epoch.clone();
        let projection_generation = projection_work.generation;
        let engine = engine.clone();
        handle.spawn(async move {
            let _projection_work = projection_work;
            let _projection_guard = tenant_work.projection_lock.lock().await;
            if projection_generation < tenant_work.generation.load(Ordering::Acquire) {
                return;
            }
            for table in tables {
                if let Err(error) = record_table_state_for_generation_async(
                    &engine,
                    &tenant_id,
                    &table,
                    &projection_epoch,
                    projection_generation,
                )
                .await
                {
                    warn!(
                        tenant_id = %tenant_id,
                        table = %table,
                        error = %error,
                        "failed to project committed table state into _nimbus"
                    );
                }
            }
        });
    }

    /// Defers one catch-up drain onto the runtime after a drop marked a scope.
    ///
    /// The drop that recorded the marker may race a concurrent guard release
    /// that already looked for catch-up work, so the marking side always gets a
    /// second look. Only one drain is ever pending, which keeps a sustained
    /// overload from turning every dropped event into a spawned task.
    fn schedule_catch_up_drain(self: &Arc<Self>) {
        if self.catch_up_drain_scheduled.swap(true, Ordering::AcqRel) {
            return;
        }
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            self.catch_up_drain_scheduled
                .store(false, Ordering::Release);
            return;
        };
        let work = self.clone();
        handle.spawn(async move {
            work.catch_up_drain_scheduled
                .store(false, Ordering::Release);
            work.drain_dirty_projections();
        });
    }

    /// Enqueues exactly one catch-up projection per dirty scope that now fits
    /// under both caps, and clears the markers it claims.
    ///
    /// Runs on guard release, so it must not block or await: it takes the
    /// tenant registry lock only long enough to snapshot the dirty tenants.
    fn drain_dirty_projections(self: &Arc<Self>) {
        if self.dirty_tenants.load(Ordering::Acquire) == 0 {
            return;
        }
        let Some(engine) = self.engine.upgrade() else {
            return;
        };
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            // Without a runtime there is nothing to spawn onto. Markers stay
            // put so the next capacity return retries them.
            return;
        };
        let candidates = {
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
        };
        for (tenant_id, tenant_work) in candidates {
            if tenant_work.in_flight.load(Ordering::Acquire) >= self.capacity
                || self.aggregate_in_flight.load(Ordering::Acquire) >= self.aggregate_capacity
            {
                continue;
            }
            // Resolve the runtime before claiming the markers so an unavailable
            // tenant keeps them instead of losing its catch-up.
            let Ok(runtime_identity) =
                engine.committed_mutation_observer_runtime_identity(&tenant_id)
            else {
                continue;
            };
            let tables = tenant_work.take_dirty_tables(&self.dirty_tenants);
            if tables.is_empty() {
                continue;
            }
            tenant_work
                .catch_up_projection_count
                .fetch_add(1, Ordering::Relaxed);
            self.spawn_projection(&engine, &handle, tenant_id, runtime_identity, tables);
        }
    }

    async fn wait_for_idle(&self, tenant_id: &TenantId) {
        let Some(tenant_work) = self.existing_tenant_work(tenant_id) else {
            return;
        };
        loop {
            let notified = tenant_work.idle.notified();
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
            catch_up_projection_count: tenant_work.as_ref().map_or(0, |work| {
                work.catch_up_projection_count.load(Ordering::Relaxed)
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
        // Capacity has returned, so spend it on the scopes an overload dropped.
        // This runs before waking idle waiters: a tenant that still owes a
        // catch-up must not look drained to the observer flush seam.
        self.work.drain_dirty_projections();
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
        let tables = event
            .commit
            .affected_tables()
            .into_iter()
            .collect::<Vec<_>>();
        if tables.is_empty() {
            return;
        }
        self.project_tables(event.tenant_id, tables);
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
        self.project_tables(event.tenant_id, vec![event.table]);
    }
}

impl TableProjectionObserver {
    fn project_tables(&self, tenant_id: TenantId, tables: Vec<TableName>) {
        self.projection_work.project_tables(tenant_id, tables);
    }
}

pub fn install_table_projection_observer(engine: &Arc<Engine>) {
    let observer = Arc::new(TableProjectionObserver {
        projection_work: Arc::new(ProjectionWork::from_env(engine)),
    });
    engine.install_committed_mutation_observer(TABLE_PROJECTION_OBSERVER, observer.clone());
    engine.install_table_schema_change_observer(TABLE_PROJECTION_OBSERVER, observer);
}

fn env_positive_usize(key: &str, default: usize) -> usize {
    std::env::var_os(key)
        .and_then(|value| value.into_string().ok())
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use nimbus_testing::EngineFixture;
    use serde_json::json;

    use super::*;

    fn test_observer(
        engine: &Arc<Engine>,
        capacity: usize,
        high_watermark: usize,
    ) -> (Arc<TableProjectionObserver>, Arc<ProjectionWork>) {
        let projection_work = Arc::new(ProjectionWork::new(engine, capacity, high_watermark));
        (
            Arc::new(TableProjectionObserver {
                projection_work: projection_work.clone(),
            }),
            projection_work,
        )
    }

    fn tenant_work(
        engine: &Engine,
        projection_work: &Arc<ProjectionWork>,
        tenant_id: &TenantId,
    ) -> Arc<TenantProjectionWork> {
        projection_work.tenant_work(
            tenant_id,
            engine
                .committed_mutation_observer_runtime_identity(tenant_id)
                .expect("tenant runtime identity should load"),
        )
    }

    async fn projected_table_row_count(
        engine: &Arc<Engine>,
        tenant_id: &TenantId,
        table: &TableName,
    ) -> Option<u64> {
        let rows = match engine
            .list_documents_async(
                crate::system_tenant_id().expect("system tenant id should build"),
                crate::schema::SystemTable::Tables
                    .table_name()
                    .expect("system tables name should build"),
            )
            .await
        {
            Ok(rows) => rows,
            // The system tenant is seeded by the first projection, so its
            // absence means nothing has been projected yet.
            Err(nimbus_core::Error::TenantNotFound(_)) => return None,
            Err(error) => panic!("projected table records should list: {error}"),
        };
        rows.into_iter()
            .find(|row| {
                row.fields.get("tenantId") == Some(&json!(tenant_id.as_str()))
                    && row.fields.get("name") == Some(&json!(table.as_str()))
            })
            .and_then(|row| {
                row.fields
                    .get("rowCount")
                    .and_then(serde_json::Value::as_u64)
            })
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn committed_observer_flush_waits_for_spawned_projection_tail() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let engine = fixture.engine();
        let tenant_id = fixture.create_tenant("projection-flush-tail", Engine::create_tenant);
        let (observer, projection_work) = test_observer(&engine, 16, 12);
        let held_projection = tenant_work(&engine, &projection_work, &tenant_id)
            .projection_lock
            .clone()
            .lock_owned()
            .await;
        engine.install_committed_mutation_observer("projection-flush-tail-test", observer);

        engine
            .insert_document_async(
                tenant_id.clone(),
                TableName::new("tasks").expect("table name should build"),
                serde_json::Map::from_iter([("title".to_string(), json!("seed"))]),
            )
            .await
            .expect("seed insert should commit");
        projection_work.wait_until_registered(&tenant_id).await;

        let mut flush = tokio::spawn({
            let engine = engine.clone();
            let tenant_id = tenant_id.clone();
            async move {
                engine
                    .flush_committed_mutation_observers_for_testing(&tenant_id)
                    .await
            }
        });
        projection_work.wait_until_flush_waits(&tenant_id).await;
        assert!(
            !flush.is_finished(),
            "the observer channel fence must not overtake registered projection work"
        );

        drop(held_projection);
        tokio::time::timeout(Duration::from_secs(5), &mut flush)
            .await
            .expect("projection tail should drain")
            .expect("flush task should join")
            .expect("observer flush should succeed");
        assert_eq!(projection_work.stats(&tenant_id).depth, 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn projection_cap_drops_are_tenant_scoped_and_reset_on_runtime_reload() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let engine = fixture.engine();
        let tenant_a = fixture.create_tenant("projection-work-cap-a", Engine::create_tenant);
        let tenant_b = fixture.create_tenant("projection-work-cap-b", Engine::create_tenant);
        let (observer, projection_work) = test_observer(&engine, 2, 1);
        let held_projection = tenant_work(&engine, &projection_work, &tenant_a)
            .projection_lock
            .clone()
            .lock_owned()
            .await;
        engine.install_committed_mutation_observer("projection-work-cap-test", observer);

        for index in 0..4 {
            engine
                .insert_document_async(
                    tenant_a.clone(),
                    TableName::new("tasks").expect("table name should build"),
                    serde_json::Map::from_iter([("index".to_string(), json!(index))]),
                )
                .await
                .expect("projection saturation must not block durable mutation responses");
        }

        let stats = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let stats = engine
                    .tenant_engine_diagnostics(&tenant_a)
                    .expect("projection diagnostics should load")
                    .mutation_journal;
                if stats.observer_spawned_work_dropped_event_count == 2 {
                    break stats;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("projection cap policy should engage");
        assert_eq!(stats.observer_spawned_work_depth, 2);
        assert_eq!(stats.observer_spawned_work_capacity, 2);
        assert_eq!(stats.observer_spawned_work_high_watermark, 1);
        assert_eq!(stats.observer_spawned_work_high_water_warning_count, 1);
        assert_eq!(stats.observer_spawned_work_cap_breach_count, 2);
        assert_eq!(stats.observer_spawned_work_dropped_event_count, 2);
        assert!(!stats.observer_spawned_work_poisoned);

        let tasks = TableName::new("tasks").expect("table name should build");
        engine
            .insert_document_async(
                tenant_b.clone(),
                tasks.clone(),
                serde_json::Map::from_iter([("index".to_string(), json!(10))]),
            )
            .await
            .expect("tenant B mutation should remain healthy");
        engine
            .flush_committed_mutation_observers_for_testing(&tenant_b)
            .await
            .expect("tenant B projection should drain independently");
        assert_eq!(
            projected_table_row_count(&engine, &tenant_b, &tasks).await,
            Some(1),
            "tenant B must continue projecting while tenant A is saturated"
        );
        let tenant_b_stats = engine
            .tenant_engine_diagnostics(&tenant_b)
            .expect("tenant B diagnostics should load")
            .mutation_journal;
        assert_eq!(tenant_b_stats.observer_spawned_work_depth, 0);
        assert_eq!(tenant_b_stats.observer_spawned_work_cap_breach_count, 0);
        assert_eq!(tenant_b_stats.observer_spawned_work_dropped_event_count, 0);
        assert!(!tenant_b_stats.observer_spawned_work_poisoned);

        drop(held_projection);
        engine
            .flush_committed_mutation_observers_for_testing(&tenant_a)
            .await
            .expect("accepted projection work should drain after the cap breach");
        assert_eq!(projection_work.stats(&tenant_a).depth, 0);

        engine
            .delete_tenant_async(tenant_a.clone())
            .await
            .expect("saturated tenant should delete");
        engine
            .create_tenant_async(tenant_a.clone())
            .await
            .expect("tenant should recreate with a fresh runtime");
        engine
            .insert_document_async(
                tenant_a.clone(),
                tasks,
                serde_json::Map::from_iter([("index".to_string(), json!(20))]),
            )
            .await
            .expect("fresh tenant runtime should accept projection work");
        engine
            .flush_committed_mutation_observers_for_testing(&tenant_a)
            .await
            .expect("fresh runtime projection should drain");
        let reloaded = projection_work.stats(&tenant_a);
        assert_eq!(reloaded.depth, 0);
        assert_eq!(reloaded.cap_breach_count, 0);
        assert_eq!(reloaded.dropped_event_count, 0);
        assert!(!reloaded.poisoned);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn projection_cap_breach_resumes_projecting_after_in_flight_work_drains() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let engine = fixture.engine();
        let tenant_id = fixture.create_tenant("projection-cap-recovery", Engine::create_tenant);
        let tasks = TableName::new("tasks").expect("table name should build");
        let (observer, projection_work) = test_observer(&engine, 2, 1);
        let held_projection = tenant_work(&engine, &projection_work, &tenant_id)
            .projection_lock
            .clone()
            .lock_owned()
            .await;
        engine.install_committed_mutation_observer("projection-cap-recovery-test", observer);

        for index in 0..4 {
            engine
                .insert_document_async(
                    tenant_id.clone(),
                    tasks.clone(),
                    serde_json::Map::from_iter([("index".to_string(), json!(index))]),
                )
                .await
                .expect("projection saturation must not block durable mutation responses");
        }
        let breached = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let stats = projection_work.stats(&tenant_id);
                if stats.dropped_event_count == 2 {
                    break stats;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the per-tenant cap should drop both events past its capacity");
        assert_eq!(breached.depth, 2);
        assert_eq!(breached.cap_breach_count, 2);
        assert!(
            !breached.poisoned,
            "a per-tenant cap breach is backpressure and must not become permanent state"
        );

        drop(held_projection);
        engine
            .flush_committed_mutation_observers_for_testing(&tenant_id)
            .await
            .expect("accepted projection work should drain after the cap breach");
        assert_eq!(projection_work.stats(&tenant_id).depth, 0);

        engine
            .insert_document_async(
                tenant_id.clone(),
                tasks.clone(),
                serde_json::Map::from_iter([("index".to_string(), json!(4))]),
            )
            .await
            .expect("post-drain mutation should commit");
        engine
            .flush_committed_mutation_observers_for_testing(&tenant_id)
            .await
            .expect("post-drain projection should drain");
        assert_eq!(
            projected_table_row_count(&engine, &tenant_id, &tasks).await,
            Some(5),
            "a drained cap breach must resume projecting without replacing the tenant runtime"
        );

        let resumed = projection_work.stats(&tenant_id);
        assert_eq!(resumed.depth, 0);
        assert_eq!(
            resumed.cap_breach_count, 2,
            "recovery must not erase the breaches that already happened"
        );
        assert_eq!(
            resumed.dropped_event_count, 2,
            "the dropped events must stay observable after the tenant recovers"
        );
        assert!(!resumed.poisoned);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn dropped_projection_events_catch_up_once_per_table_after_capacity_returns() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let engine = fixture.engine();
        let tenant_id = fixture.create_tenant("projection-catch-up", Engine::create_tenant);
        let tasks = TableName::new("tasks").expect("table name should build");
        let filler = TableName::new("filler").expect("table name should build");
        let (observer, projection_work) = test_observer(&engine, 2, 1);
        engine.install_committed_mutation_observer("projection-catch-up-test", observer);

        engine
            .insert_document_async(
                tenant_id.clone(),
                tasks.clone(),
                serde_json::Map::from_iter([("index".to_string(), json!(0))]),
            )
            .await
            .expect("seed row should commit");
        engine
            .flush_committed_mutation_observers_for_testing(&tenant_id)
            .await
            .expect("seed projection should drain");
        assert_eq!(
            projected_table_row_count(&engine, &tenant_id, &tasks).await,
            Some(1),
            "the seed projection must land before the cap is saturated"
        );

        // Saturate the cap with work that never projects `tasks`, so no
        // in-flight task can incorporate what the drops below skip.
        let saturating = (0..2)
            .map(|_| {
                projection_work
                    .register(
                        &tenant_id,
                        engine
                            .committed_mutation_observer_runtime_identity(&tenant_id)
                            .expect("tenant runtime identity should load"),
                        std::slice::from_ref(&filler),
                    )
                    .expect("saturating work should register up to the cap")
            })
            .collect::<Vec<_>>();

        for index in 1..6 {
            engine
                .insert_document_async(
                    tenant_id.clone(),
                    tasks.clone(),
                    serde_json::Map::from_iter([("index".to_string(), json!(index))]),
                )
                .await
                .expect("projection saturation must not block durable mutation responses");
        }
        let dropped = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let stats = projection_work.stats(&tenant_id);
                if stats.dropped_event_count == 5 {
                    break stats;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("every commit past the cap should drop");
        assert_eq!(
            dropped.dirty_projection_scope_count, 1,
            "five drops on one table must coalesce into a single catch-up scope"
        );
        assert_eq!(dropped.catch_up_projection_count, 0);
        assert_eq!(
            projected_table_row_count(&engine, &tenant_id, &tasks).await,
            Some(1),
            "the dropped commits must not be projected while the cap is breached"
        );

        drop(saturating);
        engine
            .flush_committed_mutation_observers_for_testing(&tenant_id)
            .await
            .expect("the catch-up projection should drain");

        assert_eq!(
            projected_table_row_count(&engine, &tenant_id, &tasks).await,
            Some(6),
            "a dropped projection event must be caught up without a further mutation"
        );
        let recovered = projection_work.stats(&tenant_id);
        assert_eq!(recovered.depth, 0);
        assert_eq!(
            recovered.dirty_projection_scope_count, 0,
            "a completed catch-up must clear its dirty marker"
        );
        assert_eq!(
            recovered.catch_up_projection_count, 1,
            "coalesced drops must cost exactly one catch-up projection"
        );
        assert_eq!(
            recovered.dropped_event_count, 5,
            "catching up must not erase the drops that already happened"
        );
        assert!(!recovered.poisoned);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn aggregate_cap_drop_catches_up_the_victim_tenant_after_capacity_returns() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let engine = fixture.engine();
        let hog = fixture.create_tenant("projection-aggregate-catch-up-hog", Engine::create_tenant);
        let victim = fixture.create_tenant(
            "projection-aggregate-catch-up-victim",
            Engine::create_tenant,
        );
        let tasks = TableName::new("tasks").expect("table name should build");
        let filler = TableName::new("filler").expect("table name should build");
        let projection_work = Arc::new(ProjectionWork::new_with_aggregate(&engine, 4, 3, 2, 1));
        let observer = Arc::new(TableProjectionObserver {
            projection_work: projection_work.clone(),
        });

        engine
            .insert_document_async(
                victim.clone(),
                tasks.clone(),
                serde_json::Map::from_iter([("index".to_string(), json!(0))]),
            )
            .await
            .expect("victim source row should commit");

        let saturating = (0..2)
            .map(|_| {
                projection_work
                    .register(
                        &hog,
                        engine
                            .committed_mutation_observer_runtime_identity(&hog)
                            .expect("hog runtime identity should load"),
                        std::slice::from_ref(&filler),
                    )
                    .expect("the hog should fill the aggregate cap")
            })
            .collect::<Vec<_>>();

        observer.project_tables(victim.clone(), vec![tasks.clone()]);
        let dropped = projection_work.stats(&victim);
        assert_eq!(
            dropped.dropped_event_count, 1,
            "the aggregate cap must reject the victim while the hog holds it"
        );
        assert_eq!(dropped.dirty_projection_scope_count, 1);
        assert_eq!(
            projected_table_row_count(&engine, &victim, &tasks).await,
            None,
            "the aggregate-cap victim must not be projected while the cap is breached"
        );

        drop(saturating);
        tokio::time::timeout(
            Duration::from_secs(5),
            projection_work.wait_for_idle(&victim),
        )
        .await
        .expect("the victim catch-up should drain");

        assert_eq!(
            projected_table_row_count(&engine, &victim, &tasks).await,
            Some(1),
            "an aggregate-cap drop must be caught up from another tenant's drain"
        );
        let recovered = projection_work.stats(&victim);
        assert_eq!(recovered.dirty_projection_scope_count, 0);
        assert_eq!(recovered.catch_up_projection_count, 1);
        assert_eq!(recovered.dropped_event_count, 1);
        assert!(!recovered.poisoned);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn tenant_scoped_diagnostics_ignore_other_tenant_projection_work() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let engine = fixture.engine();
        let tenant_a = fixture.create_tenant("projection-flush-a", Engine::create_tenant);
        let tenant_b = fixture.create_tenant("projection-flush-b", Engine::create_tenant);
        let (observer, projection_work) = test_observer(&engine, 16, 12);
        engine.install_committed_mutation_observer("projection-tenant-flush-test", observer);
        let tenant_b_work = projection_work
            .register(
                &tenant_b,
                engine
                    .committed_mutation_observer_runtime_identity(&tenant_b)
                    .expect("tenant B runtime identity should load"),
                &[TableName::new("tasks").expect("table name should build")],
            )
            .expect("tenant B background work should register");

        tokio::time::timeout(
            Duration::from_secs(1),
            engine.flush_committed_mutation_observers_for_testing(&tenant_a),
        )
        .await
        .expect("tenant A flush must not wait for tenant B")
        .expect("tenant A flush should succeed");
        assert_eq!(projection_work.stats(&tenant_a).depth, 0);
        assert_eq!(projection_work.stats(&tenant_b).depth, 1);
        drop(tenant_b_work);
        assert_eq!(projection_work.stats(&tenant_b).depth, 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn projection_work_sweeps_evicted_tenant_runtime_generations() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let engine = fixture.engine();
        let (observer, projection_work) = test_observer(&engine, 16, 12);
        engine.install_committed_mutation_observer("projection-churn-test", observer);

        for index in 0..8 {
            let tenant_id =
                TenantId::new(format!("projection-churn-{index}")).expect("tenant id should build");
            engine
                .create_tenant_async(tenant_id.clone())
                .await
                .expect("ephemeral tenant should create");
            engine
                .insert_document_async(
                    tenant_id.clone(),
                    TableName::new("tasks").expect("table name should build"),
                    serde_json::Map::from_iter([("index".to_string(), json!(index))]),
                )
                .await
                .expect("ephemeral tenant mutation should commit");
            engine
                .flush_committed_mutation_observers_for_testing(&tenant_id)
                .await
                .expect("ephemeral tenant projection should drain");
            engine
                .delete_tenant_async(tenant_id)
                .await
                .expect("ephemeral tenant should delete");
        }

        assert_eq!(
            projection_work.tenant_count(),
            0,
            "dead runtime generations must not accumulate in projection state"
        );
    }

    #[test]
    fn projection_register_hot_path_does_not_scan_before_amortized_sweep() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let engine = fixture.engine();
        let tenant_id = fixture.create_tenant("projection-hot-path", Engine::create_tenant);
        let projection_work = Arc::new(ProjectionWork::new(&engine, 64, 48));
        let tasks = TableName::new("tasks").expect("table name should build");

        for _ in 0..64 {
            let guard = projection_work
                .register(
                    &tenant_id,
                    engine
                        .committed_mutation_observer_runtime_identity(&tenant_id)
                        .expect("tenant runtime identity should load"),
                    std::slice::from_ref(&tasks),
                )
                .expect("hot-path projection should register below its cap");
            drop(guard);
        }

        assert_eq!(
            projection_work.sweep_count(),
            0,
            "ordinary projection registration must not scan the tenant map"
        );
    }

    #[test]
    fn projection_aggregate_cap_drops_then_resumes_victim_after_drain() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let engine = fixture.engine();
        let tenant_a = fixture.create_tenant("projection-aggregate-a", Engine::create_tenant);
        let tenant_b = fixture.create_tenant("projection-aggregate-b", Engine::create_tenant);
        let tenant_c = fixture.create_tenant("projection-aggregate-c", Engine::create_tenant);
        let tenant_d = fixture.create_tenant("projection-aggregate-d", Engine::create_tenant);
        let projection_work = Arc::new(ProjectionWork::new_with_aggregate(&engine, 4, 3, 2, 1));
        let tasks = TableName::new("tasks").expect("table name should build");

        let register = |tenant_id: &TenantId| {
            projection_work.register(
                tenant_id,
                engine
                    .committed_mutation_observer_runtime_identity(tenant_id)
                    .expect("tenant runtime identity should load"),
                std::slice::from_ref(&tasks),
            )
        };
        let guard_a = register(&tenant_a).expect("tenant A should fit below both caps");
        let guard_b = register(&tenant_b).expect("tenant B should fill the aggregate cap");
        assert!(
            register(&tenant_c).is_none(),
            "the aggregate cap must reject the offending third registration"
        );
        let rejected = projection_work.stats(&tenant_c);
        assert_eq!(rejected.depth, 0);
        assert_eq!(rejected.cap_breach_count, 1);
        assert_eq!(rejected.dropped_event_count, 1);
        assert!(
            !rejected.poisoned,
            "an aggregate-cap race must not permanently poison the tenant that lost it"
        );
        assert!(!projection_work.stats(&tenant_a).poisoned);
        assert!(!projection_work.stats(&tenant_b).poisoned);
        assert_eq!(
            projection_work
                .aggregate_cap_breach_count
                .load(Ordering::Relaxed),
            1
        );

        drop(guard_a);
        drop(guard_b);
        let resumed = register(&tenant_c)
            .expect("the aggregate-cap victim must resume after the hog work drains");
        let resumed_stats = projection_work.stats(&tenant_c);
        assert_eq!(resumed_stats.depth, 1);
        assert_eq!(resumed_stats.cap_breach_count, 1);
        assert_eq!(resumed_stats.dropped_event_count, 1);
        assert!(!resumed_stats.poisoned);
        drop(resumed);

        let quiet_guard = register(&tenant_d)
            .expect("a quiet process must admit a tenant below its per-tenant cap");
        assert_eq!(projection_work.stats(&tenant_d).depth, 1);
        assert!(!projection_work.stats(&tenant_d).poisoned);
        drop(quiet_guard);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn newer_projection_generation_rejects_stale_row_count_write() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let engine = fixture.engine();
        let tenant_id = fixture.create_tenant("projection-generation", Engine::create_tenant);
        let tasks = TableName::new("tasks").expect("table name should build");
        let first = engine
            .insert_document_async(
                tenant_id.clone(),
                tasks.clone(),
                serde_json::Map::from_iter([("index".to_string(), json!(1))]),
            )
            .await
            .expect("first source row should commit");
        let second = engine
            .insert_document_async(
                tenant_id.clone(),
                tasks.clone(),
                serde_json::Map::from_iter([("index".to_string(), json!(2))]),
            )
            .await
            .expect("second source row should commit");
        record_table_state_for_generation_async(
            &engine,
            &tenant_id,
            &tasks,
            "projection-test-epoch",
            2,
        )
        .await
        .expect("new-generation projection should record two rows");

        engine
            .delete_document_async(tenant_id.clone(), tasks.clone(), second)
            .await
            .expect("source row should delete");
        record_table_state_for_generation_async(
            &engine,
            &tenant_id,
            &tasks,
            "projection-test-epoch",
            1,
        )
        .await
        .expect("stale generation should be rejected without an error");
        assert_eq!(
            projected_table_row_count(&engine, &tenant_id, &tasks).await,
            Some(2),
            "an old runtime generation must not overwrite the newer projected count"
        );

        record_table_state_for_generation_async(
            &engine,
            &tenant_id,
            &tasks,
            "projection-restarted-process-epoch",
            1,
        )
        .await
        .expect("a fresh process epoch should refresh despite its lower generation");
        assert_eq!(
            projected_table_row_count(&engine, &tenant_id, &tasks).await,
            Some(1)
        );
        engine
            .delete_document_async(tenant_id, tasks, first)
            .await
            .expect("remaining source row should delete");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn reloaded_runtime_skips_parked_old_generation_projection() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let engine = fixture.engine();
        let tenant_id = fixture.create_tenant("projection-generation-race", Engine::create_tenant);
        let tasks = TableName::new("tasks").expect("table name should build");
        let (observer, projection_work) = test_observer(&engine, 16, 12);
        let held_projection = tenant_work(&engine, &projection_work, &tenant_id)
            .projection_lock
            .clone()
            .lock_owned()
            .await;

        for index in 0..2 {
            engine
                .insert_document_async(
                    tenant_id.clone(),
                    tasks.clone(),
                    serde_json::Map::from_iter([("index".to_string(), json!(index))]),
                )
                .await
                .expect("old-generation source row should commit");
        }
        observer.project_tables(tenant_id.clone(), vec![tasks.clone()]);
        projection_work.wait_until_registered(&tenant_id).await;

        engine
            .delete_tenant_async(tenant_id.clone())
            .await
            .expect("old runtime should evict while its projection is parked");
        engine
            .create_tenant_async(tenant_id.clone())
            .await
            .expect("tenant should reload with a fresh runtime generation");
        engine
            .insert_document_async(
                tenant_id.clone(),
                tasks.clone(),
                serde_json::Map::from_iter([("index".to_string(), json!(10))]),
            )
            .await
            .expect("new-generation source row should commit");
        observer.project_tables(tenant_id.clone(), vec![tasks.clone()]);
        tokio::time::timeout(Duration::from_secs(5), async {
            while projection_work.stats(&tenant_id).depth != 2 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("both runtime generations should register before the lock is released");

        drop(held_projection);
        tokio::time::timeout(
            Duration::from_secs(5),
            projection_work.wait_for_idle(&tenant_id),
        )
        .await
        .expect("new-generation projection should drain");
        assert_eq!(
            projected_table_row_count(&engine, &tenant_id, &tasks).await,
            Some(1),
            "the parked old generation must not overwrite the reloaded runtime's count"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn applied_wait_eviction_error_releases_tenant_projection_lock() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let engine = fixture.engine();
        let tenant_id = fixture.create_tenant("projection-eviction-wait", Engine::create_tenant);
        let (observer, projection_work) = test_observer(&engine, 16, 12);
        let tenant_work = tenant_work(&engine, &projection_work, &tenant_id);
        engine
            .park_applied_sequence_waiters_for_testing(&tenant_id, nimbus_core::SequenceNumber(1))
            .expect("test should expose a durable-but-unapplied target");

        observer.project_tables(
            tenant_id.clone(),
            vec![TableName::new("tasks").expect("table name should build")],
        );
        projection_work.wait_until_registered(&tenant_id).await;
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if tenant_work.projection_lock.try_lock().is_err() {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("projection task should acquire its tenant lock");

        engine
            .fail_applied_sequence_waiters_for_testing(&tenant_id)
            .expect("test eviction should wake applied waiters");
        tokio::time::timeout(
            Duration::from_secs(5),
            projection_work.wait_for_idle(&tenant_id),
        )
        .await
        .expect("eviction error must let the projection task release its work guard");
        assert_eq!(projection_work.stats(&tenant_id).depth, 0);
        assert!(
            tenant_work.projection_lock.try_lock().is_ok(),
            "the tenant projection lock must be released after the typed wait error"
        );
    }
}
