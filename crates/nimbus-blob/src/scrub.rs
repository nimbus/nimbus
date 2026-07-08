//! Operator-visible integrity scrubbers for local pack roots.
//!
//! [`LocalPackScrubber`] verifies the local content-addressed byte plane
//! without tenant keys: pack headers, record framing, and BLAKE3 over the
//! stored bytes. It keeps findings structured so an operator surface can
//! report the exact pack id and byte offset that failed, and it persists
//! per-hash quarantine so subsequent reads fail closed before touching pack
//! bytes.
//!
//! [`EncryptedBlobScrubber`] is the key-holding layer. It composes over the
//! local scrubber, then AEAD-opens every live framed ciphertext with the tenant
//! key to catch authentication failures that a hash over ciphertext cannot.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use nimbus_core::{Clock, Error, Result, StorageErrorKind, SystemClock};
use nimbus_crypto::{FramedBlobKey, open_framed_blob};

use crate::disk;
use crate::hash::BlobHash;
use crate::local::{
    self, LocalPackState, LocalPackStore, PACK_MAGIC, PackEntry, QuarantineCheck, RECORD_MAGIC,
};
use crate::root_guard::LocalPackStoreOptions;
use crate::store::BlobStore;

pub(crate) const SCRUB_CHECKPOINT_FILE: &str = "scrub-checkpoint.nbls";
const SCRUB_CHECKPOINT_MAGIC: &[u8] = b"NBLSCP1\n";
const DEFAULT_SCAN_CHUNK: usize = 64 * 1024;
const RECORD_HEADER_LEN: u64 = RECORD_MAGIC.len() as u64 + crate::BLAKE3_HASH_LEN as u64 + 8;

/// Pacing budget for a scrub pass.
///
/// The scrubber accounts actual bytes read into deterministic "ticks" rather
/// than sleeping. A caller can drive one scrub per scheduler tick later; tests
/// can assert that each reported tick stayed within the configured budget
/// without relying on wall-clock time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScrubPacing {
    bytes_per_tick: u64,
}

impl ScrubPacing {
    pub fn unlimited() -> Self {
        Self {
            bytes_per_tick: u64::MAX,
        }
    }

    pub fn bytes_per_tick(bytes: u64) -> Result<Self> {
        if bytes == 0 {
            return Err(Error::InvalidInput(
                "scrub bytes_per_tick must be greater than zero".to_string(),
            ));
        }
        Ok(Self {
            bytes_per_tick: bytes,
        })
    }

    pub fn budget(&self) -> u64 {
        self.bytes_per_tick
    }
}

impl Default for ScrubPacing {
    fn default() -> Self {
        Self::unlimited()
    }
}

/// Deterministic pacing evidence for a scrub run.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScrubPacingStatus {
    pub bytes_per_tick_budget: Option<u64>,
    pub bytes_per_tick: Vec<u64>,
}

/// Progress checkpoint evidence for a scrub run.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScrubCheckpointStatus {
    pub path: Option<PathBuf>,
    pub resumed_after_pack_id: Option<u64>,
    pub last_completed_pack_id: Option<u64>,
    pub complete: bool,
}

/// Structured corruption and repair event kinds reported by scrubbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrubFindingKind {
    MissingPack,
    TruncatedPackHeader,
    InvalidPackHeader,
    TruncatedRecord,
    InvalidRecordMagic,
    HashMismatch,
    IndexHashMismatch,
    IndexLengthMismatch,
    MissingIndexedRecord,
    OrphanRecord,
    AeadOpenFailed,
}

/// A single scrub finding with enough location data for operator reporting.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScrubFinding {
    pub kind: ScrubFindingKind,
    pub pack_id: Option<u64>,
    pub offset: Option<u64>,
    pub hash: Option<BlobHash>,
    pub expected_hash: Option<BlobHash>,
    pub actual_hash: Option<BlobHash>,
    pub message: String,
}

/// Full scrub result.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScrubReport {
    pub packs_scanned: usize,
    pub packs_skipped_via_checkpoint: usize,
    pub first_scanned_pack_id: Option<u64>,
    pub last_scanned_pack_id: Option<u64>,
    pub records_scanned: usize,
    pub records_verified: usize,
    pub bytes_scanned: u64,
    pub corrupt_records: usize,
    pub orphan_records: usize,
    pub missing_packs: usize,
    pub quarantined_hashes: Vec<BlobHash>,
    /// Live claims that were ALREADY quarantined before this run. Persistent
    /// corruption stays operator-visible on every scrub, not only the run
    /// that first quarantined it.
    pub previously_quarantined: Vec<BlobHash>,
    pub findings: Vec<ScrubFinding>,
    pub checkpoint: ScrubCheckpointStatus,
    pub pacing: ScrubPacingStatus,
    pub completed: bool,
}

/// No-key local pack scrubber.
pub struct LocalPackScrubber {
    store: LocalPackStore,
    pacing: ScrubPacing,
    clock: Arc<dyn Clock>,
}

impl LocalPackScrubber {
    pub fn new(store: LocalPackStore) -> Self {
        Self {
            store,
            pacing: ScrubPacing::default(),
            clock: Arc::new(SystemClock),
        }
    }

    pub fn with_pacing(mut self, pacing: ScrubPacing) -> Self {
        self.pacing = pacing;
        self
    }

    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// Scans every pack not covered by an incomplete checkpoint.
    pub async fn scrub(&self) -> Result<ScrubReport> {
        self.scrub_inner(None).await
    }

    /// Test/operator seam for an interrupted scrub pass.
    ///
    /// The scrubber persists the last fully verified pack before returning
    /// `completed = false`, so a subsequent [`Self::scrub`] call resumes past
    /// the already scanned pack ids.
    pub async fn scrub_with_pack_limit(&self, max_packs: usize) -> Result<ScrubReport> {
        self.scrub_inner(Some(max_packs)).await
    }

    /// Rebuilds the live index from valid pack records.
    ///
    /// Pack records carry no release tombstones, so released-but-uncompacted
    /// blobs reappear as live claims. That is a conservative repair: the data
    /// is not lost, and the next GC sweep can re-reclaim anything with no root.
    pub async fn rebuild_index_from_packs(&self) -> Result<ScrubReport> {
        let pacing = self.pacing;
        let now = self.clock.now_millis();
        self.store
            .blocking(move |mut state| {
                local::ensure_writable(&state, "rebuild_index_from_packs")?;
                let result = rebuild::rebuild_index_locked(&mut state, pacing, now);
                local::poison_on_write_failure(&mut state, &result);
                result
            })
            .await
    }

    /// Opens `root` for local index repair and rebuilds `index.log` from pack
    /// records, even when a normal open fails on index-log corruption.
    pub async fn rebuild_index_in_root(
        root: impl AsRef<Path>,
        options: LocalPackStoreOptions,
    ) -> Result<ScrubReport> {
        let root = root.as_ref().to_path_buf();
        // A MISSING index routes straight to the guard path — never through
        // `open_with_options`, which would create a provisional empty
        // `index.log` that a fail-closed rebuild (unrecoverable quarantined
        // claim) would leave behind for the next open to treat as
        // authoritative and prune quarantine against. The guard path
        // publishes the rebuilt index atomically or not at all.
        if root.join("index.log").exists() {
            match LocalPackStore::open_with_options(&root, options.clone()) {
                Ok(store) => {
                    return LocalPackScrubber::new(store)
                        .rebuild_index_from_packs()
                        .await;
                }
                Err(err) if rebuild::is_index_corruption(&err) => {}
                Err(err) => return Err(err),
            }
        }

        let repair_root = root.clone();
        let open_options = options.clone();
        let pacing = ScrubPacing::default();
        let report = tokio::task::spawn_blocking(move || {
            rebuild::rebuild_corrupt_index_under_guard(repair_root, options, pacing)
        })
        .await
        .map_err(|err| {
            Error::storage(
                StorageErrorKind::Other,
                format!("local index repair task: {err}"),
            )
        })??;

        // The rebuilt index is already durably published (single atomic
        // replace, under the root guard); this open just validates it loads.
        let _store = LocalPackStore::open_with_options(&root, open_options)?;
        Ok(report)
    }

    async fn scrub_inner(&self, max_packs: Option<usize>) -> Result<ScrubReport> {
        let snapshot = self.snapshot().await?;
        let checkpoint_path = snapshot.root.join(SCRUB_CHECKPOINT_FILE);
        let checkpoint = load_checkpoint(&checkpoint_path)?;
        let resume = match checkpoint {
            // Also bound against the CURRENT layout: within one layout pack
            // ids grow monotonically (compaction invalidates checkpoints),
            // so a max_pack_seen beyond the current active pack is damaged
            // or foreign metadata — ignore and full-scan.
            Some(checkpoint)
                if !checkpoint.complete
                    && checkpoint.max_pack_seen.is_some()
                    && checkpoint.max_pack_seen <= Some(snapshot.active_pack_id) =>
            {
                Some(checkpoint)
            }
            _ => None,
        };

        let pack_ids = snapshot.pack_ids.clone();
        // The sealed-boundary recorded in checkpoints is the ACTIVE pack at
        // snapshot time: everything below it was sealed and fully scanned;
        // the active pack itself (and anything that rolls over later) is
        // never checkpoint-skippable.
        let max_pack_seen = Some(snapshot.active_pack_id);
        let mut previously_quarantined: Vec<BlobHash> = snapshot
            .index
            .keys()
            .filter(|hash| snapshot.quarantined.contains_key(hash))
            .copied()
            .collect();
        previously_quarantined.sort();
        let mut report = ScrubReport {
            previously_quarantined,
            ..ScrubReport::default()
        };
        report.checkpoint.path = Some(checkpoint_path.clone());
        report.checkpoint.resumed_after_pack_id =
            resume.and_then(|checkpoint| checkpoint.last_completed_pack_id);

        let index_offsets = snapshot
            .index
            .values()
            .map(|entry| (entry.pack_id, entry.offset))
            .collect::<HashSet<_>>();

        let mut missing_quarantine = Vec::new();
        for (hash, entry) in &snapshot.index {
            if !pack_ids.contains(&entry.pack_id) {
                report.missing_packs += 1;
                report.corrupt_records += 1;
                report.findings.push(finding(
                    ScrubFindingKind::MissingPack,
                    Some(entry.pack_id),
                    Some(entry.offset),
                    Some(*hash),
                    None,
                    None,
                    format!("index references missing pack {}", entry.pack_id),
                ));
                // Location-bound: quarantine only if the hash still maps to
                // this exact (missing-pack) record at quarantine time.
                missing_quarantine.push((*hash, QuarantineCheck::CorruptRecord(*entry)));
            }
        }
        let mut pacing = PacingTracker::new(self.pacing);
        self.quarantine(&mut report, Some(&mut pacing), missing_quarantine)
            .await?;
        let mut scanned = 0usize;
        let mut last_checkpoint = resume.and_then(|checkpoint| checkpoint.last_completed_pack_id);
        // Once a pack produces ANY finding, the resume checkpoint freezes:
        // findings for unindexed corrupt bytes are not durable anywhere else,
        // so a resumed run must rescan from the first dirty pack or its
        // "completed" report would silently omit corruption an interrupted
        // run saw. (Quarantines are durable regardless.)
        let mut checkpoint_frozen = false;
        for pack_id in pack_ids.iter().copied() {
            if let Some(resume) = resume {
                // Only provably sealed, fully verified packs are skipped; the
                // pack that was still appendable when the interrupted run
                // scanned it is rescanned (it may have grown since).
                if resume.safe_to_skip(pack_id) {
                    report.packs_skipped_via_checkpoint += 1;
                    continue;
                }
            }
            if let Some(limit) = max_packs {
                if scanned >= limit {
                    report.checkpoint.last_completed_pack_id = last_checkpoint;
                    report.completed = false;
                    report.pacing = pacing.finish();
                    return Ok(report);
                }
            }

            let packs_dir = snapshot.packs_dir.clone();
            let state_index = snapshot.index.clone();
            let index_offsets = index_offsets.clone();
            let current_pacing = pacing.clone();
            let len_cap = if pack_id == snapshot.active_pack_id {
                Some(snapshot.active_pack_bytes)
            } else {
                None
            };
            let scan_result = tokio::task::spawn_blocking(move || {
                let mut pacing = current_pacing;
                let scan = scan_pack(&packs_dir, pack_id, len_cap, &mut pacing)?;
                Ok::<_, Error>((scan, pacing))
            })
            .await
            .map_err(|err| {
                Error::storage(StorageErrorKind::Other, format!("local scrub task: {err}"))
            })?;
            let (pack_scan, next_pacing) = match scan_result {
                Ok(scanned) => scanned,
                Err(err) => {
                    // A snapshotted pack can legitimately disappear when a
                    // same-process compaction rewrote the layout mid-scrub:
                    // skip it (its live blobs were rewritten into new packs)
                    // and freeze checkpointing for the dead layout. A pack
                    // that vanished WITHOUT compaction is external
                    // interference — fail closed.
                    let pack_gone = !local::pack_path(&snapshot.packs_dir, pack_id).exists();
                    let epoch_now = self
                        .store
                        .blocking(|state| Ok(state.compaction_epoch))
                        .await?;
                    if pack_gone && epoch_now != snapshot.compaction_epoch {
                        checkpoint_frozen = true;
                        continue;
                    }
                    return Err(err);
                }
            };
            pacing = next_pacing;

            scanned += 1;
            report.packs_scanned += 1;
            if report.first_scanned_pack_id.is_none() {
                report.first_scanned_pack_id = Some(pack_id);
            }
            report.last_scanned_pack_id = Some(pack_id);
            report.records_scanned += pack_scan.records_scanned;
            report.bytes_scanned = report.bytes_scanned.saturating_add(pack_scan.bytes_scanned);
            let findings_before = report.findings.len();
            let pack_had_scan_findings = !pack_scan.findings.is_empty();
            merge_pack_findings(
                &mut report,
                &pack_scan,
                &state_index,
                &index_offsets,
                &self.store,
                &snapshot.packs_dir,
                &mut pacing,
            )
            .await?;
            // Retire a header-discredited ACTIVE pack AFTER the quarantine
            // pass, unconditionally: `retire_pack_if_active` re-validates the
            // header under the store lock (a no-op if the pack is no longer
            // active or its header is fine), so this is correct whether the
            // pack had indexed hashes, had none, or had them released out
            // from under the scrub between snapshot and quarantine. Ordering
            // it after `merge_pack_findings` guarantees any quarantine is
            // durable before the pack rolls, and doing it here (not inside
            // quarantine_hashes_locked's insert path) means a release race
            // that skips every insert still cannot leave the corrupt pack
            // appendable.
            if !pack_scan.pack_header_valid {
                self.store.retire_pack_if_active(pack_id).await?;
            }
            if pack_had_scan_findings || report.findings.len() > findings_before {
                checkpoint_frozen = true;
            }
            if checkpoint_frozen {
                continue;
            }

            let checkpoint = ScrubCheckpoint {
                last_completed_pack_id: Some(pack_id),
                max_pack_seen,
                complete: false,
            };
            if !self
                .write_checkpoint(checkpoint, snapshot.compaction_epoch)
                .await?
            {
                // Compaction restructured the packs mid-scrub: keep scanning
                // (findings and ground-truth-validated quarantines stay
                // correct), but publish no checkpoints for the dead layout.
                report.checkpoint.path = None;
                report.checkpoint.last_completed_pack_id = None;
                continue;
            }
            last_checkpoint = Some(pack_id);
            report.checkpoint.last_completed_pack_id = Some(pack_id);
        }

        let complete_checkpoint = ScrubCheckpoint {
            last_completed_pack_id: last_checkpoint,
            max_pack_seen,
            complete: true,
        };
        if self
            .write_checkpoint(complete_checkpoint, snapshot.compaction_epoch)
            .await?
        {
            report.checkpoint.last_completed_pack_id = last_checkpoint;
            report.checkpoint.complete = true;
        } else {
            report.checkpoint.path = None;
            report.checkpoint.last_completed_pack_id = None;
        }
        report.completed = true;
        report.pacing = pacing.finish();
        Ok(report)
    }

    async fn snapshot(&self) -> Result<ScrubSnapshot> {
        self.store
            .blocking(|state| {
                local::ensure_writable(&state, "scrub")?;
                let root = root_from_index_path(&state.index_path)?;
                let pack_ids = local::pack_ids_on_disk(&state.packs_dir)?;
                Ok(ScrubSnapshot {
                    root,
                    packs_dir: state.packs_dir.clone(),
                    index: state.index.clone(),
                    active_pack_id: state.active_pack_id,
                    active_pack_bytes: state.active_pack_bytes,
                    pack_ids,
                    compaction_epoch: state.compaction_epoch,
                    quarantined: state.quarantined.clone(),
                })
            })
            .await
    }

    async fn quarantine(
        &self,
        report: &mut ScrubReport,
        pacing: Option<&mut PacingTracker>,
        requests: Vec<(BlobHash, QuarantineCheck)>,
    ) -> Result<()> {
        if requests.is_empty() {
            return Ok(());
        }
        let unique = requests
            .into_iter()
            .collect::<BTreeMap<BlobHash, QuarantineCheck>>();
        let mut outcome = self
            .store
            .quarantine_hashes(unique.into_iter().collect())
            .await?;
        // Ground-truth revalidation I/O rides the same accounting contract
        // as scanning.
        report.bytes_scanned = report
            .bytes_scanned
            .saturating_add(outcome.revalidation_bytes);
        if let Some(pacing) = pacing {
            pacing.account(outcome.revalidation_bytes);
        }
        if !outcome.inserted.is_empty() {
            report.quarantined_hashes.append(&mut outcome.inserted);
            report.quarantined_hashes.sort();
            report.quarantined_hashes.dedup();
        }
        Ok(())
    }

    /// Publishes a checkpoint iff the pack layout this scrub scanned still
    /// exists (compaction epoch unchanged under the lock). Returns whether
    /// the checkpoint landed; a `false` means a concurrent compaction
    /// restructured the packs — the on-disk checkpoint was already
    /// invalidated by that compaction, and publishing ours would resurrect a
    /// stale sealed-boundary over reused pack ids.
    async fn write_checkpoint(
        &self,
        checkpoint: ScrubCheckpoint,
        snapshot_epoch: u64,
    ) -> Result<bool> {
        self.store
            .blocking(move |mut state| {
                local::ensure_writable(&state, "write scrub checkpoint")?;
                if state.compaction_epoch != snapshot_epoch {
                    return Ok(false);
                }
                let result = write_checkpoint_locked(&state, checkpoint).map(|()| true);
                local::poison_on_write_failure(&mut state, &result);
                result
            })
            .await
    }
}

/// Key-holding scrubber for framed ciphertext stored in local packs.
pub struct EncryptedBlobScrubber {
    store: LocalPackStore,
    key: FramedBlobKey,
    local_pacing: ScrubPacing,
}

impl EncryptedBlobScrubber {
    pub fn new(store: LocalPackStore, key: FramedBlobKey) -> Self {
        Self {
            store,
            key,
            local_pacing: ScrubPacing::default(),
        }
    }

    pub fn with_local_pacing(mut self, pacing: ScrubPacing) -> Self {
        self.local_pacing = pacing;
        self
    }

    pub async fn scrub(&self) -> Result<ScrubReport> {
        let local = LocalPackScrubber::new(self.store.clone()).with_pacing(self.local_pacing);
        let mut report = local.scrub().await?;
        let entries = self.store.live_entries()?;
        let mut quarantine = Vec::new();

        for entry in entries {
            let framed = match self.store.get(&entry.hash).await {
                Ok(bytes) => bytes,
                // Already quarantined (local pass or a previous run): its
                // persistent visibility rides `report.previously_quarantined`
                // and `report.quarantined_hashes`, so skipping the AEAD open
                // here loses nothing.
                Err(err) if err.storage_kind() == Some(StorageErrorKind::Corruption) => continue,
                Err(err) => return Err(err),
            };
            if let Err(err) = open_framed_blob(&self.key, &framed) {
                report.corrupt_records += 1;
                report.findings.push(finding(
                    ScrubFindingKind::AeadOpenFailed,
                    None,
                    None,
                    Some(entry.hash),
                    None,
                    None,
                    format!(
                        "encrypted blob {hash} failed AEAD open: {err}",
                        hash = entry.hash
                    ),
                ));
                // Content-level corruption: identical bytes wherever the
                // record lives (content-addressed), so no location validation.
                quarantine.push((entry.hash, QuarantineCheck::Unconditional));
            }
        }
        local.quarantine(&mut report, None, quarantine).await?;
        Ok(report)
    }
}

#[derive(Clone)]
struct ScrubSnapshot {
    root: PathBuf,
    packs_dir: PathBuf,
    index: HashMap<BlobHash, PackEntry>,
    /// Active pack and its length at snapshot time: scanning the active pack
    /// stops here so records appended after the snapshot (possibly still
    /// in flight) are never misreported as corruption. They are covered by
    /// the next scrub.
    active_pack_id: u64,
    active_pack_bytes: u64,
    /// Pack ids enumerated under the SAME lock as the index/active-pack
    /// facts. Deriving them later (unlocked) would let a concurrent rollover
    /// desynchronize the checkpoint's sealed-boundary from the scanned caps.
    pack_ids: BTreeSet<u64>,
    /// Compaction epoch at snapshot time; checkpoint publication is refused
    /// once it moves (the pack layout this scrub scanned no longer exists).
    compaction_epoch: u64,
    quarantined: HashMap<BlobHash, local::QuarantineReason>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ScrubCheckpoint {
    last_completed_pack_id: Option<u64>,
    /// Highest pack id on disk when the checkpointed run scanned. Packs below
    /// this were sealed (only the max pack accepts appends), so only they are
    /// safe to skip on resume; the max pack may have grown and is rescanned.
    max_pack_seen: Option<u64>,
    complete: bool,
}

impl ScrubCheckpoint {
    /// Whether `pack_id` was provably sealed when this checkpoint was taken
    /// and already fully verified, i.e. safe to skip on resume.
    fn safe_to_skip(&self, pack_id: u64) -> bool {
        match (self.last_completed_pack_id, self.max_pack_seen) {
            (Some(last), Some(max_seen)) => pack_id <= last && pack_id < max_seen,
            _ => false,
        }
    }

    fn encode(self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(SCRUB_CHECKPOINT_MAGIC.len() + 8 + 8 + 1 + 32);
        bytes.extend_from_slice(SCRUB_CHECKPOINT_MAGIC);
        bytes.extend_from_slice(
            &self
                .last_completed_pack_id
                .unwrap_or(u64::MAX)
                .to_le_bytes(),
        );
        bytes.extend_from_slice(&self.max_pack_seen.unwrap_or(u64::MAX).to_le_bytes());
        bytes.push(u8::from(self.complete));
        // Integrity trailer: the checkpoint's counters authorize SKIPPING
        // verification work, so they must not be trusted off unverified
        // bytes in the same corruption domain as the packs.
        let digest = blake3::hash(&bytes);
        bytes.extend_from_slice(digest.as_bytes());
        bytes
    }
}

#[derive(Clone, Debug)]
struct PacingTracker {
    config: ScrubPacing,
    current_tick_bytes: u64,
    bytes_per_tick: Vec<u64>,
}

impl PacingTracker {
    fn new(config: ScrubPacing) -> Self {
        Self {
            config,
            current_tick_bytes: 0,
            bytes_per_tick: Vec::new(),
        }
    }

    fn next_read_len(&mut self, requested: usize) -> usize {
        if requested == 0 {
            return 0;
        }
        if self.config.bytes_per_tick == u64::MAX {
            return requested.min(DEFAULT_SCAN_CHUNK);
        }
        if self.current_tick_bytes >= self.config.bytes_per_tick {
            self.finish_tick();
        }
        let remaining = self.config.bytes_per_tick - self.current_tick_bytes;
        let capped = (requested as u64)
            .min(DEFAULT_SCAN_CHUNK as u64)
            .min(remaining)
            .max(1);
        capped as usize
    }

    fn account(&mut self, bytes: u64) {
        if bytes == 0 {
            return;
        }
        if self.config.bytes_per_tick == u64::MAX {
            self.current_tick_bytes = self.current_tick_bytes.saturating_add(bytes);
            return;
        }

        let mut remaining_bytes = bytes;
        while remaining_bytes > 0 {
            if self.current_tick_bytes >= self.config.bytes_per_tick {
                self.finish_tick();
            }
            let available = self.config.bytes_per_tick - self.current_tick_bytes;
            let take = remaining_bytes.min(available);
            self.current_tick_bytes += take;
            remaining_bytes -= take;
            if self.current_tick_bytes >= self.config.bytes_per_tick && remaining_bytes > 0 {
                self.finish_tick();
            }
        }
    }

    fn finish_tick(&mut self) {
        if self.current_tick_bytes > 0 {
            self.bytes_per_tick.push(self.current_tick_bytes);
            self.current_tick_bytes = 0;
        }
    }

    fn finish(mut self) -> ScrubPacingStatus {
        self.finish_tick();
        ScrubPacingStatus {
            bytes_per_tick_budget: if self.config.bytes_per_tick == u64::MAX {
                None
            } else {
                Some(self.config.bytes_per_tick)
            },
            bytes_per_tick: self.bytes_per_tick,
        }
    }
}

#[derive(Clone, Debug)]
struct ScannedRecord {
    pack_id: u64,
    offset: u64,
    hash: BlobHash,
    len: u64,
}

#[derive(Clone, Debug, Default)]
struct PackScan {
    pack_id: u64,
    pack_header_valid: bool,
    records_scanned: usize,
    bytes_scanned: u64,
    findings: Vec<ScrubFinding>,
    valid_records: Vec<ScannedRecord>,
    /// Structurally walked records whose body failed verification, with
    /// full coordinates and BOTH hashes (the stored hash field and the hash
    /// the body actually computes to — either may be the corrupted part).
    /// Repair uses these to keep quarantined claims locatable when the index
    /// cannot supply the entry.
    corrupt_records: Vec<(ScannedRecord, BlobHash)>,
    corrupt_offsets: BTreeSet<u64>,
    /// Offset up to which the sequential scan verified the pack. The scanner
    /// stops at the first structurally corrupt record, but records at or past
    /// this boundary may still be healthy and readable by direct index
    /// offset — they get direct verification instead of blanket quarantine.
    scan_boundary: u64,
}

fn root_from_index_path(index_path: &Path) -> Result<PathBuf> {
    index_path.parent().map(Path::to_path_buf).ok_or_else(|| {
        Error::storage(
            StorageErrorKind::Other,
            format!("index path {} has no parent", index_path.display()),
        )
    })
}

fn checkpoint_path(state: &LocalPackState) -> Result<PathBuf> {
    Ok(root_from_index_path(&state.index_path)?.join(SCRUB_CHECKPOINT_FILE))
}

fn load_checkpoint(path: &Path) -> Result<Option<ScrubCheckpoint>> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(local::io_error(
                err,
                format!("open checkpoint {}", path.display()),
            ));
        }
    };
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|err| local::io_error(err, format!("read checkpoint {}", path.display())))?;
    // A checkpoint authorizes SKIPPING verification, so an unverifiable one
    // fails SAFE: ignore it and full-scan (Ok(None)), never trust its
    // counters and never brick the scrub over its own metadata.
    let expected_len = SCRUB_CHECKPOINT_MAGIC.len() + 8 + 8 + 1 + 32;
    if bytes.len() != expected_len || !bytes.starts_with(SCRUB_CHECKPOINT_MAGIC) {
        return Ok(None);
    }
    let (payload, trailer) = bytes.split_at(expected_len - 32);
    if blake3::hash(payload).as_bytes() != trailer {
        return Ok(None);
    }
    let cursor = SCRUB_CHECKPOINT_MAGIC.len();
    let mut raw_pack = [0u8; 8];
    raw_pack.copy_from_slice(&bytes[cursor..cursor + 8]);
    let raw_pack = u64::from_le_bytes(raw_pack);
    let mut raw_max = [0u8; 8];
    raw_max.copy_from_slice(&bytes[cursor + 8..cursor + 16]);
    let raw_max = u64::from_le_bytes(raw_max);
    let complete = match bytes[cursor + 16] {
        0 => false,
        1 => true,
        _ => return Ok(None),
    };
    let checkpoint = ScrubCheckpoint {
        last_completed_pack_id: if raw_pack == u64::MAX {
            None
        } else {
            Some(raw_pack)
        },
        max_pack_seen: if raw_max == u64::MAX {
            None
        } else {
            Some(raw_max)
        },
        complete,
    };
    // Semantic bounds our writer always satisfies; anything else is not
    // ours or is damaged.
    if let (Some(last), Some(max_seen)) =
        (checkpoint.last_completed_pack_id, checkpoint.max_pack_seen)
    {
        if last > max_seen {
            return Ok(None);
        }
    }
    Ok(Some(checkpoint))
}

fn write_checkpoint_locked(state: &LocalPackState, checkpoint: ScrubCheckpoint) -> Result<()> {
    let path = checkpoint_path(state)?;
    let observer = Arc::clone(&state.observer);
    disk::write_replace_durable(&path, &checkpoint.encode(), &*observer)
        .map_err(|err| local::io_error(err, format!("write checkpoint {}", path.display())))
}

fn finding(
    kind: ScrubFindingKind,
    pack_id: Option<u64>,
    offset: Option<u64>,
    hash: Option<BlobHash>,
    expected_hash: Option<BlobHash>,
    actual_hash: Option<BlobHash>,
    message: String,
) -> ScrubFinding {
    ScrubFinding {
        kind,
        pack_id,
        offset,
        hash,
        expected_hash,
        actual_hash,
        message,
    }
}

fn scan_pack(
    packs_dir: &Path,
    pack_id: u64,
    len_cap: Option<u64>,
    pacing: &mut PacingTracker,
) -> Result<PackScan> {
    let path = local::pack_path(packs_dir, pack_id);
    let mut scan = PackScan {
        pack_id,
        ..PackScan::default()
    };
    let metadata = fs::metadata(&path)
        .map_err(|err| local::io_error(err, format!("stat pack {}", path.display())))?;
    let mut file_len = metadata.len();
    if let Some(cap) = len_cap {
        // Bytes past the snapshot length belong to writes that raced the
        // scrub; ignore them rather than misreport an in-flight append.
        file_len = file_len.min(cap);
    }
    let mut file = File::open(&path)
        .map_err(|err| local::io_error(err, format!("open pack {}", path.display())))?;
    let mut header = vec![0u8; PACK_MAGIC.len()];
    let read = read_fully_or_short(
        &mut file,
        &path,
        &mut header,
        pacing,
        &mut scan.bytes_scanned,
    )?;
    if read < PACK_MAGIC.len() {
        scan.findings.push(finding(
            ScrubFindingKind::TruncatedPackHeader,
            Some(pack_id),
            Some(0),
            None,
            None,
            None,
            format!("pack {} ended inside its header", path.display()),
        ));
        return Ok(scan);
    }
    if header != PACK_MAGIC {
        scan.findings.push(finding(
            ScrubFindingKind::InvalidPackHeader,
            Some(pack_id),
            Some(0),
            None,
            None,
            None,
            format!("pack {} has invalid header", path.display()),
        ));
        return Ok(scan);
    }
    scan.pack_header_valid = true;

    let mut offset = PACK_MAGIC.len() as u64;
    scan.scan_boundary = offset;
    while offset < file_len {
        let record_offset = offset;
        let mut magic = [0u8; 4];
        let read = read_fully_or_short(
            &mut file,
            &path,
            &mut magic,
            pacing,
            &mut scan.bytes_scanned,
        )?;
        offset = offset.saturating_add(read as u64);
        if read < magic.len() {
            scan.scan_boundary = record_offset;
            scan.corrupt_offsets.insert(record_offset);
            scan.findings.push(finding(
                ScrubFindingKind::TruncatedRecord,
                Some(pack_id),
                Some(record_offset),
                None,
                None,
                None,
                format!(
                    "pack {} offset {record_offset} ended inside record magic",
                    path.display()
                ),
            ));
            break;
        }
        if magic != RECORD_MAGIC {
            scan.scan_boundary = record_offset;
            scan.corrupt_offsets.insert(record_offset);
            scan.findings.push(finding(
                ScrubFindingKind::InvalidRecordMagic,
                Some(pack_id),
                Some(record_offset),
                None,
                None,
                None,
                format!(
                    "pack {} offset {record_offset} has invalid record magic",
                    path.display()
                ),
            ));
            break;
        }

        let mut stored_hash = [0u8; crate::BLAKE3_HASH_LEN];
        let read = read_fully_or_short(
            &mut file,
            &path,
            &mut stored_hash,
            pacing,
            &mut scan.bytes_scanned,
        )?;
        offset = offset.saturating_add(read as u64);
        if read < stored_hash.len() {
            scan.scan_boundary = record_offset;
            scan.corrupt_offsets.insert(record_offset);
            scan.findings.push(finding(
                ScrubFindingKind::TruncatedRecord,
                Some(pack_id),
                Some(record_offset),
                None,
                None,
                None,
                format!(
                    "pack {} offset {record_offset} ended inside record hash",
                    path.display()
                ),
            ));
            break;
        }
        let stored_hash = BlobHash::from_bytes(stored_hash);

        let mut len = [0u8; 8];
        let read =
            read_fully_or_short(&mut file, &path, &mut len, pacing, &mut scan.bytes_scanned)?;
        offset = offset.saturating_add(read as u64);
        if read < len.len() {
            scan.scan_boundary = record_offset;
            scan.corrupt_offsets.insert(record_offset);
            scan.findings.push(finding(
                ScrubFindingKind::TruncatedRecord,
                Some(pack_id),
                Some(record_offset),
                Some(stored_hash),
                None,
                None,
                format!(
                    "pack {} offset {record_offset} ended inside record length",
                    path.display()
                ),
            ));
            break;
        }
        let len = u64::from_le_bytes(len);
        let body_start = record_offset.saturating_add(RECORD_HEADER_LEN);
        if len > file_len.saturating_sub(body_start) {
            scan.scan_boundary = record_offset;
            scan.corrupt_offsets.insert(record_offset);
            // Coordinates are fully known here: record them so repair can
            // keep a quarantined claim for this record locatable even when
            // the index cannot supply the entry.
            scan.corrupt_records.push((
                ScannedRecord {
                    pack_id,
                    offset: record_offset,
                    hash: stored_hash,
                    len,
                },
                stored_hash,
            ));
            scan.findings.push(finding(
                ScrubFindingKind::TruncatedRecord,
                Some(pack_id),
                Some(record_offset),
                Some(stored_hash),
                None,
                None,
                format!(
                    "pack {} offset {record_offset} body length {len} extends past EOF",
                    path.display()
                ),
            ));
            break;
        }

        scan.records_scanned += 1;
        let actual = read_body_hash(&mut file, &path, len, pacing, &mut scan.bytes_scanned)?;
        offset = offset.saturating_add(len);
        if actual != stored_hash {
            scan.corrupt_offsets.insert(record_offset);
            scan.findings.push(finding(
                ScrubFindingKind::HashMismatch,
                Some(pack_id),
                Some(record_offset),
                Some(stored_hash),
                Some(stored_hash),
                Some(actual),
                format!(
                    "pack {} offset {record_offset} bytes hash to {actual}, not {stored_hash}",
                    path.display()
                ),
            ));
            scan.corrupt_records.push((
                ScannedRecord {
                    pack_id,
                    offset: record_offset,
                    hash: stored_hash,
                    len,
                },
                actual,
            ));
            // The record's structure was walked even though its content is
            // corrupt: the sequential scan boundary advances past it.
            scan.scan_boundary = offset;
            continue;
        }

        scan.valid_records.push(ScannedRecord {
            pack_id,
            offset: record_offset,
            hash: stored_hash,
            len,
        });
        scan.scan_boundary = offset;
    }
    Ok(scan)
}

fn read_fully_or_short(
    file: &mut File,
    path: &Path,
    buf: &mut [u8],
    pacing: &mut PacingTracker,
    bytes_scanned: &mut u64,
) -> Result<usize> {
    let mut filled = 0usize;
    while filled < buf.len() {
        let read_len = pacing.next_read_len(buf.len() - filled);
        match file.read(&mut buf[filled..filled + read_len]) {
            Ok(0) => break,
            Ok(n) => {
                pacing.account(n as u64);
                *bytes_scanned = bytes_scanned.saturating_add(n as u64);
                filled += n;
            }
            Err(err) => {
                return Err(local::io_error(
                    err,
                    format!("read pack {}", path.display()),
                ));
            }
        }
    }
    Ok(filled)
}

fn read_body_hash(
    file: &mut File,
    path: &Path,
    len: u64,
    pacing: &mut PacingTracker,
    bytes_scanned: &mut u64,
) -> Result<BlobHash> {
    let mut hasher = blake3::Hasher::new();
    let mut remaining = len;
    let mut buf = vec![0u8; DEFAULT_SCAN_CHUNK];
    while remaining > 0 {
        let requested = remaining.min(DEFAULT_SCAN_CHUNK as u64) as usize;
        let read_len = pacing.next_read_len(requested);
        match file.read(&mut buf[..read_len]) {
            Ok(0) => {
                return Err(local::corruption(format!(
                    "pack {} ended while hashing record body",
                    path.display()
                )));
            }
            Ok(n) => {
                pacing.account(n as u64);
                *bytes_scanned = bytes_scanned.saturating_add(n as u64);
                hasher.update(&buf[..n]);
                remaining -= n as u64;
            }
            Err(err) => {
                return Err(local::io_error(
                    err,
                    format!("read pack body {}", path.display()),
                ));
            }
        }
    }
    Ok(BlobHash::from_bytes(*hasher.finalize().as_bytes()))
}

/// Direct, paced verification of one indexed record: framing fields must
/// match the index entry and the body must hash to the expected address.
/// Reads through the same [`PacingTracker`] as sequential scanning so
/// direct verification honors the scrub I/O budget.
fn verify_record_paced(
    packs_dir: &Path,
    expected_hash: &BlobHash,
    entry: PackEntry,
    pacing: &mut PacingTracker,
    bytes_scanned: &mut u64,
) -> Result<()> {
    use std::io::{Seek, SeekFrom};

    let path = local::pack_path(packs_dir, entry.pack_id);
    let mut file = File::open(&path)
        .map_err(|err| local::io_error(err, format!("open pack {}", path.display())))?;
    file.seek(SeekFrom::Start(entry.offset))
        .map_err(|err| local::io_error(err, format!("seek pack {}", path.display())))?;

    let mut magic = [0u8; 4];
    let read = read_fully_or_short(&mut file, &path, &mut magic, pacing, bytes_scanned)?;
    if read < magic.len() || magic != RECORD_MAGIC {
        return Err(local::corruption(format!(
            "pack {} offset {} has invalid record magic",
            path.display(),
            entry.offset
        )));
    }
    let mut stored_hash = [0u8; crate::BLAKE3_HASH_LEN];
    let read = read_fully_or_short(&mut file, &path, &mut stored_hash, pacing, bytes_scanned)?;
    if read < stored_hash.len() || BlobHash::from_bytes(stored_hash) != *expected_hash {
        return Err(local::corruption(format!(
            "pack {} offset {} stores a different hash",
            path.display(),
            entry.offset
        )));
    }
    let mut len = [0u8; 8];
    let read = read_fully_or_short(&mut file, &path, &mut len, pacing, bytes_scanned)?;
    let len = u64::from_le_bytes(len);
    if read < 8 || len != entry.len {
        return Err(local::corruption(format!(
            "pack {} offset {} record length {len} does not match index len {}",
            path.display(),
            entry.offset,
            entry.len
        )));
    }
    let actual = read_body_hash(&mut file, &path, len, pacing, bytes_scanned)?;
    if actual != *expected_hash {
        return Err(local::corruption(format!(
            "pack {} offset {} bytes hash to {actual}, not {expected_hash}",
            path.display(),
            entry.offset
        )));
    }
    Ok(())
}

async fn merge_pack_findings(
    report: &mut ScrubReport,
    pack_scan: &PackScan,
    index: &HashMap<BlobHash, PackEntry>,
    index_offsets: &HashSet<(u64, u64)>,
    store: &LocalPackStore,
    packs_dir: &Path,
    pacing: &mut PacingTracker,
) -> Result<()> {
    report.corrupt_records += pack_scan.findings.len();
    report.findings.extend(pack_scan.findings.clone());

    let valid_by_offset = pack_scan
        .valid_records
        .iter()
        .map(|record| (record.offset, record))
        .collect::<BTreeMap<_, _>>();

    let mut quarantine = Vec::new();
    let mut direct_verify: Vec<(BlobHash, PackEntry)> = Vec::new();
    for (hash, entry) in index {
        if entry.pack_id != pack_scan.pack_id {
            continue;
        }
        // A corrupt/truncated PACK HEADER discredits the whole file: unlike a
        // single corrupt record behind a valid header (whose siblings get
        // direct verification), header corruption means something rewrote the
        // pack's front matter, and the store itself would refuse this file as
        // an active pack. Fail closed: quarantine every indexed record.
        if !pack_scan.pack_header_valid {
            quarantine.push((*hash, QuarantineCheck::CorruptPackHeader(*entry)));
            continue;
        }
        match valid_by_offset.get(&entry.offset) {
            Some(record) if record.hash != *hash => {
                report.corrupt_records += 1;
                report.findings.push(finding(
                    ScrubFindingKind::IndexHashMismatch,
                    Some(entry.pack_id),
                    Some(entry.offset),
                    Some(*hash),
                    Some(*hash),
                    Some(record.hash),
                    format!(
                        "index maps {hash} to pack {} offset {}, but record stores {}",
                        entry.pack_id, entry.offset, record.hash
                    ),
                ));
                quarantine.push((*hash, QuarantineCheck::CorruptRecord(*entry)));
            }
            Some(record) if record.len != entry.len => {
                report.corrupt_records += 1;
                report.findings.push(finding(
                    ScrubFindingKind::IndexLengthMismatch,
                    Some(entry.pack_id),
                    Some(entry.offset),
                    Some(*hash),
                    Some(*hash),
                    None,
                    format!(
                        "index maps {hash} to len {}, but record len is {}",
                        entry.len, record.len
                    ),
                ));
                quarantine.push((*hash, QuarantineCheck::CorruptRecord(*entry)));
            }
            Some(_) => {
                report.records_verified += 1;
            }
            None if pack_scan.corrupt_offsets.contains(&entry.offset) => {
                quarantine.push((*hash, QuarantineCheck::CorruptRecord(*entry)));
            }
            None if entry.offset >= pack_scan.scan_boundary => {
                // The sequential scan stopped at an earlier corrupt segment
                // and never reached this offset. Records past a corrupt
                // segment are still readable by direct index offset, so
                // verify this one directly instead of blanket-quarantining
                // healthy data (data-availability rule).
                direct_verify.push((*hash, *entry));
            }
            None if pack_scan.pack_header_valid => {
                report.corrupt_records += 1;
                report.findings.push(finding(
                    ScrubFindingKind::MissingIndexedRecord,
                    Some(entry.pack_id),
                    Some(entry.offset),
                    Some(*hash),
                    Some(*hash),
                    None,
                    format!(
                        "index maps {hash} to pack {} offset {}, but no record starts there",
                        entry.pack_id, entry.offset
                    ),
                ));
                quarantine.push((*hash, QuarantineCheck::CorruptRecord(*entry)));
            }
            None => {
                quarantine.push((*hash, QuarantineCheck::CorruptRecord(*entry)));
            }
        }
    }

    if !direct_verify.is_empty() {
        let packs_dir = packs_dir.to_path_buf();
        let batch = direct_verify.clone();
        let current_pacing = pacing.clone();
        let (verdicts, next_pacing, direct_bytes) = tokio::task::spawn_blocking(move || {
            let mut pacing = current_pacing;
            let mut bytes = 0u64;
            let verdicts = batch
                .into_iter()
                .map(|(hash, entry)| {
                    let verdict =
                        verify_record_paced(&packs_dir, &hash, entry, &mut pacing, &mut bytes);
                    (hash, entry, verdict)
                })
                .collect::<Vec<_>>();
            Ok::<_, Error>((verdicts, pacing, bytes))
        })
        .await
        .map_err(|err| {
            Error::storage(
                StorageErrorKind::Other,
                format!("local scrub direct-verify task: {err}"),
            )
        })??;
        *pacing = next_pacing;
        report.bytes_scanned = report.bytes_scanned.saturating_add(direct_bytes);
        for (hash, entry, verdict) in verdicts {
            match verdict {
                Ok(()) => report.records_verified += 1,
                Err(err) => {
                    report.corrupt_records += 1;
                    report.findings.push(finding(
                        ScrubFindingKind::HashMismatch,
                        Some(entry.pack_id),
                        Some(entry.offset),
                        Some(hash),
                        Some(hash),
                        None,
                        format!(
                            "direct verification of {hash} at pack {} offset {} failed: {err}",
                            entry.pack_id, entry.offset
                        ),
                    ));
                    quarantine.push((hash, QuarantineCheck::CorruptRecord(entry)));
                }
            }
        }
    }

    for record in &pack_scan.valid_records {
        if !index_offsets.contains(&(record.pack_id, record.offset)) {
            report.orphan_records += 1;
            report.findings.push(finding(
                ScrubFindingKind::OrphanRecord,
                Some(record.pack_id),
                Some(record.offset),
                Some(record.hash),
                None,
                None,
                format!(
                    "pack {} offset {} contains hash {} with no live index entry",
                    record.pack_id, record.offset, record.hash
                ),
            ));
        }
    }

    if quarantine.is_empty() {
        return Ok(());
    }
    let requests = quarantine
        .into_iter()
        .collect::<BTreeMap<BlobHash, QuarantineCheck>>();
    let mut outcome = store
        .quarantine_hashes(requests.into_iter().collect())
        .await?;
    report.bytes_scanned = report
        .bytes_scanned
        .saturating_add(outcome.revalidation_bytes);
    pacing.account(outcome.revalidation_bytes);
    if !outcome.inserted.is_empty() {
        report.quarantined_hashes.append(&mut outcome.inserted);
        report.quarantined_hashes.sort();
        report.quarantined_hashes.dedup();
    }
    Ok(())
}

mod rebuild;

#[cfg(test)]
mod tests;
