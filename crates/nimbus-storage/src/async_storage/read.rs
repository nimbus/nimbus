use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use nimbus_core::{Error, Result};
use tokio::runtime::Handle as TokioRuntimeHandle;
use tokio::sync::Semaphore;

use crate::{TenantStore, UsageStore};

use super::helpers::{map_join_error, map_permit_error};
use super::traits::{TenantReadStorage, UsageStorage};
use super::write::BlockingWriteExecutor;

const MIN_TENANT_READ_PARALLELISM: usize = 2;
const USAGE_READ_PARALLELISM: usize = 4;

pub(super) fn default_tenant_read_parallelism() -> usize {
    std::thread::available_parallelism()
        .map(|parallelism| parallelism.get().max(MIN_TENANT_READ_PARALLELISM))
        .unwrap_or(MIN_TENANT_READ_PARALLELISM)
}

pub(super) struct BlockingReadExecutor<S> {
    store: Arc<S>,
    permits: Arc<Semaphore>,
    runtime_handle: TokioRuntimeHandle,
}

impl<S> Clone for BlockingReadExecutor<S> {
    fn clone(&self) -> Self {
        Self {
            store: self.store.clone(),
            permits: self.permits.clone(),
            runtime_handle: self.runtime_handle.clone(),
        }
    }
}

impl<S> BlockingReadExecutor<S>
where
    S: Send + Sync + 'static,
{
    pub(super) fn new(
        store: Arc<S>,
        runtime_handle: TokioRuntimeHandle,
        max_concurrent_reads: usize,
    ) -> Self {
        Self {
            store,
            permits: Arc::new(Semaphore::new(max_concurrent_reads.max(1))),
            runtime_handle,
        }
    }

    pub(super) fn store(&self) -> Arc<S> {
        self.store.clone()
    }

    pub(super) async fn execute<T, F>(&self, task: F) -> Result<T>
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
        self.runtime_handle
            .spawn_blocking(move || {
                let _permit = permit;
                task(store)
            })
            .await
            .map_err(map_join_error)?
    }

    pub(super) async fn execute_cancellable<T, Fut, Check, F>(
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
        let mut handle = self.runtime_handle.spawn_blocking(move || {
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

#[derive(Clone)]
pub struct RedbTenantStorage {
    pub(super) executor: BlockingReadExecutor<TenantStore>,
    pub(super) write_executor: BlockingWriteExecutor<TenantStore>,
}

impl RedbTenantStorage {
    pub fn new(store: Arc<TenantStore>, runtime_handle: TokioRuntimeHandle) -> Self {
        Self::with_max_concurrent_reads(store, runtime_handle, default_tenant_read_parallelism())
    }

    pub fn with_max_concurrent_reads(
        store: Arc<TenantStore>,
        runtime_handle: TokioRuntimeHandle,
        max_concurrent_reads: usize,
    ) -> Self {
        Self {
            executor: BlockingReadExecutor::new(
                store.clone(),
                runtime_handle.clone(),
                max_concurrent_reads,
            ),
            write_executor: BlockingWriteExecutor::new(store, runtime_handle),
        }
    }

    pub fn store(&self) -> Arc<TenantStore> {
        self.executor.store()
    }
}

impl TenantReadStorage for RedbTenantStorage {
    type Store = TenantStore;

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

#[derive(Clone)]
pub struct RedbUsageStorage {
    executor: BlockingReadExecutor<UsageStore>,
}

impl RedbUsageStorage {
    pub(super) fn new(store: Arc<UsageStore>, runtime_handle: TokioRuntimeHandle) -> Self {
        Self {
            executor: BlockingReadExecutor::new(store, runtime_handle, USAGE_READ_PARALLELISM),
        }
    }

    pub fn store(&self) -> Arc<UsageStore> {
        self.executor.store()
    }
}

impl UsageStorage for RedbUsageStorage {
    type Store = UsageStore;

    async fn execute<T, F>(&self, task: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(Arc<UsageStore>) -> Result<T> + Send + 'static,
    {
        self.executor.execute(task).await
    }
}
