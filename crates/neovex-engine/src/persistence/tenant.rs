use std::sync::{Arc, Mutex};

use neovex_core::{
    CommitEntry, CronJob, Document, DocumentId, DurableMutationRecord, Result, ScheduledJob,
    ScheduledJobResult, Schema, SequenceNumber, TableName, TableSchema, Timestamp,
};
use neovex_storage::{
    DurableJournalBootstrap, DurableJournalPage, FaultPoint, JournalProgress,
    LibsqlReplicaFreshnessStats, LibsqlReplicaTenantStore, MySqlTenantStore, PostgresTenantStore,
    ResolvedScheduleOp, ResolvedWrite, SqliteTenantStore, TenantStore as RedbTenantStore,
};

use super::{PersistenceProvider, TenantPersistenceExecutor, TenantPersistenceSnapshot};

#[derive(Clone)]
pub(crate) enum TenantPersistence {
    Redb(Arc<RedbTenantStore>),
    Sqlite(Arc<SqliteTenantStore>),
    LibsqlReplica(Arc<LibsqlReplicaTenantStore>),
    Postgres(Arc<PostgresTenantStore>),
    MySql(Arc<MySqlTenantStore>),
}

macro_rules! delegate_store_method {
    ($(#[$meta:meta])* fn $name:ident(&self $(, $arg:ident : $ty:ty )* ) -> $ret:ty) => {
        $(#[$meta])*
        pub(crate) fn $name(&self, $($arg: $ty),*) -> $ret {
            match_tenant_persistence!(self, |store| store.$name($($arg),*))
        }
    };
}

impl TenantPersistence {
    pub(crate) fn read_storage_for_provider(
        self,
        provider: &PersistenceProvider,
    ) -> Result<TenantPersistenceExecutor> {
        match (provider, self) {
            (PersistenceProvider::Redb(provider), Self::Redb(store)) => Ok(
                TenantPersistenceExecutor::Redb(provider.read_storage_for_store(store)),
            ),
            (PersistenceProvider::Sqlite(provider), Self::Sqlite(store)) => Ok(
                TenantPersistenceExecutor::Sqlite(provider.read_storage_for_store(store)),
            ),
            (PersistenceProvider::LibsqlReplica(provider), Self::LibsqlReplica(store)) => Ok(
                TenantPersistenceExecutor::LibsqlReplica(provider.read_storage_for_store(store)),
            ),
            (PersistenceProvider::Postgres(provider), Self::Postgres(store)) => Ok(
                TenantPersistenceExecutor::Postgres(provider.read_storage_for_store(store)),
            ),
            (PersistenceProvider::MySql(provider), Self::MySql(store)) => Ok(
                TenantPersistenceExecutor::MySql(provider.read_storage_for_store(store)),
            ),
            _ => Err(neovex_core::Error::Internal(
                "persistence provider and tenant persistence mismatch".to_string(),
            )),
        }
    }
}

mod journal;
mod reads;
mod scheduler;
mod schema;
mod writes;
