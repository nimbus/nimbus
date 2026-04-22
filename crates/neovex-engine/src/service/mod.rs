mod background_executor;
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
mod usage;

use std::collections::HashMap;
use std::future::Future;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::RwLock;
use std::sync::atomic::AtomicBool;

use neovex_core::{Document, Error, Result, TenantId, Timestamp};
use neovex_storage::{
    Clock, EmbeddedProviderKind, EmbeddedRedbControlPlaneProvider, EmbeddedRedbProvider,
    EmbeddedSqliteProvider, FaultInjector, LibsqlReplicaProvider, LibsqlReplicaProviderConfig,
    LocalKeyProvider, MySqlProvider, MySqlProviderConfig, NoopFaultInjector, PostgresProvider,
    PostgresProviderConfig, SqliteTenantStore, SystemClock, TenantStore,
};
use tokio::sync::{Mutex as AsyncMutex, Notify};
use tokio::task::JoinHandle;

use crate::persistence::{ControlPlaneProvider, PersistenceProvider, TenantPersistence};
use crate::persistence_config::{
    ControlPlaneConfig, PersistenceDialect, PersistenceTopology, ProviderCredentials,
    ServicePersistenceConfig, TenantRoutingConfig,
};
use crate::tenant::TenantRuntime;
use background_executor::BackgroundExecutor;

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
    tenant_load_gate: AsyncMutex<()>,
    embedded_provider_kind: Option<EmbeddedProviderKind>,
    persistence_provider: PersistenceProvider,
    control_plane_provider: ControlPlaneProvider,
    clock: Arc<dyn Clock>,
    storage_fault_injector: Arc<dyn FaultInjector>,
    scheduler_wakeup: Notify,
    provider_hint_worker_started: AtomicBool,
    provider_hint_listener_ready: AtomicBool,
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
        Self::new_with_simulation_and_embedded_config(
            data_dir.clone(),
            data_dir,
            None, // No encryption for direct embedded provider usage
            None, // No control plane encryption for direct embedded provider usage
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
        // Initialize the configured local key provider if encryption is enabled.
        // This performs fail-fast validation for unsupported configurations.
        let key_provider = encryption::initialize_encryption(&config)?;

        // Capture encryption status from config for later assignment
        let encryption_status = encryption::EncryptionStatus::from_config(&config);

        let mut service = match (
            &config.tenant_provider.dialect,
            &config.tenant_provider.topology,
            &config.tenant_provider.routing,
            &config.control_plane,
        ) {
            (
                PersistenceDialect::Redb,
                PersistenceTopology::EmbeddedStandalone,
                TenantRoutingConfig::DirectoryPerTenant { data_dir },
                ControlPlaneConfig::EmbeddedRedb {
                    data_dir: control_data_dir,
                },
            ) => Self::new_with_simulation_and_embedded_config(
                data_dir.clone(),
                control_data_dir.clone(),
                key_provider.as_ref().map(InitializedKeyProvider::provider),
                key_provider.as_ref().map(InitializedKeyProvider::provider),
                clock,
                storage_fault_injector,
                EmbeddedProviderKind::Redb,
            ),
            (
                PersistenceDialect::Sqlite,
                PersistenceTopology::EmbeddedStandalone,
                TenantRoutingConfig::DirectoryPerTenant { data_dir },
                ControlPlaneConfig::EmbeddedRedb {
                    data_dir: control_data_dir,
                },
            ) => Self::new_with_simulation_and_embedded_config(
                data_dir.clone(),
                control_data_dir.clone(),
                key_provider.as_ref().map(InitializedKeyProvider::provider),
                key_provider.as_ref().map(InitializedKeyProvider::provider),
                clock,
                storage_fault_injector,
                EmbeddedProviderKind::Sqlite,
            ),
            (
                PersistenceDialect::Postgres,
                PersistenceTopology::ExternalPrimary,
                TenantRoutingConfig::SchemaPerTenant {
                    metadata_schema,
                    tenant_schema_prefix,
                },
                ControlPlaneConfig::EmbeddedRedb {
                    data_dir: control_data_dir,
                },
            ) => {
                let ProviderCredentials::ConnectionString(connection_string) =
                    &config.tenant_provider.credentials
                else {
                    return Err(Error::InvalidInput(
                        "Postgres tenant persistence requires a connection string".to_string(),
                    ));
                };
                Self::new_with_simulation_and_postgres_config(
                    control_data_dir.clone(),
                    key_provider.as_ref().map(InitializedKeyProvider::provider),
                    PostgresProviderConfig {
                        connection_string: connection_string.clone(),
                        metadata_schema: metadata_schema.clone(),
                        tenant_schema_prefix: tenant_schema_prefix.clone(),
                        min_connections: config.tenant_provider.pool.min_connections,
                        max_connections: config.tenant_provider.pool.max_connections,
                    },
                    clock,
                    storage_fault_injector,
                )
                .await
            }
            (
                PersistenceDialect::Sqlite,
                PersistenceTopology::ExternalPrimaryWithReplicas,
                TenantRoutingConfig::NamespacePerTenant {
                    metadata_namespace,
                    tenant_namespace_prefix,
                    replica_cache_dir,
                },
                ControlPlaneConfig::EmbeddedRedb {
                    data_dir: control_data_dir,
                },
            ) => {
                let ProviderCredentials::LibsqlReplica {
                    primary_url,
                    auth_token,
                    admin_api_url,
                    admin_auth_header,
                } = &config.tenant_provider.credentials
                else {
                    return Err(Error::InvalidInput(
                        "Replica-connected SQLite tenant persistence requires a primary URL, optional primary auth token, and admin API configuration".to_string(),
                    ));
                };
                Self::new_with_simulation_and_libsql_replica_config(
                    control_data_dir.clone(),
                    key_provider.as_ref().map(InitializedKeyProvider::provider),
                    LibsqlReplicaProviderConfig {
                        primary_url: primary_url.clone(),
                        auth_token: auth_token.clone(),
                        admin_api_url: admin_api_url.clone(),
                        admin_auth_header: admin_auth_header.clone(),
                        metadata_namespace: metadata_namespace.clone(),
                        tenant_namespace_prefix: tenant_namespace_prefix.clone(),
                        replica_cache_dir: replica_cache_dir.clone(),
                        encryption_provider: key_provider
                            .as_ref()
                            .map(InitializedKeyProvider::provider),
                    },
                    clock,
                    storage_fault_injector,
                )
                .await
            }
            (
                PersistenceDialect::MySql,
                PersistenceTopology::ExternalPrimary,
                TenantRoutingConfig::DatabasePerTenant {
                    metadata_database,
                    tenant_database_prefix,
                },
                ControlPlaneConfig::EmbeddedRedb {
                    data_dir: control_data_dir,
                },
            ) => {
                let ProviderCredentials::ConnectionString(connection_string) =
                    &config.tenant_provider.credentials
                else {
                    return Err(Error::InvalidInput(
                        "MySQL tenant persistence requires a connection string".to_string(),
                    ));
                };
                Self::new_with_simulation_and_mysql_config(
                    control_data_dir.clone(),
                    key_provider.as_ref().map(InitializedKeyProvider::provider),
                    MySqlProviderConfig {
                        connection_string: connection_string.clone(),
                        metadata_database: metadata_database.clone(),
                        tenant_database_prefix: tenant_database_prefix.clone(),
                        min_connections: config.tenant_provider.pool.min_connections,
                        max_connections: config.tenant_provider.pool.max_connections,
                    },
                    clock,
                    storage_fault_injector,
                )
                .await
            }
            _ => Err(Error::InvalidInput(
                "unsupported persistence config combination".to_string(),
            )),
        }?;

        // Set the encryption status from the config
        service.encryption_status = Some(encryption_status);
        Ok(service)
    }

    fn new_with_simulation_and_embedded_config(
        tenant_data_dir: PathBuf,
        control_data_dir: PathBuf,
        encryption_provider: Option<Arc<dyn LocalKeyProvider>>,
        control_plane_provider: Option<Arc<dyn LocalKeyProvider>>,
        clock: Arc<dyn Clock>,
        storage_fault_injector: Arc<dyn FaultInjector>,
        embedded_provider_kind: EmbeddedProviderKind,
    ) -> Result<Self> {
        std::fs::create_dir_all(&tenant_data_dir)
            .map_err(|error| Error::Internal(error.to_string()))?;
        if control_data_dir != tenant_data_dir {
            std::fs::create_dir_all(&control_data_dir)
                .map_err(|error| Error::Internal(error.to_string()))?;
        }
        let engine_executor = BackgroundExecutor::new("neovex-engine-bg", 2);
        let storage_executor = BackgroundExecutor::new("neovex-storage-bg", 1);
        let control_plane_provider = ControlPlaneProvider::EmbeddedRedb(Arc::new(
            if let Some(provider) = control_plane_provider {
                EmbeddedRedbControlPlaneProvider::new_encrypted(
                    control_data_dir,
                    provider,
                    storage_executor.handle(),
                )?
            } else {
                EmbeddedRedbControlPlaneProvider::new(control_data_dir, storage_executor.handle())?
            },
        ));
        let persistence_provider = match embedded_provider_kind {
            EmbeddedProviderKind::Redb => {
                let provider = if let Some(provider) = encryption_provider {
                    EmbeddedRedbProvider::new_encrypted(
                        tenant_data_dir.clone(),
                        provider,
                        clock.clone(),
                        storage_fault_injector.clone(),
                        storage_executor.handle(),
                    )?
                } else {
                    EmbeddedRedbProvider::new(
                        tenant_data_dir.clone(),
                        clock.clone(),
                        storage_fault_injector.clone(),
                        storage_executor.handle(),
                    )?
                };
                PersistenceProvider::Redb(Arc::new(provider))
            }
            EmbeddedProviderKind::Sqlite => {
                let provider = if let Some(provider) = encryption_provider {
                    EmbeddedSqliteProvider::new_encrypted(
                        tenant_data_dir.clone(),
                        provider,
                        clock.clone(),
                        storage_fault_injector.clone(),
                        storage_executor.handle(),
                    )?
                } else {
                    EmbeddedSqliteProvider::new(
                        tenant_data_dir.clone(),
                        clock.clone(),
                        storage_fault_injector.clone(),
                        storage_executor.handle(),
                    )?
                };
                PersistenceProvider::Sqlite(Arc::new(provider))
            }
        };
        Ok(Self {
            data_dir: tenant_data_dir,
            tenants: RwLock::new(HashMap::new()),
            tenant_load_gate: AsyncMutex::new(()),
            embedded_provider_kind: Some(embedded_provider_kind),
            persistence_provider,
            control_plane_provider,
            clock,
            storage_fault_injector,
            scheduler_wakeup: Notify::new(),
            provider_hint_worker_started: AtomicBool::new(false),
            provider_hint_listener_ready: AtomicBool::new(false),
            engine_executor,
            storage_executor,
            encryption_status: None, // Set by config-based callers
        })
    }

    async fn new_with_simulation_and_postgres_config(
        control_data_dir: PathBuf,
        control_plane_encryption_provider: Option<Arc<dyn LocalKeyProvider>>,
        provider_config: PostgresProviderConfig,
        clock: Arc<dyn Clock>,
        storage_fault_injector: Arc<dyn FaultInjector>,
    ) -> Result<Self> {
        std::fs::create_dir_all(&control_data_dir)
            .map_err(|error| Error::Internal(error.to_string()))?;
        let engine_executor = BackgroundExecutor::new("neovex-engine-bg", 2);
        let storage_executor = BackgroundExecutor::new("neovex-storage-bg", 1);
        let control_plane_provider = ControlPlaneProvider::EmbeddedRedb(Arc::new(
            if let Some(provider) = control_plane_encryption_provider {
                EmbeddedRedbControlPlaneProvider::new_encrypted(
                    control_data_dir.clone(),
                    provider,
                    storage_executor.handle(),
                )?
            } else {
                EmbeddedRedbControlPlaneProvider::new(
                    control_data_dir.clone(),
                    storage_executor.handle(),
                )?
            },
        ));
        let postgres_provider = Arc::new(
            PostgresProvider::connect_with_simulation(
                provider_config,
                storage_executor.handle(),
                clock.clone(),
                storage_fault_injector.clone(),
            )
            .await?,
        );
        Ok(Self {
            data_dir: control_data_dir,
            tenants: RwLock::new(HashMap::new()),
            tenant_load_gate: AsyncMutex::new(()),
            embedded_provider_kind: None,
            persistence_provider: PersistenceProvider::Postgres(postgres_provider),
            control_plane_provider,
            clock,
            storage_fault_injector,
            scheduler_wakeup: Notify::new(),
            provider_hint_worker_started: AtomicBool::new(false),
            provider_hint_listener_ready: AtomicBool::new(false),
            engine_executor,
            storage_executor,
            encryption_status: None, // Set by config-based callers
        })
    }

    async fn new_with_simulation_and_libsql_replica_config(
        control_data_dir: PathBuf,
        control_plane_encryption_provider: Option<Arc<dyn LocalKeyProvider>>,
        provider_config: LibsqlReplicaProviderConfig,
        clock: Arc<dyn Clock>,
        storage_fault_injector: Arc<dyn FaultInjector>,
    ) -> Result<Self> {
        std::fs::create_dir_all(&control_data_dir)
            .map_err(|error| Error::Internal(error.to_string()))?;
        let engine_executor = BackgroundExecutor::new("neovex-engine-bg", 2);
        let storage_executor = BackgroundExecutor::new("neovex-storage-bg", 1);
        let control_plane_provider = ControlPlaneProvider::EmbeddedRedb(Arc::new(
            if let Some(provider) = control_plane_encryption_provider {
                EmbeddedRedbControlPlaneProvider::new_encrypted(
                    control_data_dir.clone(),
                    provider,
                    storage_executor.handle(),
                )?
            } else {
                EmbeddedRedbControlPlaneProvider::new(
                    control_data_dir.clone(),
                    storage_executor.handle(),
                )?
            },
        ));
        let libsql_replica_provider = Arc::new(
            LibsqlReplicaProvider::connect_with_simulation(
                provider_config,
                storage_executor.handle(),
                clock.clone(),
                storage_fault_injector.clone(),
            )
            .await?,
        );
        Ok(Self {
            data_dir: control_data_dir,
            tenants: RwLock::new(HashMap::new()),
            tenant_load_gate: AsyncMutex::new(()),
            embedded_provider_kind: None,
            persistence_provider: PersistenceProvider::LibsqlReplica(libsql_replica_provider),
            control_plane_provider,
            clock,
            storage_fault_injector,
            scheduler_wakeup: Notify::new(),
            provider_hint_worker_started: AtomicBool::new(false),
            provider_hint_listener_ready: AtomicBool::new(false),
            engine_executor,
            storage_executor,
            encryption_status: None, // Set by config-based callers
        })
    }

    async fn new_with_simulation_and_mysql_config(
        control_data_dir: PathBuf,
        control_plane_encryption_provider: Option<Arc<dyn LocalKeyProvider>>,
        provider_config: MySqlProviderConfig,
        clock: Arc<dyn Clock>,
        storage_fault_injector: Arc<dyn FaultInjector>,
    ) -> Result<Self> {
        std::fs::create_dir_all(&control_data_dir)
            .map_err(|error| Error::Internal(error.to_string()))?;
        let engine_executor = BackgroundExecutor::new("neovex-engine-bg", 2);
        let storage_executor = BackgroundExecutor::new("neovex-storage-bg", 1);
        let control_plane_provider = ControlPlaneProvider::EmbeddedRedb(Arc::new(
            if let Some(provider) = control_plane_encryption_provider {
                EmbeddedRedbControlPlaneProvider::new_encrypted(
                    control_data_dir.clone(),
                    provider,
                    storage_executor.handle(),
                )?
            } else {
                EmbeddedRedbControlPlaneProvider::new(
                    control_data_dir.clone(),
                    storage_executor.handle(),
                )?
            },
        ));
        let mysql_provider = Arc::new(
            MySqlProvider::connect_with_simulation(
                provider_config,
                storage_executor.handle(),
                clock.clone(),
                storage_fault_injector.clone(),
            )
            .await?,
        );
        Ok(Self {
            data_dir: control_data_dir,
            tenants: RwLock::new(HashMap::new()),
            tenant_load_gate: AsyncMutex::new(()),
            embedded_provider_kind: None,
            persistence_provider: PersistenceProvider::MySql(mysql_provider),
            control_plane_provider,
            clock,
            storage_fault_injector,
            scheduler_wakeup: Notify::new(),
            provider_hint_worker_started: AtomicBool::new(false),
            provider_hint_listener_ready: AtomicBool::new(false),
            engine_executor,
            storage_executor,
            encryption_status: None, // Set by config-based callers
        })
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

    pub(crate) fn postgres_provider(&self) -> Option<Arc<PostgresProvider>> {
        match &self.persistence_provider {
            PersistenceProvider::Postgres(provider) => Some(provider.clone()),
            PersistenceProvider::Redb(_)
            | PersistenceProvider::Sqlite(_)
            | PersistenceProvider::LibsqlReplica(_)
            | PersistenceProvider::MySql(_) => None,
        }
    }

    pub(crate) fn libsql_replica_provider(&self) -> Option<Arc<LibsqlReplicaProvider>> {
        match &self.persistence_provider {
            PersistenceProvider::LibsqlReplica(provider) => Some(provider.clone()),
            PersistenceProvider::Redb(_)
            | PersistenceProvider::Sqlite(_)
            | PersistenceProvider::Postgres(_)
            | PersistenceProvider::MySql(_) => None,
        }
    }

    pub(crate) fn mysql_provider(&self) -> Option<Arc<MySqlProvider>> {
        match &self.persistence_provider {
            PersistenceProvider::MySql(provider) => Some(provider.clone()),
            PersistenceProvider::Redb(_)
            | PersistenceProvider::Sqlite(_)
            | PersistenceProvider::LibsqlReplica(_)
            | PersistenceProvider::Postgres(_) => None,
        }
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
        store: TenantPersistence,
    ) -> Result<Arc<TenantRuntime>> {
        let read_storage = self
            .persistence_provider
            .read_storage_for_store(store.clone())?;
        let runtime = Arc::new(TenantRuntime::from_parts(store.clone(), read_storage)?);
        let progress = store.recover_durable_journal()?;
        runtime.sync_mutation_journal_progress(progress);
        Ok(runtime)
    }

    pub(crate) fn require_embedded_provider_kind(&self) -> Result<EmbeddedProviderKind> {
        self.embedded_provider_kind.ok_or_else(|| {
            Error::InvalidInput(
                "embedded-only blocking tenant lifecycle helpers are unavailable for non-embedded persistence providers; use the async service surfaces".to_string(),
            )
        })
    }
}

fn documents_to_json(documents: Vec<Document>) -> Vec<serde_json::Value> {
    documents.into_iter().map(Document::into_json).collect()
}
