use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use nimbus_core::{Result, TableName};

pub(super) struct MaterializedWarmLoadPermit<'a> {
    in_flight_load_count: &'a AtomicU64,
}

pub(super) struct MaterializedWarmLoadOwner<'a> {
    coordinator: &'a MaterializedWarmLoadCoordinator,
    table: TableName,
}

#[derive(Default)]
pub(super) struct MaterializedWarmLoadCoordinator {
    tables: Mutex<HashMap<TableName, Arc<MaterializedWarmLoadWaitState>>>,
}

#[derive(Default)]
pub(super) struct MaterializedWarmLoadWaitState {
    completed: Mutex<bool>,
    condvar: Condvar,
}

pub(super) enum MaterializedWarmLoadDecision<'a> {
    Load(MaterializedWarmLoadOwner<'a>),
    Wait(Arc<MaterializedWarmLoadWaitState>),
}

impl Drop for MaterializedWarmLoadPermit<'_> {
    fn drop(&mut self) {
        self.in_flight_load_count.fetch_sub(1, Ordering::Relaxed);
    }
}

impl Drop for MaterializedWarmLoadOwner<'_> {
    fn drop(&mut self) {
        let wait_state = self
            .coordinator
            .tables
            .lock()
            .expect("materialized warm load coordinator lock should not be poisoned")
            .remove(&self.table);
        if let Some(wait_state) = wait_state {
            *wait_state
                .completed
                .lock()
                .expect("materialized warm load wait state lock should not be poisoned") = true;
            wait_state.condvar.notify_all();
        }
    }
}

impl MaterializedWarmLoadPermit<'_> {
    pub(super) fn new(in_flight_load_count: &AtomicU64) -> MaterializedWarmLoadPermit<'_> {
        in_flight_load_count.fetch_add(1, Ordering::Relaxed);
        MaterializedWarmLoadPermit {
            in_flight_load_count,
        }
    }
}

impl MaterializedWarmLoadCoordinator {
    pub(super) fn begin_or_wait_for_warm_load(
        &self,
        table: &TableName,
    ) -> MaterializedWarmLoadDecision<'_> {
        let mut loading_tables = self
            .tables
            .lock()
            .expect("materialized warm load coordinator lock should not be poisoned");
        if let Some(wait_state) = loading_tables.get(table) {
            return MaterializedWarmLoadDecision::Wait(wait_state.clone());
        }
        loading_tables.insert(
            table.clone(),
            Arc::new(MaterializedWarmLoadWaitState::default()),
        );
        MaterializedWarmLoadDecision::Load(MaterializedWarmLoadOwner {
            coordinator: self,
            table: table.clone(),
        })
    }

    pub(super) fn clear(&self) -> Vec<Arc<MaterializedWarmLoadWaitState>> {
        self.tables
            .lock()
            .expect("materialized warm load coordinator lock should not be poisoned")
            .drain()
            .map(|(_, wait_state)| wait_state)
            .collect()
    }
}

impl MaterializedWarmLoadWaitState {
    pub(super) fn wait_cancellable(
        &self,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<()> {
        let mut completed = self
            .completed
            .lock()
            .expect("materialized warm load wait state lock should not be poisoned");
        while !*completed {
            check_cancel()?;
            let (next_completed, _) = self
                .condvar
                .wait_timeout(completed, std::time::Duration::from_millis(10))
                .expect("materialized warm load wait state lock should not be poisoned");
            completed = next_completed;
        }
        check_cancel()?;
        Ok(())
    }

    pub(super) fn mark_completed(&self) {
        *self
            .completed
            .lock()
            .expect("materialized warm load wait state lock should not be poisoned") = true;
        self.condvar.notify_all();
    }
}
