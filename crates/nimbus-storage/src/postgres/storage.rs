use super::*;
use crate::sql::store_core::{
    SqlBlockingWriteExecutor, sql_execute_read, sql_execute_read_cancellable,
};

impl PostgresTenantStorage {
    pub fn new(store: Arc<PostgresTenantStore>, runtime_handle: TokioRuntimeHandle) -> Self {
        Self::with_max_concurrent_reads(store, runtime_handle, default_postgres_read_parallelism())
    }

    pub fn with_max_concurrent_reads(
        store: Arc<PostgresTenantStore>,
        runtime_handle: TokioRuntimeHandle,
        max_concurrent_reads: usize,
    ) -> Self {
        Self {
            store: store.clone(),
            permits: Arc::new(Semaphore::new(max_concurrent_reads.max(1))),
            runtime_handle: runtime_handle.clone(),
            write_executor: SqlBlockingWriteExecutor::new(
                store,
                runtime_handle,
                POSTGRES_TENANT_WRITE_PARALLELISM,
                POSTGRES_EXECUTOR_CONTEXT,
            ),
        }
    }

    pub fn store(&self) -> Arc<PostgresTenantStore> {
        self.store.clone()
    }
}

impl TenantReadStorage for PostgresTenantStorage {
    type Store = PostgresTenantStore;

    async fn execute<T, F>(&self, task: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(Arc<PostgresTenantStore>) -> Result<T> + Send + 'static,
    {
        sql_execute_read(
            &self.permits,
            &self.runtime_handle,
            &self.store,
            POSTGRES_EXECUTOR_CONTEXT,
            task,
        )
        .await
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
        F: FnOnce(Arc<PostgresTenantStore>, &mut dyn FnMut() -> Result<()>) -> Result<T>
            + Send
            + 'static,
    {
        sql_execute_read_cancellable(
            &self.permits,
            &self.runtime_handle,
            &self.store,
            POSTGRES_EXECUTOR_CONTEXT,
            cancel_wait,
            check_cancel,
            task,
        )
        .await
    }
}

impl TenantWriteStorage for PostgresTenantStorage {
    type WriteTransaction = PostgresWriteTransaction;

    async fn execute_write<T, F>(&self, task: F) -> Result<TenantWriteCommit<T>>
    where
        T: Send + 'static,
        F: FnOnce(&mut PostgresWriteTransaction) -> Result<T> + Send + 'static,
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
        F: FnOnce(&mut PostgresWriteTransaction) -> Result<T> + Send + 'static,
    {
        self.write_executor
            .execute_write_cancellable(cancel_wait, check_cancel, task)
            .await
    }
}
