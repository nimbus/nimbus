use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use neovex_core::Result;
use parking_lot::Mutex;
use tokio::runtime::Handle as TokioRuntimeHandle;

use crate::UsageStore;
use crate::encryption::{
    LocalKeyProvider, LocalKeySubject, ManifestCipher, resolve_database_encryption_key,
};

use super::engine::EmbeddedProviderKind;
use super::read::RedbUsageStorage;

/// Explicit control-plane provider for the retained embedded redb usage store.
///
/// This is intentionally separate from tenant persistence so the cross-tenant
/// control path can evolve without smuggling usage-store construction through
/// the tenant provider role.
#[derive(Clone)]
pub struct EmbeddedRedbControlPlaneProvider {
    state: Arc<ControlPlaneState>,
}

struct ControlPlaneState {
    path: PathBuf,
    encryption: Option<ControlPlaneEncryption>,
    storage_handle: TokioRuntimeHandle,
    opened: Mutex<Option<OpenedControlPlane>>,
}

struct ControlPlaneEncryption {
    provider: Arc<dyn LocalKeyProvider>,
    subject: LocalKeySubject,
}

#[derive(Clone)]
struct OpenedControlPlane {
    usage_store: Arc<UsageStore>,
    usage_storage: Arc<RedbUsageStorage>,
}

impl EmbeddedRedbControlPlaneProvider {
    pub fn new(data_dir: impl Into<PathBuf>, storage_handle: TokioRuntimeHandle) -> Result<Self> {
        let data_dir = data_dir.into();
        Ok(Self {
            state: Arc::new(ControlPlaneState {
                path: data_dir.join(EmbeddedProviderKind::Redb.control_database_filename()),
                encryption: None,
                storage_handle,
                opened: Mutex::new(None),
            }),
        })
    }

    pub fn new_encrypted(
        data_dir: impl Into<PathBuf>,
        provider: Arc<dyn LocalKeyProvider>,
        storage_handle: TokioRuntimeHandle,
    ) -> Result<Self> {
        let data_dir = data_dir.into();
        Ok(Self {
            state: Arc::new(ControlPlaneState {
                path: data_dir.join(EmbeddedProviderKind::Redb.control_database_filename()),
                encryption: Some(ControlPlaneEncryption {
                    provider,
                    subject: LocalKeySubject::control_plane(
                        EmbeddedProviderKind::Redb.control_database_filename(),
                    ),
                }),
                storage_handle,
                opened: Mutex::new(None),
            }),
        })
    }

    pub fn usage_store(&self) -> Result<Arc<UsageStore>> {
        Ok(self.opened()?.usage_store)
    }

    pub fn usage_storage(&self) -> Result<Arc<RedbUsageStorage>> {
        Ok(self.opened()?.usage_storage)
    }

    fn opened(&self) -> Result<OpenedControlPlane> {
        let mut opened = self.state.opened.lock();
        if let Some(opened) = opened.as_ref() {
            return Ok(opened.clone());
        }

        let opened_control_plane = self.open_control_plane()?;
        *opened = Some(opened_control_plane.clone());
        Ok(opened_control_plane)
    }

    fn open_control_plane(&self) -> Result<OpenedControlPlane> {
        let started = Instant::now();
        let usage_store = Arc::new(match &self.state.encryption {
            Some(encryption) => {
                let dek = resolve_database_encryption_key(
                    &self.state.path,
                    encryption.provider.as_ref(),
                    &encryption.subject,
                    ManifestCipher::RedbAes256GcmSiv,
                )?;
                UsageStore::open_encrypted(&self.state.path, &dek)?
            }
            None => UsageStore::open(&self.state.path)?,
        });
        let usage_storage = Arc::new(RedbUsageStorage::new(
            usage_store.clone(),
            self.state.storage_handle.clone(),
        ));

        maybe_emit_profile(
            &self.state.path,
            self.state.encryption.is_some(),
            started.elapsed(),
        );

        Ok(OpenedControlPlane {
            usage_store,
            usage_storage,
        })
    }
}

fn maybe_emit_profile(path: &Path, encrypted: bool, total: std::time::Duration) {
    if std::env::var_os("NEOVEX_CONTROL_PLANE_PROFILE").is_none() {
        return;
    }
    if std::env::var_os("NEOVEX_PROFILE_ONLY_COLD_SAMPLES").is_some()
        && !path.to_string_lossy().contains("cold-sample")
    {
        return;
    }

    eprintln!(
        "control-plane-profile path={} encrypted={} total={:?}",
        path.display(),
        encrypted,
        total,
    );
}
