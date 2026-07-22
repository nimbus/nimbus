use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, RwLock};

use nimbus_core::{Error, Result, TenantId, WallClock};
use tokio::runtime::Handle as TokioRuntimeHandle;

use crate::async_storage::{BlockingReadExecutor, BlockingWriteExecutor};
use crate::simulation::FaultInjector;
use crate::{TenantReadStorage, TenantWriteCommit, TenantWriteOutcome, TenantWriteStorage};

use super::{MemoryTenantStore, MemoryWriteTransaction};

pub struct OpenedMemoryTenant {
    pub store: Arc<MemoryTenantStore>,
    pub read_storage: Arc<MemoryTenantStorage>,
}

#[derive(Clone)]
pub struct MemoryTenantProvider {
    tenants: Arc<RwLock<HashMap<TenantId, Arc<MemoryTenantStore>>>>,
    clock: Arc<dyn WallClock>,
    fault_injector: Arc<dyn FaultInjector>,
    storage_handle: TokioRuntimeHandle,
    tenant_read_parallelism: usize,
}

impl MemoryTenantProvider {
    pub fn new(
        clock: Arc<dyn WallClock>,
        fault_injector: Arc<dyn FaultInjector>,
        storage_handle: TokioRuntimeHandle,
    ) -> Self {
        let tenant_read_parallelism =
            std::thread::available_parallelism().map_or(2, |parallelism| parallelism.get().max(2));
        Self {
            tenants: Arc::new(RwLock::new(HashMap::new())),
            clock,
            fault_injector,
            storage_handle,
            tenant_read_parallelism,
        }
    }

    fn read_tenants(
        &self,
    ) -> Result<std::sync::RwLockReadGuard<'_, HashMap<TenantId, Arc<MemoryTenantStore>>>> {
        self.tenants
            .read()
            .map_err(|_| Error::Internal("memory tenant provider lock is poisoned".to_string()))
    }

    fn write_tenants(
        &self,
    ) -> Result<std::sync::RwLockWriteGuard<'_, HashMap<TenantId, Arc<MemoryTenantStore>>>> {
        self.tenants
            .write()
            .map_err(|_| Error::Internal("memory tenant provider lock is poisoned".to_string()))
    }

    pub fn read_storage_for_store(
        &self,
        store: Arc<MemoryTenantStore>,
    ) -> Arc<MemoryTenantStorage> {
        Arc::new(MemoryTenantStorage::with_max_concurrent_reads(
            store,
            self.storage_handle.clone(),
            self.tenant_read_parallelism,
        ))
    }

    fn opened(&self, store: Arc<MemoryTenantStore>) -> OpenedMemoryTenant {
        let read_storage = self.read_storage_for_store(store.clone());
        OpenedMemoryTenant {
            store,
            read_storage,
        }
    }

    pub async fn list_tenants(&self) -> Result<Vec<TenantId>> {
        let mut tenant_ids = self.read_tenants()?.keys().cloned().collect::<Vec<_>>();
        tenant_ids.sort();
        Ok(tenant_ids)
    }

    pub async fn tenant_exists(&self, tenant_id: &TenantId) -> Result<bool> {
        Ok(self.read_tenants()?.contains_key(tenant_id))
    }

    pub async fn create_opened_tenant(&self, tenant_id: &TenantId) -> Result<OpenedMemoryTenant> {
        let mut tenants = self.write_tenants()?;
        if tenants.contains_key(tenant_id) {
            return Err(Error::AlreadyExists(format!(
                "tenant already exists: {tenant_id}"
            )));
        }
        self.fault_injector
            .check_for_tenant(crate::FaultPoint::TenantCreateBeforeRegistration, tenant_id)?;
        let tenant_faults = crate::simulation::tenant_scoped_fault_injector(
            self.fault_injector.clone(),
            tenant_id.clone(),
        );
        let store = Arc::new(MemoryTenantStore::with_simulation(
            self.clock.clone(),
            tenant_faults,
        ));
        tenants.insert(tenant_id.clone(), store.clone());
        drop(tenants);
        Ok(self.opened(store))
    }

    pub async fn open_existing_opened_tenant(
        &self,
        tenant_id: &TenantId,
    ) -> Result<Option<OpenedMemoryTenant>> {
        Ok(self
            .read_tenants()?
            .get(tenant_id)
            .cloned()
            .map(|store| self.opened(store)))
    }

    pub async fn delete_tenant(&self, tenant_id: &TenantId) -> Result<()> {
        if self.write_tenants()?.remove(tenant_id).is_none() {
            return Err(Error::TenantNotFound(tenant_id.clone()));
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct MemoryTenantStorage {
    executor: BlockingReadExecutor<MemoryTenantStore>,
    write_executor: BlockingWriteExecutor<MemoryTenantStore>,
}

impl MemoryTenantStorage {
    pub fn new(store: Arc<MemoryTenantStore>, runtime_handle: TokioRuntimeHandle) -> Self {
        Self::with_max_concurrent_reads(store, runtime_handle, 2)
    }

    pub fn with_max_concurrent_reads(
        store: Arc<MemoryTenantStore>,
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

    pub fn store(&self) -> Arc<MemoryTenantStore> {
        self.executor.store()
    }
}

impl TenantReadStorage for MemoryTenantStorage {
    type Store = MemoryTenantStore;

    async fn execute<T, F>(&self, task: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(Arc<MemoryTenantStore>) -> Result<T> + Send + 'static,
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
        F: FnOnce(Arc<MemoryTenantStore>, &mut dyn FnMut() -> Result<()>) -> Result<T>
            + Send
            + 'static,
    {
        self.executor
            .execute_cancellable(cancel_wait, check_cancel, task)
            .await
    }
}

impl TenantWriteStorage for MemoryTenantStorage {
    type WriteTransaction = MemoryWriteTransaction;

    async fn execute_write<T, F>(&self, task: F) -> Result<TenantWriteCommit<T>>
    where
        T: Send + 'static,
        F: FnOnce(&mut MemoryWriteTransaction) -> Result<T> + Send + 'static,
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
        F: FnOnce(&mut MemoryWriteTransaction) -> Result<T> + Send + 'static,
    {
        self.write_executor
            .execute_write_cancellable(cancel_wait, check_cancel, task)
            .await
    }
}
