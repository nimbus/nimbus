mod background_executor;
mod bootstrap;
mod diagnostics;
mod encryption;
mod execution_units;
mod mutations;
mod provider_hints;
mod queries;
mod scheduler;
mod schema;
mod subscriptions;
mod tenants;
mod transactions;
mod usage;

use std::collections::HashMap;
use std::future::Future;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::RwLock;
use std::sync::atomic::AtomicBool;

use neovex_core::{Error, Result, TenantId, Timestamp};
use neovex_storage::{
    Clock, EmbeddedProviderKind, FaultInjector, NoopFaultInjector, SqliteTenantStore, SystemClock,
    TenantStore,
};
use tokio::sync::{Mutex as AsyncMutex, Notify};
use tokio::task::JoinHandle;

use crate::persistence::{ControlPlaneProvider, PersistenceProvider, TenantPersistence};
use crate::persistence_config::ServicePersistenceConfig;
use crate::tenant::TenantRuntime;
use crate::triggers::{TriggerRegistration, execution::SharedTriggerInvocationExecutor};
use background_executor::BackgroundExecutor;
use transactions::TransactionSessionRegistry;

pub use encryption::{EncryptionStatus, InitializedKeyProvider};
pub use execution_units::MutationExecutionUnit;
pub(crate) use queries::{
    evaluate_with_index_cancellable_for_principal, paginate_documents_for_store_with_principal,
    query_documents_for_store_with_principal,
};
#[cfg(test)]
pub(crate) use queries::{
    paginate_documents_for_docs_with_principal, query_documents_for_docs_with_principal,
};
pub use subscriptions::SubscriptionBootstrapCancellation;

/// Top-level Neovex engine service.
pub struct Service {
    data_dir: PathBuf,
    tenants: RwLock<HashMap<TenantId, Arc<TenantRuntime>>>,
    transaction_sessions: RwLock<TransactionSessionRegistry>,
    tenant_load_gate: AsyncMutex<()>,
    embedded_provider_kind: Option<EmbeddedProviderKind>,
    persistence_provider: PersistenceProvider,
    control_plane_provider: ControlPlaneProvider,
    clock: Arc<dyn Clock>,
    storage_fault_injector: Arc<dyn FaultInjector>,
    scheduler_wakeup: Notify,
    provider_hint_worker_started: AtomicBool,
    provider_hint_listener_ready: AtomicBool,
    trigger_invocation_executor: RwLock<Option<SharedTriggerInvocationExecutor>>,
    trigger_registrations: RwLock<Vec<TriggerRegistration>>,
    engine_executor: BackgroundExecutor,
    storage_executor: BackgroundExecutor,
    encryption_status: Option<encryption::EncryptionStatus>,
}

pub(super) struct ServiceBootstrapParts {
    data_dir: PathBuf,
    embedded_provider_kind: Option<EmbeddedProviderKind>,
    persistence_provider: PersistenceProvider,
    control_plane_provider: ControlPlaneProvider,
    clock: Arc<dyn Clock>,
    storage_fault_injector: Arc<dyn FaultInjector>,
    engine_executor: BackgroundExecutor,
    storage_executor: BackgroundExecutor,
    encryption_status: Option<encryption::EncryptionStatus>,
}

tokio::task_local! {
    static SERVICE_BACKGROUND_TASK: &'static str;
}

impl Service {
    /// Creates a new service for the provided data directory.
    pub fn new(data_dir: impl Into<PathBuf>) -> Result<Self> {
        Self::new_with_embedded_provider(data_dir, EmbeddedProviderKind::default())
    }

    /// Creates a new service for the provided data directory using an explicit
    /// embedded persistence provider.
    pub fn new_with_embedded_provider(
        data_dir: impl Into<PathBuf>,
        embedded_provider_kind: EmbeddedProviderKind,
    ) -> Result<Self> {
        Self::new_with_simulation_and_embedded_provider(
            data_dir,
            Arc::new(SystemClock),
            Arc::new(NoopFaultInjector),
            embedded_provider_kind,
        )
    }

    /// Creates a new service from typed persistence configuration.
    pub async fn new_with_persistence_config(config: ServicePersistenceConfig) -> Result<Self> {
        Self::new_with_simulation_and_persistence_config(
            config,
            Arc::new(SystemClock),
            Arc::new(NoopFaultInjector),
        )
        .await
    }

    /// Creates a new service with deterministic simulation seams for time and storage faults.
    pub fn new_with_simulation(
        data_dir: impl Into<PathBuf>,
        clock: Arc<dyn Clock>,
        storage_fault_injector: Arc<dyn FaultInjector>,
    ) -> Result<Self> {
        Self::new_with_simulation_and_embedded_provider(
            data_dir,
            clock,
            storage_fault_injector,
            EmbeddedProviderKind::default(),
        )
    }

    /// Creates a new service with deterministic simulation seams and an
    /// explicit embedded persistence provider.
    ///
    /// Note: This API does not support encryption. Use
    /// `new_with_simulation_and_persistence_config` with a `LocalEncryptionConfig`
    /// to enable encrypted embedded providers.
    pub fn new_with_simulation_and_embedded_provider(
        data_dir: impl Into<PathBuf>,
        clock: Arc<dyn Clock>,
        storage_fault_injector: Arc<dyn FaultInjector>,
        embedded_provider_kind: EmbeddedProviderKind,
    ) -> Result<Self> {
        let data_dir = data_dir.into();
        bootstrap::build_embedded_service(
            data_dir.clone(),
            data_dir,
            None,
            clock,
            storage_fault_injector,
            embedded_provider_kind,
        )
    }

    /// Creates a new service with deterministic simulation seams and typed
    /// persistence configuration.
    pub async fn new_with_simulation_and_persistence_config(
        config: ServicePersistenceConfig,
        clock: Arc<dyn Clock>,
        storage_fault_injector: Arc<dyn FaultInjector>,
    ) -> Result<Self> {
        bootstrap::build_from_persistence_config(config, clock, storage_fault_injector).await
    }

    fn from_bootstrap_parts(parts: ServiceBootstrapParts) -> Self {
        Self {
            data_dir: parts.data_dir,
            tenants: RwLock::new(HashMap::new()),
            transaction_sessions: RwLock::new(TransactionSessionRegistry::default()),
            tenant_load_gate: AsyncMutex::new(()),
            embedded_provider_kind: parts.embedded_provider_kind,
            persistence_provider: parts.persistence_provider,
            control_plane_provider: parts.control_plane_provider,
            clock: parts.clock,
            storage_fault_injector: parts.storage_fault_injector,
            scheduler_wakeup: Notify::new(),
            provider_hint_worker_started: AtomicBool::new(false),
            provider_hint_listener_ready: AtomicBool::new(false),
            trigger_invocation_executor: RwLock::new(None),
            trigger_registrations: RwLock::new(Vec::new()),
            engine_executor: parts.engine_executor,
            storage_executor: parts.storage_executor,
            encryption_status: parts.encryption_status,
        }
    }

    /// Returns the service's encryption status, if configured.
    ///
    /// Returns `Some` when the service was created via `new_with_persistence_config`
    /// or `new_with_simulation_and_persistence_config`. Returns `None` for services
    /// created via direct embedded provider constructors.
    pub fn encryption_status(&self) -> Option<&encryption::EncryptionStatus> {
        self.encryption_status.as_ref()
    }

    pub(crate) fn wake_scheduler(&self) {
        self.scheduler_wakeup.notify_one();
    }

    pub(crate) fn scheduler_notifier(&self) -> &Notify {
        &self.scheduler_wakeup
    }

    pub(crate) fn provider_background_ready(&self) -> bool {
        self.provider_hint_listener_ready
            .load(std::sync::atomic::Ordering::Acquire)
    }

    pub(crate) fn spawn_background<F>(&self, name: &'static str, future: F) -> JoinHandle<()>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.engine_executor
            .spawn(SERVICE_BACKGROUND_TASK.scope(name, future))
            .expect("engine executor should accept background work before quiesce")
    }

    pub async fn quiesce(&self) {
        self.engine_executor.quiesce().await;
        self.storage_executor.quiesce().await;
    }

    #[cfg(any(test, debug_assertions))]
    pub(crate) fn assert_running_on_background_task(expected: &'static str) {
        let actual = SERVICE_BACKGROUND_TASK.try_with(|name| *name).ok();
        assert_eq!(
            actual,
            Some(expected),
            "long-lived engine worker must run on the Service-owned background runtime"
        );
    }

    pub(crate) fn now(&self) -> Timestamp {
        self.clock.now()
    }

    pub(crate) fn open_tenant_store(&self, path: &Path) -> Result<TenantPersistence> {
        match self.require_embedded_provider_kind()? {
            EmbeddedProviderKind::Redb => TenantStore::open_with_simulation(
                path,
                self.clock.clone(),
                self.storage_fault_injector.clone(),
            )
            .map(|store| TenantPersistence::Redb(Arc::new(store))),
            EmbeddedProviderKind::Sqlite => SqliteTenantStore::open_with_simulation(
                path,
                self.clock.clone(),
                self.storage_fault_injector.clone(),
            )
            .map(|store| TenantPersistence::Sqlite(Arc::new(store))),
        }
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
        tenant_id: &TenantId,
        store: TenantPersistence,
    ) -> Result<Arc<TenantRuntime>> {
        let read_storage = self
            .persistence_provider
            .read_storage_for_store(store.clone())?;
        let runtime = Arc::new(TenantRuntime::from_parts(
            tenant_id.clone(),
            store.clone(),
            read_storage,
        )?);
        runtime.replace_trigger_registrations(
            self.trigger_registrations
                .read()
                .expect("trigger registrations lock should not be poisoned")
                .clone(),
        )?;
        let progress = store.recover_durable_journal()?;
        runtime.sync_mutation_journal_progress(progress);
        self.bootstrap_trigger_candidate_feed(runtime.clone())?;
        self.bootstrap_trigger_execution(runtime.clone())?;
        Ok(runtime)
    }

    pub(crate) fn trigger_invocation_executor(&self) -> Option<SharedTriggerInvocationExecutor> {
        self.trigger_invocation_executor
            .read()
            .expect("trigger invocation executor lock should not be poisoned")
            .clone()
    }

    pub fn install_trigger_invocation_executor(
        self: &Arc<Self>,
        executor: Arc<dyn crate::triggers::TriggerInvocationExecutor>,
    ) -> Result<()> {
        {
            let mut slot = self
                .trigger_invocation_executor
                .write()
                .expect("trigger invocation executor lock should not be poisoned");
            *slot = Some(executor);
        }
        let runtimes = self
            .tenants
            .read()
            .expect("tenant registry lock should not be poisoned")
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for runtime in runtimes {
            self.bootstrap_trigger_execution(runtime)?;
        }
        Ok(())
    }

    pub fn install_trigger_registrations(
        self: &Arc<Self>,
        registrations: Vec<TriggerRegistration>,
    ) -> Result<()> {
        {
            let mut slot = self
                .trigger_registrations
                .write()
                .expect("trigger registrations lock should not be poisoned");
            *slot = registrations.clone();
        }
        let runtimes = self
            .tenants
            .read()
            .expect("tenant registry lock should not be poisoned")
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for runtime in runtimes {
            runtime.replace_trigger_registrations(registrations.clone())?;
        }
        Ok(())
    }

    pub(crate) fn require_embedded_provider_kind(&self) -> Result<EmbeddedProviderKind> {
        self.embedded_provider_kind.ok_or_else(|| {
            Error::InvalidInput(
                "embedded-only blocking tenant lifecycle helpers are unavailable for non-embedded persistence providers; use the async service surfaces".to_string(),
            )
        })
    }
}
