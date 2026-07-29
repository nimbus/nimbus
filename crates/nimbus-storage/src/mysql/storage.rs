use super::*;
use crate::sql::store_core::{
    SqlBlockingWriteExecutor, sql_execute_read, sql_execute_read_cancellable,
};

impl MySqlTenantStorage {
    pub fn new(store: Arc<MySqlTenantStore>, runtime_handle: TokioRuntimeHandle) -> Self {
        Self::with_max_concurrent_reads(store, runtime_handle, default_mysql_read_parallelism())
    }

    pub fn with_max_concurrent_reads(
        store: Arc<MySqlTenantStore>,
        runtime_handle: TokioRuntimeHandle,
        max_concurrent_reads: usize,
    ) -> Self {
        Self {
            write_executor: SqlBlockingWriteExecutor::new(
                store.clone(),
                runtime_handle.clone(),
                MYSQL_TENANT_WRITE_PARALLELISM,
                MYSQL_EXECUTOR_CONTEXT,
            ),
            store,
            permits: Arc::new(Semaphore::new(max_concurrent_reads.max(1))),
            runtime_handle,
        }
    }

    pub fn store(&self) -> Arc<MySqlTenantStore> {
        self.store.clone()
    }
}

impl TenantReadStorage for MySqlTenantStorage {
    type Store = MySqlTenantStore;

    async fn execute<T, F>(&self, task: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(Arc<MySqlTenantStore>) -> Result<T> + Send + 'static,
    {
        sql_execute_read(
            &self.permits,
            &self.runtime_handle,
            &self.store,
            MYSQL_EXECUTOR_CONTEXT,
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
        F: FnOnce(Arc<MySqlTenantStore>, &mut dyn FnMut() -> Result<()>) -> Result<T>
            + Send
            + 'static,
    {
        sql_execute_read_cancellable(
            &self.permits,
            &self.runtime_handle,
            &self.store,
            MYSQL_EXECUTOR_CONTEXT,
            cancel_wait,
            check_cancel,
            task,
        )
        .await
    }
}

impl TenantWriteStorage for MySqlTenantStorage {
    type WriteTransaction = MySqlWriteTransaction;

    async fn execute_write<T, F>(&self, task: F) -> Result<TenantWriteCommit<T>>
    where
        T: Send + 'static,
        F: FnOnce(&mut MySqlWriteTransaction) -> Result<T> + Send + 'static,
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
        F: FnOnce(&mut MySqlWriteTransaction) -> Result<T> + Send + 'static,
    {
        self.write_executor
            .execute_write_cancellable(cancel_wait, check_cancel, task)
            .await
    }
}
