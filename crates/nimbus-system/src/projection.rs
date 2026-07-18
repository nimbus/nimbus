use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};

use nimbus_core::{DocumentId, TableName, TenantId};
use nimbus_engine::{
    CommittedMutationEvent, CommittedMutationObserver, CommittedMutationObserverWorkStats, Engine,
    TableSchemaChangeEvent, TableSchemaChangeObserver, TenantRuntimeObserverIdentity,
};
use tracing::{error, warn};

use super::{is_reserved_tenant_id, record_table_state_for_generation_async};

const TABLE_PROJECTION_OBSERVER: &str = "nimbus-system-table-projection";
const DEFAULT_PROJECTION_WORK_CAPACITY: usize = 1_024;
const DEFAULT_PROJECTION_WORK_HIGH_WATERMARK: usize = 768;
const DEFAULT_PROJECTION_AGGREGATE_WORK_CAPACITY: usize = 8_192;
const DEFAULT_PROJECTION_AGGREGATE_WORK_HIGH_WATERMARK: usize = 6_144;
const PROJECTION_TENANT_SWEEP_INTERVAL: usize = 1_024;

struct TableProjectionObserver {
    engine: Weak<Engine>,
    projection_work: Arc<ProjectionWork>,
}

struct ProjectionWork {
    epoch: Arc<str>,
    capacity: usize,
    high_watermark: usize,
    aggregate_capacity: usize,
    aggregate_high_watermark: usize,
    aggregate_in_flight: AtomicUsize,
    aggregate_high_water_warning_active: AtomicBool,
    aggregate_high_water_warning_count: AtomicU64,
    aggregate_cap_breach_count: AtomicU64,
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
    cap_breach_count: AtomicU64,
    dropped_event_count: AtomicU64,
    poisoned: AtomicBool,
    idle: tokio::sync::Notify,
    #[cfg(test)]
    flush_waiting: AtomicBool,
    #[cfg(test)]
    flush_waiting_notify: tokio::sync::Notify,
}

struct ProjectionWorkGuard {
    work: Arc<ProjectionWork>,
    tenant_work: Arc<TenantProjectionWork>,
    tenant_id: TenantId,
    generation: u64,
}

impl ProjectionWork {
    fn from_env() -> Self {
        Self::new_with_aggregate(
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

    fn new(capacity: usize, high_watermark: usize) -> Self {
        let aggregate_capacity = capacity.saturating_mul(8).max(capacity).max(1);
        let aggregate_high_watermark = high_watermark
            .saturating_mul(8)
            .max(high_watermark)
            .min(aggregate_capacity);
        Self::new_with_aggregate(
            capacity,
            high_watermark,
            aggregate_capacity,
            aggregate_high_watermark,
        )
    }

    fn new_with_aggregate(
        capacity: usize,
        high_watermark: usize,
        aggregate_capacity: usize,
        aggregate_high_watermark: usize,
    ) -> Self {
        let capacity = capacity.max(1);
        let aggregate_capacity = aggregate_capacity.max(1);
        Self {
            epoch: DocumentId::new().to_string().into(),
            capacity,
            high_watermark: high_watermark.max(1).min(capacity),
            aggregate_capacity,
            aggregate_high_watermark: aggregate_high_watermark.max(1).min(aggregate_capacity),
            aggregate_in_flight: AtomicUsize::new(0),
            aggregate_high_water_warning_active: AtomicBool::new(false),
            aggregate_high_water_warning_count: AtomicU64::new(0),
            aggregate_cap_breach_count: AtomicU64::new(0),
            next_generation: AtomicU64::new(0),
            tenants: Mutex::new(ProjectionTenantRegistry::default()),
            #[cfg(test)]
            registered: tokio::sync::Notify::new(),
            #[cfg(test)]
            sweep_count: AtomicU64::new(0),
        }
    }

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
            work.cap_breach_count.store(0, Ordering::Relaxed);
            work.dropped_event_count.store(0, Ordering::Relaxed);
            work.poisoned.store(false, Ordering::Release);
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
            cap_breach_count: AtomicU64::new(0),
            dropped_event_count: AtomicU64::new(0),
            poisoned: AtomicBool::new(false),
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
        Self::sweep_dead_tenants_locked(registry);
        #[cfg(test)]
        self.sweep_count.fetch_add(1, Ordering::Relaxed);
    }

    fn sweep_dead_tenants_locked(registry: &mut ProjectionTenantRegistry) {
        registry.tenants.retain(|_, work| {
            work.in_flight.load(Ordering::Acquire) != 0
                || work
                    .runtime_identity
                    .lock()
                    .expect("projection runtime-identity lock should not be poisoned")
                    .is_live()
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
    ) -> Option<ProjectionWorkGuard> {
        let mut registry = self
            .tenants
            .lock()
            .expect("projection tenant-work lock should not be poisoned");
        self.maybe_sweep_dead_tenants_locked(&mut registry);
        let tenant_work = self.tenant_work_locked(&mut registry, tenant_id, runtime_identity);
        if tenant_work.poisoned.load(Ordering::Acquire) {
            tenant_work
                .dropped_event_count
                .fetch_add(1, Ordering::Relaxed);
            return None;
        }
        let previous = match tenant_work.in_flight.fetch_update(
            Ordering::AcqRel,
            Ordering::Acquire,
            |depth| (depth < self.capacity).then_some(depth + 1),
        ) {
            Ok(previous) => previous,
            Err(depth) => {
                tenant_work
                    .dropped_event_count
                    .fetch_add(1, Ordering::Relaxed);
                tenant_work.cap_breach_count.fetch_add(1, Ordering::Relaxed);
                if !tenant_work.poisoned.swap(true, Ordering::AcqRel) {
                    error!(
                        projection_work_depth = depth,
                        projection_work_capacity = self.capacity,
                        tenant = %tenant_id,
                        "system table projection per-tenant work cap breached; tenant projection observer poisoned and no new projection tasks will be spawned for this runtime"
                    );
                }
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
                if !tenant_work.poisoned.swap(true, Ordering::AcqRel) {
                    error!(
                        projection_aggregate_work_depth = depth,
                        projection_aggregate_work_capacity = self.aggregate_capacity,
                        tenant = %tenant_id,
                        "system table projection aggregate work cap breached; offending tenant projection observer poisoned and the committed event was dropped"
                    );
                }
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

    async fn wait_for_idle(&self, tenant_id: &TenantId) {
        let Some(tenant_work) = self.existing_tenant_work(tenant_id) else {
            return;
        };
        loop {
            let notified = tenant_work.idle.notified();
            if tenant_work.in_flight.load(Ordering::Acquire) == 0 {
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

    fn stats(&self, tenant_id: &TenantId) -> CommittedMutationObserverWorkStats {
        let tenant_work = self.existing_tenant_work(tenant_id);
        CommittedMutationObserverWorkStats {
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
            poisoned: tenant_work
                .as_ref()
                .is_some_and(|work| work.poisoned.load(Ordering::Acquire)),
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
        Self::sweep_dead_tenants_locked(&mut registry);
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
        if tenant_previous == 1 {
            self.tenant_work.idle.notify_waiters();
        }
        if tenant_previous.saturating_sub(1) < self.work.high_watermark {
            self.tenant_work
                .high_water_warning_active
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
        self.projection_work.stats(tenant_id)
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
    fn project_tables(&self, tenant_id: TenantId, mut tables: Vec<TableName>) {
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
        tables.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            warn!(
                tenant_id = %tenant_id,
                "skipping system table projection because no tokio runtime is active"
            );
            return;
        };
        // Register before spawning so a dispatcher fence followed immediately
        // by a test flush cannot overtake a task that has not been polled yet.
        let Some(projection_work) = self.projection_work.register(&tenant_id, runtime_identity)
        else {
            return;
        };
        let tenant_work = projection_work.tenant_work.clone();
        let projection_epoch = self.projection_work.epoch.clone();
        let projection_generation = projection_work.generation;
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
}

pub fn install_table_projection_observer(engine: &Arc<Engine>) {
    let observer = Arc::new(TableProjectionObserver {
        engine: Arc::downgrade(engine),
        projection_work: Arc::new(ProjectionWork::from_env()),
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
        let projection_work = Arc::new(ProjectionWork::new(capacity, high_watermark));
        (
            Arc::new(TableProjectionObserver {
                engine: Arc::downgrade(engine),
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
        let rows = engine
            .list_documents_async(
                crate::system_tenant_id().expect("system tenant id should build"),
                crate::schema::SystemTable::Tables
                    .table_name()
                    .expect("system tables name should build"),
            )
            .await
            .expect("projected table records should list");
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
    async fn projection_poison_is_tenant_scoped_counts_drops_and_reload_rearms() {
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
        assert_eq!(stats.observer_spawned_work_cap_breach_count, 1);
        assert_eq!(stats.observer_spawned_work_dropped_event_count, 2);
        assert!(stats.observer_spawned_work_poisoned);

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
            "tenant B must continue projecting while tenant A is poisoned"
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
            .expect("accepted projection work should drain after poison");
        assert_eq!(projection_work.stats(&tenant_a).depth, 0);

        engine
            .delete_tenant_async(tenant_a.clone())
            .await
            .expect("poisoned tenant should delete");
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
        assert!(!reloaded.poisoned, "a fresh runtime must re-arm projection");
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
        let projection_work = Arc::new(ProjectionWork::new(64, 48));

        for _ in 0..64 {
            let guard = projection_work
                .register(
                    &tenant_id,
                    engine
                        .committed_mutation_observer_runtime_identity(&tenant_id)
                        .expect("tenant runtime identity should load"),
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
    fn projection_aggregate_cap_rejects_only_the_offending_tenant() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let engine = fixture.engine();
        let tenant_a = fixture.create_tenant("projection-aggregate-a", Engine::create_tenant);
        let tenant_b = fixture.create_tenant("projection-aggregate-b", Engine::create_tenant);
        let tenant_c = fixture.create_tenant("projection-aggregate-c", Engine::create_tenant);
        let tenant_d = fixture.create_tenant("projection-aggregate-d", Engine::create_tenant);
        let projection_work = Arc::new(ProjectionWork::new_with_aggregate(4, 3, 2, 1));

        let register = |tenant_id: &TenantId| {
            projection_work.register(
                tenant_id,
                engine
                    .committed_mutation_observer_runtime_identity(tenant_id)
                    .expect("tenant runtime identity should load"),
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
        assert!(rejected.poisoned);
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
