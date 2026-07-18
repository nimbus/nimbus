mod background_executor;
mod bootstrap;
pub(crate) mod committed_mutations;
mod diagnostics;
mod encryption;
mod execution_units;
mod kv;
mod latency;
mod mutations;
pub(crate) use mutations::prepared::PreparedCommit;
pub(crate) use mutations::write_log::{WriteLog, WriteLogConfig};
mod object_placement;
mod objects;
mod occ_retry;
mod provider_hints;
mod queries;
mod scheduler;
mod schema;
mod subscriptions;
mod tenant_load_gate;
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

use nimbus_core::{DocumentId, Error, IdSource, Result, SystemIdSource, TenantId, Timestamp};
use nimbus_storage::{
    Clock, EmbeddedProviderKind, FaultInjector, NoopFaultInjector, SqliteTenantStore, SystemClock,
    TenantStore,
};
use tokio::sync::Notify;
use tokio::task::JoinHandle;

use crate::persistence::{ControlPlaneProvider, PersistenceProvider, TenantPersistence};
use crate::persistence_config::EnginePersistenceConfig;
use crate::tenant::{PublisherErrorCounts, TenantRuntime};
use crate::triggers::{TriggerRegistration, execution::SharedTriggerInvocationExecutor};
use background_executor::BackgroundExecutor;
use tenant_load_gate::{TenantLoadGate, TenantLoadGateGuard};
use transactions::TransactionSessionRegistry;

pub use committed_mutations::{
    CommittedMutationEvent, CommittedMutationObserver, CommittedMutationObserverWorkStats,
    TenantRuntimeObserverIdentity,
};
pub use committed_mutations::{TableSchemaChangeEvent, TableSchemaChangeObserver};
pub use encryption::{EncryptionStatus, InitializedKeyProvider};
pub use execution_units::MutationExecutionUnit;
#[cfg(any(test, feature = "test-hooks"))]
pub use execution_units::{CommitFaultHandle, Fault, labels as commit_fault_labels};
pub use mutations::phase_metrics::CommitPhaseMetricsSnapshot;
pub(crate) use mutations::phase_metrics::{
    CommitPhaseDurations, CommitPhaseMetrics, CommitTraceSample, maybe_emit_commit_trace,
};
pub use mutations::{AsyncMutationContext, MutationActor, MutationIsolatePermit};
pub use objects::TenantObjectMeta;
pub(crate) use provider_hints::ProviderPollWorker;
pub(crate) use queries::{
    evaluate_with_index_cancellable_for_principal, paginate_documents_for_store_with_principal,
    query_documents_for_store_with_principal,
};
#[cfg(test)]
pub(crate) use queries::{
    paginate_documents_for_docs_with_principal, query_documents_for_docs_with_principal,
};
pub use subscriptions::{SubscribeOptions, SubscriptionBootstrapCancellation};

/// Top-level Nimbus engine coordinator.
pub struct Engine {
    data_dir: PathBuf,
    tenants: RwLock<HashMap<TenantId, Arc<TenantRuntime>>>,
    publisher_failure_diagnostics: RwLock<HashMap<TenantId, PublisherErrorCounts>>,
    transaction_sessions: RwLock<TransactionSessionRegistry>,
    tenant_load_gate: TenantLoadGate,
    embedded_provider_kind: Option<EmbeddedProviderKind>,
    persistence_provider: PersistenceProvider,
    control_plane_provider: ControlPlaneProvider,
    clock: Arc<dyn Clock>,
    id_source: Arc<dyn IdSource>,
    commit_faults: execution_units::CommitFaultClient,
    storage_fault_injector: Arc<dyn FaultInjector>,
    scheduler_wakeup: Notify,
    provider_hint_worker_started: AtomicBool,
    provider_hint_listener_ready: AtomicBool,
    trigger_invocation_executor: RwLock<Option<SharedTriggerInvocationExecutor>>,
    trigger_registrations: RwLock<Vec<TriggerRegistration>>,
    committed_mutation_observers: RwLock<committed_mutations::CommittedMutationObserverRegistry>,
    table_schema_change_observers: RwLock<committed_mutations::TableSchemaChangeObserverRegistry>,
    engine_executor: BackgroundExecutor,
    storage_executor: BackgroundExecutor,
    encryption_status: Option<encryption::EncryptionStatus>,
}

pub(super) struct EngineBootstrapParts {
    data_dir: PathBuf,
    embedded_provider_kind: Option<EmbeddedProviderKind>,
    persistence_provider: PersistenceProvider,
    control_plane_provider: ControlPlaneProvider,
    clock: Arc<dyn Clock>,
    id_source: Arc<dyn IdSource>,
    storage_fault_injector: Arc<dyn FaultInjector>,
    engine_executor: BackgroundExecutor,
    storage_executor: BackgroundExecutor,
    encryption_status: Option<encryption::EncryptionStatus>,
}

tokio::task_local! {
    static ENGINE_BACKGROUND_TASK: &'static str;
}

impl Engine {
    /// Creates a new engine for the provided data directory.
    pub fn new(data_dir: impl Into<PathBuf>) -> Result<Self> {
        Self::new_with_embedded_provider(data_dir, EmbeddedProviderKind::default())
    }

    /// The engine's root data directory. Host-side subsystems (e.g. the deploy
    /// source-package store) root their own content directories under this.
    pub fn data_dir(&self) -> &std::path::Path {
        &self.data_dir
    }

    /// Creates a new engine for the provided data directory using an explicit
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

    /// Creates a new engine from typed persistence configuration.
    pub async fn new_with_persistence_config(config: EnginePersistenceConfig) -> Result<Self> {
        Self::new_with_simulation_and_persistence_config(
            config,
            Arc::new(SystemClock),
            Arc::new(NoopFaultInjector),
        )
        .await
    }

    /// Creates a new engine with deterministic simulation seams for time and storage faults.
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

    /// Creates a new engine with deterministic simulation seams and an
    /// injected source for generated document and job identifiers.
    pub fn new_with_simulation_and_id_source(
        data_dir: impl Into<PathBuf>,
        clock: Arc<dyn Clock>,
        storage_fault_injector: Arc<dyn FaultInjector>,
        id_source: Arc<dyn IdSource>,
    ) -> Result<Self> {
        let data_dir = data_dir.into();
        bootstrap::build_embedded_engine(
            data_dir.clone(),
            data_dir,
            None,
            clock,
            storage_fault_injector,
            id_source,
            EmbeddedProviderKind::default(),
        )
    }

    /// Creates a test-only engine whose tenant persistence is process-local memory.
    #[cfg(any(test, feature = "test-hooks"))]
    pub fn new_with_memory_persistence(data_dir: impl Into<PathBuf>) -> Result<Self> {
        Self::new_with_simulation_and_memory_persistence(
            data_dir,
            Arc::new(SystemClock),
            Arc::new(NoopFaultInjector),
            Arc::new(SystemIdSource),
        )
    }

    /// Creates a test-only memory-persistence engine with deterministic seams.
    #[cfg(any(test, feature = "test-hooks"))]
    pub fn new_with_simulation_and_memory_persistence(
        data_dir: impl Into<PathBuf>,
        clock: Arc<dyn Clock>,
        storage_fault_injector: Arc<dyn FaultInjector>,
        id_source: Arc<dyn IdSource>,
    ) -> Result<Self> {
        bootstrap::build_memory_engine(data_dir.into(), clock, storage_fault_injector, id_source)
    }

    /// Creates a new engine with deterministic simulation seams and an
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
        bootstrap::build_embedded_engine(
            data_dir.clone(),
            data_dir,
            None,
            clock,
            storage_fault_injector,
            Arc::new(SystemIdSource),
            embedded_provider_kind,
        )
    }

    /// Creates a new engine with deterministic simulation seams and typed
    /// persistence configuration.
    pub async fn new_with_simulation_and_persistence_config(
        config: EnginePersistenceConfig,
        clock: Arc<dyn Clock>,
        storage_fault_injector: Arc<dyn FaultInjector>,
    ) -> Result<Self> {
        bootstrap::build_from_persistence_config(
            config,
            clock,
            storage_fault_injector,
            Arc::new(SystemIdSource),
        )
        .await
    }

    fn from_bootstrap_parts(parts: EngineBootstrapParts) -> Self {
        Self {
            data_dir: parts.data_dir,
            tenants: RwLock::new(HashMap::new()),
            publisher_failure_diagnostics: RwLock::new(HashMap::new()),
            transaction_sessions: RwLock::new(TransactionSessionRegistry::default()),
            tenant_load_gate: TenantLoadGate::new(),
            embedded_provider_kind: parts.embedded_provider_kind,
            persistence_provider: parts.persistence_provider,
            control_plane_provider: parts.control_plane_provider,
            clock: parts.clock,
            id_source: parts.id_source,
            commit_faults: execution_units::CommitFaultClient::default(),
            storage_fault_injector: parts.storage_fault_injector,
            scheduler_wakeup: Notify::new(),
            provider_hint_worker_started: AtomicBool::new(false),
            provider_hint_listener_ready: AtomicBool::new(false),
            trigger_invocation_executor: RwLock::new(None),
            trigger_registrations: RwLock::new(Vec::new()),
            committed_mutation_observers: RwLock::new(HashMap::new()),
            table_schema_change_observers: RwLock::new(HashMap::new()),
            engine_executor: parts.engine_executor,
            storage_executor: parts.storage_executor,
            encryption_status: parts.encryption_status,
        }
    }

    /// Returns the engine's encryption status, if configured.
    ///
    /// Returns `Some` when the engine was created via `new_with_persistence_config`
    /// or `new_with_simulation_and_persistence_config`. Returns `None` for engines
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

    pub(crate) fn background_shutdown_started(&self) -> bool {
        self.engine_executor.shutdown_token().is_cancelled()
    }

    pub(crate) fn spawn_background<F>(&self, name: &'static str, future: F) -> JoinHandle<()>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.engine_executor
            .spawn(ENGINE_BACKGROUND_TASK.scope(name, future))
            .expect("engine executor should accept background work before quiesce")
    }

    pub(crate) fn start_committer_actor(&self, runtime: Arc<TenantRuntime>) {
        let receiver = runtime.take_committer_receiver();
        if runtime.publisher_pipeline_capable() {
            let publisher_receiver = runtime.take_publisher_receiver();
            let engine_shutdown = self.engine_executor.shutdown_token();
            let tenant_shutdown = runtime.committer_shutdown_token();
            let publisher_runtime = Arc::downgrade(&runtime);
            self.spawn_background("mutation_publisher", async move {
                crate::engine::mutations::run_ordered_publisher(
                    publisher_runtime,
                    publisher_receiver,
                    engine_shutdown,
                    tenant_shutdown,
                )
                .await;
            });
        }

        let observer_receiver = runtime.take_observer_dispatch_receiver();
        let observer_runtime = Arc::downgrade(&runtime);
        self.spawn_background("committed_mutation_observers", async move {
            committed_mutations::run_committed_mutation_observer_dispatcher(
                observer_receiver,
                observer_runtime,
            )
            .await;
        });

        let engine_shutdown = self.engine_executor.shutdown_token();
        let tenant_shutdown = runtime.committer_shutdown_token();
        let closes_observer_dispatch = !runtime.publisher_pipeline_capable();
        let runtime = Arc::downgrade(&runtime);
        self.spawn_background("mutation_committer", async move {
            crate::tenant::run_committer_actor(
                runtime,
                receiver,
                engine_shutdown,
                tenant_shutdown,
                closes_observer_dispatch,
            )
            .await;
        });
    }

    pub async fn quiesce(&self) {
        self.engine_executor.quiesce().await;
        self.storage_executor.quiesce().await;
    }

    #[cfg(any(test, debug_assertions))]
    pub(crate) fn assert_running_on_background_task(expected: &'static str) {
        let actual = ENGINE_BACKGROUND_TASK.try_with(|name| *name).ok();
        assert_eq!(
            actual,
            Some(expected),
            "long-lived engine worker must run on the Engine-owned background runtime"
        );
    }

    pub(crate) fn now(&self) -> Timestamp {
        self.clock.now()
    }

    pub(crate) fn next_document_id(&self) -> DocumentId {
        self.id_source.next_document_id()
    }

    pub(in crate::engine) fn wait_for_commit_fault(
        &self,
        label: execution_units::Label,
    ) -> Result<()> {
        self.commit_faults.wait(label).into_result()
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

    pub(in crate::engine) fn lock_tenant_load_gate_blocking(&self) -> TenantLoadGateGuard<'_> {
        self.tenant_load_gate.blocking_lock()
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
        self.restore_publisher_error_counts(&runtime);
        self.start_committer_actor(runtime.clone());
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

    pub(crate) fn restore_publisher_error_counts(&self, runtime: &TenantRuntime) {
        if let Some(counts) = self
            .publisher_failure_diagnostics
            .read()
            .expect("publisher failure diagnostics lock should not be poisoned")
            .get(runtime.tenant_id())
            .copied()
        {
            runtime.restore_publisher_error_counts(counts);
        }
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
                "embedded-only blocking tenant lifecycle helpers are unavailable for non-embedded persistence providers; use the async engine surfaces".to_string(),
            )
        })
    }
}
