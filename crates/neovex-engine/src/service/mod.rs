mod execution_units;
mod mutations;
mod queries;
mod scheduler;
mod schema;
mod subscriptions;
mod tenants;
mod usage;

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::RwLock;

use neovex_core::{Document, Error, Result, TenantId, Timestamp};
use neovex_storage::{
    Clock, FaultInjector, NoopFaultInjector, RedbStorageEngine, RedbUsageStorage, SystemClock,
    TenantStore, UsageStore,
};
use tokio::sync::{Mutex as AsyncMutex, Notify};

use crate::tenant::TenantRuntime;

pub use execution_units::MutationExecutionUnit;

/// Top-level Neovex engine service.
pub struct Service {
    data_dir: PathBuf,
    tenants: RwLock<HashMap<TenantId, Arc<TenantRuntime>>>,
    tenant_load_gate: AsyncMutex<()>,
    storage_engine: Arc<RedbStorageEngine>,
    usage_store: Arc<UsageStore>,
    usage_storage: Arc<RedbUsageStorage>,
    clock: Arc<dyn Clock>,
    storage_fault_injector: Arc<dyn FaultInjector>,
    scheduler_wakeup: Notify,
}

impl Service {
    /// Creates a new service for the provided data directory.
    pub fn new(data_dir: impl Into<PathBuf>) -> Result<Self> {
        Self::new_with_simulation(data_dir, Arc::new(SystemClock), Arc::new(NoopFaultInjector))
    }

    /// Creates a new service with deterministic simulation seams for time and storage faults.
    pub fn new_with_simulation(
        data_dir: impl Into<PathBuf>,
        clock: Arc<dyn Clock>,
        storage_fault_injector: Arc<dyn FaultInjector>,
    ) -> Result<Self> {
        let data_dir = data_dir.into();
        std::fs::create_dir_all(&data_dir).map_err(|error| Error::Internal(error.to_string()))?;
        let storage_engine = Arc::new(RedbStorageEngine::new(
            data_dir.clone(),
            clock.clone(),
            storage_fault_injector.clone(),
        )?);
        let usage_store = storage_engine.usage_store();
        let usage_storage = storage_engine.usage_storage();
        Ok(Self {
            data_dir,
            tenants: RwLock::new(HashMap::new()),
            tenant_load_gate: AsyncMutex::new(()),
            storage_engine,
            usage_store,
            usage_storage,
            clock,
            storage_fault_injector,
            scheduler_wakeup: Notify::new(),
        })
    }

    pub(crate) fn wake_scheduler(&self) {
        self.scheduler_wakeup.notify_one();
    }

    pub(crate) fn scheduler_notifier(&self) -> &Notify {
        &self.scheduler_wakeup
    }

    pub(crate) fn now(&self) -> Timestamp {
        self.clock.now()
    }

    pub(crate) fn open_tenant_store(&self, path: &Path) -> Result<TenantStore> {
        TenantStore::open_with_simulation(
            path,
            self.clock.clone(),
            self.storage_fault_injector.clone(),
        )
    }

    pub(crate) fn lock_tenant_load_gate_blocking(&self) -> tokio::sync::MutexGuard<'_, ()> {
        loop {
            if let Ok(guard) = self.tenant_load_gate.try_lock() {
                return guard;
            }
            std::thread::yield_now();
        }
    }

    pub(crate) fn build_loaded_tenant_runtime(
        &self,
        store: TenantStore,
    ) -> Result<Arc<TenantRuntime>> {
        let store = Arc::new(store);
        let read_storage = self.storage_engine.read_storage_for_store(store.clone());
        let runtime = Arc::new(TenantRuntime::from_parts(store.clone(), read_storage)?);
        let progress = store.recover_durable_journal()?;
        runtime.sync_mutation_journal_progress(progress);
        Ok(runtime)
    }
}

fn documents_to_json(documents: Vec<Document>) -> Vec<serde_json::Value> {
    documents
        .into_iter()
        .map(|document| document.to_json())
        .collect()
}
