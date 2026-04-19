use std::sync::Arc;

use neovex_core::{Error, Result, TenantId};
use neovex_storage::{
    EmbeddedPersistenceProvider, EmbeddedRedbProvider, EmbeddedSqliteProvider,
    LibsqlReplicaProvider, MySqlProvider, OpenedEmbeddedRedbTenant, OpenedEmbeddedSqliteTenant,
    OpenedLibsqlReplicaTenant, OpenedMySqlTenant, OpenedPostgresTenant, PostgresProvider,
};

use super::{TenantPersistence, TenantPersistenceExecutor};

#[derive(Clone)]
pub(crate) enum PersistenceProvider {
    Redb(Arc<EmbeddedRedbProvider>),
    Sqlite(Arc<EmbeddedSqliteProvider>),
    LibsqlReplica(Arc<LibsqlReplicaProvider>),
    Postgres(Arc<PostgresProvider>),
    MySql(Arc<MySqlProvider>),
}

pub(crate) struct OpenedTenantPersistence {
    pub persistence: TenantPersistence,
    pub executor: TenantPersistenceExecutor,
}

impl PersistenceProvider {
    pub(crate) async fn list_tenants(&self) -> Result<Vec<TenantId>> {
        match self {
            Self::Redb(engine) => engine.list_tenants().await,
            Self::Sqlite(engine) => engine.list_tenants().await,
            Self::LibsqlReplica(engine) => engine.list_tenants().await,
            Self::Postgres(engine) => engine.list_tenants().await,
            Self::MySql(engine) => engine.list_tenants().await,
        }
    }

    pub(crate) async fn create_tenant(
        &self,
        tenant_id: &TenantId,
    ) -> Result<OpenedTenantPersistence> {
        match self {
            Self::Redb(engine) => map_opened_redb_tenant(engine.create_tenant(tenant_id).await),
            Self::Sqlite(engine) => map_opened_sqlite_tenant(engine.create_tenant(tenant_id).await),
            Self::LibsqlReplica(engine) => {
                map_opened_libsql_replica_tenant(engine.create_opened_tenant(tenant_id).await)
            }
            Self::Postgres(engine) => {
                map_opened_postgres_tenant(engine.create_opened_tenant(tenant_id).await)
            }
            Self::MySql(engine) => {
                map_opened_mysql_tenant(engine.create_opened_tenant(tenant_id).await)
            }
        }
    }

    pub(crate) async fn open_existing_tenant(
        &self,
        tenant_id: &TenantId,
    ) -> Result<Option<OpenedTenantPersistence>> {
        match self {
            Self::Redb(engine) => engine
                .open_existing_tenant(tenant_id)
                .await
                .map(|opened| opened.map(map_opened_redb_tenant_sync)),
            Self::Sqlite(engine) => engine
                .open_existing_tenant(tenant_id)
                .await
                .map(|opened| opened.map(map_opened_sqlite_tenant_sync)),
            Self::LibsqlReplica(engine) => engine
                .open_existing_opened_tenant(tenant_id)
                .await
                .map(|opened| opened.map(map_opened_libsql_replica_tenant_sync)),
            Self::Postgres(engine) => engine
                .open_existing_opened_tenant(tenant_id)
                .await
                .map(|opened| opened.map(map_opened_postgres_tenant_sync)),
            Self::MySql(engine) => engine
                .open_existing_opened_tenant(tenant_id)
                .await
                .map(|opened| opened.map(map_opened_mysql_tenant_sync)),
        }
    }

    pub(crate) async fn delete_tenant(&self, tenant_id: &TenantId) -> Result<()> {
        match self {
            Self::Redb(engine) => engine.delete_tenant(tenant_id).await,
            Self::Sqlite(engine) => engine.delete_tenant(tenant_id).await,
            Self::LibsqlReplica(engine) => engine.delete_tenant(tenant_id).await,
            Self::Postgres(engine) => engine.delete_tenant(tenant_id).await,
            Self::MySql(engine) => engine.delete_tenant(tenant_id).await,
        }
    }

    pub(crate) async fn tenant_exists(&self, tenant_id: &TenantId) -> Result<bool> {
        match self {
            Self::Redb(engine) => engine.tenant_exists(tenant_id).await,
            Self::Sqlite(engine) => engine.tenant_exists(tenant_id).await,
            Self::LibsqlReplica(engine) => engine.tenant_exists(tenant_id).await,
            Self::Postgres(engine) => engine.tenant_exists(tenant_id).await,
            Self::MySql(engine) => engine.tenant_exists(tenant_id).await,
        }
    }

    pub(crate) fn read_storage_for_store(
        &self,
        store: TenantPersistence,
    ) -> Result<TenantPersistenceExecutor> {
        match (self, store) {
            (Self::Redb(engine), TenantPersistence::Redb(store)) => Ok(
                TenantPersistenceExecutor::Redb(engine.read_storage_for_store(store)),
            ),
            (Self::Sqlite(engine), TenantPersistence::Sqlite(store)) => Ok(
                TenantPersistenceExecutor::Sqlite(engine.read_storage_for_store(store)),
            ),
            (Self::LibsqlReplica(engine), TenantPersistence::LibsqlReplica(store)) => Ok(
                TenantPersistenceExecutor::LibsqlReplica(engine.read_storage_for_store(store)),
            ),
            (Self::Postgres(engine), TenantPersistence::Postgres(store)) => Ok(
                TenantPersistenceExecutor::Postgres(engine.read_storage_for_store(store)),
            ),
            (Self::MySql(engine), TenantPersistence::MySql(store)) => Ok(
                TenantPersistenceExecutor::MySql(engine.read_storage_for_store(store)),
            ),
            _ => Err(Error::Internal(
                "persistence provider and tenant persistence mismatch".to_string(),
            )),
        }
    }
}

fn map_opened_redb_tenant(
    result: Result<OpenedEmbeddedRedbTenant>,
) -> Result<OpenedTenantPersistence> {
    result.map(map_opened_redb_tenant_sync)
}

fn map_opened_redb_tenant_sync(opened: OpenedEmbeddedRedbTenant) -> OpenedTenantPersistence {
    OpenedTenantPersistence {
        persistence: TenantPersistence::Redb(opened.store),
        executor: TenantPersistenceExecutor::Redb(opened.read_storage),
    }
}

fn map_opened_sqlite_tenant(
    result: Result<OpenedEmbeddedSqliteTenant>,
) -> Result<OpenedTenantPersistence> {
    result.map(map_opened_sqlite_tenant_sync)
}

fn map_opened_sqlite_tenant_sync(opened: OpenedEmbeddedSqliteTenant) -> OpenedTenantPersistence {
    OpenedTenantPersistence {
        persistence: TenantPersistence::Sqlite(opened.store),
        executor: TenantPersistenceExecutor::Sqlite(opened.read_storage),
    }
}

fn map_opened_libsql_replica_tenant(
    result: Result<OpenedLibsqlReplicaTenant>,
) -> Result<OpenedTenantPersistence> {
    result.map(map_opened_libsql_replica_tenant_sync)
}

fn map_opened_libsql_replica_tenant_sync(
    opened: OpenedLibsqlReplicaTenant,
) -> OpenedTenantPersistence {
    OpenedTenantPersistence {
        persistence: TenantPersistence::LibsqlReplica(opened.store),
        executor: TenantPersistenceExecutor::LibsqlReplica(opened.read_storage),
    }
}

fn map_opened_postgres_tenant(
    result: Result<OpenedPostgresTenant>,
) -> Result<OpenedTenantPersistence> {
    result.map(map_opened_postgres_tenant_sync)
}

fn map_opened_postgres_tenant_sync(opened: OpenedPostgresTenant) -> OpenedTenantPersistence {
    OpenedTenantPersistence {
        persistence: TenantPersistence::Postgres(opened.store),
        executor: TenantPersistenceExecutor::Postgres(opened.read_storage),
    }
}

fn map_opened_mysql_tenant(result: Result<OpenedMySqlTenant>) -> Result<OpenedTenantPersistence> {
    result.map(map_opened_mysql_tenant_sync)
}

fn map_opened_mysql_tenant_sync(opened: OpenedMySqlTenant) -> OpenedTenantPersistence {
    OpenedTenantPersistence {
        persistence: TenantPersistence::MySql(opened.store),
        executor: TenantPersistenceExecutor::MySql(opened.read_storage),
    }
}
