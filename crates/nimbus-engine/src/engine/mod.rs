mod background_executor;
mod bootstrap;
pub(crate) mod committed_mutations;
mod diagnostics;
mod encryption;
mod execution_units;
mod kv;
mod latency;
mod mutations;
pub(crate) use mutations::durable_outcome::{
    DurableWriteOutcome, DurableWriteRoute, classify_durable_write_error,
};
pub(crate) use mutations::prepared::PreparedCommit;
pub(crate) use mutations::write_log::{WriteLog, WriteLogConfig};
mod object_placement;
mod objects;
mod occ_retry;
mod provider_hints;
mod queries;
pub use queries::DocumentReadFilter;
mod scheduler;
mod schema;
mod subscriptions;
mod tenant_load_gate;
mod tenants;
pub use tenants::TenantDeletionLease;
mod transactions;
mod usage;

use std::collections::HashMap;
use std::future::Future;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::RwLock;
use std::sync::Weak;
use std::sync::atomic::AtomicBool;

use nimbus_core::{
    DocumentId, Error, IdSource, MonotonicClock, Result, SystemIdSource, SystemMonotonicClock,
    SystemWallClock, TenantId, Timestamp, WallClock,
};
use nimbus_storage::{
    EmbeddedProviderKind, FaultInjector, NoopFaultInjector, SqliteTenantStore, TenantStore,
};
use tokio::sync::Notify;
use tokio::task::JoinHandle;

use crate::persistence::{ControlPlaneProvider, PersistenceProvider, TenantPersistence};
use crate::persistence_config::EnginePersistenceConfig;
use crate::tenant::{
    LeaseRenewalClock, PublisherErrorCounts, SystemLeaseRenewalClock, TenantRuntime,
    TenantRuntimeEnvironment,
};
use crate::triggers::{TriggerRegistration, execution::SharedTriggerInvocationExecutor};
use background_executor::BackgroundExecutor;
use tenant_load_gate::{TenantLoadGate, TenantLoadGateGuard};
use transactions::TransactionSessionRegistry;

pub use committed_mutations::{
    CommittedMutationEvent, CommittedMutationObserver, CommittedMutationObserverWorkStats,
    ProjectionReconciliationSnapshot, ProjectionToken, TenantRuntimeLoadedEvent,
    TenantRuntimeObserver, TenantRuntimeObserverIdentity,
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
pub(crate) use mutations::{begin_definitive_fence_eviction, begin_durable_recovery_eviction};
pub use objects::TenantObjectMeta;
#[cfg(any(feature = "libsql", feature = "mysql"))]
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
pub use tenants::TenantAdmissionOutcome;

/// Top-level Nimbus engine coordinator.
pub struct Engine {
    data_dir: PathBuf,
    tenants: Arc<RwLock<HashMap<TenantId, Arc<TenantRuntime>>>>,
    publisher_failure_diagnostics: Arc<RwLock<HashMap<TenantId, PublisherErrorCounts>>>,
    transaction_sessions: RwLock<TransactionSessionRegistry>,
    tenant_load_gate: TenantLoadGate,
    embedded_provider_kind: Option<EmbeddedProviderKind>,
    persistence_provider: PersistenceProvider,
    control_plane_provider: ControlPlaneProvider,
    clock: Arc<dyn WallClock>,
    monotonic_clock: Arc<dyn MonotonicClock>,
    committer_lease_clock: Arc<dyn LeaseRenewalClock>,
    id_source: Arc<dyn IdSource>,
    // One provider-lease identity per Engine lets an evicted tenant runtime
    // resume its own still-live lease without weakening cross-engine fencing.
    // It stays lazy so embedded engines do not consume deterministic IDs.
    committer_owner_id: OnceLock<String>,
    commit_faults: execution_units::CommitFaultClient,
    storage_fault_injector: Arc<dyn FaultInjector>,
    scheduler_wakeup: Notify,
    #[cfg(any(feature = "libsql", feature = "mysql", feature = "postgres"))]
    provider_hint_worker_started: AtomicBool,
    provider_hint_listener_ready: AtomicBool,
    trigger_invocation_executor: RwLock<Option<SharedTriggerInvocationExecutor>>,
    trigger_registrations: RwLock<Vec<TriggerRegistration>>,
    committed_mutation_observers: RwLock<committed_mutations::CommittedMutationObserverRegistry>,
    table_schema_change_observers: RwLock<committed_mutations::TableSchemaChangeObserverRegistry>,
    tenant_runtime_observers: RwLock<committed_mutations::TenantRuntimeObserverRegistry>,
    engine_executor: BackgroundExecutor,
    storage_executor: BackgroundExecutor,
    encryption_status: Option<encryption::EncryptionStatus>,
}

#[derive(Clone)]
pub(crate) struct TenantEvictionRegistry {
    tenants: Weak<RwLock<HashMap<TenantId, Arc<TenantRuntime>>>>,
    publisher_failure_diagnostics: Weak<RwLock<HashMap<TenantId, PublisherErrorCounts>>>,
}

impl TenantEvictionRegistry {
    pub(crate) async fn finish(&self, runtime: Arc<TenantRuntime>) {
        let tenant_id = runtime.tenant_id().clone();
        let completion = runtime.eviction_completion();
        // The committer actor reaches this point only after publisher,
        // observer, and operation ownership has drained. Retire transport
        // state before unregistering the runtime so a replacement cannot race
        // stale provider sessions from the evicted generation. Engine
        // quiescence handles only runtimes still present in the registry and
        // therefore cannot own this crash-and-replay path.
        if let Err(error) = runtime.store.retire_after_drain().await {
            tracing::warn!(
                tenant_id = %tenant_id,
                error = %error,
                "failed to retire tenant persistence during runtime eviction"
            );
        }
        if let Some(diagnostics) = self.publisher_failure_diagnostics.upgrade() {
            diagnostics
                .write()
                .expect("publisher failure diagnostics lock should not be poisoned")
                .insert(tenant_id.clone(), runtime.publisher_error_counts());
        }
        if let Some(tenants) = self.tenants.upgrade() {
            let mut tenants = tenants
                .write()
                .expect("tenant registry lock should not be poisoned");
            let removed = if tenants
                .get(&tenant_id)
                .is_some_and(|loaded| Arc::ptr_eq(loaded, &runtime))
            {
                tenants.remove(&tenant_id)
            } else {
                None
            };
            // Keep the registry write-locked until both Engine ownership and
            // the actor's final runtime owner are gone. A loader cannot observe
            // an empty slot and reopen redb while the failed handle is live.
            drop(removed);
            drop(runtime);
            drop(tenants);
        } else {
            // The Engine already dropped its registry. There is nothing left
            // to deregister, but wake any accessor still holding this runtime.
            tracing::debug!(tenant = %tenant_id, "engine registry dropped before eviction finished");
            drop(runtime);
        }
        // This token owns lifecycle state only, never persistence. Waking it
        // therefore proves transport retirement was attempted and the actor
        // and registry no longer own the old store.
        completion.finish();
    }
}

pub(super) struct EngineBootstrapParts {
    data_dir: PathBuf,
    embedded_provider_kind: Option<EmbeddedProviderKind>,
    persistence_provider: PersistenceProvider,
    control_plane_provider: ControlPlaneProvider,
    clock: Arc<dyn WallClock>,
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
            Arc::new(SystemWallClock),
            Arc::new(NoopFaultInjector),
            embedded_provider_kind,
        )
    }

    /// Creates a new engine from typed persistence configuration.
    pub async fn new_with_persistence_config(config: EnginePersistenceConfig) -> Result<Self> {
        Self::new_with_simulation_and_persistence_config(
            config,
            Arc::new(SystemWallClock),
            Arc::new(NoopFaultInjector),
        )
        .await
    }

    /// Creates a new engine with deterministic simulation seams for time and storage faults.
    pub fn new_with_simulation(
        data_dir: impl Into<PathBuf>,
        clock: Arc<dyn WallClock>,
        storage_fault_injector: Arc<dyn FaultInjector>,
    ) -> Result<Self> {
        Self::new_with_simulation_and_embedded_provider(
            data_dir,
            clock,
            storage_fault_injector,
            EmbeddedProviderKind::default(),
        )
    }

    /// Creates an engine with independently controlled wall and monotonic clocks.
    pub fn new_with_simulation_clocks(
        data_dir: impl Into<PathBuf>,
        clock: Arc<dyn WallClock>,
        monotonic_clock: Arc<dyn MonotonicClock>,
        storage_fault_injector: Arc<dyn FaultInjector>,
    ) -> Result<Self> {
        let mut engine = Self::new_with_simulation(data_dir, clock, storage_fault_injector)?;
        engine.monotonic_clock = monotonic_clock;
        Ok(engine)
    }

    /// Creates a new engine with deterministic simulation seams and an
    /// injected source for generated document and job identifiers.
    pub fn new_with_simulation_and_id_source(
        data_dir: impl Into<PathBuf>,
        clock: Arc<dyn WallClock>,
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
            Arc::new(SystemWallClock),
            Arc::new(NoopFaultInjector),
            Arc::new(SystemIdSource),
        )
    }

    /// Creates a test-only memory-persistence engine with deterministic seams.
    #[cfg(any(test, feature = "test-hooks"))]
    pub fn new_with_simulation_and_memory_persistence(
        data_dir: impl Into<PathBuf>,
        clock: Arc<dyn WallClock>,
        storage_fault_injector: Arc<dyn FaultInjector>,
        id_source: Arc<dyn IdSource>,
    ) -> Result<Self> {
        bootstrap::build_memory_engine(data_dir.into(), clock, storage_fault_injector, id_source)
    }

    /// Creates a memory-persistence engine with independent deterministic clocks.
    #[cfg(any(test, feature = "test-hooks"))]
    pub fn new_with_simulation_clocks_and_memory_persistence(
        data_dir: impl Into<PathBuf>,
        clock: Arc<dyn WallClock>,
        monotonic_clock: Arc<dyn MonotonicClock>,
        storage_fault_injector: Arc<dyn FaultInjector>,
        id_source: Arc<dyn IdSource>,
    ) -> Result<Self> {
        let mut engine = Self::new_with_simulation_and_memory_persistence(
            data_dir,
            clock,
            storage_fault_injector,
            id_source,
        )?;
        engine.monotonic_clock = monotonic_clock;
        Ok(engine)
    }

    /// Creates a new engine with deterministic simulation seams and an
    /// explicit embedded persistence provider.
    ///
    /// Note: This API does not support encryption. Use
    /// `new_with_simulation_and_persistence_config` with a `LocalEncryptionConfig`
    /// to enable encrypted embedded providers.
    pub fn new_with_simulation_and_embedded_provider(
        data_dir: impl Into<PathBuf>,
        clock: Arc<dyn WallClock>,
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

    /// Creates a test engine for an embedded adapter with every ambient source
    /// used by the PPSC scenario contract supplied explicitly.
    #[cfg(any(test, feature = "test-hooks"))]
    #[doc(hidden)]
    pub fn new_with_simulation_clocks_id_source_and_embedded_provider(
        data_dir: impl Into<PathBuf>,
        clock: Arc<dyn WallClock>,
        monotonic_clock: Arc<dyn MonotonicClock>,
        storage_fault_injector: Arc<dyn FaultInjector>,
        id_source: Arc<dyn IdSource>,
        embedded_provider_kind: EmbeddedProviderKind,
    ) -> Result<Self> {
        let data_dir = data_dir.into();
        let mut engine = bootstrap::build_embedded_engine(
            data_dir.clone(),
            data_dir,
            None,
            clock,
            storage_fault_injector,
            id_source,
            embedded_provider_kind,
        )?;
        engine.monotonic_clock = monotonic_clock;
        Ok(engine)
    }

    /// Creates a new engine with deterministic simulation seams and typed
    /// persistence configuration.
    pub async fn new_with_simulation_and_persistence_config(
        config: EnginePersistenceConfig,
        clock: Arc<dyn WallClock>,
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

    /// Creates a configured engine with independently controlled wall and monotonic clocks.
    pub async fn new_with_simulation_clocks_and_persistence_config(
        config: EnginePersistenceConfig,
        clock: Arc<dyn WallClock>,
        monotonic_clock: Arc<dyn MonotonicClock>,
        storage_fault_injector: Arc<dyn FaultInjector>,
    ) -> Result<Self> {
        let mut engine =
            Self::new_with_simulation_and_persistence_config(config, clock, storage_fault_injector)
                .await?;
        engine.monotonic_clock = monotonic_clock;
        Ok(engine)
    }

    /// Creates a test engine for a configured persistence adapter with every
    /// ambient source used by the PPSC scenario contract supplied explicitly.
    #[cfg(any(test, feature = "test-hooks"))]
    #[doc(hidden)]
    pub async fn new_with_simulation_clocks_id_source_and_persistence_config(
        config: EnginePersistenceConfig,
        clock: Arc<dyn WallClock>,
        monotonic_clock: Arc<dyn MonotonicClock>,
        storage_fault_injector: Arc<dyn FaultInjector>,
        id_source: Arc<dyn IdSource>,
    ) -> Result<Self> {
        let mut engine = bootstrap::build_from_persistence_config(
            config,
            clock,
            storage_fault_injector,
            id_source,
        )
        .await?;
        engine.monotonic_clock = monotonic_clock;
        Ok(engine)
    }

    /// Creates a test provider engine whose libSQL primary and replica-cache
    /// fault adapters can be controlled independently.
    #[cfg(any(test, feature = "test-hooks"))]
    #[doc(hidden)]
    pub async fn new_with_simulation_and_persistence_config_and_libsql_faults(
        config: EnginePersistenceConfig,
        clock: Arc<dyn WallClock>,
        remote_fault_injector: Arc<dyn FaultInjector>,
        replica_fault_injector: Arc<dyn FaultInjector>,
    ) -> Result<Self> {
        bootstrap::build_from_persistence_config_with_libsql_replica_faults(
            config,
            clock,
            remote_fault_injector,
            Some(replica_fault_injector),
            Arc::new(SystemIdSource),
        )
        .await
    }

    /// Creates a provider engine with independent deterministic wall and
    /// monotonic clocks for lease-lifecycle tests.
    #[cfg(all(test, any(feature = "libsql", feature = "postgres")))]
    pub(crate) async fn new_with_simulation_and_persistence_config_and_lease_clock(
        config: EnginePersistenceConfig,
        clock: Arc<dyn WallClock>,
        storage_fault_injector: Arc<dyn FaultInjector>,
        committer_lease_clock: Arc<dyn LeaseRenewalClock>,
    ) -> Result<Self> {
        let mut engine =
            Self::new_with_simulation_and_persistence_config(config, clock, storage_fault_injector)
                .await?;
        engine.committer_lease_clock = committer_lease_clock;
        Ok(engine)
    }

    fn from_bootstrap_parts(parts: EngineBootstrapParts) -> Self {
        Self {
            data_dir: parts.data_dir,
            tenants: Arc::new(RwLock::new(HashMap::new())),
            publisher_failure_diagnostics: Arc::new(RwLock::new(HashMap::new())),
            transaction_sessions: RwLock::new(TransactionSessionRegistry::default()),
            tenant_load_gate: TenantLoadGate::new(),
            embedded_provider_kind: parts.embedded_provider_kind,
            persistence_provider: parts.persistence_provider,
            control_plane_provider: parts.control_plane_provider,
            clock: parts.clock,
            monotonic_clock: Arc::new(SystemMonotonicClock),
            committer_lease_clock: Arc::new(SystemLeaseRenewalClock),
            id_source: parts.id_source,
            committer_owner_id: OnceLock::new(),
            commit_faults: execution_units::CommitFaultClient::default(),
            storage_fault_injector: parts.storage_fault_injector,
            scheduler_wakeup: Notify::new(),
            #[cfg(any(feature = "libsql", feature = "mysql", feature = "postgres"))]
            provider_hint_worker_started: AtomicBool::new(false),
            provider_hint_listener_ready: AtomicBool::new(false),
            trigger_invocation_executor: RwLock::new(None),
            trigger_registrations: RwLock::new(Vec::new()),
            committed_mutation_observers: RwLock::new(HashMap::new()),
            table_schema_change_observers: RwLock::new(HashMap::new()),
            tenant_runtime_observers: RwLock::new(HashMap::new()),
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

    pub(crate) fn try_spawn_background<F>(
        &self,
        name: &'static str,
        future: F,
    ) -> std::result::Result<JoinHandle<()>, (Error, F)>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.engine_executor
            .spawn_mapped(future, |future| ENGINE_BACKGROUND_TASK.scope(name, future))
    }

    pub(crate) fn start_committer_actor(&self, runtime: Arc<TenantRuntime>) -> Result<()> {
        let spawn_permit = self.engine_executor.acquire_spawn_permit()?;
        let receiver = runtime.take_committer_receiver();
        if runtime.uses_ordered_publisher() {
            let publisher_receiver = runtime.take_publisher_receiver();
            let engine_shutdown = self.engine_executor.shutdown_token();
            let tenant_shutdown = runtime.committer_shutdown_token();
            let publisher_runtime = Arc::downgrade(&runtime);
            spawn_permit.spawn(
                ENGINE_BACKGROUND_TASK.scope("mutation_publisher", async move {
                    crate::engine::mutations::run_ordered_publisher(
                        publisher_runtime,
                        publisher_receiver,
                        engine_shutdown,
                        tenant_shutdown,
                    )
                    .await;
                }),
            );
        }

        let observer_receiver = runtime.take_observer_dispatch_receiver();
        let observer_runtime = Arc::downgrade(&runtime);
        spawn_permit.spawn(ENGINE_BACKGROUND_TASK.scope(
            "committed_mutation_observers",
            async move {
                committed_mutations::run_committed_mutation_observer_dispatcher(
                    observer_receiver,
                    observer_runtime,
                )
                .await;
            },
        ));

        let engine_shutdown = self.engine_executor.shutdown_token();
        let tenant_shutdown = runtime.committer_shutdown_token();
        let closes_observer_dispatch = !runtime.uses_ordered_publisher();
        let eviction_registry = TenantEvictionRegistry {
            tenants: Arc::downgrade(&self.tenants),
            publisher_failure_diagnostics: Arc::downgrade(&self.publisher_failure_diagnostics),
        };
        let runtime = Arc::downgrade(&runtime);
        spawn_permit.spawn(
            ENGINE_BACKGROUND_TASK.scope("mutation_committer", async move {
                crate::tenant::run_committer_actor(
                    runtime,
                    receiver,
                    engine_shutdown,
                    tenant_shutdown,
                    closes_observer_dispatch,
                    eviction_registry,
                )
                .await;
            }),
        );
        Ok(())
    }

    pub async fn quiesce(&self) {
        let runtimes = self
            .tenants
            .read()
            .expect("tenant registry lock should not be poisoned")
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for runtime in &runtimes {
            runtime.shutdown_committer_lease_renewal();
        }
        self.engine_executor.quiesce().await;
        self.storage_executor.quiesce().await;
        for runtime in runtimes {
            if let Err(error) = runtime.store.retire_after_drain().await {
                tracing::warn!(
                    tenant_id = %runtime.tenant_id(),
                    error = %error,
                    "failed to retire tenant persistence during engine quiesce"
                );
            }
        }
        if let Err(error) = self.persistence_provider.retire_after_drain().await {
            tracing::warn!(
                error = %error,
                "failed to retire provider persistence during engine quiesce"
            );
        }
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

    pub(crate) fn monotonic_now(&self) -> std::time::Instant {
        self.monotonic_clock.now()
    }

    pub(crate) fn next_document_id(&self) -> DocumentId {
        self.id_source.next_document_id()
    }

    fn committer_owner_id_for_store(&self, store: &TenantPersistence) -> Option<String> {
        store.requires_committer_lease().then(|| {
            self.committer_owner_id
                .get_or_init(|| format!("nimbus-{}", self.id_source.next_committer_owner_id()))
                .clone()
        })
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
        let tenant_incarnation = self.control_plane_provider.tenant_incarnation(tenant_id)?;
        let read_storage = self
            .persistence_provider
            .read_storage_for_store(store.clone())?;
        let runtime = Arc::new(TenantRuntime::from_parts(
            tenant_id.clone(),
            tenant_incarnation,
            store.clone(),
            read_storage,
            TenantRuntimeEnvironment::new(
                self.monotonic_clock.clone(),
                self.committer_lease_clock.clone(),
                self.committer_owner_id_for_store(&store),
                self.id_source.clone(),
            ),
        )?);
        self.restore_publisher_error_counts(&runtime);
        self.start_committer_actor(runtime.clone())?;
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
