macro_rules! match_persistence_provider {
    ($value:expr, |$provider:ident| $body:expr) => {
        match $value {
            crate::persistence::PersistenceProvider::Redb($provider) => $body,
            crate::persistence::PersistenceProvider::Sqlite($provider) => $body,
            crate::persistence::PersistenceProvider::LibsqlReplica($provider) => $body,
            crate::persistence::PersistenceProvider::Postgres($provider) => $body,
            crate::persistence::PersistenceProvider::MySql($provider) => $body,
            #[cfg(any(test, feature = "test-hooks"))]
            crate::persistence::PersistenceProvider::Memory($provider) => $body,
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
            #[cfg(any(test, feature = "test-hooks"))]
            crate::persistence::TenantPersistence::Memory($store) => $body,
        }
    };
}

macro_rules! match_tenant_persistence_executor {
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
            #[cfg(any(test, feature = "test-hooks"))]
            crate::persistence::TenantPersistenceExecutor::Memory($storage) => {
                let $wrap = crate::persistence::TenantPersistence::Memory;
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
            #[cfg(any(test, feature = "test-hooks"))]
            crate::persistence::TenantPersistenceSnapshot::Memory($snapshot) => $body,
        }
    };
}

mod control;
mod executor;
mod provider;
mod query;
mod read_capabilities;
mod runtime_hooks;
mod snapshot;
mod tenant;

pub(crate) use control::ControlPlaneProvider;
pub(crate) use executor::TenantPersistenceExecutor;
pub(crate) use provider::PersistenceProvider;
pub(crate) use runtime_hooks::{
    LibsqlReplicaRuntimeHooks, MySqlRuntimeHooks, PostgresRuntimeHooks, RuntimeHooks, WorkerContext,
};
pub(crate) use snapshot::TenantPersistenceSnapshot;
pub(crate) use tenant::TenantPersistence;
