use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Weak};

use nimbus_core::{TableName, TenantId};
use nimbus_engine::{
    CommittedMutationEvent, CommittedMutationObserver, Engine, TableSchemaChangeEvent,
    TableSchemaChangeObserver,
};
use tracing::warn;

use super::{is_reserved_tenant_id, record_table_state_async};

const TABLE_PROJECTION_OBSERVER: &str = "nimbus-system-table-projection";

struct TableProjectionObserver {
    engine: Weak<Engine>,
    projection_lock: Arc<tokio::sync::Mutex<()>>,
    projection_work: Arc<ProjectionWork>,
}

#[derive(Default)]
struct ProjectionWork {
    in_flight: AtomicUsize,
    idle: tokio::sync::Notify,
    #[cfg(test)]
    registered: tokio::sync::Notify,
    #[cfg(test)]
    flush_waiting: std::sync::atomic::AtomicBool,
    #[cfg(test)]
    flush_waiting_notify: tokio::sync::Notify,
}

struct ProjectionWorkGuard {
    work: Arc<ProjectionWork>,
}

impl ProjectionWork {
    fn register(self: &Arc<Self>) -> ProjectionWorkGuard {
        self.in_flight.fetch_add(1, Ordering::AcqRel);
        #[cfg(test)]
        self.registered.notify_waiters();
        ProjectionWorkGuard { work: self.clone() }
    }

    async fn wait_for_idle(&self) {
        loop {
            let notified = self.idle.notified();
            if self.in_flight.load(Ordering::Acquire) == 0 {
                return;
            }
            #[cfg(test)]
            {
                self.flush_waiting
                    .store(true, std::sync::atomic::Ordering::Release);
                self.flush_waiting_notify.notify_waiters();
            }
            notified.await;
        }
    }

    #[cfg(test)]
    async fn wait_until_registered(&self) {
        loop {
            let notified = self.registered.notified();
            if self.in_flight.load(Ordering::Acquire) != 0 {
                return;
            }
            notified.await;
        }
    }

    #[cfg(test)]
    async fn wait_until_flush_waits(&self) {
        loop {
            let notified = self.flush_waiting_notify.notified();
            if self
                .flush_waiting
                .load(std::sync::atomic::Ordering::Acquire)
            {
                return;
            }
            notified.await;
        }
    }
}

impl Drop for ProjectionWorkGuard {
    fn drop(&mut self) {
        let previous = self.work.in_flight.fetch_sub(1, Ordering::AcqRel);
        debug_assert!(previous != 0, "projection work count cannot underflow");
        if previous == 1 {
            self.work.idle.notify_waiters();
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

    fn flush_spawned_work_for_testing(&self) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(self.projection_work.wait_for_idle())
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
        let projection_work = self.projection_work.register();
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
        projection_work: Arc::new(ProjectionWork::default()),
    });
    engine.install_committed_mutation_observer(TABLE_PROJECTION_OBSERVER, observer.clone());
    engine.install_table_schema_change_observer(TABLE_PROJECTION_OBSERVER, observer);
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
        let projection_work = Arc::new(ProjectionWork::default());
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
        projection_work.wait_until_registered().await;

        let mut flush = tokio::spawn({
            let engine = engine.clone();
            let tenant_id = tenant_id.clone();
            async move {
                engine
                    .flush_committed_mutation_observers_for_testing(&tenant_id)
                    .await
            }
        });
        projection_work.wait_until_flush_waits().await;
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
}
