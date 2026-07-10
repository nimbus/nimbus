use nimbus_core::{Error, Result, StorageErrorKind};

use crate::{LocalPackStats, LocalPackStore};

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
            if self.is_read_only() {
                // Read-only status: FRESH per-drive open per attempt, with a
                // STABILITY check bracketing the computation — if any pack
                // the fresh index references vanished while stats ran, a
                // writer restructured the drive mid-call (torn accounting
                // risk) and we retry; three unstable attempts in a row
                // report Busy. Errors observed while the snapshot IS stable
                // are real (Corruption/Io keep their kind); a torn window
                // cannot produce silent wrong numbers because acceptance
                // requires post-computation stability.
                let mut accepted = None;
                let mut last_unstable_err: Option<Error> = None;
                for _ in 0..3 {
                    let fresh = LocalPackStore::open_read_only_with_identity(
                        &self.drive_roots[index],
                        Some(super::store::drive_identity(&self.config.leg_id, index)),
                    )?;
                    let outcome = fresh.stats().await;
                    let stable = fresh.index_packs_present_on_disk()?;
                    match (outcome, stable) {
                        (Ok(stats), true) => {
                            accepted = Some(stats);
                            break;
                        }
                        (Err(err), true) => return Err(err),
                        (_, false) => {
                            last_unstable_err = Some(Error::storage(
                                StorageErrorKind::Busy,
                                format!(
                                    "erasure status raced a writer restructure on drive \
                                     {index} (re-open or retry to inspect)"
                                ),
                            ));
                        }
                    }
                }
                match accepted {
                    Some(stats) => per_drive.push(stats),
                    None => {
                        return Err(last_unstable_err.unwrap_or_else(|| {
                            Error::storage(
                                StorageErrorKind::Busy,
                                "erasure status could not obtain a stable snapshot".to_string(),
                            )
                        }));
                    }
                }
            } else {
                per_drive.push(store.stats().await?);
            }
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
