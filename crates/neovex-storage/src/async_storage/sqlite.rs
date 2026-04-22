use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use neovex_core::{Error, Result, TenantId};
use tokio::runtime::Handle as TokioRuntimeHandle;
use tokio::sync::Semaphore;

use crate::encryption::{
    KeyManifest, LocalKeyProvider, LocalKeySubject, ManifestCipher, resolve_database_encryption_key,
};
use crate::sqlite::{SqliteTenantStore, SqliteWriteTransaction};
use crate::{Clock, FaultInjector, TenantWriteCommit};

use super::EmbeddedProviderKind;
use super::helpers::{map_join_error, map_permit_error};
use super::read::{BlockingReadExecutor, default_tenant_read_parallelism};
use super::traits::{TenantReadStorage, TenantWriteOutcome, TenantWriteStorage};

const SQLITE_TENANT_WRITE_PARALLELISM: usize = 1;

pub struct OpenedEmbeddedSqliteTenant {
    pub store: Arc<SqliteTenantStore>,
    pub read_storage: Arc<SqliteTenantStorage>,
}

#[derive(Clone)]
pub struct EmbeddedSqliteProvider {
    data_dir: PathBuf,
    encryption_provider: Option<Arc<dyn LocalKeyProvider>>,
    clock: Arc<dyn Clock>,
    fault_injector: Arc<dyn FaultInjector>,
    storage_handle: TokioRuntimeHandle,
    tenant_read_parallelism: usize,
}

#[derive(Clone)]
pub struct SqliteTenantStorage {
    executor: BlockingReadExecutor<SqliteTenantStore>,
    write_executor: SqliteBlockingWriteExecutor,
}

impl EmbeddedSqliteProvider {
    pub fn new(
        data_dir: impl Into<PathBuf>,
        clock: Arc<dyn Clock>,
        fault_injector: Arc<dyn FaultInjector>,
        storage_handle: TokioRuntimeHandle,
    ) -> Result<Self> {
        Self::new_internal(data_dir, None, clock, fault_injector, storage_handle)
    }

    /// Creates a new embedded SQLite provider with encryption enabled.
    ///
    /// When encrypted, tenant databases resolve a per-path DEK from the
    /// configured manifest-backed key provider.
    pub fn new_encrypted(
        data_dir: impl Into<PathBuf>,
        provider: Arc<dyn LocalKeyProvider>,
        clock: Arc<dyn Clock>,
        fault_injector: Arc<dyn FaultInjector>,
        storage_handle: TokioRuntimeHandle,
    ) -> Result<Self> {
        Self::new_internal(
            data_dir,
            Some(provider),
            clock,
            fault_injector,
            storage_handle,
        )
    }

    fn new_internal(
        data_dir: impl Into<PathBuf>,
        encryption_provider: Option<Arc<dyn LocalKeyProvider>>,
        clock: Arc<dyn Clock>,
        fault_injector: Arc<dyn FaultInjector>,
        storage_handle: TokioRuntimeHandle,
    ) -> Result<Self> {
        let data_dir = data_dir.into();
        std::fs::create_dir_all(&data_dir).map_err(|error| Error::Internal(error.to_string()))?;
        Ok(Self {
            data_dir,
            encryption_provider,
            clock,
            fault_injector,
            storage_handle,
            tenant_read_parallelism: default_tenant_read_parallelism(),
        })
    }

    /// Returns whether this provider uses encryption for tenant databases.
    pub fn is_encrypted(&self) -> bool {
        self.encryption_provider.is_some()
    }

    pub fn read_storage_for_store(
        &self,
        store: Arc<SqliteTenantStore>,
    ) -> Arc<SqliteTenantStorage> {
        Arc::new(SqliteTenantStorage::with_max_concurrent_reads(
            store,
            self.storage_handle.clone(),
            self.tenant_read_parallelism,
        ))
    }

    pub async fn create_tenant(&self, tenant_id: &TenantId) -> Result<OpenedEmbeddedSqliteTenant> {
        let path = self.tenant_path(tenant_id);
        if tokio::fs::try_exists(&path)
            .await
            .map_err(|error| Error::Internal(error.to_string()))?
        {
            return Err(Error::AlreadyExists(format!(
                "tenant already exists: {tenant_id}"
            )));
        }
        self.open_tenant_at_path(tenant_id.clone(), path).await
    }

    pub async fn open_existing_tenant(
        &self,
        tenant_id: &TenantId,
    ) -> Result<Option<OpenedEmbeddedSqliteTenant>> {
        let path = self.tenant_path(tenant_id);
        if !tokio::fs::try_exists(&path)
            .await
            .map_err(|error| Error::Internal(error.to_string()))?
        {
            return Ok(None);
        }
        Ok(Some(
            self.open_tenant_at_path(tenant_id.clone(), path).await?,
        ))
    }

    pub async fn delete_tenant(&self, tenant_id: &TenantId) -> Result<()> {
        let path = self.tenant_path(tenant_id);
        tokio::fs::remove_file(&path)
            .await
            .map_err(|error| Error::Internal(error.to_string()))?;
        if self.encryption_provider.is_some() {
            let manifest_path = KeyManifest::manifest_path(&path);
            let _ = tokio::fs::remove_file(manifest_path).await;
        }
        Ok(())
    }

    pub async fn tenant_exists(&self, tenant_id: &TenantId) -> Result<bool> {
        tokio::fs::try_exists(self.tenant_path(tenant_id))
            .await
            .map_err(|error| Error::Internal(error.to_string()))
    }

    pub async fn list_tenants(&self) -> Result<Vec<TenantId>> {
        let data_dir = self.data_dir.clone();
        self.storage_handle
            .spawn_blocking(move || {
                let mut tenants = Vec::new();
                let entries = std::fs::read_dir(&data_dir)
                    .map_err(|error| Error::Internal(error.to_string()))?;
                for entry in entries {
                    let entry = entry.map_err(|error| Error::Internal(error.to_string()))?;
                    let path = entry.path();
                    if path.extension().is_some_and(|extension| {
                        extension == EmbeddedProviderKind::Sqlite.tenant_file_extension()
                    }) && let Some(stem) = path.file_stem()
                    {
                        tenants.push(TenantId::new(stem.to_string_lossy().to_string())?);
                    }
                }
                tenants.sort();
                Ok(tenants)
            })
            .await
            .map_err(map_join_error)?
    }

    fn tenant_path(&self, tenant_id: &TenantId) -> PathBuf {
        self.data_dir.join(format!(
            "{}.{}",
            tenant_id.as_str(),
            EmbeddedProviderKind::Sqlite.tenant_file_extension()
        ))
    }

    async fn open_tenant_at_path(
        &self,
        tenant_id: TenantId,
        path: PathBuf,
    ) -> Result<OpenedEmbeddedSqliteTenant> {
        let clock = self.clock.clone();
        let fault_injector = self.fault_injector.clone();
        let read_parallelism = self.tenant_read_parallelism;
        let provider = self.encryption_provider.clone();
        let store = self
            .storage_handle
            .spawn_blocking(move || {
                if let Some(provider) = provider {
                    let logical_name = path
                        .file_name()
                        .map(|name| name.to_string_lossy().to_string())
                        .unwrap_or_else(|| "tenant.sqlite3".to_string());
                    let subject = LocalKeySubject::sqlite_tenant(tenant_id, logical_name);
                    let dek = resolve_database_encryption_key(
                        &path,
                        provider.as_ref(),
                        &subject,
                        ManifestCipher::SqlCipher,
                    )?;
                    SqliteTenantStore::open_encrypted_with_simulation_and_max_read_connections(
                        path,
                        &dek,
                        clock,
                        fault_injector,
                        read_parallelism,
                    )
                } else {
                    SqliteTenantStore::open_with_simulation_and_max_read_connections(
                        path,
                        clock,
                        fault_injector,
                        read_parallelism,
                    )
                }
            })
            .await
            .map_err(map_join_error)??;
        let store = Arc::new(store);
        let read_storage = self.read_storage_for_store(store.clone());
        Ok(OpenedEmbeddedSqliteTenant {
            store,
            read_storage,
        })
    }
}

impl SqliteTenantStorage {
    pub fn new(store: Arc<SqliteTenantStore>, runtime_handle: TokioRuntimeHandle) -> Self {
        Self::with_max_concurrent_reads(store, runtime_handle, default_tenant_read_parallelism())
    }

    pub fn with_max_concurrent_reads(
        store: Arc<SqliteTenantStore>,
        runtime_handle: TokioRuntimeHandle,
        max_concurrent_reads: usize,
    ) -> Self {
        let read_parallelism = max_concurrent_reads.min(store.max_read_connections());
        Self {
            executor: BlockingReadExecutor::new(
                store.clone(),
                runtime_handle.clone(),
                read_parallelism,
            ),
            write_executor: SqliteBlockingWriteExecutor::new(store, runtime_handle),
        }
    }

    pub fn store(&self) -> Arc<SqliteTenantStore> {
        self.executor.store()
    }
}

impl TenantReadStorage for SqliteTenantStorage {
    type Store = SqliteTenantStore;

    async fn execute<T, F>(&self, task: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(Arc<SqliteTenantStore>) -> Result<T> + Send + 'static,
    {
        self.executor.execute(task).await
    }

    async fn execute_cancellable<T, Fut, Check, F>(
        &self,
        cancel_wait: Fut,
        check_cancel: Check,
        task: F,
    ) -> Result<T>
    where
        T: Send + 'static,
        Fut: Future<Output = ()> + Send,
        Check: Fn() -> Result<()> + Send + 'static,
        F: FnOnce(Arc<SqliteTenantStore>, &mut dyn FnMut() -> Result<()>) -> Result<T>
            + Send
            + 'static,
    {
        self.executor
            .execute_cancellable(cancel_wait, check_cancel, task)
            .await
    }
}

impl TenantWriteStorage for SqliteTenantStorage {
    type WriteTransaction = SqliteWriteTransaction;

    async fn execute_write<T, F>(&self, task: F) -> Result<TenantWriteCommit<T>>
    where
        T: Send + 'static,
        F: FnOnce(&mut SqliteWriteTransaction) -> Result<T> + Send + 'static,
    {
        self.write_executor.execute_write(task).await
    }

    async fn execute_write_cancellable<T, Fut, Check, F>(
        &self,
        cancel_wait: Fut,
        check_cancel: Check,
        task: F,
    ) -> Result<TenantWriteOutcome<T>>
    where
        T: Send + 'static,
        Fut: Future<Output = ()> + Send,
        Check: Fn() -> Result<()> + Send + 'static,
        F: FnOnce(&mut SqliteWriteTransaction) -> Result<T> + Send + 'static,
    {
        self.write_executor
            .execute_write_cancellable(cancel_wait, check_cancel, task)
            .await
    }
}

struct SqliteBlockingWriteExecutor {
    store: Arc<SqliteTenantStore>,
    permits: Arc<Semaphore>,
    runtime_handle: TokioRuntimeHandle,
}

impl Clone for SqliteBlockingWriteExecutor {
    fn clone(&self) -> Self {
        Self {
            store: self.store.clone(),
            permits: self.permits.clone(),
            runtime_handle: self.runtime_handle.clone(),
        }
    }
}

impl SqliteBlockingWriteExecutor {
    fn new(store: Arc<SqliteTenantStore>, runtime_handle: TokioRuntimeHandle) -> Self {
        Self {
            store,
            permits: Arc::new(Semaphore::new(SQLITE_TENANT_WRITE_PARALLELISM)),
            runtime_handle,
        }
    }

    async fn execute_write<T, F>(&self, task: F) -> Result<TenantWriteCommit<T>>
    where
        T: Send + 'static,
        F: FnOnce(&mut SqliteWriteTransaction) -> Result<T> + Send + 'static,
    {
        let permit = self
            .permits
            .clone()
            .acquire_owned()
            .await
            .map_err(map_permit_error)?;
        let store = self.store.clone();
        self.runtime_handle
            .spawn_blocking(move || {
                let _permit = permit;
                store.execute_write(task)
            })
            .await
            .map_err(map_join_error)?
    }

    async fn execute_write_cancellable<T, Fut, Check, F>(
        &self,
        cancel_wait: Fut,
        check_cancel: Check,
        task: F,
    ) -> Result<TenantWriteOutcome<T>>
    where
        T: Send + 'static,
        Fut: Future<Output = ()> + Send,
        Check: Fn() -> Result<()> + Send + 'static,
        F: FnOnce(&mut SqliteWriteTransaction) -> Result<T> + Send + 'static,
    {
        tokio::pin!(cancel_wait);

        let permit = tokio::select! {
            _ = &mut cancel_wait => return Ok(TenantWriteOutcome::CancelledBeforeCommit),
            permit = self.permits.clone().acquire_owned() => permit.map_err(map_permit_error)?,
        };

        let cancelled = Arc::new(AtomicBool::new(false));
        let store = self.store.clone();
        let cancelled_for_task = cancelled.clone();
        let mut handle = self.runtime_handle.spawn_blocking(move || {
            let _permit = permit;
            store.execute_write_cancellable(
                move || {
                    if cancelled_for_task.load(Ordering::SeqCst) {
                        return Err(Error::Cancelled);
                    }
                    check_cancel()
                },
                task,
            )
        });

        tokio::select! {
            result = &mut handle => map_write_result(result.map_err(map_join_error)?),
            _ = &mut cancel_wait => {
                cancelled.store(true, Ordering::SeqCst);
                map_write_result(handle.await.map_err(map_join_error)?)
            }
        }
    }
}

fn map_write_result<T>(result: Result<TenantWriteCommit<T>>) -> Result<TenantWriteOutcome<T>> {
    match result {
        Ok(committed) => Ok(TenantWriteOutcome::Committed(committed)),
        Err(Error::Cancelled) => Ok(TenantWriteOutcome::CancelledBeforeCommit),
        Err(error) => Err(error),
    }
}
