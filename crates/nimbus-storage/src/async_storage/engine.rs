use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, Weak};

use nimbus_core::{Error, IdSource, Result, SystemIdSource, TenantId, WallClock};
use tokio::runtime::Handle as TokioRuntimeHandle;

use crate::{FaultInjector, TenantStore};
use nimbus_crypto::{
    KeyManifest, LocalKeyProvider, LocalKeySubject, ManifestCipher, resolve_subject_encryption_key,
};

use super::read::{RedbTenantStorage, default_tenant_read_parallelism};
use super::task_error::map_join_error;

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
            Self::Redb => "nimbus-control.db",
            Self::Sqlite => "nimbus-control.sqlite3",
        }
    }
}

pub struct OpenedEmbeddedRedbTenant {
    pub store: Arc<TenantStore>,
    pub read_storage: Arc<RedbTenantStorage>,
}

#[derive(Default)]
struct RedbTenantOpenRegistry {
    stores: Mutex<HashMap<TenantId, Weak<TenantStore>>>,
    gates: Mutex<HashMap<TenantId, Weak<tokio::sync::Mutex<()>>>>,
}

impl RedbTenantOpenRegistry {
    fn gate(&self, tenant_id: &TenantId) -> Arc<tokio::sync::Mutex<()>> {
        let mut gates = self
            .gates
            .lock()
            .expect("redb tenant open-gate registry should not be poisoned");
        if let Some(gate) = gates.get(tenant_id).and_then(Weak::upgrade) {
            return gate;
        }
        let gate = Arc::new(tokio::sync::Mutex::new(()));
        gates.insert(tenant_id.clone(), Arc::downgrade(&gate));
        gate
    }

    fn live_store(&self, tenant_id: &TenantId) -> Option<Arc<TenantStore>> {
        self.stores
            .lock()
            .expect("redb tenant store registry should not be poisoned")
            .get(tenant_id)
            .and_then(Weak::upgrade)
    }

    fn remember(&self, tenant_id: TenantId, store: &Arc<TenantStore>) {
        self.stores
            .lock()
            .expect("redb tenant store registry should not be poisoned")
            .insert(tenant_id, Arc::downgrade(store));
    }

    fn forget(&self, tenant_id: &TenantId) {
        self.stores
            .lock()
            .expect("redb tenant store registry should not be poisoned")
            .remove(tenant_id);
    }
}

#[derive(Clone)]
pub struct EmbeddedRedbProvider {
    data_dir: PathBuf,
    clock: Arc<dyn WallClock>,
    fault_injector: Arc<dyn FaultInjector>,
    id_source: Arc<dyn IdSource>,
    storage_handle: TokioRuntimeHandle,
    tenant_read_parallelism: usize,
    encryption_provider: Option<Arc<dyn LocalKeyProvider>>,
    open_registry: Arc<RedbTenantOpenRegistry>,
}

impl EmbeddedRedbProvider {
    pub fn new(
        data_dir: impl Into<PathBuf>,
        clock: Arc<dyn WallClock>,
        fault_injector: Arc<dyn FaultInjector>,
        storage_handle: TokioRuntimeHandle,
    ) -> Result<Self> {
        Self::new_with_id_source(
            data_dir,
            clock,
            fault_injector,
            storage_handle,
            Arc::new(SystemIdSource),
        )
    }

    pub fn new_with_id_source(
        data_dir: impl Into<PathBuf>,
        clock: Arc<dyn WallClock>,
        fault_injector: Arc<dyn FaultInjector>,
        storage_handle: TokioRuntimeHandle,
        id_source: Arc<dyn IdSource>,
    ) -> Result<Self> {
        let data_dir = data_dir.into();
        Ok(Self {
            data_dir,
            clock,
            fault_injector,
            id_source,
            storage_handle,
            tenant_read_parallelism: default_tenant_read_parallelism(),
            encryption_provider: None,
            open_registry: Arc::new(RedbTenantOpenRegistry::default()),
        })
    }

    pub fn new_encrypted(
        data_dir: impl Into<PathBuf>,
        provider: Arc<dyn LocalKeyProvider>,
        clock: Arc<dyn WallClock>,
        fault_injector: Arc<dyn FaultInjector>,
        storage_handle: TokioRuntimeHandle,
    ) -> Result<Self> {
        Self::new_encrypted_with_id_source(
            data_dir,
            provider,
            clock,
            fault_injector,
            storage_handle,
            Arc::new(SystemIdSource),
        )
    }

    pub fn new_encrypted_with_id_source(
        data_dir: impl Into<PathBuf>,
        provider: Arc<dyn LocalKeyProvider>,
        clock: Arc<dyn WallClock>,
        fault_injector: Arc<dyn FaultInjector>,
        storage_handle: TokioRuntimeHandle,
        id_source: Arc<dyn IdSource>,
    ) -> Result<Self> {
        let data_dir = data_dir.into();
        Ok(Self {
            data_dir,
            clock,
            fault_injector,
            id_source,
            storage_handle,
            tenant_read_parallelism: default_tenant_read_parallelism(),
            encryption_provider: Some(provider),
            open_registry: Arc::new(RedbTenantOpenRegistry::default()),
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
        let open_gate = self.open_registry.gate(tenant_id);
        let _open_guard = open_gate.lock().await;
        let path = self.tenant_path(tenant_id);
        if self.open_registry.live_store(tenant_id).is_some()
            || tokio::fs::try_exists(&path)
                .await
                .map_err(|error| Error::Internal(error.to_string()))?
        {
            return Err(Error::AlreadyExists(format!(
                "tenant already exists: {tenant_id}"
            )));
        }

        self.open_tenant_at_path_locked(tenant_id.clone(), path)
            .await
    }

    pub async fn open_existing_tenant(
        &self,
        tenant_id: &TenantId,
    ) -> Result<Option<OpenedEmbeddedRedbTenant>> {
        let open_gate = self.open_registry.gate(tenant_id);
        let _open_guard = open_gate.lock().await;
        let path = self.tenant_path(tenant_id);
        if !tokio::fs::try_exists(&path)
            .await
            .map_err(|error| Error::Internal(error.to_string()))?
        {
            return Ok(None);
        }

        // redb rejects a second Database::open while any handle or read
        // snapshot for the same file remains alive. A durable-recovery runtime
        // restart must not depend on every stale, already-fenced Engine handle
        // being dropped at the exact registry handoff.
        if let Some(store) = self.open_registry.live_store(tenant_id) {
            let read_storage = self.read_storage_for_store(store.clone());
            return Ok(Some(OpenedEmbeddedRedbTenant {
                store,
                read_storage,
            }));
        }

        Ok(Some(
            self.open_tenant_at_path_locked(tenant_id.clone(), path)
                .await?,
        ))
    }

    pub async fn delete_tenant(&self, tenant_id: &TenantId) -> Result<()> {
        let open_gate = self.open_registry.gate(tenant_id);
        let _open_guard = open_gate.lock().await;
        let path = self.tenant_path(tenant_id);
        tokio::fs::remove_file(&path)
            .await
            .map_err(|error| Error::Internal(error.to_string()))?;
        self.open_registry.forget(tenant_id);
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

    pub async fn list_tenants(&self) -> Result<Vec<TenantId>> {
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

    fn tenant_path(&self, tenant_id: &TenantId) -> PathBuf {
        self.data_dir.join(format!(
            "{}.{}",
            tenant_id.as_str(),
            EmbeddedProviderKind::Redb.tenant_file_extension()
        ))
    }

    async fn open_tenant_at_path_locked(
        &self,
        tenant_id: TenantId,
        path: PathBuf,
    ) -> Result<OpenedEmbeddedRedbTenant> {
        // The caller holds this tenant's open gate across the full blocking
        // open, so unrelated tenants remain parallel while same-tenant create,
        // open, and delete cannot race one another.
        let clock = self.clock.clone();
        let fault_injector = crate::simulation::tenant_scoped_fault_injector(
            self.fault_injector.clone(),
            tenant_id.clone(),
        );
        let provider = self.encryption_provider.clone();
        let id_source = self.id_source.clone();
        let tenant_id_for_open = tenant_id.clone();
        let store = self
            .storage_handle
            .spawn_blocking(move || {
                if let Some(provider) = provider {
                    let logical_name = path
                        .file_name()
                        .map(|name| name.to_string_lossy().to_string())
                        .unwrap_or_else(|| "tenant.redb".to_string());
                    let subject = LocalKeySubject::redb_tenant(tenant_id_for_open, logical_name);
                    let dek = resolve_subject_encryption_key(
                        &path,
                        provider.as_ref(),
                        &subject,
                        ManifestCipher::RedbAes256GcmSiv,
                    )?;
                    TenantStore::open_encrypted_with_simulation_and_id_source(
                        path,
                        &dek,
                        clock,
                        fault_injector,
                        id_source,
                    )
                } else {
                    TenantStore::open_with_simulation_and_id_source(
                        path,
                        clock,
                        fault_injector,
                        id_source,
                    )
                }
            })
            .await
            .map_err(map_join_error)??;

        let store = Arc::new(store);
        self.open_registry.remember(tenant_id, &store);
        let read_storage = self.read_storage_for_store(store.clone());
        Ok(OpenedEmbeddedRedbTenant {
            store,
            read_storage,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nimbus_core::{SystemWallClock, TenantId};

    use super::*;
    use crate::simulation::NoopFaultInjector;

    #[tokio::test(flavor = "multi_thread")]
    async fn embedded_redb_provider_reuses_live_tenant_store() {
        let data_dir = tempfile::tempdir().expect("redb provider data dir should build");
        let provider = EmbeddedRedbProvider::new(
            data_dir.path(),
            Arc::new(SystemWallClock),
            Arc::new(NoopFaultInjector),
            tokio::runtime::Handle::current(),
        )
        .expect("redb provider should build");
        let tenant_id =
            TenantId::new("reuse-live-redb-store").expect("redb provider test tenant should parse");

        let first = provider
            .create_tenant(&tenant_id)
            .await
            .expect("first tenant open should succeed");
        let second = provider
            .open_existing_tenant(&tenant_id)
            .await
            .expect("reopen should not fail while the first handle remains live")
            .expect("created tenant should exist");

        assert!(
            Arc::ptr_eq(&first.store, &second.store),
            "one embedded provider must reuse its live Nimbus-owned redb tenant store"
        );
    }
}
