use std::sync::Arc;

use nimbus_core::{Result, TenantId};
use nimbus_storage::async_storage::{OpenedEmbeddedRedbTenant, OpenedEmbeddedSqliteTenant};
use nimbus_storage::libsql::OpenedLibsqlReplicaTenant;
use nimbus_storage::mysql::OpenedMySqlTenant;
use nimbus_storage::postgres::OpenedPostgresTenant;
use nimbus_storage::{
    EmbeddedRedbProvider, EmbeddedSqliteProvider, LibsqlReplicaProvider, MySqlProvider,
    PostgresProvider,
};
#[cfg(any(test, feature = "test-hooks"))]
use nimbus_storage::{MemoryTenantProvider, OpenedMemoryTenant};

use super::{
    LibsqlReplicaRuntimeHooks, MySqlRuntimeHooks, PostgresRuntimeHooks, RuntimeHooks,
    TenantPersistence, TenantPersistenceExecutor,
};

#[derive(Clone)]
pub(crate) enum PersistenceProvider {
    Redb(Arc<EmbeddedRedbProvider>),
    Sqlite(Arc<EmbeddedSqliteProvider>),
    LibsqlReplica(Arc<LibsqlReplicaProvider>),
    Postgres(Arc<PostgresProvider>),
    MySql(Arc<MySqlProvider>),
    #[cfg(any(test, feature = "test-hooks"))]
    Memory(Arc<MemoryTenantProvider>),
}

pub(crate) struct OpenedTenantPersistence {
    pub persistence: TenantPersistence,
    pub executor: TenantPersistenceExecutor,
    /// Provider-global generation for external persistence. Embedded adapters
    /// obtain the same invariant from the engine control plane.
    pub incarnation: Option<u64>,
}

pub(crate) struct TenantPage {
    pub tenant_ids: Vec<TenantId>,
    pub next_after: Option<TenantId>,
}

impl TenantPage {
    fn from_ordered(tenant_ids: Vec<TenantId>, limit: usize) -> Result<Self> {
        if tenant_ids.len() > limit {
            return Err(nimbus_core::Error::Internal(format!(
                "provider tenant page returned {} rows above its {limit}-row limit",
                tenant_ids.len()
            )));
        }
        if tenant_ids.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(nimbus_core::Error::Internal(
                "provider tenant page must be strictly ordered by tenant id".to_string(),
            ));
        }
        let next_after = (tenant_ids.len() == limit).then(|| {
            tenant_ids
                .last()
                .expect("a full provider tenant page must have a last tenant")
                .clone()
        });
        Ok(Self {
            tenant_ids,
            next_after,
        })
    }
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
    pub(crate) fn owns_tenant_incarnations(&self) -> bool {
        matches!(
            self,
            Self::Postgres(_) | Self::LibsqlReplica(_) | Self::MySql(_)
        )
    }

    pub(crate) fn runtime_hooks(&self) -> Option<Box<dyn RuntimeHooks>> {
        match self {
            Self::Postgres(provider) => Some(Box::new(PostgresRuntimeHooks::new(provider.clone()))),
            Self::LibsqlReplica(_) => Some(Box::new(LibsqlReplicaRuntimeHooks)),
            Self::MySql(_) => Some(Box::new(MySqlRuntimeHooks)),
            Self::Redb(_) | Self::Sqlite(_) => None,
            #[cfg(any(test, feature = "test-hooks"))]
            Self::Memory(_) => None,
        }
    }

    pub(crate) async fn list_tenants(&self) -> Result<Vec<TenantId>> {
        match_persistence_provider!(self, |provider| provider.list_tenants().await)
    }

    /// Lists one stable tenant-id page for bounded provider sweeps.
    ///
    /// External adapters push the cursor and limit into their metadata query.
    /// Embedded adapters do not run the provider poll worker, so their fallback
    /// preserves the same ordering contract without expanding their file
    /// registry interface.
    pub(crate) async fn list_tenants_page(
        &self,
        after: Option<&TenantId>,
        limit: usize,
    ) -> Result<TenantPage> {
        if limit == 0 {
            return Err(nimbus_core::Error::InvalidInput(
                "tenant page limit must be greater than zero".to_string(),
            ));
        }
        let tenant_ids = match self {
            Self::Postgres(provider) => provider.list_tenants_page(after, limit).await,
            Self::MySql(provider) => provider.list_tenants_page(after, limit).await,
            Self::LibsqlReplica(provider) => provider.list_tenants_page(after, limit).await,
            Self::Redb(_) | Self::Sqlite(_) => {
                let tenants = self.list_tenants().await?;
                Ok(tenants
                    .into_iter()
                    .filter(|tenant_id| after.is_none_or(|after| tenant_id > after))
                    .take(limit)
                    .collect())
            }
            #[cfg(any(test, feature = "test-hooks"))]
            Self::Memory(_) => {
                let tenants = self.list_tenants().await?;
                Ok(tenants
                    .into_iter()
                    .filter(|tenant_id| after.is_none_or(|after| tenant_id > after))
                    .take(limit)
                    .collect())
            }
        }?;
        TenantPage::from_ordered(tenant_ids, limit)
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

    /// Opens an unloaded tenant only when durable scheduler state requires a runtime.
    ///
    /// libSQL performs the predicate before its expensive remote-snapshot and
    /// local-replica bootstrap. Other adapters retain their existing open then
    /// inspect behavior behind the same engine-owned lifecycle interface.
    pub(crate) async fn open_existing_tenant_with_scheduled_work(
        &self,
        tenant_id: &TenantId,
    ) -> Result<Option<OpenedTenantPersistence>> {
        if let Self::LibsqlReplica(provider) = self {
            return provider
                .open_existing_opened_tenant_with_scheduled_work(tenant_id)
                .await
                .map(|opened| opened.map(Into::into));
        }

        let Some(opened) = self.open_existing_tenant(tenant_id).await? else {
            return Ok(None);
        };
        if !opened
            .persistence
            .has_scheduled_work_async(&opened.executor)
            .await?
        {
            return Ok(None);
        }
        Ok(Some(opened))
    }

    pub(crate) async fn delete_tenant(&self, tenant_id: &TenantId) -> Result<()> {
        match_persistence_provider!(self, |provider| provider.delete_tenant(tenant_id).await)
    }

    /// Retires provider-global transport state after every tenant runtime and
    /// engine-owned worker has drained.
    pub(crate) async fn retire_after_drain(&self) -> Result<()> {
        match self {
            Self::LibsqlReplica(provider) => provider.retire_after_drain().await,
            Self::Redb(_) | Self::Sqlite(_) | Self::Postgres(_) | Self::MySql(_) => Ok(()),
            #[cfg(any(test, feature = "test-hooks"))]
            Self::Memory(_) => Ok(()),
        }
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
        self.create_tenant(tenant_id) // tenant-lifecycle: provider-adapter-internal
            .await
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
        self.create_tenant(tenant_id) // tenant-lifecycle: provider-adapter-internal
            .await
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

#[cfg(any(test, feature = "test-hooks"))]
impl OpenedTenantProvider for MemoryTenantProvider {
    type OpenedTenant = OpenedMemoryTenant;

    async fn create_opened_tenant(&self, tenant_id: &TenantId) -> Result<Self::OpenedTenant> {
        MemoryTenantProvider::create_opened_tenant(self, tenant_id).await
    }

    async fn open_existing_opened_tenant(
        &self,
        tenant_id: &TenantId,
    ) -> Result<Option<Self::OpenedTenant>> {
        MemoryTenantProvider::open_existing_opened_tenant(self, tenant_id).await
    }
}

impl From<OpenedEmbeddedRedbTenant> for OpenedTenantPersistence {
    fn from(opened: OpenedEmbeddedRedbTenant) -> Self {
        Self {
            persistence: TenantPersistence::Redb(opened.store),
            executor: TenantPersistenceExecutor::Redb(opened.read_storage),
            incarnation: None,
        }
    }
}

impl From<OpenedEmbeddedSqliteTenant> for OpenedTenantPersistence {
    fn from(opened: OpenedEmbeddedSqliteTenant) -> Self {
        Self {
            persistence: TenantPersistence::Sqlite(opened.store),
            executor: TenantPersistenceExecutor::Sqlite(opened.read_storage),
            incarnation: None,
        }
    }
}

impl From<OpenedLibsqlReplicaTenant> for OpenedTenantPersistence {
    fn from(opened: OpenedLibsqlReplicaTenant) -> Self {
        Self {
            persistence: TenantPersistence::LibsqlReplica(opened.store),
            executor: TenantPersistenceExecutor::LibsqlReplica(opened.read_storage),
            incarnation: Some(opened.incarnation),
        }
    }
}

impl From<OpenedPostgresTenant> for OpenedTenantPersistence {
    fn from(opened: OpenedPostgresTenant) -> Self {
        Self {
            persistence: TenantPersistence::Postgres(opened.store),
            executor: TenantPersistenceExecutor::Postgres(opened.read_storage),
            incarnation: Some(opened.incarnation),
        }
    }
}

impl From<OpenedMySqlTenant> for OpenedTenantPersistence {
    fn from(opened: OpenedMySqlTenant) -> Self {
        Self {
            persistence: TenantPersistence::MySql(opened.store),
            executor: TenantPersistenceExecutor::MySql(opened.read_storage),
            incarnation: Some(opened.incarnation),
        }
    }
}

#[cfg(any(test, feature = "test-hooks"))]
impl From<OpenedMemoryTenant> for OpenedTenantPersistence {
    fn from(opened: OpenedMemoryTenant) -> Self {
        Self {
            persistence: TenantPersistence::Memory(opened.store),
            executor: TenantPersistenceExecutor::Memory(opened.read_storage),
            incarnation: None,
        }
    }
}

#[cfg(test)]
mod tenant_page_tests {
    use super::*;

    fn tenant(name: &str) -> TenantId {
        TenantId::new(name).expect("test tenant id should parse")
    }

    #[test]
    fn full_ordered_page_advances_from_last_tenant() {
        let first = tenant("tenant-a");
        let second = tenant("tenant-b");
        let page = TenantPage::from_ordered(vec![first.clone(), second.clone()], 2)
            .expect("ordered full page should build");

        assert_eq!(page.tenant_ids, vec![first, second.clone()]);
        assert_eq!(page.next_after, Some(second));
    }

    #[test]
    fn short_page_ends_current_sweep() {
        let first = tenant("tenant-a");
        let page = TenantPage::from_ordered(vec![first.clone()], 2)
            .expect("ordered short page should build");

        assert_eq!(page.tenant_ids, vec![first]);
        assert_eq!(page.next_after, None);
    }

    #[test]
    fn page_rejects_over_limit_and_non_increasing_provider_results() {
        let first = tenant("tenant-a");
        let second = tenant("tenant-b");
        assert!(matches!(
            TenantPage::from_ordered(vec![first.clone(), second.clone()], 1),
            Err(nimbus_core::Error::Internal(_))
        ));
        assert!(matches!(
            TenantPage::from_ordered(vec![second, first.clone()], 2),
            Err(nimbus_core::Error::Internal(_))
        ));
        assert!(matches!(
            TenantPage::from_ordered(vec![first.clone(), first], 2),
            Err(nimbus_core::Error::Internal(_))
        ));
    }
}
