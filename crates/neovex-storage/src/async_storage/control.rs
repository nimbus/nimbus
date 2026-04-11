use std::path::PathBuf;
use std::sync::Arc;

use neovex_core::Result;
use tokio::runtime::Handle as TokioRuntimeHandle;

use crate::UsageStore;

use super::engine::EmbeddedProviderKind;
use super::read::RedbUsageStorage;

/// Explicit control-plane provider for the retained embedded redb usage store.
///
/// This is intentionally separate from tenant persistence so the cross-tenant
/// control path can evolve without smuggling usage-store construction through
/// the tenant provider role.
#[derive(Clone)]
pub struct EmbeddedRedbControlPlaneProvider {
    usage_storage: Arc<RedbUsageStorage>,
}

impl EmbeddedRedbControlPlaneProvider {
    pub fn new(data_dir: impl Into<PathBuf>, storage_handle: TokioRuntimeHandle) -> Result<Self> {
        let data_dir = data_dir.into();
        let usage_store = Arc::new(UsageStore::open(
            data_dir.join(EmbeddedProviderKind::Redb.control_database_filename()),
        )?);
        Ok(Self {
            usage_storage: Arc::new(RedbUsageStorage::new(usage_store, storage_handle)),
        })
    }

    pub fn usage_store(&self) -> Arc<UsageStore> {
        self.usage_storage.store()
    }

    pub fn usage_storage(&self) -> Arc<RedbUsageStorage> {
        self.usage_storage.clone()
    }
}
