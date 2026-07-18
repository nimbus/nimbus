use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};

use nimbus_core::{TableName, TenantId};
use nimbus_engine::{
    CommittedMutationEvent, CommittedMutationObserver, CommittedMutationObserverWorkStats, Engine,
    TableSchemaChangeEvent, TableSchemaChangeObserver,
};
use tracing::{error, warn};

use super::{is_reserved_tenant_id, record_table_state_async};

const TABLE_PROJECTION_OBSERVER: &str = "nimbus-system-table-projection";
const DEFAULT_PROJECTION_WORK_CAPACITY: usize = 4_096;
const DEFAULT_PROJECTION_WORK_HIGH_WATERMARK: usize = 3_072;

struct TableProjectionObserver {
    engine: Weak<Engine>,
    projection_lock: Arc<tokio::sync::Mutex<()>>,
    projection_work: Arc<ProjectionWork>,
}

struct ProjectionWork {
    in_flight: AtomicUsize,
    capacity: usize,
    high_watermark: usize,
    high_water_warning_active: AtomicBool,
    high_water_warning_count: AtomicU64,
    cap_breach_count: AtomicU64,
    poisoned: AtomicBool,
    tenants: Mutex<HashMap<TenantId, Arc<TenantProjectionWork>>>,
    #[cfg(test)]
    registered: tokio::sync::Notify,
}

#[derive(Default)]
struct TenantProjectionWork {
    in_flight: AtomicUsize,
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
}

impl ProjectionWork {
    fn from_env() -> Self {
        Self::new(
            env_positive_usize(
                "NIMBUS_SYSTEM_PROJECTION_WORK_CAPACITY",
                DEFAULT_PROJECTION_WORK_CAPACITY,
            ),
            env_positive_usize(
                "NIMBUS_SYSTEM_PROJECTION_WORK_HIGH_WATERMARK",
                DEFAULT_PROJECTION_WORK_HIGH_WATERMARK,
            ),
        )
    }

    fn new(capacity: usize, high_watermark: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            in_flight: AtomicUsize::new(0),
            capacity,
            high_watermark: high_watermark.max(1).min(capacity),
            high_water_warning_active: AtomicBool::new(false),
            high_water_warning_count: AtomicU64::new(0),
            cap_breach_count: AtomicU64::new(0),
            poisoned: AtomicBool::new(false),
            tenants: Mutex::new(HashMap::new()),
            #[cfg(test)]
            registered: tokio::sync::Notify::new(),
        }
    }

    fn tenant_work(&self, tenant_id: &TenantId) -> Arc<TenantProjectionWork> {
        self.tenants
            .lock()
            .expect("projection tenant-work lock should not be poisoned")
            .entry(tenant_id.clone())
            .or_default()
            .clone()
    }

    fn register(self: &Arc<Self>, tenant_id: &TenantId) -> Option<ProjectionWorkGuard> {
        if self.poisoned.load(Ordering::Acquire) {
            return None;
        }
        let previous = match self.in_flight.fetch_update(
            Ordering::AcqRel,
            Ordering::Acquire,
            |depth| {
                (!self.poisoned.load(Ordering::Acquire) && depth < self.capacity)
                    .then_some(depth + 1)
            },
        ) {
            Ok(previous) => previous,
            Err(depth) => {
                if self.poisoned.load(Ordering::Acquire) {
                    return None;
                }
                self.cap_breach_count.fetch_add(1, Ordering::Relaxed);
                if !self.poisoned.swap(true, Ordering::AcqRel) {
                    error!(
                        projection_work_depth = depth,
                        projection_work_capacity = self.capacity,
                        tenant = %tenant_id,
                        "system table projection work cap breached; projection observer poisoned and no new projection tasks will be spawned"
                    );
                }
                return None;
            }
        };
        let depth = previous + 1;
        if depth >= self.high_watermark
            && !self.high_water_warning_active.swap(true, Ordering::AcqRel)
        {
            self.high_water_warning_count
                .fetch_add(1, Ordering::Relaxed);
            warn!(
                projection_work_depth = depth,
                projection_work_high_watermark = self.high_watermark,
                projection_work_capacity = self.capacity,
                "system table projection work crossed its high-water mark"
            );
        }
        let tenant_work = self.tenant_work(tenant_id);
        tenant_work.in_flight.fetch_add(1, Ordering::AcqRel);
        #[cfg(test)]
        self.registered.notify_waiters();
        Some(ProjectionWorkGuard {
            work: self.clone(),
            tenant_work,
            tenant_id: tenant_id.clone(),
        })
    }

    async fn wait_for_idle(&self, tenant_id: &TenantId) {
        let tenant_work = self.tenant_work(tenant_id);
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

    fn stats(&self) -> CommittedMutationObserverWorkStats {
        CommittedMutationObserverWorkStats {
            depth: self.in_flight.load(Ordering::Acquire),
            capacity: self.capacity,
            high_watermark: self.high_watermark,
            high_water_warning_count: self.high_water_warning_count.load(Ordering::Relaxed),
            cap_breach_count: self.cap_breach_count.load(Ordering::Relaxed),
            poisoned: self.poisoned.load(Ordering::Acquire),
        }
    }

    #[cfg(test)]
    async fn wait_until_registered(&self, tenant_id: &TenantId) {
        let tenant_work = self.tenant_work(tenant_id);
        loop {
            let notified = self.registered.notified();
            if tenant_work.in_flight.load(Ordering::Acquire) != 0 {
                return;
            }
            notified.await;
        }
    }

    #[cfg(test)]
    async fn wait_until_flush_waits(&self, tenant_id: &TenantId) {
        let tenant_work = self.tenant_work(tenant_id);
        loop {
            let notified = tenant_work.flush_waiting_notify.notified();
            if tenant_work.flush_waiting.load(Ordering::Acquire) {
                return;
            }
            notified.await;
        }
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
        let previous = self.work.in_flight.fetch_sub(1, Ordering::AcqRel);
        debug_assert!(previous != 0, "projection work count cannot underflow");
        if previous.saturating_sub(1) < self.work.high_watermark {
            self.work
                .high_water_warning_active
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

    fn spawned_work_stats(&self, _tenant_id: &TenantId) -> CommittedMutationObserverWorkStats {
        self.projection_work.stats()
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
        let projection_lock = self.projection_lock.clone();
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
        let Some(projection_work) = self.projection_work.register(&tenant_id) else {
            return;
        };
        handle.spawn(async move {
            let _projection_work = projection_work;
            let _projection_guard = projection_lock.lock().await;
            for table in tables {
                if let Err(error) = record_table_state_async(&engine, &tenant_id, &table).await {
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
        projection_lock: Arc::new(tokio::sync::Mutex::new(())),
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

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn committed_observer_flush_waits_for_spawned_projection_tail() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let engine = fixture.engine();
        let tenant_id = fixture.create_tenant("projection-flush-tail", Engine::create_tenant);
        let projection_lock = Arc::new(tokio::sync::Mutex::new(()));
        let held_projection = projection_lock.clone().lock_owned().await;
        let projection_work = Arc::new(ProjectionWork::new(16, 12));
        let observer = Arc::new(TableProjectionObserver {
            engine: Arc::downgrade(&engine),
            projection_lock,
            projection_work: projection_work.clone(),
        });
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
        assert_eq!(projection_work.in_flight.load(Ordering::Acquire), 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn projection_backlog_is_visible_and_poisoned_at_the_hard_cap() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let engine = fixture.engine();
        let tenant_id = fixture.create_tenant("projection-work-cap", Engine::create_tenant);
        let projection_lock = Arc::new(tokio::sync::Mutex::new(()));
        let held_projection = projection_lock.clone().lock_owned().await;
        let projection_work = Arc::new(ProjectionWork::new(2, 1));
        let observer = Arc::new(TableProjectionObserver {
            engine: Arc::downgrade(&engine),
            projection_lock,
            projection_work: projection_work.clone(),
        });
        engine.install_committed_mutation_observer("projection-work-cap-test", observer);

        for index in 0..3 {
            engine
                .insert_document_async(
                    tenant_id.clone(),
                    TableName::new("tasks").expect("table name should build"),
                    serde_json::Map::from_iter([("index".to_string(), json!(index))]),
                )
                .await
                .expect("projection saturation must not block durable mutation responses");
        }

        let stats = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let stats = engine
                    .tenant_engine_diagnostics(&tenant_id)
                    .expect("projection diagnostics should load")
                    .mutation_journal;
                if stats.observer_spawned_work_poisoned {
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

        drop(held_projection);
        engine
            .flush_committed_mutation_observers_for_testing(&tenant_id)
            .await
            .expect("accepted projection work should drain after poison");
        assert_eq!(projection_work.in_flight.load(Ordering::Acquire), 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn committed_observer_flush_ignores_other_tenant_projection_work() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let engine = fixture.engine();
        let tenant_a = fixture.create_tenant("projection-flush-a", Engine::create_tenant);
        let tenant_b = fixture.create_tenant("projection-flush-b", Engine::create_tenant);
        let projection_work = Arc::new(ProjectionWork::new(16, 12));
        let observer = Arc::new(TableProjectionObserver {
            engine: Arc::downgrade(&engine),
            projection_lock: Arc::new(tokio::sync::Mutex::new(())),
            projection_work: projection_work.clone(),
        });
        engine.install_committed_mutation_observer("projection-tenant-flush-test", observer);
        let tenant_b_work = projection_work
            .register(&tenant_b)
            .expect("tenant B background work should register");

        tokio::time::timeout(
            Duration::from_secs(1),
            engine.flush_committed_mutation_observers_for_testing(&tenant_a),
        )
        .await
        .expect("tenant A flush must not wait for tenant B")
        .expect("tenant A flush should succeed");
        assert_eq!(projection_work.in_flight.load(Ordering::Acquire), 1);
        assert_eq!(
            projection_work
                .tenant_work(&tenant_b)
                .in_flight
                .load(Ordering::Acquire),
            1
        );
        drop(tenant_b_work);
        assert_eq!(projection_work.in_flight.load(Ordering::Acquire), 0);
    }
}
