use std::sync::Arc;

use bytes::Bytes;
use nimbus_core::{Clock, Error, Result, StorageErrorKind, SystemClock};

use crate::BlobHash;
use crate::store::BlobStore;

use super::manifest::{self, ErasureManifest, ShardRef};
use super::store::ErasureBlobStore;
use super::stripe;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HealPacing {
    max_bytes_per_run: u64,
}

impl HealPacing {
    pub fn unlimited() -> Self {
        Self {
            max_bytes_per_run: u64::MAX,
        }
    }

    pub fn max_bytes_per_run(bytes: u64) -> Result<Self> {
        if bytes == 0 {
            return Err(Error::InvalidInput(
                "heal max_bytes_per_run must be greater than zero".to_string(),
            ));
        }
        Ok(Self {
            max_bytes_per_run: bytes,
        })
    }

    pub fn budget(&self) -> Option<u64> {
        if self.max_bytes_per_run == u64::MAX {
            None
        } else {
            Some(self.max_bytes_per_run)
        }
    }
}

impl Default for HealPacing {
    fn default() -> Self {
        Self::unlimited()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HealReport {
    pub blobs_examined: usize,
    pub stripes_repaired: usize,
    pub shards_rewritten: usize,
    pub degraded: usize,
    pub beyond_repair: Vec<BlobHash>,
    pub exhausted: bool,
    pub at_millis: u64,
}

impl HealReport {
    fn summary(&self) -> HealSummary {
        HealSummary {
            blobs_examined: self.blobs_examined,
            stripes_repaired: self.stripes_repaired,
            shards_rewritten: self.shards_rewritten,
            degraded_blobs: self.degraded,
            beyond_repair_blobs: self.beyond_repair.len(),
            exhausted: self.exhausted,
            at_millis: self.at_millis,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HealSummary {
    pub blobs_examined: usize,
    pub stripes_repaired: usize,
    pub shards_rewritten: usize,
    pub degraded_blobs: usize,
    pub beyond_repair_blobs: usize,
    pub exhausted: bool,
    pub at_millis: u64,
}

pub struct ErasureHealer {
    store: ErasureBlobStore,
    pacing: HealPacing,
    clock: Arc<dyn Clock>,
}

impl ErasureHealer {
    pub fn new(store: ErasureBlobStore) -> Self {
        Self {
            store,
            pacing: HealPacing::default(),
            clock: Arc::new(SystemClock),
        }
    }

    pub fn with_pacing(mut self, pacing: HealPacing) -> Self {
        self.pacing = pacing;
        self
    }

    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    pub async fn heal(&self) -> Result<HealReport> {
        self.store.ensure_live()?;
        let manifests = self.visible_manifests().await?;
        let mut budget = HealBudget::new(self.pacing);
        let mut report = HealReport {
            at_millis: self.clock.now_millis(),
            ..HealReport::default()
        };

        for manifest in &manifests {
            self.store
                .validate_manifest(&manifest.blob_hash, manifest)?;
            report.blobs_examined += 1;
            if self
                .heal_manifest(manifest, &mut report, &mut budget)
                .await?
            {
                report.exhausted = true;
                break;
            }
        }

        self.store.set_last_heal(report.summary())?;
        Ok(report)
    }

    async fn visible_manifests(&self) -> Result<Vec<ErasureManifest>> {
        let drive_roots = self.store.drive_roots.clone();
        let quorum = self.store.visibility_quorum();
        blocking(move || manifest::list_visible(&drive_roots, quorum)).await
    }

    async fn heal_manifest(
        &self,
        initial: &ErasureManifest,
        report: &mut HealReport,
        budget: &mut HealBudget,
    ) -> Result<bool> {
        let _mutation = self.store.mutation.lock().await;
        // Re-check the fail-stop UNDER the lock: a put queued ahead of us
        // can poison the leg (nondurable rollback) while we waited, and
        // healing against the now-ambiguous manifest view — or publishing a
        // generation bump over it — would violate the poison contract.
        self.store.ensure_live()?;
        let Some(mut manifest) = self.store.load_manifest(&initial.blob_hash).await? else {
            return Ok(false);
        };
        self.store
            .validate_manifest(&manifest.blob_hash, &manifest)?;
        let _pins = self
            .store
            .leg_pins
            .pin_all(manifest_shard_hashes(&manifest));

        let mut repaired_any = false;
        let mut degraded = false;
        let mut exhausted = false;
        for stripe_index in 0..manifest.stripes.len() {
            let probe = self.probe_stripe(&manifest, stripe_index).await?;
            if probe.bad.len() > manifest.parity_shards {
                push_beyond(report, manifest.blob_hash);
                return Ok(false);
            }
            if probe.bad.is_empty() {
                continue;
            }

            let stripe_len = ErasureBlobStore::stripe_true_len(&manifest, stripe_index)? as u64;
            if !budget.can_spend(stripe_len) {
                exhausted = true;
                break;
            }

            match self.repair_stripe(&manifest, stripe_index, &probe).await? {
                RepairOutcome::Rewritten(shards) => {
                    degraded = true;
                    repaired_any = true;
                    report.stripes_repaired += 1;
                    report.shards_rewritten += shards;
                    budget.spend(stripe_len);
                }
                RepairOutcome::VerificationFailed => {
                    push_beyond(report, manifest.blob_hash);
                    return Ok(false);
                }
            }
        }

        if degraded {
            report.degraded += 1;
        }
        if repaired_any {
            manifest.generation = manifest.generation.checked_add(1).ok_or_else(|| {
                Error::storage(
                    StorageErrorKind::Corruption,
                    "erasure manifest generation overflow",
                )
            })?;
            self.store.publish_manifest(manifest).await?;
        }
        Ok(exhausted)
    }

    async fn probe_stripe(
        &self,
        manifest: &ErasureManifest,
        stripe_index: usize,
    ) -> Result<StripeProbe> {
        let stripe = manifest.stripes.get(stripe_index).ok_or_else(|| {
            corruption(format!(
                "erasure manifest missing stripe {stripe_index} during heal"
            ))
        })?;
        let total = manifest.data_shards + manifest.parity_shards;
        let mut healthy = Vec::new();
        let mut bad = Vec::new();
        for shard in stripe {
            let shard_index = shard.shard_index as usize;
            let drive = stripe::drive_for(shard_index, stripe_index, total);
            let store = &self.store.stores[drive];
            if !store.has(&shard.shard_hash).await? {
                bad.push((shard_index, false));
                continue;
            }
            match store.get(&shard.shard_hash).await {
                Ok(bytes) => healthy.push((shard_index, bytes)),
                Err(err) if err.storage_kind() == Some(StorageErrorKind::Corruption) => {
                    bad.push((shard_index, true));
                }
                Err(Error::NotFound(_)) => bad.push((shard_index, false)),
                Err(err) => return Err(err),
            }
        }
        Ok(StripeProbe { healthy, bad })
    }

    async fn repair_stripe(
        &self,
        manifest: &ErasureManifest,
        stripe_index: usize,
        probe: &StripeProbe,
    ) -> Result<RepairOutcome> {
        let decoded =
            stripe::decode_stripe(manifest.data_shards, manifest.parity_shards, &probe.healthy)?;
        let true_len = ErasureBlobStore::stripe_true_len(manifest, stripe_index)?;
        let stripe_bytes = stripe::reassemble_stripe(&decoded, true_len)?;
        let expected_stripe = manifest.stripe_hashes.get(stripe_index).ok_or_else(|| {
            corruption(format!(
                "erasure manifest missing stripe hash for stripe {stripe_index}"
            ))
        })?;
        if BlobHash::of(&stripe_bytes) != *expected_stripe {
            return Ok(RepairOutcome::VerificationFailed);
        }

        let encoded =
            stripe::encode_stripe(&stripe_bytes, manifest.data_shards, manifest.parity_shards)?;
        let mut writes = Vec::with_capacity(probe.bad.len());
        for (shard_index, present_but_corrupt) in &probe.bad {
            let shard = shard_ref(manifest, stripe_index, *shard_index)?;
            let shard_bytes = encoded.get(*shard_index).ok_or_else(|| {
                corruption(format!("erasure encoder omitted shard {shard_index}"))
            })?;
            if BlobHash::of(shard_bytes) != shard.shard_hash {
                return Ok(RepairOutcome::VerificationFailed);
            }
            writes.push((
                *shard_index,
                shard.shard_hash,
                shard_bytes.clone(),
                *present_but_corrupt,
            ));
        }

        let total = manifest.data_shards + manifest.parity_shards;
        for (shard_index, expected_hash, shard_bytes, present_but_corrupt) in &writes {
            let drive = stripe::drive_for(*shard_index, stripe_index, total);
            let store = &self.store.stores[drive];
            if *present_but_corrupt {
                // A corrupt record can still be indexed; put would be an
                // idempotent no-op leaving the bad bytes in place. Release
                // the claim first so the rewrite appends fresh bytes (this
                // also clears an orphaned quarantine per RFS5). The shard
                // was already unreadable, so no reader regresses during the
                // release->put window.
                store.release(expected_hash).await?;
            }
            let actual = store.put(shard_bytes.clone()).await?;
            if actual != *expected_hash {
                return Err(corruption(format!(
                    "healed erasure shard {shard_index} wrote {actual}, expected {expected_hash}"
                )));
            }
            // Read back: the repair only counts if the drive actually
            // serves the healed bytes (guards any further idempotent-no-op
            // class in the underlying store).
            let served = store.get(expected_hash).await?;
            if served != *shard_bytes {
                return Err(corruption(format!(
                    "healed erasure shard {shard_index} does not serve the rewritten bytes"
                )));
            }
        }
        Ok(RepairOutcome::Rewritten(writes.len()))
    }
}

struct StripeProbe {
    healthy: Vec<(usize, Bytes)>,
    /// (shard_index, present_but_corrupt): a corrupt-but-still-indexed
    /// record must be RELEASED before rewriting — LocalPackStore::put is an
    /// idempotent no-op for an indexed hash, so a bare re-put would count
    /// as repaired while the corrupt bytes stay on disk.
    bad: Vec<(usize, bool)>,
}

enum RepairOutcome {
    Rewritten(usize),
    VerificationFailed,
}

#[derive(Clone, Copy)]
struct HealBudget {
    max: u64,
    spent: u64,
}

impl HealBudget {
    fn new(pacing: HealPacing) -> Self {
        Self {
            max: pacing.max_bytes_per_run,
            spent: 0,
        }
    }

    fn can_spend(&self, bytes: u64) -> bool {
        // Strict cap: max_bytes_per_run is a MAXIMUM, never exceeded — a
        // single stripe larger than the whole budget is refused (the run
        // reports exhausted with zero progress, signaling the operator to
        // raise the budget above stripe_width).
        self.max == u64::MAX || self.spent.saturating_add(bytes) <= self.max
    }

    fn spend(&mut self, bytes: u64) {
        if self.max != u64::MAX {
            self.spent = self.spent.saturating_add(bytes);
        }
    }
}

fn shard_ref(
    manifest: &ErasureManifest,
    stripe_index: usize,
    shard_index: usize,
) -> Result<&ShardRef> {
    manifest
        .stripes
        .get(stripe_index)
        .and_then(|stripe| {
            stripe
                .iter()
                .find(|candidate| candidate.shard_index as usize == shard_index)
        })
        .ok_or_else(|| {
            corruption(format!(
                "erasure manifest missing shard {shard_index} for stripe {stripe_index}"
            ))
        })
}

fn manifest_shard_hashes(manifest: &ErasureManifest) -> impl Iterator<Item = BlobHash> + '_ {
    manifest
        .stripes
        .iter()
        .flat_map(|stripe| stripe.iter().map(|shard| shard.shard_hash))
}

fn push_beyond(report: &mut HealReport, hash: BlobHash) {
    if !report.beyond_repair.contains(&hash) {
        report.beyond_repair.push(hash);
    }
}

async fn blocking<T>(op: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T>
where
    T: Send + 'static,
{
    tokio::task::spawn_blocking(op).await.map_err(|err| {
        Error::storage(StorageErrorKind::Other, format!("erasure heal task: {err}"))
    })?
}

fn corruption(message: impl Into<String>) -> Error {
    Error::storage(StorageErrorKind::Corruption, message)
}
