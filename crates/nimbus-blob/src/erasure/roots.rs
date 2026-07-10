use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
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
    /// The owning leg's poison flag (shared). A poisoned leg's manifest
    /// view is AMBIGUOUS (a nondurable rollback's replicas may resurface
    /// after a crash), so root enumeration must fail closed — otherwise a
    /// sweep could reclaim shards whose manifests reappear, which is
    /// data loss. An erroring root provider makes the whole sweep fail
    /// before anything is released.
    poisoned: Arc<AtomicBool>,
}

impl ManifestShardRoots {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        drive_roots: Vec<PathBuf>,
        drive_index: usize,
        data_shards: usize,
        parity_shards: usize,
        stripe_width: usize,
        quorum: usize,
        poisoned: Arc<AtomicBool>,
    ) -> Self {
        Self {
            drive_roots,
            drive_index,
            data_shards,
            parity_shards,
            stripe_width,
            quorum,
            poisoned,
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
        if self.poisoned.load(Ordering::SeqCst) {
            return Err(Error::storage(
                StorageErrorKind::Io,
                "erasure leg poisoned: manifest view is ambiguous; refusing to \
                 enumerate GC roots (a sweep could reclaim shards whose \
                 manifests resurface after a crash)",
            ));
        }
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
    /// Builds the RFS6 sweep for one drive. Fails while the leg is
    /// poisoned, and the returned sweep ALSO fails at enumeration time if
    /// the leg poisons after construction — the fail-stop covers GC end to
    /// end (review round: a sweep against an ambiguous manifest view is a
    /// data-loss path, not a cleanup).
    pub fn shard_gc(
        &self,
        drive_index: usize,
        grace: Duration,
    ) -> Result<BlobGc<CompositeBlobRoots>> {
        self.ensure_live()?;
        if drive_index >= self.stores.len() {
            return Err(Error::InvalidInput(format!(
                "erasure drive index {drive_index} out of bounds for {} drives",
                self.stores.len()
            )));
        }
        let roots = ManifestShardRoots::new(
            self.drive_roots.clone(),
            drive_index,
            self.config.data_shards,
            self.config.parity_shards,
            self.config.stripe_width,
            self.visibility_quorum(),
            self.poison_flag(),
        );
        let roots = CompositeBlobRoots::new().with(Arc::new(roots));
        let poisoned = self.poison_flag();
        let release_guard: Arc<dyn Fn() -> nimbus_core::Result<()> + Send + Sync> =
            Arc::new(move || {
                if poisoned.load(Ordering::SeqCst) {
                    return Err(Error::storage(
                        StorageErrorKind::Io,
                        "erasure leg poisoned mid-sweep: refusing to release shards \
                         the ambiguous manifest state may still reference",
                    ));
                }
                Ok(())
            });
        Ok(BlobGc::new(self.stores[drive_index].clone(), roots, grace)
            .with_pins(self.leg_pins.clone())
            .with_release_guard(release_guard))
    }
}
