use nimbus_core::{Error, Result, StorageErrorKind};

use crate::LocalPackStats;

use super::heal::HealSummary;
use super::manifest;
use super::store::ErasureBlobStore;

/// Per-drive-consistent aggregate view of an erasure leg.
///
/// Each `LocalPackStats` entry is internally consistent because the owning
/// drive store computes it under one local state lock. The aggregate is not a
/// global transaction across drives: this keeps monitoring cheap and avoids
/// serializing the hot path on a cross-drive stats lock.
///
/// `degraded_blobs` and `beyond_repair_blobs` intentionally come from the most
/// recent heal summary. `stats()` does not re-probe shard contents.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ErasureStats {
    pub per_drive: Vec<LocalPackStats>,
    pub blob_count: usize,
    pub degraded_blobs: usize,
    pub beyond_repair_blobs: usize,
    pub last_heal: Option<HealSummary>,
}

impl ErasureBlobStore {
    pub async fn stats(&self) -> Result<ErasureStats> {
        self.ensure_live()?;
        let mut per_drive = Vec::with_capacity(self.stores.len());
        for store in &self.stores {
            per_drive.push(store.stats().await?);
        }

        let drive_roots = self.drive_roots.clone();
        let quorum = self.visibility_quorum();
        let manifests =
            tokio::task::spawn_blocking(move || manifest::list_visible(&drive_roots, quorum))
                .await
                .map_err(|err| {
                    Error::storage(
                        StorageErrorKind::Other,
                        format!("erasure stats task: {err}"),
                    )
                })??;
        for manifest in &manifests {
            self.validate_manifest(&manifest.blob_hash, manifest)?;
        }

        let last_heal = self.last_heal()?;
        Ok(ErasureStats {
            per_drive,
            blob_count: manifests.len(),
            degraded_blobs: last_heal
                .map(|summary| summary.degraded_blobs)
                .unwrap_or_default(),
            beyond_repair_blobs: last_heal
                .map(|summary| summary.beyond_repair_blobs)
                .unwrap_or_default(),
            last_heal,
        })
    }
}
