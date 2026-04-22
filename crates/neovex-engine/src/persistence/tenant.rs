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

use super::TenantPersistenceSnapshot;

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
            match self {
                Self::Redb(store) => store.$name($($arg),*),
                Self::Sqlite(store) => store.$name($($arg),*),
                Self::LibsqlReplica(store) => store.$name($($arg),*),
                Self::Postgres(store) => store.$name($($arg),*),
                Self::MySql(store) => store.$name($($arg),*),
            }
        }
    };
}

mod journal;
mod reads;
mod scheduler;
mod schema;
mod writes;
