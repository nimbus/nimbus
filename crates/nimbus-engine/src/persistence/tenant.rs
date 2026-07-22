use std::sync::{Arc, Mutex};

use nimbus_core::{
    CollectionName, CommitEntry, CronJob, Document, DocumentId, DocumentLocator,
    ResourcePathBinding, Result, ScheduledJob, ScheduledJobResult, Schema, SequenceNumber, TableId,
    TableName, TableSchema, TenantEventRecord, Timestamp, TriggerDeliveryCursor,
    TriggerInvocationRecord,
};
#[cfg(any(test, feature = "test-hooks"))]
use nimbus_storage::MemoryTenantStore;
use nimbus_storage::{
    ChangefeedBootstrap, ChangefeedCursor, ChangefeedPage, DurableJournalBootstrap,
    DurableJournalPage, FaultPoint, JournalProgress, LibsqlReplicaFreshnessStats,
    LibsqlReplicaTenantStore, MySqlTenantStore, PointInTimeRestoreArchive,
    PointInTimeRestoreTarget, PostgresTenantStore, ProviderWritePipelineDiagnostic,
    ResolvedScheduleOp, ResolvedWrite, RetentionGcConfig, SchedulerWrite,
    SchedulerWriteOutcomeStore, SchedulerWriteResult, SchedulerWriteStore, SqliteTenantStore,
    TenantStore as RedbTenantStore,
};

use super::{PersistenceProvider, TenantPersistenceExecutor, TenantPersistenceSnapshot};

#[derive(Clone)]
pub(crate) enum TenantPersistence {
    Redb(Arc<RedbTenantStore>),
    Sqlite(Arc<SqliteTenantStore>),
    LibsqlReplica(Arc<LibsqlReplicaTenantStore>),
    Postgres(Arc<PostgresTenantStore>),
    MySql(Arc<MySqlTenantStore>),
    #[cfg(any(test, feature = "test-hooks"))]
    Memory(Arc<MemoryTenantStore>),
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
    pub(crate) fn provider_write_pipeline_diagnostic(
        &self,
    ) -> Option<ProviderWritePipelineDiagnostic> {
        match self {
            Self::Postgres(store) => Some(store.write_pipeline_diagnostic()),
            Self::MySql(store) => Some(store.write_pipeline_diagnostic()),
            Self::Redb(_) | Self::Sqlite(_) | Self::LibsqlReplica(_) => None,
            #[cfg(any(test, feature = "test-hooks"))]
            Self::Memory(_) => None,
        }
    }

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
            #[cfg(any(test, feature = "test-hooks"))]
            (PersistenceProvider::Memory(provider), Self::Memory(store)) => Ok(
                TenantPersistenceExecutor::Memory(provider.read_storage_for_store(store)),
            ),
            _ => Err(nimbus_core::Error::Internal(
                "persistence provider and tenant persistence mismatch".to_string(),
            )),
        }
    }
}

mod committer_lease;
mod journal;
mod objects;
mod provider_state;
mod reads;
mod resource_paths;
mod scheduler;
mod schema;
mod trigger_delivery;
mod trigger_invocations;
mod writes;
