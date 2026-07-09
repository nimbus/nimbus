use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use nimbus_core::{Error, Result, StorageErrorKind};

use crate::{BlobGc, BlobGcRoots, BlobHash, CompositeBlobRoots};

use super::manifest::{self, ErasureManifest};
use super::store::ErasureBlobStore;
use super::stripe;

/// GC roots for the shards assigned to one drive of an erasure leg.
///
/// The root set is enumerated inside each `BlobGc::sweep` call, not cached at
/// `shard_gc` construction time. Manifest mutations serialize on the leg
/// mutation lock, while a sweep does not take that lock: a put writes shards
/// before its manifest is visible, so a concurrent sweep can miss those
/// brand-new shards. The caller-provided grace window is the safety net for
/// that write-before-publish window, matching the RFS6 local-pack GC idiom.
#[derive(Clone)]
pub(crate) struct ManifestShardRoots {
    drive_roots: Vec<PathBuf>,
    drive_index: usize,
    data_shards: usize,
    parity_shards: usize,
    stripe_width: usize,
    quorum: usize,
}

impl ManifestShardRoots {
    pub(crate) fn new(
        drive_roots: Vec<PathBuf>,
        drive_index: usize,
        data_shards: usize,
        parity_shards: usize,
        stripe_width: usize,
        quorum: usize,
    ) -> Self {
        Self {
            drive_roots,
            drive_index,
            data_shards,
            parity_shards,
            stripe_width,
            quorum,
        }
    }

    fn validate_manifest(&self, manifest: &ErasureManifest) -> Result<()> {
        if manifest.data_shards != self.data_shards
            || manifest.parity_shards != self.parity_shards
            || manifest.stripe_width != self.stripe_width
        {
            return Err(Error::storage(
                StorageErrorKind::Corruption,
                "erasure manifest layout does not match shard GC roots",
            ));
        }
        Ok(())
    }
}

#[async_trait]
impl BlobGcRoots for ManifestShardRoots {
    async fn live_blob_hashes(&self) -> Result<BTreeSet<BlobHash>> {
        let drive_roots = self.drive_roots.clone();
        let quorum = self.quorum;
        let manifests =
            tokio::task::spawn_blocking(move || manifest::list_visible(&drive_roots, quorum))
                .await
                .map_err(|err| {
                    Error::storage(
                        StorageErrorKind::Other,
                        format!("erasure roots task: {err}"),
                    )
                })??;

        let total = self.data_shards + self.parity_shards;
        let mut roots = BTreeSet::new();
        for manifest in manifests {
            self.validate_manifest(&manifest)?;
            for (stripe_index, stripe) in manifest.stripes.iter().enumerate() {
                for shard in stripe {
                    let shard_index = shard.shard_index as usize;
                    if stripe::drive_for(shard_index, stripe_index, total) == self.drive_index {
                        roots.insert(shard.shard_hash);
                    }
                }
            }
        }
        Ok(roots)
    }
}

impl ErasureBlobStore {
    pub fn shard_gc(&self, drive_index: usize, grace: Duration) -> BlobGc<CompositeBlobRoots> {
        assert!(
            drive_index < self.stores.len(),
            "erasure drive index {drive_index} out of bounds for {} drives",
            self.stores.len()
        );
        let roots = ManifestShardRoots::new(
            self.drive_roots.clone(),
            drive_index,
            self.config.data_shards,
            self.config.parity_shards,
            self.config.stripe_width,
            self.visibility_quorum(),
        );
        let roots = CompositeBlobRoots::new().with(Arc::new(roots));
        BlobGc::new(self.stores[drive_index].clone(), roots, grace)
            .with_pins(self.heal_pins.clone())
    }
}
