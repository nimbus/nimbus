use std::sync::Arc;

use neovex_core::{Result, TenantId};
use neovex_storage::async_storage::{OpenedEmbeddedRedbTenant, OpenedEmbeddedSqliteTenant};
use neovex_storage::libsql::OpenedLibsqlReplicaTenant;
use neovex_storage::mysql::OpenedMySqlTenant;
use neovex_storage::postgres::OpenedPostgresTenant;
use neovex_storage::{
    EmbeddedPersistenceProvider, EmbeddedRedbProvider, EmbeddedSqliteProvider,
    LibsqlReplicaProvider, MySqlProvider, PostgresProvider,
};
use tokio_util::sync::CancellationToken;

use super::{TenantPersistence, TenantPersistenceExecutor};
use crate::service::Service;

#[derive(Clone)]
pub(crate) enum PersistenceProvider {
    Redb(Arc<EmbeddedRedbProvider>),
    Sqlite(Arc<EmbeddedSqliteProvider>),
    LibsqlReplica(Arc<LibsqlReplicaProvider>),
    Postgres(Arc<PostgresProvider>),
    MySql(Arc<MySqlProvider>),
}

#[derive(Clone)]
pub(crate) enum ProviderBackgroundTask {
    Postgres(Arc<PostgresProvider>),
    LibsqlReplica(Arc<LibsqlReplicaProvider>),
    MySql(Arc<MySqlProvider>),
}

pub(crate) struct OpenedTenantPersistence {
    pub persistence: TenantPersistence,
    pub executor: TenantPersistenceExecutor,
}

trait OpenedTenantProvider {
    type OpenedTenant;

    async fn create_opened_tenant(&self, tenant_id: &TenantId) -> Result<Self::OpenedTenant>;

    async fn open_existing_opened_tenant(
        &self,
        tenant_id: &TenantId,
    ) -> Result<Option<Self::OpenedTenant>>;
}

// Keep provider dispatch typed for now: the engine still needs background-task
// ownership plus provider-specific opened-tenant shapes without introducing an
// erased provider object layer that would just rewrap the same enum split.

impl PersistenceProvider {
    pub(crate) fn background_task(&self) -> Option<ProviderBackgroundTask> {
        match self {
            Self::Postgres(provider) => Some(ProviderBackgroundTask::Postgres(provider.clone())),
            Self::LibsqlReplica(provider) => {
                Some(ProviderBackgroundTask::LibsqlReplica(provider.clone()))
            }
            Self::MySql(provider) => Some(ProviderBackgroundTask::MySql(provider.clone())),
            Self::Redb(_) | Self::Sqlite(_) => None,
        }
    }

    pub(crate) async fn list_tenants(&self) -> Result<Vec<TenantId>> {
        match_persistence_provider!(self, |provider| provider.list_tenants().await)
    }

    pub(crate) async fn create_tenant(
        &self,
        tenant_id: &TenantId,
    ) -> Result<OpenedTenantPersistence> {
        match_persistence_provider!(self, |provider| {
            create_opened_tenant(provider.as_ref(), tenant_id).await
        })
    }

    pub(crate) async fn open_existing_tenant(
        &self,
        tenant_id: &TenantId,
    ) -> Result<Option<OpenedTenantPersistence>> {
        match_persistence_provider!(self, |provider| {
            open_existing_opened_tenant(provider.as_ref(), tenant_id).await
        })
    }

    pub(crate) async fn delete_tenant(&self, tenant_id: &TenantId) -> Result<()> {
        match_persistence_provider!(self, |provider| provider.delete_tenant(tenant_id).await)
    }

    pub(crate) async fn tenant_exists(&self, tenant_id: &TenantId) -> Result<bool> {
        match_persistence_provider!(self, |provider| provider.tenant_exists(tenant_id).await)
    }

    pub(crate) fn read_storage_for_store(
        &self,
        store: TenantPersistence,
    ) -> Result<TenantPersistenceExecutor> {
        store.read_storage_for_provider(self)
    }
}

impl ProviderBackgroundTask {
    pub(crate) fn task_name(&self) -> &'static str {
        match self {
            Self::Postgres(_) => "postgres_provider_hints",
            Self::LibsqlReplica(_) => "libsql_replica_provider_poll",
            Self::MySql(_) => "mysql_provider_poll",
        }
    }

    pub(crate) async fn run(self, service: Arc<Service>, shutdown: CancellationToken) {
        match self {
            Self::Postgres(provider) => {
                service
                    .run_postgres_provider_hint_worker(provider, shutdown)
                    .await;
            }
            Self::LibsqlReplica(provider) => {
                service
                    .run_libsql_replica_provider_poll_worker(provider, shutdown)
                    .await;
            }
            Self::MySql(provider) => {
                service
                    .run_mysql_provider_poll_worker(provider, shutdown)
                    .await;
            }
        }
    }
}

async fn create_opened_tenant<P>(
    provider: &P,
    tenant_id: &TenantId,
) -> Result<OpenedTenantPersistence>
where
    P: OpenedTenantProvider + ?Sized,
    OpenedTenantPersistence: From<P::OpenedTenant>,
{
    provider
        .create_opened_tenant(tenant_id)
        .await
        .map(Into::into)
}

async fn open_existing_opened_tenant<P>(
    provider: &P,
    tenant_id: &TenantId,
) -> Result<Option<OpenedTenantPersistence>>
where
    P: OpenedTenantProvider + ?Sized,
    OpenedTenantPersistence: From<P::OpenedTenant>,
{
    provider
        .open_existing_opened_tenant(tenant_id)
        .await
        .map(|opened| opened.map(Into::into))
}

impl OpenedTenantProvider for EmbeddedRedbProvider {
    type OpenedTenant = OpenedEmbeddedRedbTenant;

    async fn create_opened_tenant(&self, tenant_id: &TenantId) -> Result<Self::OpenedTenant> {
        self.create_tenant(tenant_id).await
    }

    async fn open_existing_opened_tenant(
        &self,
        tenant_id: &TenantId,
    ) -> Result<Option<Self::OpenedTenant>> {
        self.open_existing_tenant(tenant_id).await
    }
}

impl OpenedTenantProvider for EmbeddedSqliteProvider {
    type OpenedTenant = OpenedEmbeddedSqliteTenant;

    async fn create_opened_tenant(&self, tenant_id: &TenantId) -> Result<Self::OpenedTenant> {
        self.create_tenant(tenant_id).await
    }

    async fn open_existing_opened_tenant(
        &self,
        tenant_id: &TenantId,
    ) -> Result<Option<Self::OpenedTenant>> {
        self.open_existing_tenant(tenant_id).await
    }
}

impl OpenedTenantProvider for LibsqlReplicaProvider {
    type OpenedTenant = OpenedLibsqlReplicaTenant;

    async fn create_opened_tenant(&self, tenant_id: &TenantId) -> Result<Self::OpenedTenant> {
        LibsqlReplicaProvider::create_opened_tenant(self, tenant_id).await
    }

    async fn open_existing_opened_tenant(
        &self,
        tenant_id: &TenantId,
    ) -> Result<Option<Self::OpenedTenant>> {
        LibsqlReplicaProvider::open_existing_opened_tenant(self, tenant_id).await
    }
}

impl OpenedTenantProvider for PostgresProvider {
    type OpenedTenant = OpenedPostgresTenant;

    async fn create_opened_tenant(&self, tenant_id: &TenantId) -> Result<Self::OpenedTenant> {
        PostgresProvider::create_opened_tenant(self, tenant_id).await
    }

    async fn open_existing_opened_tenant(
        &self,
        tenant_id: &TenantId,
    ) -> Result<Option<Self::OpenedTenant>> {
        PostgresProvider::open_existing_opened_tenant(self, tenant_id).await
    }
}

impl OpenedTenantProvider for MySqlProvider {
    type OpenedTenant = OpenedMySqlTenant;

    async fn create_opened_tenant(&self, tenant_id: &TenantId) -> Result<Self::OpenedTenant> {
        MySqlProvider::create_opened_tenant(self, tenant_id).await
    }

    async fn open_existing_opened_tenant(
        &self,
        tenant_id: &TenantId,
    ) -> Result<Option<Self::OpenedTenant>> {
        MySqlProvider::open_existing_opened_tenant(self, tenant_id).await
    }
}

impl From<OpenedEmbeddedRedbTenant> for OpenedTenantPersistence {
    fn from(opened: OpenedEmbeddedRedbTenant) -> Self {
        Self {
            persistence: TenantPersistence::Redb(opened.store),
            executor: TenantPersistenceExecutor::Redb(opened.read_storage),
        }
    }
}

impl From<OpenedEmbeddedSqliteTenant> for OpenedTenantPersistence {
    fn from(opened: OpenedEmbeddedSqliteTenant) -> Self {
        Self {
            persistence: TenantPersistence::Sqlite(opened.store),
            executor: TenantPersistenceExecutor::Sqlite(opened.read_storage),
        }
    }
}

impl From<OpenedLibsqlReplicaTenant> for OpenedTenantPersistence {
    fn from(opened: OpenedLibsqlReplicaTenant) -> Self {
        Self {
            persistence: TenantPersistence::LibsqlReplica(opened.store),
            executor: TenantPersistenceExecutor::LibsqlReplica(opened.read_storage),
        }
    }
}

impl From<OpenedPostgresTenant> for OpenedTenantPersistence {
    fn from(opened: OpenedPostgresTenant) -> Self {
        Self {
            persistence: TenantPersistence::Postgres(opened.store),
            executor: TenantPersistenceExecutor::Postgres(opened.read_storage),
        }
    }
}

impl From<OpenedMySqlTenant> for OpenedTenantPersistence {
    fn from(opened: OpenedMySqlTenant) -> Self {
        Self {
            persistence: TenantPersistence::MySql(opened.store),
            executor: TenantPersistenceExecutor::MySql(opened.read_storage),
        }
    }
}
