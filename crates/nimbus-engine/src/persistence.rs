macro_rules! match_persistence_provider {
    ($value:expr, |$provider:ident| $body:expr) => {
        match $value {
            crate::persistence::PersistenceProvider::Redb($provider) => $body,
            crate::persistence::PersistenceProvider::Sqlite($provider) => $body,
            crate::persistence::PersistenceProvider::LibsqlReplica($provider) => $body,
            crate::persistence::PersistenceProvider::Postgres($provider) => $body,
            crate::persistence::PersistenceProvider::MySql($provider) => $body,
        }
    };
}

macro_rules! match_tenant_persistence {
    ($value:expr, |$store:ident| $body:expr) => {
        match $value {
            crate::persistence::TenantPersistence::Redb($store) => $body,
            crate::persistence::TenantPersistence::Sqlite($store) => $body,
            crate::persistence::TenantPersistence::LibsqlReplica($store) => $body,
            crate::persistence::TenantPersistence::Postgres($store) => $body,
            crate::persistence::TenantPersistence::MySql($store) => $body,
        }
    };
}

macro_rules! match_tenant_persistence_executor {
    ($value:expr, |$storage:ident| $body:expr) => {
        match $value {
            crate::persistence::TenantPersistenceExecutor::Redb($storage) => $body,
            crate::persistence::TenantPersistenceExecutor::Sqlite($storage) => $body,
            crate::persistence::TenantPersistenceExecutor::LibsqlReplica($storage) => $body,
            crate::persistence::TenantPersistenceExecutor::Postgres($storage) => $body,
            crate::persistence::TenantPersistenceExecutor::MySql($storage) => $body,
        }
    };
    ($value:expr, |$wrap:ident, $storage:ident| $body:expr) => {
        match $value {
            crate::persistence::TenantPersistenceExecutor::Redb($storage) => {
                let $wrap = crate::persistence::TenantPersistence::Redb;
                $body
            }
            crate::persistence::TenantPersistenceExecutor::Sqlite($storage) => {
                let $wrap = crate::persistence::TenantPersistence::Sqlite;
                $body
            }
            crate::persistence::TenantPersistenceExecutor::LibsqlReplica($storage) => {
                let $wrap = crate::persistence::TenantPersistence::LibsqlReplica;
                $body
            }
            crate::persistence::TenantPersistenceExecutor::Postgres($storage) => {
                let $wrap = crate::persistence::TenantPersistence::Postgres;
                $body
            }
            crate::persistence::TenantPersistenceExecutor::MySql($storage) => {
                let $wrap = crate::persistence::TenantPersistence::MySql;
                $body
            }
        }
    };
}

macro_rules! match_tenant_persistence_snapshot {
    ($value:expr, |$snapshot:ident| $body:expr) => {
        match $value {
            crate::persistence::TenantPersistenceSnapshot::Redb($snapshot) => $body,
            crate::persistence::TenantPersistenceSnapshot::Sqlite(snapshot) => {
                let guard = snapshot
                    .lock()
                    .expect("sqlite read snapshot lock should not be poisoned");
                let $snapshot = &*guard;
                $body
            }
            crate::persistence::TenantPersistenceSnapshot::LibsqlReplica(snapshot) => {
                let guard = snapshot
                    .lock()
                    .expect("sqlite read snapshot lock should not be poisoned");
                let $snapshot = &*guard;
                $body
            }
            crate::persistence::TenantPersistenceSnapshot::Postgres($snapshot) => $body,
            crate::persistence::TenantPersistenceSnapshot::MySql($snapshot) => $body,
        }
    };
}

mod control;
mod executor;
mod provider;
mod query;
mod snapshot;
mod tenant;
mod write_ops;

pub(crate) use control::ControlPlaneProvider;
pub(crate) use executor::TenantPersistenceExecutor;
pub(crate) use provider::PersistenceProvider;
pub(crate) use snapshot::TenantPersistenceSnapshot;
pub(crate) use tenant::TenantPersistence;
pub(crate) use write_ops::TenantPersistenceWriteOps;
