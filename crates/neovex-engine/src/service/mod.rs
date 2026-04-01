mod execution_units;
mod mutations;
mod queries;
mod scheduler;
mod schema;
mod subscriptions;
mod tenants;
mod usage;

use std::collections::HashMap;
use std::future::Future;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::RwLock;

use neovex_core::{Document, Error, Result, TenantId, Timestamp};
use neovex_storage::{
    Clock, FaultInjector, NoopFaultInjector, SystemClock, TenantStore, UsageStore,
};
use tokio::sync::Notify;

use crate::tenant::TenantRuntime;

pub use execution_units::MutationExecutionUnit;

/// Top-level Neovex engine service.
pub struct Service {
    data_dir: PathBuf,
    tenants: RwLock<HashMap<TenantId, Arc<TenantRuntime>>>,
    usage_store: Arc<UsageStore>,
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
        let usage_store = Arc::new(UsageStore::open(data_dir.join("neovex-control.db"))?);
        Ok(Self {
            data_dir,
            tenants: RwLock::new(HashMap::new()),
            usage_store,
            clock,
            storage_fault_injector,
            scheduler_wakeup: Notify::new(),
        })
    }

    pub(crate) async fn call_blocking<T, F>(self: &Arc<Self>, task: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(Arc<Self>) -> Result<T> + Send + 'static,
    {
        let service = self.clone();
        tokio::task::spawn_blocking(move || task(service))
            .await
            .map_err(|error| Error::Internal(format!("blocking task failed: {error}")))?
    }

    pub(crate) async fn call_blocking_cancellable<T, Fut, F>(
        self: &Arc<Self>,
        cancel_wait: Fut,
        task: F,
    ) -> Result<T>
    where
        T: Send + 'static,
        Fut: Future<Output = ()> + Send,
        F: FnOnce(Arc<Self>) -> Result<T> + Send + 'static,
    {
        let service = self.clone();
        let handle = tokio::task::spawn_blocking(move || task(service));
        tokio::pin!(cancel_wait);

        tokio::select! {
            _ = &mut cancel_wait => Err(Error::Cancelled),
            result = handle => result
                .map_err(|error| Error::Internal(format!("blocking task failed: {error}")))?,
        }
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
}

fn documents_to_json(documents: Vec<Document>) -> Vec<serde_json::Value> {
    documents
        .into_iter()
        .map(|document| document.to_json())
        .collect()
}
