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
/// The health counters are prefixed `last_heal_` because that is exactly
/// what they are: the most recent heal run's findings, NOT a live probe —
/// `stats()` never re-reads shard contents (it must stay cheap). A blob
/// repaired since that run still appears in `last_heal_degraded_blobs`
/// until the next heal.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ErasureStats {
    pub per_drive: Vec<LocalPackStats>,
    pub blob_count: usize,
    pub last_heal_degraded_blobs: usize,
    pub last_heal_beyond_repair_blobs: usize,
    pub last_heal: Option<HealSummary>,
}

impl ErasureBlobStore {
    /// The content addresses of every VISIBLE blob (quorum rule), in hash
    /// order. Read-only-safe; used by backup-root enumeration for erasure
    /// legs (the pack-leg equivalent walks live pack entries).
    pub async fn visible_blob_hashes(&self) -> Result<Vec<crate::BlobHash>> {
        // Poisoned WRITABLE handles refuse (fail-stop contract); read-only
        // inspectors pass (ensure_live ignores shared poison when
        // read-only).
        self.ensure_live()?;
        let drive_roots = self.drive_roots.clone();
        let quorum = self.visibility_quorum();
        let manifests =
            tokio::task::spawn_blocking(move || manifest::list_visible(&drive_roots, quorum))
                .await
                .map_err(|err| {
                    Error::storage(
                        StorageErrorKind::Other,
                        format!("erasure visible-blob task: {err}"),
                    )
                })??;
        let mut hashes = Vec::with_capacity(manifests.len());
        for manifest in &manifests {
            // Layout must match THIS store's configuration; a stray quorum
            // of foreign-layout manifests must fail closed rather than be
            // exported as backup roots.
            self.validate_manifest(&manifest.blob_hash, manifest)?;
            hashes.push(manifest.blob_hash);
        }
        hashes.sort();
        Ok(hashes)
    }

    pub async fn stats(&self) -> Result<ErasureStats> {
        self.ensure_live()?;
        let mut per_drive = Vec::with_capacity(self.stores.len());
        for (index, store) in self.stores.iter().enumerate() {
            // An absent drive root (fresh tenant, failed/unmounted drive
            // within parity tolerance) reports empty stats instead of
            // failing the whole status view — both are supported read-only
            // states.
            if !self.drive_roots[index].exists() {
                per_drive.push(LocalPackStats::default());
                continue;
            }
            // Read-only coherence: a frozen index over a compacted drive
            // yields either a torn-but-SUCCESSFUL accounting (old entries
            // counted live, replacement packs counted reclaimable) or an
            // error. Both are discriminated by comparing the frozen
            // compaction epoch against a fresh on-disk view: an epoch bump
            // means the writer restructured packs since our snapshot →
            // Busy with a re-open hint. Without an epoch bump, an error is
            // REAL (stable corruption / Io) and keeps its original kind,
            // and a success is at worst an append-lag approximation
            // (documented). Writable handles never take this path.
            let outcome = store.stats().await;
            if self.is_read_only() {
                // Compaction is the state change that makes a frozen index
                // produce torn-but-successful accounting (old entries
                // counted live against replacement packs counted
                // reclaimable): it REMOVES pack files the frozen index
                // still references — plain appends never remove packs. If
                // any frozen-referenced pack is gone from disk, the
                // snapshot predates a restructure: Busy with a re-open
                // hint. Real errors without a restructure keep their
                // original kind; successes are at worst append-lag
                // approximations (documented).
                if store.frozen_packs_missing_on_disk()? {
                    return Err(Error::storage(
                        StorageErrorKind::Busy,
                        format!(
                            "erasure status snapshot predates a pack restructure on \
                             drive {index} (re-open to inspect)"
                        ),
                    ));
                }
            }
            per_drive.push(outcome?);
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
            last_heal_degraded_blobs: last_heal
                .map(|summary| summary.degraded_blobs)
                .unwrap_or_default(),
            last_heal_beyond_repair_blobs: last_heal
                .map(|summary| summary.beyond_repair_blobs)
                .unwrap_or_default(),
            last_heal,
        })
    }
}
