//! Blob lifecycle and garbage-collection seam.
//!
//! `BlobStore::release` only drops one tenant-local claim. `BlobGc` is the
//! explicit lifecycle protocol that compares local byte-plane entries against
//! metadata/snapshot roots, applies a grace window for in-flight writes, and
//! then compacts packs.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use nimbus_core::{Clock, Result, SystemClock};

use crate::local::{GcSummary, LocalBlobEntry};
use crate::pins::BlobPinRegistry;
use crate::store::BlobStore;
use crate::{BlobHash, CompactionStats, LocalPackStore};

/// Provides the live blob roots for one tenant.
#[async_trait]
pub trait BlobGcRoots: Send + Sync {
    async fn live_blob_hashes(&self) -> Result<BTreeSet<BlobHash>>;
}

/// Static root provider used by tests and by early single-process callers.
#[derive(Clone, Debug, Default)]
pub struct StaticBlobRoots {
    live: BTreeSet<BlobHash>,
}

impl StaticBlobRoots {
    pub fn new(live: impl IntoIterator<Item = BlobHash>) -> Self {
        Self {
            live: live.into_iter().collect(),
        }
    }
}

#[async_trait]
impl BlobGcRoots for StaticBlobRoots {
    async fn live_blob_hashes(&self) -> Result<BTreeSet<BlobHash>> {
        Ok(self.live.clone())
    }
}

/// Unions several [`BlobGcRoots`] providers into one root set.
///
/// The sweep's mark set is the union of every independently-owned root source
/// — committed object manifests, retained snapshots, and in-flight multipart
/// upload parts (all enumerated above the `nimbus-blob` dependency fence, in
/// `nimbus-object-storage`). Each provider contributes its hashes; a blob is
/// rooted if ANY provider names it.
#[derive(Clone, Default)]
pub struct CompositeBlobRoots {
    providers: Vec<Arc<dyn BlobGcRoots>>,
}

impl CompositeBlobRoots {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a provider to the union.
    pub fn with(mut self, provider: Arc<dyn BlobGcRoots>) -> Self {
        self.providers.push(provider);
        self
    }
}

#[async_trait]
impl BlobGcRoots for CompositeBlobRoots {
    async fn live_blob_hashes(&self) -> Result<BTreeSet<BlobHash>> {
        let mut union = BTreeSet::new();
        for provider in &self.providers {
            union.extend(provider.live_blob_hashes().await?);
        }
        Ok(union)
    }
}

/// Why the sweep keeps or reclaims one blob. Exactly one class applies per
/// entry, and **only [`RetentionClass::Reclaim`] releases the blob** — this
/// makes "the sweep released a rooted/pinned/quarantined blob" structurally
/// unrepresentable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RetentionClass {
    /// Named by a mark-root provider (manifest/snapshot/multipart).
    Rooted,
    /// Held by a write-intent pin.
    Pinned,
    /// Held by an in-flight backup's pin window.
    BackupHeld,
    /// Quarantined by scrub — retained at ANY age; reclamation is the
    /// repair/release decision's, not the sweep's.
    Quarantined,
    /// Written recently enough to be inside the grace window.
    Grace,
    /// Unreferenced, unpinned, unquarantined, past grace — reclaim.
    Reclaim,
}

/// Classifies one live entry. Precedence is total and ordered:
/// Rooted > Pinned > BackupHeld > **Quarantined (before Grace)** > Grace >
/// Reclaim. Quarantined outranks Grace so a past-grace quarantined blob is
/// still retained.
fn classify(
    entry: &LocalBlobEntry,
    roots: &BTreeSet<BlobHash>,
    snapshot_position: (u64, u64),
    now_millis: u64,
    grace_millis: u64,
    pins: &BlobPinRegistry,
    backups: &BlobPinRegistry,
) -> RetentionClass {
    if roots.contains(&entry.hash) {
        RetentionClass::Rooted
    } else if pins.is_held(&entry.hash) {
        RetentionClass::Pinned
    } else if backups.is_held(&entry.hash) {
        RetentionClass::BackupHeld
    } else if entry.quarantined {
        RetentionClass::Quarantined
    } else if entry.position >= snapshot_position {
        // Root-snapshot boundary: the root set was enumerated when the
        // store's append position was `snapshot_position`, so it can say
        // nothing about entries appended at or after it. An entry that
        // landed mid-sweep (e.g. a concurrent put whose roots/pins resolve
        // after our snapshot) is retained UNCONDITIONALLY — even at zero
        // grace — and re-judged by the next sweep. The position marker is
        // strictly monotonic and clock-free: a millisecond snapshot would
        // either race same-tick writes (strict >) or leak same-tick
        // pre-snapshot entries forever under a non-advancing clock (>=).
        RetentionClass::Grace
    } else if now_millis.saturating_sub(entry.written_at_millis) < grace_millis {
        // Age-based grace: an entry stamped at or after `now` (clock
        // regression) has age 0 and is retained by any positive grace window.
        RetentionClass::Grace
    } else {
        RetentionClass::Reclaim
    }
}

/// Result of a mark-and-sweep pass.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BlobGcReport {
    pub referenced_retained: usize,
    pub grace_retained: usize,
    /// Retained by an active [`BlobPinRegistry`] write-intent hold (GR9),
    /// past grace and unrooted. The fallback the pin arm covers for is
    /// exactly the put→pin ordering window; see `pins` module docs.
    pub intent_retained: usize,
    /// Retained by an in-flight backup's pin window (`with_backup_pins`).
    pub backup_retained: usize,
    /// Retained because quarantined — the sweep never reclaims a quarantined
    /// blob; repair or explicit release owns that decision.
    pub quarantine_retained: usize,
    pub swept: usize,
    pub compaction: CompactionStats,
}

/// Mark-and-sweep lifecycle coordinator for local packs.
pub struct BlobGc<R> {
    store: LocalPackStore,
    roots: R,
    grace_window: Duration,
    clock: Arc<dyn Clock>,
    pins: BlobPinRegistry,
    backups: BlobPinRegistry,
}

impl<R> BlobGc<R>
where
    R: BlobGcRoots,
{
    pub fn new(store: LocalPackStore, roots: R, grace_window: Duration) -> Self {
        Self {
            store,
            roots,
            grace_window,
            clock: Arc::new(SystemClock),
            pins: BlobPinRegistry::new(),
            backups: BlobPinRegistry::new(),
        }
    }

    /// Overrides the sweep-cutoff clock (e.g. `ManualClock` for
    /// deterministic tests). Defaults to the real system clock.
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// Shares a write-intent [`BlobPinRegistry`] with this GC so `sweep`
    /// honors holds taken by other components (e.g. a sandbox volume
    /// snapshotting onto the same byte plane). Defaults to a private,
    /// always-empty registry, so `sweep`'s pin check costs one cheap map
    /// lookup per entry and changes nothing when no caller ever pins.
    pub fn with_pins(mut self, pins: BlobPinRegistry) -> Self {
        self.pins = pins;
        self
    }

    /// Shares the in-flight-backup [`BlobPinRegistry`] with this GC so `sweep`
    /// retains the roots an active backup is reading. Must be the SAME
    /// registry instance a concurrent
    /// [`crate::ObjectBackup::export_bundle_with_pins`] pins against —
    /// otherwise the backup-safety window is silently lost. Kept distinct from
    /// [`Self::with_pins`] so `backup_retained` and `intent_retained` report
    /// separately.
    pub fn with_backup_pins(mut self, backups: BlobPinRegistry) -> Self {
        self.backups = backups;
        self
    }

    /// Runs mark-and-sweep over local blobs and then compacts dead pack bytes.
    ///
    /// Each entry is classified exactly once ([`classify`]); only the
    /// [`RetentionClass::Reclaim`] arm releases a blob. The resulting summary
    /// is recorded on the store for [`LocalPackStore::stats`].
    pub async fn sweep(&self) -> Result<BlobGcReport> {
        // Snapshot the store's append position BEFORE enumerating roots:
        // entries appended after this point are outside the root set's
        // authority and classify() keeps them unconditionally (see the
        // snapshot-boundary arm).
        let snapshot = self.store.write_position()?;
        let roots = self.roots.live_blob_hashes().await?;
        let now = self.clock.now_millis();
        let grace_millis = self.grace_window.as_millis() as u64;
        let mut report = BlobGcReport::default();

        for entry in self.store.live_entries()? {
            match classify(
                &entry,
                &roots,
                snapshot,
                now,
                grace_millis,
                &self.pins,
                &self.backups,
            ) {
                RetentionClass::Rooted => report.referenced_retained += 1,
                RetentionClass::Pinned => report.intent_retained += 1,
                RetentionClass::BackupHeld => report.backup_retained += 1,
                RetentionClass::Quarantined => report.quarantine_retained += 1,
                RetentionClass::Grace => report.grace_retained += 1,
                RetentionClass::Reclaim => {
                    self.store.release(&entry.hash).await?;
                    report.swept += 1;
                }
            }
        }

        report.compaction = self.store.compact().await?;
        self.store.set_last_gc(GcSummary {
            referenced_retained: report.referenced_retained,
            intent_retained: report.intent_retained,
            backup_retained: report.backup_retained,
            quarantine_retained: report.quarantine_retained,
            grace_retained: report.grace_retained,
            swept: report.swept,
            packs_removed: report.compaction.packs_removed,
            bytes_rewritten: report.compaction.bytes_rewritten,
            at_millis: now,
        })?;
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use std::fs::OpenOptions;
    use std::io::{Read, Seek, SeekFrom, Write};
    use std::ops::Range;

    use bytes::Bytes;
    use nimbus_core::{ManualClock, StorageErrorKind, Timestamp};

    use super::*;
    use crate::local::RECORD_MAGIC;
    use crate::store::ByteStream;
    use crate::{BackupRequest, BlobStore, KeyEscrow, LocalPackScrubber, ObjectBackup};

    fn open_temp(target: u64) -> (tempfile::TempDir, LocalPackStore) {
        let dir = tempfile::tempdir().expect("tempdir should create");
        let store =
            LocalPackStore::open_with_pack_target(dir.path(), target).expect("store should open");
        (dir, store)
    }

    /// Puts `payload`, corrupts its record body on disk, then scrubs so the
    /// blob is quarantined. Returns the (still-indexed, fail-closed) hash.
    async fn quarantine_one(
        dir: &tempfile::TempDir,
        store: &LocalPackStore,
        payload: &'static [u8],
    ) -> BlobHash {
        let hash = store.put(Bytes::from_static(payload)).await.unwrap();
        let entry = store
            .blocking(move |state| Ok(state.index.get(&hash).copied().expect("indexed")))
            .await
            .unwrap();
        let path = dir
            .path()
            .join("packs")
            .join(format!("pack-{:016}.npack", entry.pack_id));
        let body_offset =
            entry.offset + RECORD_MAGIC.len() as u64 + crate::BLAKE3_HASH_LEN as u64 + 8;
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        file.seek(SeekFrom::Start(body_offset)).unwrap();
        let mut byte = [0u8; 1];
        file.read_exact(&mut byte).unwrap();
        byte[0] ^= 0xff;
        file.seek(SeekFrom::Start(body_offset)).unwrap();
        file.write_all(&byte).unwrap();
        file.sync_data().unwrap();
        let report = LocalPackScrubber::new(store.clone()).scrub().await.unwrap();
        assert!(
            report.quarantined_hashes.contains(&hash),
            "scrub quarantined the corrupt blob"
        );
        hash
    }

    /// A BlobStore wrapper whose `get` asserts a registry holds the hash — used
    /// to prove the backup safety window covers every sequential read.
    struct AssertHeldStore {
        inner: LocalPackStore,
        backups: BlobPinRegistry,
    }

    #[async_trait]
    impl BlobStore for AssertHeldStore {
        async fn put(&self, bytes: Bytes) -> Result<BlobHash> {
            self.inner.put(bytes).await
        }
        async fn put_stream(&self, src: ByteStream) -> Result<BlobHash> {
            self.inner.put_stream(src).await
        }
        async fn get(&self, hash: &BlobHash) -> Result<Bytes> {
            assert!(
                self.backups.is_held(hash),
                "backup window must hold {hash} for the whole export read"
            );
            self.inner.get(hash).await
        }
        async fn get_stream(&self, hash: &BlobHash) -> Result<ByteStream> {
            self.inner.get_stream(hash).await
        }
        async fn get_range(&self, hash: &BlobHash, range: Range<u64>) -> Result<Bytes> {
            self.inner.get_range(hash, range).await
        }
        async fn has(&self, hash: &BlobHash) -> Result<bool> {
            self.inner.has(hash).await
        }
        async fn release(&self, hash: &BlobHash) -> Result<()> {
            self.inner.release(hash).await
        }
    }

    #[tokio::test]
    async fn referenced_blob_is_never_swept() {
        let (_dir, store) = open_temp(128);
        let hash = store.put(Bytes::from_static(b"referenced")).await.unwrap();
        let gc = BlobGc::new(store.clone(), StaticBlobRoots::new([hash]), Duration::ZERO);

        let report = gc.sweep().await.unwrap();

        assert_eq!(report.referenced_retained, 1);
        assert_eq!(report.swept, 0);
        assert!(store.has(&hash).await.unwrap());
    }

    #[tokio::test]
    async fn partial_upload_without_manifest_is_reclaimed_after_grace_window() {
        let (_dir, store) = open_temp(128);
        let hash = store
            .put(Bytes::from_static(b"partial upload"))
            .await
            .unwrap();
        let gc = BlobGc::new(store.clone(), StaticBlobRoots::default(), Duration::ZERO);

        let report = gc.sweep().await.unwrap();

        assert_eq!(report.swept, 1);
        assert!(!store.has(&hash).await.unwrap());
    }

    #[tokio::test]
    async fn grace_window_retains_recent_unreferenced_blob() {
        let (_dir, store) = open_temp(128);
        let hash = store.put(Bytes::from_static(b"recent")).await.unwrap();
        let gc = BlobGc::new(
            store.clone(),
            StaticBlobRoots::default(),
            Duration::from_secs(60),
        );

        let report = gc.sweep().await.unwrap();

        assert_eq!(report.grace_retained, 1);
        assert_eq!(report.swept, 0);
        assert!(store.has(&hash).await.unwrap());
    }

    #[tokio::test]
    async fn gc_grace_retains_when_clock_regresses() {
        let (_dir, store) = open_temp(128);
        let clock = Arc::new(ManualClock::new(Timestamp(100_000)));
        let store = store.with_clock(clock.clone());
        let hash = store
            .put(Bytes::from_static(b"future write"))
            .await
            .unwrap();

        // The clock regresses far below the write timestamp. Age saturates to
        // zero, so any positive grace window retains the blob.
        let regressed = Arc::new(ManualClock::new(Timestamp(1_000)));
        let gc = BlobGc::new(
            store.clone(),
            StaticBlobRoots::default(),
            Duration::from_secs(60),
        )
        .with_clock(regressed);

        let report = gc.sweep().await.unwrap();

        assert_eq!(report.grace_retained, 1);
        assert_eq!(report.swept, 0);
        assert!(store.has(&hash).await.unwrap());
    }

    #[tokio::test]
    async fn sweep_runs_pack_compaction_after_releasing_dead_blobs() {
        let (dir, store) = open_temp(96);
        let keep = store.put(Bytes::from_static(b"keep this")).await.unwrap();
        let drop_hash = store.put(Bytes::from_static(b"drop this")).await.unwrap();
        let gc = BlobGc::new(store.clone(), StaticBlobRoots::new([keep]), Duration::ZERO);

        let report = gc.sweep().await.unwrap();

        assert_eq!(report.swept, 1);
        assert!(store.has(&keep).await.unwrap());
        assert!(!store.has(&drop_hash).await.unwrap());
        assert!(
            report.compaction.packs_removed >= 1,
            "sweep should compact released pack bytes"
        );
        let pack_count = std::fs::read_dir(dir.path().join("packs"))
            .unwrap()
            .filter_map(std::result::Result::ok)
            .count();
        assert_eq!(pack_count, 1);
    }

    #[tokio::test]
    async fn pinned_blob_past_grace_is_retained_as_intent_not_swept() {
        let (_dir, store) = open_temp(128);
        let clock = Arc::new(ManualClock::new(Timestamp(0)));
        let store = store.with_clock(clock.clone());
        let hash = store.put(Bytes::from_static(b"pinned")).await.unwrap();

        let pins = BlobPinRegistry::new();
        let pin = pins.pin(hash);
        clock.advance(Duration::from_secs(120));

        let gc = BlobGc::new(
            store.clone(),
            StaticBlobRoots::default(),
            Duration::from_secs(60),
        )
        .with_clock(clock.clone())
        .with_pins(pins);

        let report = gc.sweep().await.unwrap();

        assert_eq!(report.intent_retained, 1);
        assert_eq!(report.grace_retained, 0);
        assert_eq!(report.swept, 0);
        assert!(store.has(&hash).await.unwrap());

        drop(pin);
    }

    #[tokio::test]
    async fn dropped_pin_past_grace_and_unrooted_is_swept() {
        let (_dir, store) = open_temp(128);
        let clock = Arc::new(ManualClock::new(Timestamp(0)));
        let store = store.with_clock(clock.clone());
        let hash = store.put(Bytes::from_static(b"was-pinned")).await.unwrap();

        let pins = BlobPinRegistry::new();
        drop(pins.pin(hash));
        clock.advance(Duration::from_secs(120));

        let gc = BlobGc::new(
            store.clone(),
            StaticBlobRoots::default(),
            Duration::from_secs(60),
        )
        .with_clock(clock)
        .with_pins(pins);

        let report = gc.sweep().await.unwrap();

        assert_eq!(report.intent_retained, 0);
        assert_eq!(report.swept, 1);
        assert!(!store.has(&hash).await.unwrap());
    }

    #[tokio::test]
    async fn refcounted_pin_keeps_blob_retained_until_last_guard_drops() {
        let (_dir, store) = open_temp(128);
        let clock = Arc::new(ManualClock::new(Timestamp(0)));
        let store = store.with_clock(clock.clone());
        let hash = store
            .put(Bytes::from_static(b"double-pinned"))
            .await
            .unwrap();

        let pins = BlobPinRegistry::new();
        let first = pins.pin(hash);
        let second = pins.pin(hash);
        clock.advance(Duration::from_secs(120));

        let gc = BlobGc::new(
            store.clone(),
            StaticBlobRoots::default(),
            Duration::from_secs(60),
        )
        .with_clock(clock)
        .with_pins(pins);

        drop(first);
        let report = gc.sweep().await.unwrap();
        assert_eq!(
            report.intent_retained, 1,
            "second guard should still hold the pin"
        );
        assert!(store.has(&hash).await.unwrap());

        drop(second);
        let report = gc.sweep().await.unwrap();
        assert_eq!(report.swept, 1);
        assert!(!store.has(&hash).await.unwrap());
    }

    #[tokio::test]
    async fn gc_leaves_quarantined_packs() {
        let (dir, store) = open_temp(64 * 1024);
        let clock = Arc::new(ManualClock::new(Timestamp(0)));
        let store = store.with_clock(clock.clone());
        let victim = quarantine_one(&dir, &store, b"quarantined bytes").await;

        // Advance well past grace: a quarantined blob is retained regardless of
        // age (Quarantined is classified before Grace).
        clock.advance(Duration::from_secs(3600));
        let gc = BlobGc::new(
            store.clone(),
            StaticBlobRoots::default(),
            Duration::from_secs(60),
        )
        .with_clock(clock);
        let report = gc.sweep().await.unwrap();

        assert_eq!(
            report.quarantine_retained, 1,
            "quarantined blob is retained"
        );
        assert_eq!(
            report.swept, 0,
            "the sweep never reclaims a quarantined blob"
        );
        assert!(store.has(&victim).await.unwrap(), "still indexed");
        let err = store.get(&victim).await.unwrap_err();
        assert_eq!(
            err.storage_kind(),
            Some(StorageErrorKind::Corruption),
            "the quarantine decision is intact (pending release/repair)"
        );
        // Its pack is retained (not deleted by the sweep's compaction).
        assert_eq!(report.compaction.packs_removed, 0);
    }

    #[tokio::test]
    async fn gc_respects_backup_in_progress() {
        let (_dir, store) = open_temp(64 * 1024);
        let clock = Arc::new(ManualClock::new(Timestamp(0)));
        let store = store.with_clock(clock.clone());
        let a = store
            .put(Bytes::from_static(b"backup root a"))
            .await
            .unwrap();
        let b = store
            .put(Bytes::from_static(b"backup root b"))
            .await
            .unwrap();
        clock.advance(Duration::from_secs(3600)); // both are past grace

        let backups = BlobPinRegistry::new();

        // Run a backup export through a wrapper that asserts the window holds
        // each root during its sequential read. Concurrently the GC shares the
        // SAME backups registry.
        let held_store = AssertHeldStore {
            inner: store.clone(),
            backups: backups.clone(),
        };
        let escrow = KeyEscrow::new("tenant", Bytes::from_static(b"wrapped")).unwrap();
        let request = BackupRequest::new(
            [a, b],
            Bytes::from_static(b"manifest"),
            Bytes::from_static(b"commit-log seg"),
            escrow,
        )
        .unwrap();

        // While the export holds the window, a concurrent sweep must retain
        // both roots. We interleave by taking the window explicitly here (the
        // export helper does the same internally; this makes the sweep-during
        // assertion deterministic without threads).
        let window = backups.pin_all([a, b]);
        let gc = BlobGc::new(store.clone(), StaticBlobRoots::default(), Duration::ZERO)
            .with_clock(clock.clone())
            .with_backup_pins(backups.clone());
        let during = gc.sweep().await.unwrap();
        assert_eq!(during.backup_retained, 2, "in-flight backup roots retained");
        assert_eq!(during.swept, 0);

        // The export itself completes with the window held (its get() asserts
        // is_held for each root).
        let bundle = ObjectBackup::export_bundle_with_pins(&held_store, request, &backups)
            .await
            .unwrap();
        assert_eq!(bundle.chunks().len(), 2);

        // Drop the window: the same roots are now reclaimable.
        drop(window);
        let after = gc.sweep().await.unwrap();
        assert_eq!(after.backup_retained, 0);
        assert_eq!(
            after.swept, 2,
            "roots reclaimed once the backup window drops"
        );
        assert!(!store.has(&a).await.unwrap());
        assert!(!store.has(&b).await.unwrap());
    }

    #[tokio::test]
    async fn gc_never_reclaims_pinned_or_rooted() {
        let (_dir, store) = open_temp(64 * 1024);
        let clock = Arc::new(ManualClock::new(Timestamp(0)));
        let store = store.with_clock(clock.clone());
        let manifest_root = store
            .put(Bytes::from_static(b"manifest root"))
            .await
            .unwrap();
        let snapshot_root = store
            .put(Bytes::from_static(b"snapshot root"))
            .await
            .unwrap();
        let pinned = store.put(Bytes::from_static(b"pinned blob")).await.unwrap();
        let doomed = store
            .put(Bytes::from_static(b"unrooted expired"))
            .await
            .unwrap();
        clock.advance(Duration::from_secs(3600)); // all past grace

        // Composite roots union a manifest provider and a snapshot provider.
        let roots = CompositeBlobRoots::new()
            .with(Arc::new(StaticBlobRoots::new([manifest_root])))
            .with(Arc::new(StaticBlobRoots::new([snapshot_root])));
        let pins = BlobPinRegistry::new();
        let _pin = pins.pin(pinned);

        let gc = BlobGc::new(store.clone(), roots, Duration::from_secs(60))
            .with_clock(clock)
            .with_pins(pins);
        let report = gc.sweep().await.unwrap();

        assert_eq!(
            report.referenced_retained, 2,
            "both composite roots retained"
        );
        assert_eq!(report.intent_retained, 1, "pinned retained");
        assert_eq!(
            report.swept, 1,
            "only the unrooted expired blob is reclaimed"
        );
        assert!(store.has(&manifest_root).await.unwrap());
        assert!(store.has(&snapshot_root).await.unwrap());
        assert!(store.has(&pinned).await.unwrap());
        assert!(!store.has(&doomed).await.unwrap());
    }

    /// Roots provider that writes an UNROOTED blob into the store during
    /// enumeration — deterministically reproducing the mid-sweep-write
    /// TOCTOU (roots snapshotted at t0, blob committed at t1 > t0,
    /// classification at t2).
    struct WritesDuringEnumeration {
        store: LocalPackStore,
        payload: Bytes,
        written: std::sync::Mutex<Option<BlobHash>>,
    }

    #[async_trait::async_trait]
    impl BlobGcRoots for WritesDuringEnumeration {
        async fn live_blob_hashes(&self) -> Result<BTreeSet<BlobHash>> {
            let hash = self.store.put(self.payload.clone()).await?;
            *self.written.lock().unwrap() = Some(hash);
            Ok(BTreeSet::new())
        }
    }

    #[tokio::test]
    async fn blob_written_during_root_enumeration_survives_zero_grace_sweep() {
        // Snapshot-boundary rule: the root set has no authority over
        // entries written after it was enumerated, so a just-committed
        // blob survives even a ZERO-grace sweep and is re-judged next time.
        let (_dir, store) = open_temp(64 * 1024);
        let roots = std::sync::Arc::new(WritesDuringEnumeration {
            store: store.clone(),
            payload: Bytes::from_static(b"committed mid-sweep"),
            written: std::sync::Mutex::new(None),
        });
        let gc = BlobGc::new(
            store.clone(),
            CompositeBlobRoots::new().with(roots.clone() as std::sync::Arc<dyn BlobGcRoots>),
            Duration::ZERO,
        );

        let report = gc.sweep().await.unwrap();
        let hash = roots.written.lock().unwrap().expect("provider wrote");

        assert_eq!(report.swept, 0, "mid-sweep write must not be reclaimed");
        assert_eq!(
            report.grace_retained, 1,
            "retained by the snapshot boundary"
        );
        assert!(store.has(&hash).await.unwrap());

        // The NEXT sweep (fresh snapshot, blob now pre-snapshot and
        // unrooted past zero grace) reclaims it — the boundary defers,
        // never leaks.
        let noop_roots = CompositeBlobRoots::new();
        let gc2 = BlobGc::new(store.clone(), noop_roots, Duration::ZERO);
        let report2 = gc2.sweep().await.unwrap();
        assert_eq!(report2.swept, 1);
        assert!(!store.has(&hash).await.unwrap());
    }

    #[tokio::test]
    async fn same_tick_pre_sweep_entry_is_reclaimed_under_frozen_clock() {
        // Review regression (Phase B round 4, P2): the snapshot boundary is
        // POSITION-based, not timestamp-based — under a non-advancing
        // clock, an unrooted blob written before the sweep (same
        // millisecond) is still reclaimed at zero grace instead of being
        // grace-retained forever.
        let (_dir, store) = open_temp(64 * 1024);
        let clock = Arc::new(ManualClock::new(Timestamp(1_000)));
        let store = store.with_clock(clock.clone());
        let hash = store.put(Bytes::from_static(b"same tick")).await.unwrap();

        let gc = BlobGc::new(store.clone(), StaticBlobRoots::default(), Duration::ZERO)
            .with_clock(clock);
        let report = gc.sweep().await.unwrap();
        assert_eq!(report.swept, 1, "pre-sweep same-tick entry must reclaim");
        assert_eq!(report.grace_retained, 0);
        assert!(!store.has(&hash).await.unwrap());
    }
}
