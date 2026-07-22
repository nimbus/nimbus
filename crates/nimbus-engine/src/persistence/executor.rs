use std::future::Future;
use std::sync::Arc;

use nimbus_core::Result;
#[cfg(any(test, feature = "test-hooks"))]
use nimbus_storage::MemoryTenantStorage;
use nimbus_storage::{
    LibsqlReplicaTenantStorage, MySqlTenantStorage, PostgresTenantStorage, RedbTenantStorage,
    SqliteTenantStorage, TenantReadStorage,
};

use super::TenantPersistence;

#[derive(Clone)]
pub(crate) enum TenantPersistenceExecutor {
    Redb(Arc<RedbTenantStorage>),
    Sqlite(Arc<SqliteTenantStorage>),
    LibsqlReplica(Arc<LibsqlReplicaTenantStorage>),
    Postgres(Arc<PostgresTenantStorage>),
    MySql(Arc<MySqlTenantStorage>),
    #[cfg(any(test, feature = "test-hooks"))]
    Memory(Arc<MemoryTenantStorage>),
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
}
