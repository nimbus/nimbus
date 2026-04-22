use std::future::Future;
use std::sync::Arc;

use neovex_core::Result;
use neovex_storage::{
    LibsqlReplicaTenantStorage, MySqlTenantStorage, PostgresTenantStorage, RedbTenantStorage,
    SqliteTenantStorage, TenantReadStorage, TenantWriteCommit, TenantWriteOutcome,
    TenantWriteStorage,
};

use super::{TenantPersistence, TenantPersistenceWriteOps};

#[derive(Clone)]
pub(crate) enum TenantPersistenceExecutor {
    Redb(Arc<RedbTenantStorage>),
    Sqlite(Arc<SqliteTenantStorage>),
    LibsqlReplica(Arc<LibsqlReplicaTenantStorage>),
    Postgres(Arc<PostgresTenantStorage>),
    MySql(Arc<MySqlTenantStorage>),
}

impl TenantPersistenceExecutor {
    pub(crate) async fn execute<T, F>(&self, task: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(TenantPersistence) -> Result<T> + Send + 'static,
    {
        match_tenant_persistence_executor!(self, |wrap, storage| {
            storage.execute(move |store| task(wrap(store))).await
        })
    }

    pub(crate) async fn execute_cancellable<T, Fut, Check, F>(
        &self,
        cancel_wait: Fut,
        check_cancel: Check,
        task: F,
    ) -> Result<T>
    where
        T: Send + 'static,
        Fut: Future<Output = ()> + Send,
        Check: Fn() -> Result<()> + Send + 'static,
        F: FnOnce(TenantPersistence, &mut dyn FnMut() -> Result<()>) -> Result<T> + Send + 'static,
    {
        match_tenant_persistence_executor!(self, |wrap, storage| {
            storage
                .execute_cancellable(cancel_wait, check_cancel, move |store, check_cancel| {
                    task(wrap(store), check_cancel)
                })
                .await
        })
    }

    pub(crate) async fn execute_write<T, F>(&self, task: F) -> Result<TenantWriteCommit<T>>
    where
        T: Send + 'static,
        F: FnOnce(&mut dyn TenantPersistenceWriteOps) -> Result<T> + Send + 'static,
    {
        match_tenant_persistence_executor!(self, |storage| {
            storage
                .execute_write(move |transaction| task(transaction))
                .await
        })
    }

    pub(crate) async fn execute_write_cancellable<T, Fut, Check, F>(
        &self,
        cancel_wait: Fut,
        check_cancel: Check,
        task: F,
    ) -> Result<TenantWriteOutcome<T>>
    where
        T: Send + 'static,
        Fut: Future<Output = ()> + Send,
        Check: Fn() -> Result<()> + Send + 'static,
        F: FnOnce(&mut dyn TenantPersistenceWriteOps) -> Result<T> + Send + 'static,
    {
        match_tenant_persistence_executor!(self, |storage| {
            storage
                .execute_write_cancellable(cancel_wait, check_cancel, move |transaction| {
                    task(transaction)
                })
                .await
        })
    }
}
