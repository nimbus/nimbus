#![allow(async_fn_in_trait)]

use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use neovex_core::{Error, Result, TenantId};
use tokio::sync::Semaphore;

use crate::{
    Clock, FaultInjector, TenantStore, TenantWriteCommit, TenantWriteTransaction, UsageStore,
};

const MIN_TENANT_READ_PARALLELISM: usize = 2;
const TENANT_WRITE_PARALLELISM: usize = 1;
const USAGE_READ_PARALLELISM: usize = 4;

pub trait StorageEngine {
    type TenantRead: TenantReadStorage;
    type Usage: UsageStorage;

    async fn list_tenants(&self) -> Result<Vec<TenantId>>;
}

pub trait TenantReadStorage: Send + Sync {
    async fn execute<T, F>(&self, task: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(Arc<TenantStore>) -> Result<T> + Send + 'static;

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
        F: FnOnce(Arc<TenantStore>, &mut dyn FnMut() -> Result<()>) -> Result<T> + Send + 'static;
}

pub enum TenantWriteOutcome<T> {
    CancelledBeforeCommit,
    Committed(TenantWriteCommit<T>),
}

pub trait TenantWriteStorage: Send + Sync {
    async fn execute_write<T, F>(&self, task: F) -> Result<TenantWriteCommit<T>>
    where
        T: Send + 'static,
        F: FnOnce(&mut TenantWriteTransaction) -> Result<T> + Send + 'static;

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
        F: FnOnce(&mut TenantWriteTransaction) -> Result<T> + Send + 'static;
}

pub trait UsageStorage: Send + Sync {
    async fn execute<T, F>(&self, task: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(Arc<UsageStore>) -> Result<T> + Send + 'static;
}

struct BlockingReadExecutor<S> {
    store: Arc<S>,
    permits: Arc<Semaphore>,
}

impl<S> Clone for BlockingReadExecutor<S> {
    fn clone(&self) -> Self {
        Self {
            store: self.store.clone(),
            permits: self.permits.clone(),
        }
    }
}

impl<S> BlockingReadExecutor<S>
where
    S: Send + Sync + 'static,
{
    fn new(store: Arc<S>, max_concurrent_reads: usize) -> Self {
        Self {
            store,
            permits: Arc::new(Semaphore::new(max_concurrent_reads.max(1))),
        }
    }

    fn store(&self) -> Arc<S> {
        self.store.clone()
    }

    async fn execute<T, F>(&self, task: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(Arc<S>) -> Result<T> + Send + 'static,
    {
        let permit = self
            .permits
            .clone()
            .acquire_owned()
            .await
            .map_err(map_permit_error)?;
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            task(store)
        })
        .await
        .map_err(map_join_error)?
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
        F: FnOnce(Arc<S>, &mut dyn FnMut() -> Result<()>) -> Result<T> + Send + 'static,
    {
        tokio::pin!(cancel_wait);

        let permit = tokio::select! {
            _ = &mut cancel_wait => return Err(Error::Cancelled),
            permit = self.permits.clone().acquire_owned() => permit.map_err(map_permit_error)?,
        };

        let cancelled = Arc::new(AtomicBool::new(false));
        let store = self.store.clone();
        let cancelled_for_task = cancelled.clone();
        let mut handle = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let mut combined_cancel = || {
                if cancelled_for_task.load(Ordering::SeqCst) {
                    return Err(Error::Cancelled);
                }
                check_cancel()
            };
            task(store, &mut combined_cancel)
        });

        tokio::select! {
            _ = &mut cancel_wait => {
                cancelled.store(true, Ordering::SeqCst);
                handle.abort();
                Err(Error::Cancelled)
            }
            result = &mut handle => result.map_err(map_join_error)?,
        }
    }
}

struct BlockingWriteExecutor {
    store: Arc<TenantStore>,
    permits: Arc<Semaphore>,
}

impl Clone for BlockingWriteExecutor {
    fn clone(&self) -> Self {
        Self {
            store: self.store.clone(),
            permits: self.permits.clone(),
        }
    }
}

impl BlockingWriteExecutor {
    fn new(store: Arc<TenantStore>) -> Self {
        Self {
            store,
            permits: Arc::new(Semaphore::new(TENANT_WRITE_PARALLELISM)),
        }
    }

    async fn execute_write<T, F>(&self, task: F) -> Result<TenantWriteCommit<T>>
    where
        T: Send + 'static,
        F: FnOnce(&mut TenantWriteTransaction) -> Result<T> + Send + 'static,
    {
        let permit = self
            .permits
            .clone()
            .acquire_owned()
            .await
            .map_err(map_permit_error)?;
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || {
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
        F: FnOnce(&mut TenantWriteTransaction) -> Result<T> + Send + 'static,
    {
        tokio::pin!(cancel_wait);

        let permit = tokio::select! {
            _ = &mut cancel_wait => return Ok(TenantWriteOutcome::CancelledBeforeCommit),
            permit = self.permits.clone().acquire_owned() => permit.map_err(map_permit_error)?,
        };

        let cancelled = Arc::new(AtomicBool::new(false));
        let store = self.store.clone();
        let cancelled_for_task = cancelled.clone();
        let mut handle = tokio::task::spawn_blocking(move || {
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
            result = &mut handle => {
                map_write_result(result.map_err(map_join_error)?)
            }
            _ = &mut cancel_wait => {
                cancelled.store(true, Ordering::SeqCst);
                map_write_result(handle.await.map_err(map_join_error)?)
            }
        }
    }
}

#[derive(Clone)]
pub struct RedbTenantStorage {
    executor: BlockingReadExecutor<TenantStore>,
    write_executor: BlockingWriteExecutor,
}

impl RedbTenantStorage {
    pub fn new(store: Arc<TenantStore>) -> Self {
        Self::with_max_concurrent_reads(store, default_tenant_read_parallelism())
    }

    pub fn with_max_concurrent_reads(store: Arc<TenantStore>, max_concurrent_reads: usize) -> Self {
        Self {
            executor: BlockingReadExecutor::new(store.clone(), max_concurrent_reads),
            write_executor: BlockingWriteExecutor::new(store),
        }
    }

    pub fn store(&self) -> Arc<TenantStore> {
        self.executor.store()
    }
}

impl TenantReadStorage for RedbTenantStorage {
    async fn execute<T, F>(&self, task: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(Arc<TenantStore>) -> Result<T> + Send + 'static,
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
        F: FnOnce(Arc<TenantStore>, &mut dyn FnMut() -> Result<()>) -> Result<T> + Send + 'static,
    {
        self.executor
            .execute_cancellable(cancel_wait, check_cancel, task)
            .await
    }
}

impl TenantWriteStorage for RedbTenantStorage {
    async fn execute_write<T, F>(&self, task: F) -> Result<TenantWriteCommit<T>>
    where
        T: Send + 'static,
        F: FnOnce(&mut TenantWriteTransaction) -> Result<T> + Send + 'static,
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
        F: FnOnce(&mut TenantWriteTransaction) -> Result<T> + Send + 'static,
    {
        self.write_executor
            .execute_write_cancellable(cancel_wait, check_cancel, task)
            .await
    }
}

#[derive(Clone)]
pub struct RedbUsageStorage {
    executor: BlockingReadExecutor<UsageStore>,
}

impl RedbUsageStorage {
    fn new(store: Arc<UsageStore>) -> Self {
        Self {
            executor: BlockingReadExecutor::new(store, USAGE_READ_PARALLELISM),
        }
    }

    pub fn store(&self) -> Arc<UsageStore> {
        self.executor.store()
    }
}

impl UsageStorage for RedbUsageStorage {
    async fn execute<T, F>(&self, task: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(Arc<UsageStore>) -> Result<T> + Send + 'static,
    {
        self.executor.execute(task).await
    }
}

pub struct OpenedRedbTenant {
    pub store: Arc<TenantStore>,
    pub read_storage: Arc<RedbTenantStorage>,
}

#[derive(Clone)]
pub struct RedbStorageEngine {
    data_dir: PathBuf,
    clock: Arc<dyn Clock>,
    fault_injector: Arc<dyn FaultInjector>,
    usage_storage: Arc<RedbUsageStorage>,
    tenant_read_parallelism: usize,
}

impl RedbStorageEngine {
    pub fn new(
        data_dir: impl Into<PathBuf>,
        clock: Arc<dyn Clock>,
        fault_injector: Arc<dyn FaultInjector>,
    ) -> Result<Self> {
        let data_dir = data_dir.into();
        let usage_store = Arc::new(UsageStore::open(data_dir.join("neovex-control.db"))?);
        Ok(Self {
            data_dir,
            clock,
            fault_injector,
            usage_storage: Arc::new(RedbUsageStorage::new(usage_store)),
            tenant_read_parallelism: default_tenant_read_parallelism(),
        })
    }

    pub fn usage_store(&self) -> Arc<UsageStore> {
        self.usage_storage.store()
    }

    pub fn usage_storage(&self) -> Arc<RedbUsageStorage> {
        self.usage_storage.clone()
    }

    pub fn read_storage_for_store(&self, store: Arc<TenantStore>) -> Arc<RedbTenantStorage> {
        Arc::new(RedbTenantStorage::with_max_concurrent_reads(
            store,
            self.tenant_read_parallelism,
        ))
    }

    pub async fn create_tenant(&self, tenant_id: &TenantId) -> Result<OpenedRedbTenant> {
        let path = self.tenant_path(tenant_id);
        if tokio::fs::try_exists(&path)
            .await
            .map_err(|error| Error::Internal(error.to_string()))?
        {
            return Err(Error::AlreadyExists(format!(
                "tenant already exists: {tenant_id}"
            )));
        }

        self.open_tenant_at_path(path).await
    }

    pub async fn open_existing_tenant(
        &self,
        tenant_id: &TenantId,
    ) -> Result<Option<OpenedRedbTenant>> {
        let path = self.tenant_path(tenant_id);
        if !tokio::fs::try_exists(&path)
            .await
            .map_err(|error| Error::Internal(error.to_string()))?
        {
            return Ok(None);
        }

        Ok(Some(self.open_tenant_at_path(path).await?))
    }

    pub async fn delete_tenant(&self, tenant_id: &TenantId) -> Result<()> {
        tokio::fs::remove_file(self.tenant_path(tenant_id))
            .await
            .map_err(|error| Error::Internal(error.to_string()))
    }

    pub async fn tenant_exists(&self, tenant_id: &TenantId) -> Result<bool> {
        tokio::fs::try_exists(self.tenant_path(tenant_id))
            .await
            .map_err(|error| Error::Internal(error.to_string()))
    }

    fn tenant_path(&self, tenant_id: &TenantId) -> PathBuf {
        self.data_dir.join(format!("{}.redb", tenant_id.as_str()))
    }

    async fn open_tenant_at_path(&self, path: PathBuf) -> Result<OpenedRedbTenant> {
        let clock = self.clock.clone();
        let fault_injector = self.fault_injector.clone();
        let store = tokio::task::spawn_blocking(move || {
            TenantStore::open_with_simulation(path, clock, fault_injector)
        })
        .await
        .map_err(map_join_error)??;

        let store = Arc::new(store);
        let read_storage = self.read_storage_for_store(store.clone());
        Ok(OpenedRedbTenant {
            store,
            read_storage,
        })
    }
}

impl StorageEngine for RedbStorageEngine {
    type TenantRead = RedbTenantStorage;
    type Usage = RedbUsageStorage;

    async fn list_tenants(&self) -> Result<Vec<TenantId>> {
        let data_dir = self.data_dir.clone();
        tokio::task::spawn_blocking(move || {
            let mut tenants = Vec::new();
            let entries =
                std::fs::read_dir(&data_dir).map_err(|error| Error::Internal(error.to_string()))?;
            for entry in entries {
                let entry = entry.map_err(|error| Error::Internal(error.to_string()))?;
                let path = entry.path();
                if path
                    .extension()
                    .is_some_and(|extension| extension == "redb")
                    && let Some(stem) = path.file_stem()
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
}

fn default_tenant_read_parallelism() -> usize {
    std::thread::available_parallelism()
        .map(|parallelism| parallelism.get().max(MIN_TENANT_READ_PARALLELISM))
        .unwrap_or(MIN_TENANT_READ_PARALLELISM)
}

fn map_join_error(error: tokio::task::JoinError) -> Error {
    Error::Internal(format!("blocking storage task failed: {error}"))
}

fn map_permit_error(_error: tokio::sync::AcquireError) -> Error {
    Error::Internal("blocking storage permit was closed".to_string())
}

fn map_write_result<T>(result: Result<TenantWriteCommit<T>>) -> Result<TenantWriteOutcome<T>> {
    match result {
        Ok(committed) => Ok(TenantWriteOutcome::Committed(committed)),
        Err(Error::Cancelled) => Ok(TenantWriteOutcome::CancelledBeforeCommit),
        Err(error) => Err(error),
    }
}
