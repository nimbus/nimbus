//! [`LocalPackStore`] - durable append-only local [`BlobStore`] implementation.
//!
//! The pack store keeps one tenant's immutable blobs in a small set of pack
//! files plus an append-only binary index.
//!
//! ## Durability invariants
//!
//! - **Pack bytes are durable before the index record is published.** A crash
//!   may leave an unindexed orphaned record but never a visible blob whose
//!   bytes were not written (`crash_bytes_written_index_missing`).
//! - **The index is append-only with per-record sync.** Index publication is
//!   an append followed by `fdatasync` — the chosen RFS3 invariant (append+sync,
//!   not temp+rename: the index is a log, so a torn tail is truncated at the
//!   next open rather than atomically replaced). Torn trailing records are
//!   self-healed at open; garbage mid-file fails closed as corruption.
//! - **Commit-point files that stay in place go through the durable-replace
//!   recipe** (temp → fdatasync → rename → parent-dir fsync; see
//!   [`crate::disk`]) — today that is the root format marker.
//! - **Directory entries are fsynced at creation commit points** (new pack
//!   files, the index file, compaction removals), so a freshly created file
//!   survives power loss along with its contents.
//!
//! ## Root ownership and the single-writer assumption
//!
//! Opening a root takes an exclusive advisory lock, validates/stamps the
//! format marker, and sweeps crash leftovers — see [`crate::root_guard`].
//! [`LocalPackStore::open_read_only`] is the lock-free inspection mode:
//! point-in-time reads, never mutates (no marker stamping, no cleanup, no
//! torn-tail truncation), and refuses writes with
//! [`StorageErrorKind::Busy`]. Every crash-window guarantee above assumes a
//! **single writer**: in-process writes serialize on the state mutex, and the
//! root lock excludes any second writable handle for the store's lifetime.
//!
//! ## Write failures are fail-stop
//!
//! A failed `fdatasync` can leave the page cache clean while the data never
//! reached disk, so a retried write may falsely succeed (the "fsyncgate"
//! failure mode). Any I/O or corruption error on the write path therefore
//! **poisons** the store: every subsequent mutation fails with
//! [`StorageErrorKind::Unavailable`] until the store is reopened (reopen
//! revalidates everything from disk). Reads stay available — they verify the
//! BLAKE3 content address, so they cannot return wrong bytes.
//!
//! ## Filesystem contract
//!
//! The recipes here assume a local POSIX filesystem where same-directory
//! rename is atomic and `fsync`/`fdatasync` are honored (APFS, ext4, XFS).
//! Network and overlay filesystems (NFS, 9p, overlayfs) are outside the
//! contract; do not place tenant byte roots on them.

use std::collections::{BTreeSet, HashMap};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use async_trait::async_trait;
use bytes::Bytes;
use nimbus_core::{Clock, Error, Result, StorageErrorKind, SystemClock};
use tokio::io::AsyncReadExt;

use crate::disk::{self, SyncObserver};
use crate::hash::BlobHash;
use crate::root_guard::{self, LocalPackStoreOptions, OpenReport, RootLock};
use crate::store::{BlobStore, ByteStream};

pub(crate) const PACK_MAGIC: &[u8] = b"NBLPACK1\n";
pub(crate) const RECORD_MAGIC: &[u8] = b"NBLR";
pub(crate) const INDEX_MAGIC: &[u8] = b"NBLIDX2\n";
pub(crate) const INDEX_PUT: u8 = 1;
pub(crate) const INDEX_RELEASE: u8 = 2;
pub(crate) const DEFAULT_PACK_TARGET_BYTES: u64 = 128 * 1024 * 1024;
const MAX_PACK_TARGET_BYTES: u64 = 512 * 1024 * 1024;

/// Result of a local pack compaction run.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CompactionStats {
    /// Number of live blobs rewritten into fresh packs.
    pub blobs_rewritten: usize,
    /// Number of old pack files removed after rewrite.
    pub packs_removed: usize,
    /// Number of live blob payload bytes rewritten.
    pub bytes_rewritten: u64,
}

/// Live local blob metadata used by the GC seam.
///
/// Self-reports quarantine state and payload size so the sweep and the stats
/// snapshot never take a second lock: both are read straight from the pack
/// index under the same lock that produced the entry (see [`LocalPackStore::
/// live_entries`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalBlobEntry {
    pub hash: BlobHash,
    pub written_at_millis: u64,
    /// Payload byte length of the blob's pack record (excludes framing).
    pub len: u64,
    /// Whether this blob is currently quarantined. A quarantined blob is
    /// retained by GC regardless of roots/grace — the scrub/repair/release
    /// decision owns its reclamation, not the sweep.
    pub quarantined: bool,
    /// Strictly monotonic append position (compaction epoch, pack id,
    /// record offset): later writes always compare greater. The epoch leads
    /// because compaction restructures pack ids wholesale (the empty-store
    /// branch resets them), so a bare (pack_id, offset) is NOT monotonic
    /// across compactions. Clock-free ordering marker for GC's
    /// root-snapshot boundary (a millisecond timestamp cannot distinguish
    /// same-tick writes; a non-advancing test clock would retain them
    /// forever).
    pub position: (u64, u64, u64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PackEntry {
    pub(crate) pack_id: u64,
    pub(crate) offset: u64,
    pub(crate) len: u64,
    pub(crate) written_at_millis: u64,
}

pub(crate) struct LocalPackState {
    pub(crate) packs_dir: PathBuf,
    pub(crate) index_path: PathBuf,
    pub(crate) quarantine_path: PathBuf,
    pub(crate) pack_target_bytes: u64,
    pub(crate) active_pack_id: u64,
    pub(crate) active_pack_bytes: u64,
    pub(crate) index: HashMap<BlobHash, PackEntry>,
    pub(crate) quarantined: HashMap<BlobHash, QuarantineReason>,
    /// Bumped by every compaction. Scrub checkpoints capture it at snapshot
    /// and refuse publication when it moved: a checkpoint derived from a
    /// pre-compaction pack layout must never land after compaction reused
    /// pack ids (cross-process is excluded by the root flock; this guards
    /// the in-process shared state).
    pub(crate) compaction_epoch: u64,
    /// Read-only inspection handle: refuses every mutation.
    pub(crate) read_only: bool,
    /// Set on any write-path I/O/corruption failure; all further mutations
    /// fail until the store is reopened (fail-stop, see module docs).
    pub(crate) poisoned: bool,
    /// This open created `index.log` from missing, so the empty index on disk
    /// is provisional — a rebuild that fails closed removes it (rather than
    /// leaving an authoritative-looking empty index the next open would prune
    /// quarantine against).
    pub(crate) index_provisional: bool,
    /// What this open observed and repaired.
    pub(crate) report: OpenReport,
    /// Receives every durability-relevant sync/rename, in order.
    pub(crate) observer: Arc<dyn SyncObserver>,
    /// Summary of the most recent GC sweep run through this shared state, for
    /// operator status. `None` until a sweep records one.
    pub(crate) last_gc: Option<GcSummary>,
    /// Summary of the most recent scrub run through this shared state.
    pub(crate) last_scrub: Option<ScrubSummary>,
    /// Advisory exclusive root lock; released when the last clone drops.
    /// `None` for read-only handles.
    _lock: Option<RootLock>,
    /// When armed (tests only), `compact_locked` returns an injected error at
    /// the named commit point to exercise crash recovery.
    #[cfg(test)]
    compact_crash_point: Option<CompactionCrashPoint>,
    /// Body bytes actually read off disk by `get_range`, tracked only in test
    /// builds to prove a range read stays bounded instead of pulling the
    /// whole pack record.
    #[cfg(test)]
    body_bytes_read: u64,
}

/// Operator-facing summary of the most recent GC sweep. A derived digest of
/// [`crate::BlobGcReport`] (kept here rather than embedding that type so
/// `local` never depends on `gc`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GcSummary {
    pub referenced_retained: usize,
    pub intent_retained: usize,
    pub backup_retained: usize,
    pub quarantine_retained: usize,
    pub grace_retained: usize,
    pub swept: usize,
    pub packs_removed: usize,
    pub bytes_rewritten: u64,
    pub at_millis: u64,
}

/// Operator-facing summary of the most recent scrub. A derived digest of
/// `ScrubReport` (kept here so `local` never depends on `scrub`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScrubSummary {
    pub packs_scanned: usize,
    pub records_scanned: usize,
    pub findings: usize,
    pub quarantined: usize,
    pub at_millis: u64,
}

/// A consistent, single-lock snapshot of the local pack store's physical
/// accounting plus the last GC/scrub summaries. Every field is computed under
/// one state-lock acquisition (see [`LocalPackStore::stats`]) so it is
/// TOCTOU-free against concurrent puts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LocalPackStats {
    /// Payload bytes of live (non-quarantined) blobs.
    pub live_bytes: u64,
    /// Dead on-disk record space a compaction would reclaim **right now** —
    /// unreferenced record bytes in packs a compaction can rewrite. Excludes
    /// dead space trapped inside retained quarantined packs (see
    /// [`Self::quarantine_blocked_bytes`]); reserves pack + record framing.
    pub reclaimable_bytes: u64,
    /// Dead on-disk record space trapped inside packs that hold a quarantined
    /// blob (compaction retains those packs whole — RFS6). Becomes reclaimable
    /// only once the quarantine is repaired or released.
    pub quarantine_blocked_bytes: u64,
    /// Payload bytes of quarantined blobs (retained until repair/release).
    pub quarantined_bytes: u64,
    /// Number of pack files on disk.
    pub pack_count: usize,
    /// Number of live (non-quarantined) index entries.
    pub live_blob_count: usize,
    /// Number of quarantined index entries.
    pub quarantined_blob_count: usize,
    pub last_gc: Option<GcSummary>,
    pub last_scrub: Option<ScrubSummary>,
}

/// Commit points at which [`compact_locked`] can inject a simulated crash
/// (tests only), to prove no live blob is lost when compaction is interrupted.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CompactionCrashPoint {
    /// After `n` live records have been rewritten + index-published, before
    /// any old pack is unlinked.
    AfterRewrites(usize),
    /// After all rewrites, immediately before removing any old pack.
    BeforePackRemoval,
    /// After `n` old packs have been removed (mid-removal).
    DuringPackRemoval(usize),
    /// In the empty-store branch, after `n` old packs have been removed.
    DuringEmptyStoreRemoval(usize),
}

/// Durable local byte-plane store backed by append-only pack files.
///
/// One [`LocalPackStore`] instance serves one tenant. The store is content
/// addressed by BLAKE3 over the stored bytes and keeps bytes immutable after
/// admission. `release` drops this tenant's current claim on a hash; global
/// reclamation is still owned by the NOS lifecycle/GC seam.
#[derive(Clone)]
pub struct LocalPackStore {
    state: Arc<Mutex<LocalPackState>>,
    clock: Arc<dyn Clock>,
}

impl std::fmt::Debug for LocalPackStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut debug = f.debug_struct("LocalPackStore");
        match self.state.lock() {
            Ok(state) => debug
                .field("packs_dir", &state.packs_dir)
                .field("read_only", &state.read_only)
                .field("live_blobs", &state.index.len())
                .finish(),
            Err(_) => debug.field("state", &"<poisoned>").finish(),
        }
    }
}

/// Process-wide registry of live writable roots (canonical path -> state).
///
/// Exists so same-process opens of one root alias a single state instead of
/// fighting over the flock: the flock guards against a *second process*, the
/// shared mutex serializes writers *within* this process. Entries are weak;
/// dead ones are purged on the next open. This is deliberately the crate's
/// only global — it carries no configuration, only liveness.
fn open_roots() -> &'static Mutex<HashMap<PathBuf, std::sync::Weak<Mutex<LocalPackState>>>> {
    static OPEN_ROOTS: std::sync::OnceLock<
        Mutex<HashMap<PathBuf, std::sync::Weak<Mutex<LocalPackState>>>>,
    > = std::sync::OnceLock::new();
    OPEN_ROOTS.get_or_init(|| Mutex::new(HashMap::new()))
}

impl LocalPackStore {
    /// Opens or creates a local pack store rooted at `root`.
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        Self::open_with_options(root, LocalPackStoreOptions::default())
    }

    /// Opens or creates a local pack store with a custom pack target.
    ///
    /// This is public so operators/tests can choose smaller stores, but the
    /// hard cap preserves the plan's 512 MB launch bound.
    pub fn open_with_pack_target(root: impl AsRef<Path>, pack_target_bytes: u64) -> Result<Self> {
        Self::open_with_options(
            root,
            LocalPackStoreOptions {
                pack_target_bytes,
                ..LocalPackStoreOptions::default()
            },
        )
    }

    /// Opens or creates a local pack store with full [`LocalPackStoreOptions`].
    ///
    /// **Same-process opens of one root share one live store state** (the
    /// state, its mutex, and the advisory flock), so independent components
    /// composing over the same tenant root — a server resolver plus an
    /// in-process backup task, two resolvers over one engine — alias safely
    /// instead of failing. The format marker (and any identity binding) is
    /// still validated on every open, and a **second process** is still
    /// excluded by the flock with [`StorageErrorKind::Busy`]. The first open
    /// fixes `pack_target_bytes` for the shared state's lifetime.
    ///
    /// A fresh (non-shared) open validates or stamps the format marker,
    /// sweeps crash-leftover temp files, and self-heals a torn trailing index
    /// record. See [`LocalPackStore::open_report`].
    pub fn open_with_options(
        root: impl AsRef<Path>,
        options: LocalPackStoreOptions,
    ) -> Result<Self> {
        if options.pack_target_bytes == 0 || options.pack_target_bytes > MAX_PACK_TARGET_BYTES {
            return Err(Error::InvalidInput(format!(
                "local pack target must be 1..={MAX_PACK_TARGET_BYTES} bytes, got {}",
                options.pack_target_bytes
            )));
        }

        let root = root.as_ref().to_path_buf();
        let observer: Arc<dyn SyncObserver> = Arc::new(disk::NoopSyncObserver);
        let clock: Arc<dyn Clock> = Arc::new(SystemClock);

        // Serialize opens through the process-wide registry so two concurrent
        // first-opens of one root cannot race the flock (one would spuriously
        // fail Busy against its own process).
        let mut registry = open_roots()
            .lock()
            .map_err(|_| Error::storage(StorageErrorKind::Other, "open-root registry poisoned"))?;
        registry.retain(|_, weak| weak.strong_count() > 0);

        // Shape check + root creation happen before canonicalization (which
        // requires the directory to exist).
        root_guard::check_writable_root_shape(&root, &options)?;
        let canonical = root
            .canonicalize()
            .map_err(|err| io_error(err, format!("canonicalize blob root {}", root.display())))?;

        if let Some(existing) = registry.get(&canonical).and_then(std::sync::Weak::upgrade) {
            // Alias the live state; still enforce marker/identity binding so
            // a foreign-tenant open is refused even while shared.
            root_guard::validate_marker_for_shared_open(&canonical, &options, &*observer)?;
            return Ok(Self {
                state: existing,
                clock,
            });
        }

        let packs_dir = canonical.join("packs");
        let index_path = canonical.join("index.log");
        let quarantine_path = canonical.join(QUARANTINE_FILE);
        let guard = root_guard::guard_writable_root(
            &canonical,
            &packs_dir,
            &options,
            clock.now_millis(),
            &*observer,
        )?;
        let mut report = guard.report;

        fs::create_dir_all(&packs_dir).map_err(|err| {
            io_error(
                err,
                format!("create local pack directory {}", packs_dir.display()),
            )
        })?;
        let index_was_missing = ensure_index_file(&index_path, &canonical, &*observer)?;

        let index = load_index(
            &index_path,
            IndexLoadMode::HealTornTail,
            &mut report,
            &*observer,
        )?;
        let mut quarantined = load_quarantine(&quarantine_path)?;
        // Prune stale entries: a crash between release's index tombstone and
        // the quarantine side-file rewrite leaves an absent-claim entry that
        // would otherwise poison a future reintroduction of the same content
        // hash (release is the operation that lifts content quarantines).
        //
        // Prune ONLY when the index log CONTAINS RECORDS — a crash-durable
        // signal read from disk, not the in-memory map or a restart-fragile
        // flag. Three cases:
        //   * log has records (PUT/RELEASE) → authoritative log that has been
        //     written to (even if the net live map is empty because every
        //     blob was released); an absent quarantine entry is a genuinely
        //     released claim → prune.
        //   * log is magic-only → either a fresh store (no quarantine to
        //     lose) or a provisional/index-loss state (a crash after a
        //     provisional open, before rebuild) → never prune; preserve the
        //     only claim-tracking evidence.
        // The compaction guard (see `compact_locked`) protects the
        // magic-only-with-claims case from pack deletion until rebuild
        // republishes the claims into a real index.
        let index_has_records = fs::metadata(&index_path)
            .map(|meta| meta.len() > INDEX_MAGIC.len() as u64)
            .unwrap_or(false);
        let before = quarantined.len();
        if index_has_records {
            quarantined.retain(|hash, _| index.contains_key(hash));
        }
        report.stale_quarantine_entries_pruned = before - quarantined.len();
        if report.stale_quarantine_entries_pruned > 0 {
            disk::write_replace_durable(
                &quarantine_path,
                &encode_quarantine(&quarantined),
                &*observer,
            )
            .map_err(|err| {
                io_error(
                    err,
                    format!("prune stale quarantine {}", quarantine_path.display()),
                )
            })?;
        }
        report.quarantine_entries_loaded = quarantined.len();
        // Active = max over index AND disk: a fresh pack rolled by pack
        // retirement (or a crash) has no index entries yet, but it — not the
        // retired/full pack below it — must be selected as active.
        let index_max = index.values().map(|entry| entry.pack_id).max().unwrap_or(0);
        let disk_max = pack_ids_on_disk(&packs_dir)?.into_iter().max().unwrap_or(0);
        let mut active_pack_id = index_max.max(disk_max);
        // A header-corrupt candidate-active pack with ZERO live index
        // references retires at open (roll past it — zero data loss,
        // reported): refusing would brick the root over a file no claim
        // depends on, and appending behind the bad header is worse. A
        // REFERENCED corrupt pack still fails closed below; scrub owns its
        // quarantine + retirement.
        // "Referenced" means referenced by a NON-quarantined live claim:
        // quarantined claims read fail-closed regardless, so a corrupt pack
        // holding only quarantined records (e.g. a crash landed between the
        // quarantine write and the retirement roll) must not brick reopen.
        let disk_max_referenced = index
            .iter()
            .any(|(hash, entry)| entry.pack_id == disk_max && !quarantined.contains_key(hash));
        if active_pack_id == disk_max
            && !disk_max_referenced
            && pack_path(&packs_dir, disk_max).exists()
            && !pack_header_is_valid(&packs_dir, disk_max)
        {
            report.unreferenced_corrupt_packs_retired += 1;
            active_pack_id = disk_max.saturating_add(1);
        }
        let mut active_pack_bytes = ensure_pack_file(&packs_dir, active_pack_id, &*observer)?;
        if active_pack_bytes >= options.pack_target_bytes
            && active_pack_bytes > PACK_MAGIC.len() as u64
        {
            active_pack_id = active_pack_id.saturating_add(1);
            active_pack_bytes = ensure_pack_file(&packs_dir, active_pack_id, &*observer)?;
        }

        let state = Arc::new(Mutex::new(LocalPackState {
            packs_dir,
            index_path,
            quarantine_path,
            pack_target_bytes: options.pack_target_bytes,
            active_pack_id,
            active_pack_bytes,
            index,
            quarantined,
            compaction_epoch: 0,
            read_only: false,
            poisoned: false,
            index_provisional: index_was_missing,
            report,
            observer,
            last_gc: None,
            last_scrub: None,
            _lock: guard.lock,
            #[cfg(test)]
            compact_crash_point: None,
            #[cfg(test)]
            body_bytes_read: 0,
        }));
        registry.insert(canonical, Arc::downgrade(&state));
        Ok(Self { state, clock })
    }

    /// Opens a lock-free, read-only inspection handle over `root`.
    ///
    /// Coexists with a live writable owner (it takes no lock), so reads are a
    /// point-in-time snapshot: a record compacted away after this open reads
    /// as missing. Never mutates the root — no marker stamping, no temp
    /// cleanup, no torn-tail truncation (a torn trailing index record is
    /// ignored in memory) — and every write method fails with
    /// [`StorageErrorKind::Busy`].
    pub fn open_read_only(root: impl AsRef<Path>) -> Result<Self> {
        Self::open_read_only_with_identity(root, None)
    }

    /// Read-only open that validates the root's marker identity when
    /// declared (see `guard_read_only_root_with_identity`): inspection of a
    /// foreign identity's root fails closed instead of serving its blobs.
    pub fn open_read_only_with_identity(
        root: impl AsRef<Path>,
        identity: Option<[u8; crate::BLAKE3_HASH_LEN]>,
    ) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        let packs_dir = root.join("packs");
        let index_path = root.join("index.log");
        let quarantine_path = root.join(QUARANTINE_FILE);
        let observer: Arc<dyn SyncObserver> = Arc::new(disk::NoopSyncObserver);

        root_guard::guard_read_only_root_with_identity(&root, identity)?;

        let mut report = OpenReport::default();
        let index = if index_path.exists() {
            load_index(
                &index_path,
                IndexLoadMode::IgnoreTornTail,
                &mut report,
                &*observer,
            )?
        } else {
            HashMap::new()
        };
        let quarantined = load_quarantine(&quarantine_path)?;
        report.quarantine_entries_loaded = quarantined.len();
        let active_pack_id = index.values().map(|entry| entry.pack_id).max().unwrap_or(0);

        Ok(Self {
            state: Arc::new(Mutex::new(LocalPackState {
                packs_dir,
                index_path,
                quarantine_path,
                pack_target_bytes: DEFAULT_PACK_TARGET_BYTES,
                active_pack_id,
                active_pack_bytes: 0,
                index,
                quarantined,
                compaction_epoch: 0,
                read_only: true,
                poisoned: false,
                index_provisional: false,
                report,
                observer,
                last_gc: None,
                last_scrub: None,
                _lock: None,
                #[cfg(test)]
                compact_crash_point: None,
                #[cfg(test)]
                body_bytes_read: 0,
            })),
            clock: Arc::new(SystemClock),
        })
    }

    /// Overrides the write-timestamp clock (e.g. `ManualClock` for
    /// deterministic GC tests). Defaults to the real system clock.
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// What this open observed and repaired (stale temps, torn index tail).
    pub fn open_report(&self) -> Result<OpenReport> {
        Ok(lock(&self.state)?.report)
    }

    /// Number of live blobs in the local index.
    pub fn len(&self) -> Result<usize> {
        Ok(lock(&self.state)?.index.len())
    }

    /// Whether the local index contains no live blobs.
    pub fn is_empty(&self) -> Result<bool> {
        Ok(self.len()? == 0)
    }

    /// Returns a stable snapshot of live local blob entries.
    ///
    /// Each entry self-reports its payload `len` and whether it is
    /// `quarantined`, both read under this one lock, so the GC sweep decides
    /// retention without any second store call.
    pub fn live_entries(&self) -> Result<Vec<LocalBlobEntry>> {
        let state = lock(&self.state)?;
        let mut entries = state
            .index
            .iter()
            .map(|(hash, entry)| LocalBlobEntry {
                hash: *hash,
                written_at_millis: entry.written_at_millis,
                len: entry.len,
                quarantined: state.quarantined.contains_key(hash),
                position: (state.compaction_epoch, entry.pack_id, entry.offset),
            })
            .collect::<Vec<_>>();
        drop(state);
        entries.sort_by_key(|entry| entry.hash);
        Ok(entries)
    }

    /// The next append position (compaction epoch, active pack id, byte
    /// offset). Entries with `position < write_position()` were durably
    /// indexed before this call; later appends — and post-compaction
    /// relocations, whose pack ids may be REUSED but whose epoch is bumped —
    /// always compare strictly greater. Used by GC to bound a root
    /// snapshot's authority without depending on clock granularity.
    pub fn write_position(&self) -> Result<(u64, u64, u64)> {
        let state = lock(&self.state)?;
        Ok((
            state.compaction_epoch,
            state.active_pack_id,
            state.active_pack_bytes,
        ))
    }

    /// Snapshot of a root's ON-DISK pack layout: (pack id, byte size)
    /// pairs in id order, read directly from the filesystem WITHOUT
    /// opening a store (no index load, no locks). Read-only stats brackets
    /// its whole open+compute sequence with two of these — inequality
    /// means a writer appended, added, or removed packs anywhere in the
    /// window (torn-accounting risk), regardless of whether the affected
    /// packs are referenced by any frozen index.
    pub fn disk_pack_listing(root: impl AsRef<Path>) -> Result<Vec<(u64, u64)>> {
        let packs_dir = root.as_ref().join("packs");
        // An existing-but-uninitialized root (no packs/ yet — e.g. created
        // by an ownership guard that never wrote) is an EMPTY layout, not
        // an inspection error.
        if !packs_dir.exists() {
            return Ok(Vec::new());
        }
        let mut listing = Vec::new();
        for pack_id in pack_ids_on_disk(&packs_dir)? {
            let path = pack_path(&packs_dir, pack_id);
            match fs::metadata(&path) {
                Ok(meta) => listing.push((pack_id, meta.len())),
                // A pack that vanishes between the directory listing and
                // its stat was removed by a concurrent compaction: record
                // the instability (the caller's bracket comparison will
                // differ and retry) instead of failing the probe with Io.
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    listing.push((pack_id, u64::MAX));
                }
                Err(err) => {
                    return Err(io_error(err, format!("stat pack {}", path.display())));
                }
            }
        }
        Ok(listing)
    }

    /// Records the most recent GC sweep summary for [`Self::stats`].
    pub(crate) fn set_last_gc(&self, summary: GcSummary) -> Result<()> {
        lock(&self.state)?.last_gc = Some(summary);
        Ok(())
    }

    /// Records the most recent scrub summary for [`Self::stats`].
    pub(crate) fn set_last_scrub(&self, summary: ScrubSummary) -> Result<()> {
        lock(&self.state)?.last_scrub = Some(summary);
        Ok(())
    }

    /// Returns a consistent physical-accounting snapshot of this store.
    ///
    /// Every metric — including each pack file's on-disk size — is computed
    /// under ONE state-lock acquisition, so it is TOCTOU-free against
    /// concurrent puts (which mutate under the same lock). Runs the disk stat
    /// off the async runtime via [`Self::blocking`].
    pub async fn stats(&self) -> Result<LocalPackStats> {
        self.blocking(|state| stats_locked(&state)).await
    }

    /// Arms a compaction crash-injection point (tests only).
    #[cfg(test)]
    pub(crate) async fn arm_compaction_crash(&self, point: CompactionCrashPoint) -> Result<()> {
        self.blocking(move |mut state| {
            state.compact_crash_point = Some(point);
            Ok(())
        })
        .await
    }

    /// Rewrites live blobs into fresh packs and removes packs no live index entry
    /// references afterward.
    pub async fn compact(&self) -> Result<CompactionStats> {
        self.blocking(|mut state| {
            ensure_writable(&state, "compact")?;
            let result = compact_locked(&mut state);
            poison_on_write_failure(&mut state, &result);
            result
        })
        .await
    }

    pub(crate) async fn blocking<T>(
        &self,
        op: impl FnOnce(MutexGuard<'_, LocalPackState>) -> Result<T> + Send + 'static,
    ) -> Result<T>
    where
        T: Send + 'static,
    {
        let state = self.state.clone();
        tokio::task::spawn_blocking(move || {
            let guard = lock(&state)?;
            op(guard)
        })
        .await
        .map_err(|err| Error::storage(StorageErrorKind::Other, format!("local pack task: {err}")))?
    }

    /// Quarantines hashes, revalidating each finding against CURRENT on-disk
    /// ground truth under the store lock (see [`QuarantineCheck`]) so a stale
    /// scrub snapshot can never quarantine a healthy blob. Returns the hashes
    /// actually inserted.
    /// Retires `pack_id` iff it is the current active pack and its header no
    /// longer validates: rolls to a fresh validated active pack so new puts
    /// never land behind the bad header and reopen selects the fresh pack.
    /// Idempotent and finding-free (used by scrub for an unreferenced corrupt
    /// active pack that has no hashes to quarantine).
    pub(crate) async fn retire_pack_if_active(&self, pack_id: u64) -> Result<()> {
        self.blocking(move |mut state| {
            ensure_writable(&state, "retire pack")?;
            let result = (|state: &mut LocalPackState| {
                if state.active_pack_id == pack_id
                    && !quarantine::pack_header_is_valid(&state.packs_dir, pack_id)
                {
                    let observer = Arc::clone(&state.observer);
                    state.active_pack_id = state.active_pack_id.saturating_add(1);
                    state.active_pack_bytes =
                        ensure_pack_file(&state.packs_dir, state.active_pack_id, &*observer)?;
                }
                Ok(())
            })(&mut state);
            poison_on_write_failure(&mut state, &result);
            result
        })
        .await
    }

    pub(crate) async fn quarantine_hashes(
        &self,
        requests: Vec<(BlobHash, QuarantineCheck)>,
    ) -> Result<quarantine::QuarantineOutcome> {
        self.blocking(move |mut state| {
            ensure_writable(&state, "quarantine")?;
            let result = quarantine_hashes_locked(&mut state, &requests);
            poison_on_write_failure(&mut state, &result);
            result
        })
        .await
    }

    /// Replaces the sync observer so tests can assert fsync ordering.
    #[cfg(test)]
    fn set_sync_observer(&self, observer: Arc<dyn SyncObserver>) {
        lock(&self.state).expect("state lock").observer = observer;
    }

    /// Returns and resets the body bytes read by `get_range` so far.
    ///
    /// Test-only instrumentation: proves a range read stayed bounded to the
    /// requested window instead of materializing the whole pack record.
    #[cfg(test)]
    async fn take_body_bytes_read(&self) -> Result<u64> {
        self.blocking(|mut state| {
            let value = state.body_bytes_read;
            state.body_bytes_read = 0;
            Ok(value)
        })
        .await
    }
}

#[async_trait]
impl BlobStore for LocalPackStore {
    async fn put(&self, bytes: Bytes) -> Result<BlobHash> {
        let clock = Arc::clone(&self.clock);
        self.blocking(move |mut state| {
            ensure_writable(&state, "put")?;
            let result = put_locked(&mut state, bytes, clock.now_millis());
            poison_on_write_failure(&mut state, &result);
            result
        })
        .await
    }

    async fn put_stream(&self, mut src: ByteStream) -> Result<BlobHash> {
        // Refuse before consuming the stream: a read-only or poisoned handle
        // must fail-stop immediately, not after buffering arbitrary input.
        // `put` re-checks under the same lock, so the gap is benign.
        self.blocking(|state| ensure_writable(&state, "put_stream"))
            .await?;
        let mut buf = Vec::new();
        src.read_to_end(&mut buf).await.map_err(|err| {
            Error::storage(StorageErrorKind::Io, format!("read blob stream: {err}"))
        })?;
        self.put(Bytes::from(buf)).await
    }

    async fn get(&self, hash: &BlobHash) -> Result<Bytes> {
        let hash = *hash;
        self.blocking(move |state| read_blob_locked(&state, &hash))
            .await
    }

    async fn get_stream(&self, hash: &BlobHash) -> Result<ByteStream> {
        Ok(Box::new(std::io::Cursor::new(self.get(hash).await?)))
    }

    async fn get_range(&self, hash: &BlobHash, range: Range<u64>) -> Result<Bytes> {
        if range.start > range.end {
            return Err(Error::InvalidInput(format!(
                "range {}..{} out of bounds: start after end",
                range.start, range.end
            )));
        }
        let hash = *hash;
        self.blocking(move |mut state| read_blob_range_locked(&mut state, &hash, range))
            .await
    }

    async fn has(&self, hash: &BlobHash) -> Result<bool> {
        let hash = *hash;
        self.blocking(move |state| Ok(state.index.contains_key(&hash)))
            .await
    }

    async fn release(&self, hash: &BlobHash) -> Result<()> {
        let hash = *hash;
        self.blocking(move |mut state| {
            ensure_writable(&state, "release")?;
            let result = (|state: &mut LocalPackState| {
                let observer = Arc::clone(&state.observer);
                // Append the release tombstone only when there is an indexed
                // claim to drop.
                if state.index.contains_key(&hash) {
                    append_release_index_record(&state.index_path, &hash, &*observer)?;
                    state.index.remove(&hash);
                }
                // ALWAYS lift the quarantine entry, even for an ORPHANED claim
                // (no index entry — the index-loss state). This is the
                // documented recovery: `release` of an unrecoverable
                // quarantined claim must durably clear it so compaction stops
                // returning Busy and rebuild stops failing closed. Without
                // this, an orphaned claim would wedge recovery.
                if state.quarantined.contains_key(&hash) {
                    let mut next = state.quarantined.clone();
                    next.remove(&hash);
                    write_quarantine_locked(state, &next, &*observer)?;
                    state.quarantined = next;
                }
                Ok(())
            })(&mut state);
            poison_on_write_failure(&mut state, &result);
            result
        })
        .await
    }
}

pub(crate) fn ensure_writable(state: &LocalPackState, operation: &str) -> Result<()> {
    if state.read_only {
        return Err(Error::storage(
            StorageErrorKind::Busy,
            format!("read-only inspection handle refuses {operation}"),
        ));
    }
    if state.poisoned {
        return Err(Error::storage(
            StorageErrorKind::Unavailable,
            format!(
                "local pack store is poisoned by an earlier write failure; \
                 refusing {operation} — reopen the store to recover"
            ),
        ));
    }
    Ok(())
}

/// Fail-stop: after an I/O or corruption error on the write path, a retried
/// sync may falsely succeed against a clean-but-lost page cache (fsyncgate),
/// so the store stops accepting mutations until it is reopened.
pub(crate) fn poison_on_write_failure<T>(state: &mut LocalPackState, result: &Result<T>) {
    // Nested `if` rather than a let-chain: nimbus-blob's MSRV is 1.86 and
    // let-chains need 1.88.
    if let Err(err) = result {
        if matches!(
            err.storage_kind(),
            Some(StorageErrorKind::Io) | Some(StorageErrorKind::Corruption)
        ) {
            state.poisoned = true;
        }
    }
}

pub(crate) fn lock(state: &Mutex<LocalPackState>) -> Result<MutexGuard<'_, LocalPackState>> {
    state.lock().map_err(|_| {
        Error::storage(
            StorageErrorKind::Corruption,
            "local pack store lock poisoned",
        )
    })
}

pub(crate) fn io_error(error: std::io::Error, context: impl Into<String>) -> Error {
    Error::storage(StorageErrorKind::Io, format!("{}: {error}", context.into()))
}

pub(crate) fn corruption(message: impl Into<String>) -> Error {
    Error::storage(StorageErrorKind::Corruption, message)
}

pub(crate) fn pack_path(packs_dir: &Path, pack_id: u64) -> PathBuf {
    packs_dir.join(format!("pack-{pack_id:016}.npack"))
}

/// Returns `true` if it created a fresh empty index (the original was
/// missing), `false` if one already existed.
pub(crate) fn ensure_index_file(
    index_path: &Path,
    root: &Path,
    observer: &dyn SyncObserver,
) -> Result<bool> {
    if index_path.exists() {
        return Ok(false);
    }
    if let Some(parent) = index_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| io_error(err, format!("create index parent {}", parent.display())))?;
    }
    // Atomic first-create: the durable-replace recipe means a crash leaves
    // either the complete header file or nothing (a swept temp) — never a
    // torn header that would brick the root as "corruption" on reopen.
    let _ = root;
    disk::write_replace_durable(index_path, INDEX_MAGIC, observer)
        .map_err(|err| io_error(err, format!("create index {}", index_path.display())))?;
    Ok(true)
}

pub(crate) fn ensure_pack_file(
    packs_dir: &Path,
    pack_id: u64,
    observer: &dyn SyncObserver,
) -> Result<u64> {
    fs::create_dir_all(packs_dir)
        .map_err(|err| io_error(err, format!("create packs dir {}", packs_dir.display())))?;
    let path = pack_path(packs_dir, pack_id);
    if !path.exists() {
        // Atomic first-create (see ensure_index_file): complete header or
        // nothing, never a torn header misread as corruption.
        disk::write_replace_durable(&path, PACK_MAGIC, observer)
            .map_err(|err| io_error(err, format!("create pack {}", path.display())))?;
    }
    let len = fs::metadata(&path)
        .map_err(|err| io_error(err, format!("stat pack {}", path.display())))?
        .len();
    if len < PACK_MAGIC.len() as u64 {
        return Err(corruption(format!(
            "pack {} is shorter than header",
            path.display()
        )));
    }
    let mut file =
        File::open(&path).map_err(|err| io_error(err, format!("open pack {}", path.display())))?;
    let mut magic = vec![0u8; PACK_MAGIC.len()];
    file.read_exact(&mut magic)
        .map_err(|err| io_error(err, format!("read pack header {}", path.display())))?;
    if magic != PACK_MAGIC {
        return Err(corruption(format!(
            "pack {} has invalid header",
            path.display()
        )));
    }
    Ok(len)
}

/// How [`load_index`] treats a torn (crash-truncated) trailing record.
#[derive(Clone, Copy, PartialEq, Eq)]
enum IndexLoadMode {
    /// Truncate the file back to the last whole record and report the bytes.
    HealTornTail,
    /// Parse up to the tear and ignore it (read-only handles never write).
    IgnoreTornTail,
}

/// Loads the index, distinguishing a torn tail from mid-file corruption.
///
/// A record that ends early **at end-of-file** is a crash artifact of the
/// append+sync protocol (the append raced power loss) and is recoverable: the
/// blob it named was never acknowledged. Anything else — bad magic, an
/// unknown record tag — is real corruption and fails closed.
fn load_index(
    index_path: &Path,
    mode: IndexLoadMode,
    report: &mut OpenReport,
    observer: &dyn SyncObserver,
) -> Result<HashMap<BlobHash, PackEntry>> {
    let mut file = File::open(index_path)
        .map_err(|err| io_error(err, format!("open index {}", index_path.display())))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|err| io_error(err, format!("read index {}", index_path.display())))?;
    if !bytes.starts_with(INDEX_MAGIC) {
        return Err(corruption(format!(
            "index {} has invalid magic",
            index_path.display()
        )));
    }

    let mut index = HashMap::new();
    let mut cursor = INDEX_MAGIC.len();
    let mut valid_len = cursor;
    while cursor < bytes.len() {
        let record_start = cursor;
        match parse_index_record(&bytes, &mut cursor) {
            Ok((tag, hash)) => match tag {
                IndexRecord::Put(entry) => {
                    index.insert(hash, entry);
                    valid_len = cursor;
                }
                IndexRecord::Release => {
                    index.remove(&hash);
                    valid_len = cursor;
                }
            },
            Err(IndexParseError::TornTail) => {
                let torn = (bytes.len() - record_start) as u64;
                if mode == IndexLoadMode::HealTornTail {
                    truncate_index(index_path, record_start as u64, observer)?;
                    report.torn_index_bytes_truncated = torn;
                }
                break;
            }
            Err(IndexParseError::Corrupt(message)) => {
                return Err(corruption(format!(
                    "index {}: {message}",
                    index_path.display()
                )));
            }
        }
    }
    let _ = valid_len;
    Ok(index)
}

enum IndexRecord {
    Put(PackEntry),
    Release,
}

enum IndexParseError {
    /// The record ran out of bytes at end-of-file (crash-torn append).
    TornTail,
    /// Structurally invalid content (unknown tag).
    Corrupt(String),
}

fn parse_index_record(
    bytes: &[u8],
    cursor: &mut usize,
) -> std::result::Result<(IndexRecord, BlobHash), IndexParseError> {
    let tag = bytes[*cursor];
    *cursor += 1;
    // Fail closed on the tag BEFORE parsing the record body: an unknown tag
    // torn at EOF is still corruption, never a healable torn tail — only
    // known PUT/RELEASE records are eligible for torn-tail healing.
    if tag != INDEX_PUT && tag != INDEX_RELEASE {
        return Err(IndexParseError::Corrupt(format!(
            "unknown record tag {tag}"
        )));
    }
    let hash = read_hash(bytes, cursor)?;
    match tag {
        INDEX_PUT => {
            let pack_id = read_u64(bytes, cursor)?;
            let offset = read_u64(bytes, cursor)?;
            let len = read_u64(bytes, cursor)?;
            let written_at_millis = read_u64(bytes, cursor)?;
            Ok((
                IndexRecord::Put(PackEntry {
                    pack_id,
                    offset,
                    len,
                    written_at_millis,
                }),
                hash,
            ))
        }
        INDEX_RELEASE => Ok((IndexRecord::Release, hash)),
        _ => unreachable!("tag validated above"),
    }
}

fn truncate_index(index_path: &Path, valid_len: u64, observer: &dyn SyncObserver) -> Result<()> {
    let file = OpenOptions::new()
        .write(true)
        .open(index_path)
        .map_err(|err| io_error(err, format!("open index {}", index_path.display())))?;
    file.set_len(valid_len)
        .map_err(|err| io_error(err, format!("truncate torn index {}", index_path.display())))?;
    disk::sync_file_data(&file, index_path, observer).map_err(|err| {
        io_error(
            err,
            format!("sync truncated index {}", index_path.display()),
        )
    })?;
    Ok(())
}

fn read_hash(bytes: &[u8], cursor: &mut usize) -> std::result::Result<BlobHash, IndexParseError> {
    if bytes.len().saturating_sub(*cursor) < crate::BLAKE3_HASH_LEN {
        return Err(IndexParseError::TornTail);
    }
    let mut hash = [0u8; crate::BLAKE3_HASH_LEN];
    hash.copy_from_slice(&bytes[*cursor..*cursor + crate::BLAKE3_HASH_LEN]);
    *cursor += crate::BLAKE3_HASH_LEN;
    Ok(BlobHash::from_bytes(hash))
}

fn read_u64(bytes: &[u8], cursor: &mut usize) -> std::result::Result<u64, IndexParseError> {
    if bytes.len().saturating_sub(*cursor) < 8 {
        return Err(IndexParseError::TornTail);
    }
    let mut raw = [0u8; 8];
    raw.copy_from_slice(&bytes[*cursor..*cursor + 8]);
    *cursor += 8;
    Ok(u64::from_le_bytes(raw))
}

fn put_locked(
    state: &mut LocalPackState,
    bytes: Bytes,
    written_at_millis: u64,
) -> Result<BlobHash> {
    let hash = BlobHash::of(&bytes);
    // Only a RECORD-level quarantine is repairable by re-upload: the fresh
    // bytes produce a fresh verified record. A CONTENT-level quarantine
    // (AEAD failure) would reproduce the identical failure from identical
    // bytes, so it never lifts here.
    let quarantined = state.quarantined.get(&hash) == Some(&QuarantineReason::Record);
    if state.index.contains_key(&hash) && !quarantined {
        return Ok(hash);
    }

    // A quarantined hash's indexed record is corrupt on disk; the caller just
    // handed us verified-good bytes for the same content address, so write a
    // fresh record and lift the quarantine (content-addressed self-repair).
    // Ordering: publish the good record first, un-quarantine last — a crash
    // in between leaves the blob unreadable-but-repairable, never a
    // quarantine lifted without its replacement record being durable.
    // NOTE: a header-corrupt pack is retired (rolled off) at quarantine time
    // (see quarantine_hashes_locked), so a heal here always appends behind a
    // validated active-pack header.
    let entry = append_pack_record(state, &hash, &bytes, written_at_millis)?;
    let observer = Arc::clone(&state.observer);
    append_put_index_record(&state.index_path, &hash, entry, &*observer)?;
    state.index.insert(hash, entry);
    if quarantined {
        let mut next = state.quarantined.clone();
        next.remove(&hash);
        disk::write_replace_durable(
            &state.quarantine_path,
            &encode_quarantine(&next),
            &*observer,
        )
        .map_err(|err| {
            io_error(
                err,
                format!(
                    "lift quarantine {} after re-upload",
                    state.quarantine_path.display()
                ),
            )
        })?;
        state.quarantined = next;
    }
    Ok(hash)
}

fn append_pack_record(
    state: &mut LocalPackState,
    hash: &BlobHash,
    bytes: &[u8],
    written_at_millis: u64,
) -> Result<PackEntry> {
    let observer = Arc::clone(&state.observer);
    let record_len = record_len(bytes.len() as u64);
    // Never append behind a discredited pack header. The scrub's retirement
    // runs off a lock-free snapshot, so a writer holding the mutex here could
    // otherwise land a record in an active pack whose header went corrupt
    // (externally, or a scrub found it but has not retired it yet). Validate
    // under the lock; if the header is bad, QUARANTINE every live claim in
    // that pack (a corrupt header discredits the whole file — those blocks
    // must fail closed on read and stay fail-closed across reopen) and roll
    // to a fresh pack. Routing through the CorruptPackHeader quarantine check
    // reuses the same ground-truth revalidation + retirement as scrub.
    let active_pack_id = state.active_pack_id;
    if !quarantine::pack_header_is_valid(&state.packs_dir, active_pack_id) {
        let requests: Vec<(BlobHash, QuarantineCheck)> = state
            .index
            .iter()
            .filter(|(_, entry)| entry.pack_id == active_pack_id)
            .map(|(hash, entry)| (*hash, QuarantineCheck::CorruptPackHeader(*entry)))
            .collect();
        // Quarantines the live claims AND retires (rolls off) the corrupt
        // active pack, all durably under the lock.
        quarantine::quarantine_hashes_locked(state, &requests)?;
        // If the pack had no live claims, the quarantine batch did not retire
        // it (no CorruptPackHeader request was produced) — roll here.
        if state.active_pack_id == active_pack_id {
            state.active_pack_id = state.active_pack_id.saturating_add(1);
            state.active_pack_bytes =
                ensure_pack_file(&state.packs_dir, state.active_pack_id, &*observer)?;
        }
    }
    if state.active_pack_bytes > PACK_MAGIC.len() as u64
        && state.active_pack_bytes.saturating_add(record_len) > state.pack_target_bytes
    {
        state.active_pack_id = state.active_pack_id.saturating_add(1);
        state.active_pack_bytes =
            ensure_pack_file(&state.packs_dir, state.active_pack_id, &*observer)?;
    }

    let path = pack_path(&state.packs_dir, state.active_pack_id);
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .read(true)
        .open(&path)
        .map_err(|err| io_error(err, format!("open pack {}", path.display())))?;
    let offset = file
        .metadata()
        .map_err(|err| io_error(err, format!("stat pack {}", path.display())))?
        .len();
    file.write_all(RECORD_MAGIC)
        .map_err(|err| io_error(err, format!("write record magic {}", path.display())))?;
    file.write_all(hash.as_bytes())
        .map_err(|err| io_error(err, format!("write record hash {}", path.display())))?;
    file.write_all(&(bytes.len() as u64).to_le_bytes())
        .map_err(|err| io_error(err, format!("write record len {}", path.display())))?;
    file.write_all(bytes)
        .map_err(|err| io_error(err, format!("write record body {}", path.display())))?;
    disk::sync_file_data(&file, &path, &*observer)
        .map_err(|err| io_error(err, format!("sync pack {}", path.display())))?;
    state.active_pack_bytes = offset.saturating_add(record_len);
    Ok(PackEntry {
        pack_id: state.active_pack_id,
        offset,
        len: bytes.len() as u64,
        written_at_millis,
    })
}

fn append_put_index_record(
    index_path: &Path,
    hash: &BlobHash,
    entry: PackEntry,
    observer: &dyn SyncObserver,
) -> Result<()> {
    append_index_record(index_path, observer, |file| {
        file.write_all(&[INDEX_PUT])?;
        file.write_all(hash.as_bytes())?;
        file.write_all(&entry.pack_id.to_le_bytes())?;
        file.write_all(&entry.offset.to_le_bytes())?;
        file.write_all(&entry.len.to_le_bytes())?;
        file.write_all(&entry.written_at_millis.to_le_bytes())?;
        Ok(())
    })
}

fn append_release_index_record(
    index_path: &Path,
    hash: &BlobHash,
    observer: &dyn SyncObserver,
) -> Result<()> {
    append_index_record(index_path, observer, |file| {
        file.write_all(&[INDEX_RELEASE])?;
        file.write_all(hash.as_bytes())?;
        Ok(())
    })
}

fn append_index_record(
    index_path: &Path,
    observer: &dyn SyncObserver,
    write: impl FnOnce(&mut File) -> std::io::Result<()>,
) -> Result<()> {
    let mut file = OpenOptions::new()
        .append(true)
        .open(index_path)
        .map_err(|err| io_error(err, format!("open index {}", index_path.display())))?;
    write(&mut file)
        .map_err(|err| io_error(err, format!("append index {}", index_path.display())))?;
    disk::sync_file_data(&file, index_path, observer)
        .map_err(|err| io_error(err, format!("sync index {}", index_path.display())))
}

fn read_blob_locked(state: &LocalPackState, hash: &BlobHash) -> Result<Bytes> {
    // Liveness first: an unindexed hash is NotFound even if a stale
    // quarantine entry survives; only a LIVE quarantined claim reads as
    // corruption.
    let entry = state
        .index
        .get(hash)
        .copied()
        .ok_or_else(|| Error::NotFound(format!("blob {hash}")))?;
    if state.quarantined.contains_key(hash) {
        return Err(corruption(format!("blob {hash} is quarantined by scrub")));
    }
    read_pack_entry(&state.packs_dir, hash, entry)
}

/// Reads a bounded byte window of `hash` directly from its pack file.
///
/// Trust-model decision: a range read verifies the record framing (magic +
/// stored record hash + stored record len) but does **not** re-verify the
/// whole-blob BLAKE3 content address the way [`read_pack_entry`] (used by
/// `get`) does — recomputing BLAKE3 requires every byte of the blob, which
/// would defeat the point of a bounded read. Corruption of bytes strictly
/// outside a requested window is caught only by a subsequent whole-blob
/// `get()`/compaction pass, not by the range read itself. Pack bytes are
/// trusted based on having been verified once, at write time (the caller
/// already computed `hash` from the exact bytes being appended in
/// `put_locked`). This mirrors the same non-guarantee any bounded-I/O byte
/// plane offers (e.g. S3/GCS ranged GETs are not checksummed against a
/// whole-object digest either).
fn read_blob_range_locked(
    state: &mut LocalPackState,
    hash: &BlobHash,
    range: Range<u64>,
) -> Result<Bytes> {
    let entry = state
        .index
        .get(hash)
        .copied()
        .ok_or_else(|| Error::NotFound(format!("blob {hash}")))?;
    if state.quarantined.contains_key(hash) {
        return Err(corruption(format!("blob {hash} is quarantined by scrub")));
    }
    if range.end > entry.len {
        return Err(Error::InvalidInput(format!(
            "range {}..{} out of bounds for blob of {} bytes",
            range.start, range.end, entry.len
        )));
    }
    let bytes = read_pack_entry_range(&state.packs_dir, hash, entry, range)?;
    #[cfg(test)]
    {
        state.body_bytes_read += bytes.len() as u64;
    }
    Ok(bytes)
}

pub(crate) fn read_pack_entry(
    packs_dir: &Path,
    expected_hash: &BlobHash,
    entry: PackEntry,
) -> Result<Bytes> {
    let path = pack_path(packs_dir, entry.pack_id);
    let mut file =
        File::open(&path).map_err(|err| io_error(err, format!("open pack {}", path.display())))?;
    file.seek(SeekFrom::Start(entry.offset))
        .map_err(|err| io_error(err, format!("seek pack {}", path.display())))?;

    let mut magic = [0u8; 4];
    file.read_exact(&mut magic)
        .map_err(|err| io_error(err, format!("read record magic {}", path.display())))?;
    if magic != RECORD_MAGIC {
        return Err(corruption(format!(
            "pack {} offset {} has invalid record magic",
            path.display(),
            entry.offset
        )));
    }

    let mut stored_hash = [0u8; crate::BLAKE3_HASH_LEN];
    file.read_exact(&mut stored_hash)
        .map_err(|err| io_error(err, format!("read record hash {}", path.display())))?;
    let stored_hash = BlobHash::from_bytes(stored_hash);
    if &stored_hash != expected_hash {
        return Err(corruption(format!(
            "pack {} offset {} stores hash {stored_hash} for requested {expected_hash}",
            path.display(),
            entry.offset
        )));
    }

    let mut len = [0u8; 8];
    file.read_exact(&mut len)
        .map_err(|err| io_error(err, format!("read record len {}", path.display())))?;
    let len = u64::from_le_bytes(len);
    if len != entry.len {
        return Err(corruption(format!(
            "pack {} offset {} len {len} does not match index len {}",
            path.display(),
            entry.offset,
            entry.len
        )));
    }
    let mut bytes = vec![0u8; len as usize];
    file.read_exact(&mut bytes)
        .map_err(|err| io_error(err, format!("read record body {}", path.display())))?;
    let actual = BlobHash::of(&bytes);
    if &actual != expected_hash {
        return Err(corruption(format!(
            "blob {expected_hash} content address mismatch (stored bytes hash to {actual})"
        )));
    }
    Ok(Bytes::from(bytes))
}

/// Reads exactly `range` of a pack record's body, without materializing the
/// rest of the record. See [`read_blob_range_locked`] for the trust-model
/// decision this implies (framing/record-hash checked, whole-blob BLAKE3 not
/// recomputed).
fn read_pack_entry_range(
    packs_dir: &Path,
    expected_hash: &BlobHash,
    entry: PackEntry,
    range: Range<u64>,
) -> Result<Bytes> {
    let path = pack_path(packs_dir, entry.pack_id);
    let mut file =
        File::open(&path).map_err(|err| io_error(err, format!("open pack {}", path.display())))?;
    file.seek(SeekFrom::Start(entry.offset))
        .map_err(|err| io_error(err, format!("seek pack {}", path.display())))?;

    let mut magic = [0u8; 4];
    file.read_exact(&mut magic)
        .map_err(|err| io_error(err, format!("read record magic {}", path.display())))?;
    if magic != RECORD_MAGIC {
        return Err(corruption(format!(
            "pack {} offset {} has invalid record magic",
            path.display(),
            entry.offset
        )));
    }

    let mut stored_hash = [0u8; crate::BLAKE3_HASH_LEN];
    file.read_exact(&mut stored_hash)
        .map_err(|err| io_error(err, format!("read record hash {}", path.display())))?;
    let stored_hash = BlobHash::from_bytes(stored_hash);
    if &stored_hash != expected_hash {
        return Err(corruption(format!(
            "pack {} offset {} stores hash {stored_hash} for requested {expected_hash}",
            path.display(),
            entry.offset
        )));
    }

    let mut len = [0u8; 8];
    file.read_exact(&mut len)
        .map_err(|err| io_error(err, format!("read record len {}", path.display())))?;
    let len = u64::from_le_bytes(len);
    if len != entry.len {
        return Err(corruption(format!(
            "pack {} offset {} len {len} does not match index len {}",
            path.display(),
            entry.offset,
            entry.len
        )));
    }

    if range.start == range.end {
        return Ok(Bytes::new());
    }
    file.seek(SeekFrom::Current(range.start as i64))
        .map_err(|err| io_error(err, format!("seek pack body {}", path.display())))?;
    let range_len = (range.end - range.start) as usize;
    let mut bytes = vec![0u8; range_len];
    file.read_exact(&mut bytes)
        .map_err(|err| io_error(err, format!("read record range {}", path.display())))?;
    Ok(Bytes::from(bytes))
}

fn compact_locked(state: &mut LocalPackState) -> Result<CompactionStats> {
    // Refuse to compact while ANY quarantine claim survives only in the side
    // file with no index entry (the index-loss state, whether or not this
    // handle created the provisional index): those claims' bytes live in
    // packs this function would delete. This is unconditional — after a
    // crash the `index_provisional` flag is gone but the empty index and
    // orphaned claims persist, and compaction must still refuse. There is no
    // deadlock: a non-empty (authoritative) index has its stale entries
    // pruned at open, so by compaction time no claim is orphaned there.
    if state
        .quarantined
        .keys()
        .any(|hash| !state.index.contains_key(hash))
    {
        // Busy, not Corruption: this is a precondition refusal, not a disk
        // fault, so it must NOT poison the store (the caller resolves the
        // condition — rebuild or release — then retries).
        return Err(Error::storage(
            StorageErrorKind::Busy,
            "refusing to compact: quarantine claims exist with no index entry \
             (index-loss state); rebuild or release them first",
        ));
    }
    let observer = Arc::clone(&state.observer);
    state.compaction_epoch = state.compaction_epoch.saturating_add(1);
    // Compaction restructures pack ids wholesale (and the empty-store branch
    // even reuses pack id 0), so any scrub checkpoint taken against the old
    // pack layout is meaningless — and dangerously so on resume, where a
    // reused pack id would be skipped as "already verified". Invalidate it.
    invalidate_scrub_checkpoint(state, &*observer)?;
    let original_packs = pack_ids_on_disk(&state.packs_dir)?;
    if state.index.is_empty() {
        let mut stats = CompactionStats::default();
        for pack_id in original_packs {
            let path = pack_path(&state.packs_dir, pack_id);
            fs::remove_file(&path)
                .map_err(|err| io_error(err, format!("remove empty pack {}", path.display())))?;
            stats.packs_removed += 1;
            #[cfg(test)]
            if state.compact_crash_point
                == Some(CompactionCrashPoint::DuringEmptyStoreRemoval(
                    stats.packs_removed,
                ))
            {
                return Err(injected_compaction_crash());
            }
        }
        state.active_pack_id = 0;
        state.active_pack_bytes =
            ensure_pack_file(&state.packs_dir, state.active_pack_id, &*observer)?;
        // Persist the removals along with the fresh pack's directory entry.
        disk::fsync_dir(&state.packs_dir, &*observer).map_err(|err| {
            io_error(err, format!("sync packs dir {}", state.packs_dir.display()))
        })?;
        return Ok(stats);
    }

    // Quarantined entries are corrupt on disk: they cannot be re-read and
    // rewritten, so compaction skips them. Their index entries stay pointing
    // at the old pack, which keeps that pack in `referenced_packs` below —
    // corrupt bytes are never deleted by compaction, only by an explicit
    // release/repair decision (RFS6 rule).
    let live: Vec<(BlobHash, PackEntry, Bytes)> = state
        .index
        .iter()
        .filter(|(hash, _)| !state.quarantined.contains_key(hash))
        .map(|(hash, entry)| {
            read_pack_entry(&state.packs_dir, hash, *entry).map(|bytes| (*hash, *entry, bytes))
        })
        .collect::<Result<Vec<_>>>()?;

    state.active_pack_id = state.active_pack_id.saturating_add(1);
    state.active_pack_bytes = ensure_pack_file(&state.packs_dir, state.active_pack_id, &*observer)?;

    let mut stats = CompactionStats::default();
    for (hash, old_entry, bytes) in live {
        let entry = append_pack_record(state, &hash, &bytes, old_entry.written_at_millis)?;
        append_put_index_record(&state.index_path, &hash, entry, &*observer)?;
        state.index.insert(hash, entry);
        stats.blobs_rewritten += 1;
        stats.bytes_rewritten += bytes.len() as u64;
        // Crash after this record is rewritten + published but before any old
        // pack is unlinked: recovery must still serve every pre-compaction
        // hash from the intact old packs (the rebuilt records are orphaned
        // and reclaimed by the next compaction).
        #[cfg(test)]
        if state.compact_crash_point
            == Some(CompactionCrashPoint::AfterRewrites(stats.blobs_rewritten))
        {
            return Err(injected_compaction_crash());
        }
    }

    // Crash after all rewrites, before the first old-pack unlink.
    #[cfg(test)]
    if state.compact_crash_point == Some(CompactionCrashPoint::BeforePackRemoval) {
        return Err(injected_compaction_crash());
    }

    let referenced_packs: BTreeSet<u64> = state.index.values().map(|entry| entry.pack_id).collect();
    let mut removed_any = false;
    for pack_id in original_packs {
        if referenced_packs.contains(&pack_id) {
            continue;
        }
        let path = pack_path(&state.packs_dir, pack_id);
        if path.exists() {
            fs::remove_file(&path).map_err(|err| {
                io_error(err, format!("remove compacted pack {}", path.display()))
            })?;
            stats.packs_removed += 1;
            removed_any = true;
            // Crash mid-removal: the rewritten index already references the new
            // pack, so every live blob is served from it; the not-yet-removed
            // old packs are orphans the next compaction reclaims.
            #[cfg(test)]
            if state.compact_crash_point
                == Some(CompactionCrashPoint::DuringPackRemoval(stats.packs_removed))
            {
                return Err(injected_compaction_crash());
            }
        }
    }
    if removed_any {
        // Persist the unlink entries so a power loss cannot resurrect packs
        // the rewritten index no longer references.
        disk::fsync_dir(&state.packs_dir, &*observer).map_err(|err| {
            io_error(err, format!("sync packs dir {}", state.packs_dir.display()))
        })?;
    }
    Ok(stats)
}

/// On-disk framed size of one pack record: magic + stored hash + length field
/// + payload. The single source of truth for pack-space accounting.
pub(crate) fn record_len(payload_len: u64) -> u64 {
    RECORD_MAGIC.len() as u64 + crate::BLAKE3_HASH_LEN as u64 + 8 + payload_len
}

/// The error a compaction crash-injection point returns (tests only) — an
/// `Io` error so it flows through the normal fail-stop path.
#[cfg(test)]
fn injected_compaction_crash() -> Error {
    Error::storage(StorageErrorKind::Io, "injected compaction crash")
}

/// Computes a [`LocalPackStats`] snapshot under the caller's held state lock.
///
/// Dead record space is accounted **per pack** so the two reclaim figures are
/// operator-honest: a pack that holds a quarantined blob is retained whole by
/// compaction (RFS6), so its dead space is `quarantine_blocked_bytes` (not
/// freeable until repair/release); every other pack's dead space is
/// `reclaimable_bytes` (freeable by a compaction right now). Both reserve
/// per-pack `PACK_MAGIC` and per-record framing (`record_len`) and use
/// `saturating_sub` so neither underflows immediately after a compaction.
fn stats_locked(state: &LocalPackState) -> Result<LocalPackStats> {
    let mut live_bytes = 0u64;
    let mut live_blob_count = 0usize;
    let mut quarantined_bytes = 0u64;
    let mut quarantined_blob_count = 0usize;
    // Per-pack: referenced record bytes, and whether the pack holds a
    // quarantined entry (which makes compaction retain it whole).
    let mut per_pack: std::collections::HashMap<u64, (u64, bool)> =
        std::collections::HashMap::new();
    for (hash, entry) in &state.index {
        let quarantined = state.quarantined.contains_key(hash);
        let slot = per_pack.entry(entry.pack_id).or_insert((0, false));
        slot.0 = slot.0.saturating_add(record_len(entry.len));
        slot.1 |= quarantined;
        if quarantined {
            quarantined_bytes = quarantined_bytes.saturating_add(entry.len);
            quarantined_blob_count += 1;
        } else {
            live_bytes = live_bytes.saturating_add(entry.len);
            live_blob_count += 1;
        }
    }

    let pack_ids = pack_ids_on_disk(&state.packs_dir)?;
    let mut reclaimable_bytes = 0u64;
    let mut quarantine_blocked_bytes = 0u64;
    for pack_id in &pack_ids {
        let path = pack_path(&state.packs_dir, *pack_id);
        let size = fs::metadata(&path)
            .map(|meta| meta.len())
            .map_err(|err| io_error(err, format!("stat pack {}", path.display())))?;
        let region = size.saturating_sub(PACK_MAGIC.len() as u64);
        let (referenced, blocked) = per_pack.get(pack_id).copied().unwrap_or((0, false));
        let dead = region.saturating_sub(referenced);
        if blocked {
            quarantine_blocked_bytes = quarantine_blocked_bytes.saturating_add(dead);
        } else {
            reclaimable_bytes = reclaimable_bytes.saturating_add(dead);
        }
    }

    Ok(LocalPackStats {
        live_bytes,
        reclaimable_bytes,
        quarantine_blocked_bytes,
        quarantined_bytes,
        pack_count: pack_ids.len(),
        live_blob_count,
        quarantined_blob_count,
        last_gc: state.last_gc,
        last_scrub: state.last_scrub,
    })
}

pub(crate) fn pack_ids_on_disk(packs_dir: &Path) -> Result<BTreeSet<u64>> {
    let mut pack_ids = BTreeSet::new();
    for entry in fs::read_dir(packs_dir)
        .map_err(|err| io_error(err, format!("read packs dir {}", packs_dir.display())))?
    {
        let entry = entry
            .map_err(|err| io_error(err, format!("read packs dir {}", packs_dir.display())))?;
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        let Some(raw_id) = name
            .strip_prefix("pack-")
            .and_then(|value| value.strip_suffix(".npack"))
        else {
            continue;
        };
        let pack_id = raw_id.parse::<u64>().map_err(|err| {
            corruption(format!(
                "pack file {} has invalid numeric id: {err}",
                entry.path().display()
            ))
        })?;
        pack_ids.insert(pack_id);
    }
    Ok(pack_ids)
}

mod quarantine;

pub(crate) use quarantine::{
    QUARANTINE_FILE, QuarantineCheck, QuarantineReason, encode_quarantine,
    invalidate_scrub_checkpoint, load_quarantine, pack_header_is_valid, quarantine_hashes_locked,
    salvage_index_prefix, write_quarantine_locked,
};

#[cfg(test)]
mod tests;
