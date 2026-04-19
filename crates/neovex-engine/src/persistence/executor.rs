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
        match self {
            Self::Redb(storage) => {
                storage
                    .execute(move |store| task(TenantPersistence::Redb(store)))
                    .await
            }
            Self::Sqlite(storage) => {
                storage
                    .execute(move |store| task(TenantPersistence::Sqlite(store)))
                    .await
            }
            Self::LibsqlReplica(storage) => {
                storage
                    .execute(move |store| task(TenantPersistence::LibsqlReplica(store)))
                    .await
            }
            Self::Postgres(storage) => {
                storage
                    .execute(move |store| task(TenantPersistence::Postgres(store)))
                    .await
            }
            Self::MySql(storage) => {
                storage
                    .execute(move |store| task(TenantPersistence::MySql(store)))
                    .await
            }
        }
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
        match self {
            Self::Redb(storage) => {
                storage
                    .execute_cancellable(cancel_wait, check_cancel, move |store, check_cancel| {
                        task(TenantPersistence::Redb(store), check_cancel)
                    })
                    .await
            }
            Self::Sqlite(storage) => {
                storage
                    .execute_cancellable(cancel_wait, check_cancel, move |store, check_cancel| {
                        task(TenantPersistence::Sqlite(store), check_cancel)
                    })
                    .await
            }
            Self::LibsqlReplica(storage) => {
                storage
                    .execute_cancellable(cancel_wait, check_cancel, move |store, check_cancel| {
                        task(TenantPersistence::LibsqlReplica(store), check_cancel)
                    })
                    .await
            }
            Self::Postgres(storage) => {
                storage
                    .execute_cancellable(cancel_wait, check_cancel, move |store, check_cancel| {
                        task(TenantPersistence::Postgres(store), check_cancel)
                    })
                    .await
            }
            Self::MySql(storage) => {
                storage
                    .execute_cancellable(cancel_wait, check_cancel, move |store, check_cancel| {
                        task(TenantPersistence::MySql(store), check_cancel)
                    })
                    .await
            }
        }
    }

    pub(crate) async fn execute_write<T, F>(&self, task: F) -> Result<TenantWriteCommit<T>>
    where
        T: Send + 'static,
        F: FnOnce(&mut dyn TenantPersistenceWriteOps) -> Result<T> + Send + 'static,
    {
        match self {
            Self::Redb(storage) => {
                storage
                    .execute_write(move |transaction| task(transaction))
                    .await
            }
            Self::Sqlite(storage) => {
                storage
                    .execute_write(move |transaction| task(transaction))
                    .await
            }
            Self::LibsqlReplica(storage) => {
                storage
                    .execute_write(move |transaction| task(transaction))
                    .await
            }
            Self::Postgres(storage) => {
                storage
                    .execute_write(move |transaction| task(transaction))
                    .await
            }
            Self::MySql(storage) => {
                storage
                    .execute_write(move |transaction| task(transaction))
                    .await
            }
        }
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
        match self {
            Self::Redb(storage) => {
                storage
                    .execute_write_cancellable(cancel_wait, check_cancel, move |transaction| {
                        task(transaction)
                    })
                    .await
            }
            Self::Sqlite(storage) => {
                storage
                    .execute_write_cancellable(cancel_wait, check_cancel, move |transaction| {
                        task(transaction)
                    })
                    .await
            }
            Self::LibsqlReplica(storage) => {
                storage
                    .execute_write_cancellable(cancel_wait, check_cancel, move |transaction| {
                        task(transaction)
                    })
                    .await
            }
            Self::Postgres(storage) => {
                storage
                    .execute_write_cancellable(cancel_wait, check_cancel, move |transaction| {
                        task(transaction)
                    })
                    .await
            }
            Self::MySql(storage) => {
                storage
                    .execute_write_cancellable(cancel_wait, check_cancel, move |transaction| {
                        task(transaction)
                    })
                    .await
            }
        }
    }
}
