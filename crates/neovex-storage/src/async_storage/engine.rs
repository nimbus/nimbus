use std::path::PathBuf;
use std::sync::Arc;

use neovex_core::{Error, Result, TenantId};
use tokio::runtime::Handle as TokioRuntimeHandle;

use crate::encryption::{
    KeyManifest, LocalKeyProvider, LocalKeySubject, ManifestCipher, resolve_database_encryption_key,
};
use crate::{Clock, FaultInjector, TenantStore};

use super::helpers::map_join_error;
use super::read::{RedbTenantStorage, default_tenant_read_parallelism};
use super::traits::EmbeddedPersistenceProvider;

/// Selects the retained embedded persistence provider from the composition root.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EmbeddedProviderKind {
    #[default]
    Sqlite,
    Redb,
}

impl EmbeddedProviderKind {
    pub const fn tenant_file_extension(self) -> &'static str {
        match self {
            Self::Redb => "redb",
            Self::Sqlite => "sqlite3",
        }
    }

    pub const fn control_database_filename(self) -> &'static str {
        match self {
            Self::Redb => "neovex-control.db",
            Self::Sqlite => "neovex-control.sqlite3",
        }
    }
}

pub struct OpenedEmbeddedRedbTenant {
    pub store: Arc<TenantStore>,
    pub read_storage: Arc<RedbTenantStorage>,
}

#[derive(Clone)]
pub struct EmbeddedRedbProvider {
    data_dir: PathBuf,
    clock: Arc<dyn Clock>,
    fault_injector: Arc<dyn FaultInjector>,
    storage_handle: TokioRuntimeHandle,
    tenant_read_parallelism: usize,
    encryption_provider: Option<Arc<dyn LocalKeyProvider>>,
}

impl EmbeddedRedbProvider {
    pub fn new(
        data_dir: impl Into<PathBuf>,
        clock: Arc<dyn Clock>,
        fault_injector: Arc<dyn FaultInjector>,
        storage_handle: TokioRuntimeHandle,
    ) -> Result<Self> {
        let data_dir = data_dir.into();
        Ok(Self {
            data_dir,
            clock,
            fault_injector,
            storage_handle,
            tenant_read_parallelism: default_tenant_read_parallelism(),
            encryption_provider: None,
        })
    }

    pub fn new_encrypted(
        data_dir: impl Into<PathBuf>,
        provider: Arc<dyn LocalKeyProvider>,
        clock: Arc<dyn Clock>,
        fault_injector: Arc<dyn FaultInjector>,
        storage_handle: TokioRuntimeHandle,
    ) -> Result<Self> {
        let data_dir = data_dir.into();
        Ok(Self {
            data_dir,
            clock,
            fault_injector,
            storage_handle,
            tenant_read_parallelism: default_tenant_read_parallelism(),
            encryption_provider: Some(provider),
        })
    }

    pub fn is_encrypted(&self) -> bool {
        self.encryption_provider.is_some()
    }

    pub fn read_storage_for_store(&self, store: Arc<TenantStore>) -> Arc<RedbTenantStorage> {
        Arc::new(RedbTenantStorage::with_max_concurrent_reads(
            store,
            self.storage_handle.clone(),
            self.tenant_read_parallelism,
        ))
    }

    pub async fn create_tenant(&self, tenant_id: &TenantId) -> Result<OpenedEmbeddedRedbTenant> {
        let path = self.tenant_path(tenant_id);
        if tokio::fs::try_exists(&path)
            .await
            .map_err(|error| Error::Internal(error.to_string()))?
        {
            return Err(Error::AlreadyExists(format!(
                "tenant already exists: {tenant_id}"
            )));
        }

        self.open_tenant_at_path(tenant_id.clone(), path).await
    }

    pub async fn open_existing_tenant(
        &self,
        tenant_id: &TenantId,
    ) -> Result<Option<OpenedEmbeddedRedbTenant>> {
        let path = self.tenant_path(tenant_id);
        if !tokio::fs::try_exists(&path)
            .await
            .map_err(|error| Error::Internal(error.to_string()))?
        {
            return Ok(None);
        }

        Ok(Some(
            self.open_tenant_at_path(tenant_id.clone(), path).await?,
        ))
    }

    pub async fn delete_tenant(&self, tenant_id: &TenantId) -> Result<()> {
        let path = self.tenant_path(tenant_id);
        tokio::fs::remove_file(&path)
            .await
            .map_err(|error| Error::Internal(error.to_string()))?;
        if self.encryption_provider.is_some() {
            let manifest_path = KeyManifest::manifest_path(&path);
            let _ = tokio::fs::remove_file(manifest_path).await;
        }
        Ok(())
    }

    pub async fn tenant_exists(&self, tenant_id: &TenantId) -> Result<bool> {
        tokio::fs::try_exists(self.tenant_path(tenant_id))
            .await
            .map_err(|error| Error::Internal(error.to_string()))
    }

    fn tenant_path(&self, tenant_id: &TenantId) -> PathBuf {
        self.data_dir.join(format!(
            "{}.{}",
            tenant_id.as_str(),
            EmbeddedProviderKind::Redb.tenant_file_extension()
        ))
    }

    async fn open_tenant_at_path(
        &self,
        tenant_id: TenantId,
        path: PathBuf,
    ) -> Result<OpenedEmbeddedRedbTenant> {
        let clock = self.clock.clone();
        let fault_injector = self.fault_injector.clone();
        let provider = self.encryption_provider.clone();
        let store = self
            .storage_handle
            .spawn_blocking(move || {
                if let Some(provider) = provider {
                    let logical_name = path
                        .file_name()
                        .map(|name| name.to_string_lossy().to_string())
                        .unwrap_or_else(|| "tenant.redb".to_string());
                    let subject = LocalKeySubject::redb_tenant(tenant_id, logical_name);
                    let dek = resolve_database_encryption_key(
                        &path,
                        provider.as_ref(),
                        &subject,
                        ManifestCipher::RedbAes256GcmSiv,
                    )?;
                    TenantStore::open_encrypted_with_simulation(path, &dek, clock, fault_injector)
                } else {
                    TenantStore::open_with_simulation(path, clock, fault_injector)
                }
            })
            .await
            .map_err(map_join_error)??;

        let store = Arc::new(store);
        let read_storage = self.read_storage_for_store(store.clone());
        Ok(OpenedEmbeddedRedbTenant {
            store,
            read_storage,
        })
    }
}

impl EmbeddedPersistenceProvider for EmbeddedRedbProvider {
    type TenantRead = RedbTenantStorage;

    async fn list_tenants(&self) -> Result<Vec<TenantId>> {
        let data_dir = self.data_dir.clone();
        self.storage_handle
            .spawn_blocking(move || {
                let mut tenants = Vec::new();
                let entries = std::fs::read_dir(&data_dir)
                    .map_err(|error| Error::Internal(error.to_string()))?;
                for entry in entries {
                    let entry = entry.map_err(|error| Error::Internal(error.to_string()))?;
                    let path = entry.path();
                    if path.extension().is_some_and(|extension| {
                        extension == EmbeddedProviderKind::Redb.tenant_file_extension()
                    }) && let Some(stem) = path.file_stem()
                    {
                        tenants.push(TenantId::new(stem.to_string_lossy().to_string())?);
                    }
                }
                tenants.sort();
                Ok(tenants)
            })
            .await
            .map_err(map_join_error)?
    }
}
