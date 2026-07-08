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

const PACK_MAGIC: &[u8] = b"NBLPACK1\n";
const RECORD_MAGIC: &[u8] = b"NBLR";
const INDEX_MAGIC: &[u8] = b"NBLIDX2\n";
const INDEX_PUT: u8 = 1;
const INDEX_RELEASE: u8 = 2;
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalBlobEntry {
    pub hash: BlobHash,
    pub written_at_millis: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PackEntry {
    pack_id: u64,
    offset: u64,
    len: u64,
    written_at_millis: u64,
}

struct LocalPackState {
    packs_dir: PathBuf,
    index_path: PathBuf,
    pack_target_bytes: u64,
    active_pack_id: u64,
    active_pack_bytes: u64,
    index: HashMap<BlobHash, PackEntry>,
    /// Read-only inspection handle: refuses every mutation.
    read_only: bool,
    /// Set on any write-path I/O/corruption failure; all further mutations
    /// fail until the store is reopened (fail-stop, see module docs).
    poisoned: bool,
    /// What this open observed and repaired.
    report: OpenReport,
    /// Receives every durability-relevant sync/rename, in order.
    observer: Arc<dyn SyncObserver>,
    /// Advisory exclusive root lock; released when the last clone drops.
    /// `None` for read-only handles.
    _lock: Option<RootLock>,
    /// Body bytes actually read off disk by `get_range`, tracked only in test
    /// builds to prove a range read stays bounded instead of pulling the
    /// whole pack record.
    #[cfg(test)]
    body_bytes_read: u64,
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
        ensure_index_file(&index_path, &canonical, &*observer)?;

        let index = load_index(
            &index_path,
            IndexLoadMode::HealTornTail,
            &mut report,
            &*observer,
        )?;
        let mut active_pack_id = index.values().map(|entry| entry.pack_id).max().unwrap_or(0);
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
            pack_target_bytes: options.pack_target_bytes,
            active_pack_id,
            active_pack_bytes,
            index,
            read_only: false,
            poisoned: false,
            report,
            observer,
            _lock: guard.lock,
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
        let root = root.as_ref().to_path_buf();
        let packs_dir = root.join("packs");
        let index_path = root.join("index.log");
        let observer: Arc<dyn SyncObserver> = Arc::new(disk::NoopSyncObserver);

        root_guard::guard_read_only_root(&root)?;

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
        let active_pack_id = index.values().map(|entry| entry.pack_id).max().unwrap_or(0);

        Ok(Self {
            state: Arc::new(Mutex::new(LocalPackState {
                packs_dir,
                index_path,
                pack_target_bytes: DEFAULT_PACK_TARGET_BYTES,
                active_pack_id,
                active_pack_bytes: 0,
                index,
                read_only: true,
                poisoned: false,
                report,
                observer,
                _lock: None,
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
    pub fn live_entries(&self) -> Result<Vec<LocalBlobEntry>> {
        let mut entries = lock(&self.state)?
            .index
            .iter()
            .map(|(hash, entry)| LocalBlobEntry {
                hash: *hash,
                written_at_millis: entry.written_at_millis,
            })
            .collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.hash);
        Ok(entries)
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

    async fn blocking<T>(
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
                if state.index.contains_key(&hash) {
                    let observer = Arc::clone(&state.observer);
                    append_release_index_record(&state.index_path, &hash, &*observer)?;
                    state.index.remove(&hash);
                }
                Ok(())
            })(&mut state);
            poison_on_write_failure(&mut state, &result);
            result
        })
        .await
    }
}

fn ensure_writable(state: &LocalPackState, operation: &str) -> Result<()> {
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
fn poison_on_write_failure<T>(state: &mut LocalPackState, result: &Result<T>) {
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

fn lock(state: &Mutex<LocalPackState>) -> Result<MutexGuard<'_, LocalPackState>> {
    state.lock().map_err(|_| {
        Error::storage(
            StorageErrorKind::Corruption,
            "local pack store lock poisoned",
        )
    })
}

fn io_error(error: std::io::Error, context: impl Into<String>) -> Error {
    Error::storage(StorageErrorKind::Io, format!("{}: {error}", context.into()))
}

fn corruption(message: impl Into<String>) -> Error {
    Error::storage(StorageErrorKind::Corruption, message)
}

fn pack_path(packs_dir: &Path, pack_id: u64) -> PathBuf {
    packs_dir.join(format!("pack-{pack_id:016}.npack"))
}

fn ensure_index_file(index_path: &Path, root: &Path, observer: &dyn SyncObserver) -> Result<()> {
    if index_path.exists() {
        return Ok(());
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
    Ok(())
}

fn ensure_pack_file(packs_dir: &Path, pack_id: u64, observer: &dyn SyncObserver) -> Result<u64> {
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
    if state.index.contains_key(&hash) {
        return Ok(hash);
    }

    let entry = append_pack_record(state, &hash, &bytes, written_at_millis)?;
    let observer = Arc::clone(&state.observer);
    append_put_index_record(&state.index_path, &hash, entry, &*observer)?;
    state.index.insert(hash, entry);
    Ok(hash)
}

fn append_pack_record(
    state: &mut LocalPackState,
    hash: &BlobHash,
    bytes: &[u8],
    written_at_millis: u64,
) -> Result<PackEntry> {
    let observer = Arc::clone(&state.observer);
    let record_len =
        RECORD_MAGIC.len() as u64 + crate::BLAKE3_HASH_LEN as u64 + 8 + bytes.len() as u64;
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
    let entry = state
        .index
        .get(hash)
        .copied()
        .ok_or_else(|| Error::NotFound(format!("blob {hash}")))?;
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

fn read_pack_entry(packs_dir: &Path, expected_hash: &BlobHash, entry: PackEntry) -> Result<Bytes> {
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
    let observer = Arc::clone(&state.observer);
    let original_packs = pack_ids_on_disk(&state.packs_dir)?;
    if state.index.is_empty() {
        let mut stats = CompactionStats::default();
        for pack_id in original_packs {
            let path = pack_path(&state.packs_dir, pack_id);
            fs::remove_file(&path)
                .map_err(|err| io_error(err, format!("remove empty pack {}", path.display())))?;
            stats.packs_removed += 1;
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

    let live: Vec<(BlobHash, PackEntry, Bytes)> = state
        .index
        .iter()
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

fn pack_ids_on_disk(packs_dir: &Path) -> Result<BTreeSet<u64>> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::disk::SyncEvent;
    use crate::disk::recorder::RecordingSyncObserver;
    use crate::root_guard::FORMAT_FILE;

    fn open_temp(target: u64) -> (tempfile::TempDir, LocalPackStore) {
        let dir = tempfile::tempdir().expect("tempdir should create");
        let store =
            LocalPackStore::open_with_pack_target(dir.path(), target).expect("store should open");
        (dir, store)
    }

    #[tokio::test]
    async fn put_reopen_get_round_trips() {
        let (dir, store) = open_temp(256);
        let hash = store
            .put(Bytes::from_static(b"durable payload"))
            .await
            .unwrap();
        drop(store);

        let reopened = LocalPackStore::open_with_pack_target(dir.path(), 256).unwrap();
        assert_eq!(
            reopened.get(&hash).await.unwrap(),
            Bytes::from_static(b"durable payload")
        );
    }

    #[tokio::test]
    async fn put_is_idempotent_and_indexes_once() {
        let (_dir, store) = open_temp(256);
        let first = store.put(Bytes::from_static(b"same")).await.unwrap();
        let second = store.put(Bytes::from_static(b"same")).await.unwrap();
        assert_eq!(first, second);
        assert_eq!(store.len().unwrap(), 1);
        assert_eq!(store.live_entries().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn get_range_slices_verified_bytes() {
        let (_dir, store) = open_temp(256);
        let hash = store.put(Bytes::from_static(b"0123456789")).await.unwrap();
        assert_eq!(
            store.get_range(&hash, 4..8).await.unwrap(),
            Bytes::from_static(b"4567")
        );
    }

    #[tokio::test]
    async fn local_pack_store_range_read_transfers_only_inner_bytes_served() {
        let (_dir, store) = open_temp(64 * 1024 * 1024);
        let big: Vec<u8> = (0..1_048_576usize).map(|i| (i % 251) as u8).collect();
        let hash = store.put(Bytes::from(big.clone())).await.unwrap();
        // `put` only writes; drain any bytes the write path itself may have
        // touched so the counter below reflects only the `get_range` call.
        store.take_body_bytes_read().await.unwrap();

        let slice = store.get_range(&hash, 4096..8192).await.unwrap();

        assert_eq!(slice, Bytes::copy_from_slice(&big[4096..8192]));
        let body_bytes = store.take_body_bytes_read().await.unwrap();
        assert_eq!(
            body_bytes, 4096,
            "range read should transfer exactly the requested body window, not the whole 1MiB blob"
        );
    }

    #[tokio::test]
    async fn get_range_rejects_end_past_blob_length() {
        let (_dir, store) = open_temp(256);
        let hash = store.put(Bytes::from_static(b"0123456789")).await.unwrap();
        let err = store.get_range(&hash, 4..100).await.unwrap_err();
        assert!(matches!(err, Error::InvalidInput(_)));
    }

    #[tokio::test]
    #[allow(clippy::reversed_empty_ranges)]
    async fn get_range_rejects_start_after_end() {
        let (_dir, store) = open_temp(256);
        let hash = store.put(Bytes::from_static(b"0123456789")).await.unwrap();
        let err = store.get_range(&hash, 8..4).await.unwrap_err();
        assert!(matches!(err, Error::InvalidInput(_)));
    }

    #[tokio::test]
    async fn release_removes_index_entry_without_deleting_other_blobs() {
        let (_dir, store) = open_temp(128);
        let keep = store.put(Bytes::from_static(b"keep")).await.unwrap();
        let drop_hash = store.put(Bytes::from_static(b"drop")).await.unwrap();

        store.release(&drop_hash).await.unwrap();

        assert!(!store.has(&drop_hash).await.unwrap());
        assert_eq!(store.get(&keep).await.unwrap(), Bytes::from_static(b"keep"));
    }

    #[test]
    fn open_rejects_corrupted_pack_header() {
        let (dir, store) = open_temp(256);
        drop(store);

        let path = pack_path(&dir.path().join("packs"), 0);
        let mut file = OpenOptions::new().write(true).open(path).unwrap();
        file.seek(SeekFrom::Start(0)).unwrap();
        file.write_all(b"BAD").unwrap();
        file.sync_data().unwrap();

        let err = match LocalPackStore::open_with_pack_target(dir.path(), 256) {
            Ok(_) => panic!("corrupted pack header should fail to open"),
            Err(err) => err,
        };
        assert_eq!(err.storage_kind(), Some(StorageErrorKind::Corruption));
    }

    #[tokio::test]
    async fn compact_rewrites_live_blobs_and_removes_dead_packs() {
        let (dir, store) = open_temp(96);
        let keep = store
            .put(Bytes::from_static(b"keep this payload"))
            .await
            .unwrap();
        let drop_hash = store
            .put(Bytes::from_static(b"drop this payload"))
            .await
            .unwrap();
        store.release(&drop_hash).await.unwrap();

        let stats = store.compact().await.unwrap();

        assert_eq!(stats.blobs_rewritten, 1);
        assert!(stats.packs_removed >= 1);
        assert_eq!(
            store.get(&keep).await.unwrap(),
            Bytes::from_static(b"keep this payload")
        );
        assert!(!store.has(&drop_hash).await.unwrap());
        let pack_count = fs::read_dir(dir.path().join("packs"))
            .unwrap()
            .filter_map(std::result::Result::ok)
            .count();
        assert_eq!(pack_count, 1, "dead packs should be removed");
    }

    #[tokio::test]
    async fn put_stream_and_get_stream_round_trip() {
        let (_dir, store) = open_temp(256);
        let src: ByteStream = Box::new(std::io::Cursor::new(Bytes::from_static(b"streamed")));
        let hash = store.put_stream(src).await.unwrap();

        let mut reader = store.get_stream(&hash).await.unwrap();
        let mut out = Vec::new();
        reader.read_to_end(&mut out).await.unwrap();
        assert_eq!(out, b"streamed");
    }

    // ---- RFS2: root ownership and format guard ----

    #[tokio::test]
    async fn local_pack_second_open_shares_live_state() {
        let (dir, store) = open_temp(256);
        let hash = store.put(Bytes::from_static(b"shared")).await.unwrap();

        // A second same-process writable open aliases the SAME live state:
        // no Busy, immediate visibility, one flock, one writer mutex.
        let second = LocalPackStore::open_with_pack_target(dir.path(), 256)
            .expect("same-process open shares the live root state");
        assert_eq!(
            second.get(&hash).await.unwrap(),
            Bytes::from_static(b"shared")
        );
        let via_second = second.put(Bytes::from_static(b"both ways")).await.unwrap();
        assert!(store.has(&via_second).await.unwrap());

        // Dropping every handle releases the state; a fresh open re-reads disk.
        drop(store);
        drop(second);
        let reopened = LocalPackStore::open_with_pack_target(dir.path(), 256).unwrap();
        assert!(reopened.has(&hash).await.unwrap());
    }

    #[tokio::test]
    async fn root_lock_excludes_second_process() {
        use fs2::FileExt;

        let (dir, store) = open_temp(256);

        // Probe the flock the way another process would: a separate file
        // description on root/lock. flock conflicts across descriptions, so
        // this is exactly the cross-process exclusion contract.
        let lock_path = dir.path().canonicalize().unwrap().join("lock");
        let probe = OpenOptions::new().write(true).open(&lock_path).unwrap();
        assert!(
            probe.try_lock_exclusive().is_err(),
            "a live store holds the exclusive root flock"
        );

        drop(store);
        probe
            .try_lock_exclusive()
            .expect("dropping the last handle releases the flock");
        fs2::FileExt::unlock(&probe).unwrap();
    }

    #[test]
    fn local_pack_format_marker_roundtrip() {
        let (dir, store) = open_temp(256);
        drop(store);

        assert!(
            dir.path().join(FORMAT_FILE).exists(),
            "open stamps a marker"
        );
        let reopened = LocalPackStore::open_with_pack_target(dir.path(), 256).unwrap();
        assert_eq!(reopened.open_report().unwrap(), OpenReport::default());
    }

    #[test]
    fn local_pack_rejects_foreign_or_future_marker() {
        let (dir, store) = open_temp(256);
        drop(store);
        let marker_path = dir.path().join(FORMAT_FILE);

        // Foreign marker: not ours at all.
        fs::write(&marker_path, b"someone else's format file").unwrap();
        let err = LocalPackStore::open_with_pack_target(dir.path(), 256).unwrap_err();
        assert_eq!(err.storage_kind(), Some(StorageErrorKind::Corruption));

        // Future-versioned marker: fail closed instead of guessing.
        let mut future = fs::read({
            // Restore a valid marker first, then bump its version field.
            drop(LocalPackStore::open_with_pack_target(dir.path(), 256));
            fs::remove_file(&marker_path).unwrap();
            drop(LocalPackStore::open_with_pack_target(dir.path(), 256).unwrap());
            &marker_path
        })
        .unwrap();
        future[8..12].copy_from_slice(&99u32.to_le_bytes());
        fs::write(&marker_path, &future).unwrap();
        let err = LocalPackStore::open_with_pack_target(dir.path(), 256).unwrap_err();
        assert_eq!(err.storage_kind(), Some(StorageErrorKind::Corruption));
    }

    #[test]
    fn local_pack_startup_cleanup_removes_stale_temp() {
        let (dir, store) = open_temp(256);
        drop(store);

        let root_temp = dir.path().join(format!("{}stale", disk::TMP_PREFIX));
        let packs_temp = dir
            .path()
            .join("packs")
            .join(format!("{}stale", disk::TMP_PREFIX));
        fs::write(&root_temp, b"crash leftover").unwrap();
        fs::write(&packs_temp, b"crash leftover").unwrap();

        let reopened = LocalPackStore::open_with_pack_target(dir.path(), 256).unwrap();
        assert!(!root_temp.exists(), "root temp removed");
        assert!(!packs_temp.exists(), "packs temp removed");
        assert_eq!(
            reopened.open_report().unwrap().stale_temp_files_removed,
            2,
            "cleanup is reported, not silent"
        );
    }

    #[cfg(unix)]
    #[test]
    fn local_pack_rejects_symlinked_root() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        fs::create_dir_all(&real).unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let err = LocalPackStore::open(&link).unwrap_err();
        assert!(matches!(err, Error::InvalidInput(_)));
    }

    #[tokio::test]
    async fn local_pack_read_only_serves_reads_and_rejects_writes() {
        let (dir, owner) = open_temp(256);
        let hash = owner.put(Bytes::from_static(b"inspect me")).await.unwrap();

        // Coexists with the live writable owner: no lock conflict.
        let inspector = LocalPackStore::open_read_only(dir.path()).unwrap();
        assert_eq!(
            inspector.get(&hash).await.unwrap(),
            Bytes::from_static(b"inspect me")
        );
        assert_eq!(inspector.len().unwrap(), 1);

        for err in [
            inspector
                .put(Bytes::from_static(b"nope"))
                .await
                .unwrap_err(),
            inspector.release(&hash).await.unwrap_err(),
            inspector.compact().await.unwrap_err(),
        ] {
            assert_eq!(
                err.storage_kind(),
                Some(StorageErrorKind::Busy),
                "read-only handle refuses mutations"
            );
        }

        // The owner is unaffected.
        assert_eq!(
            owner.get(&hash).await.unwrap(),
            Bytes::from_static(b"inspect me")
        );
    }

    #[tokio::test]
    async fn shared_open_still_refuses_foreign_identity() {
        let dir = tempfile::tempdir().unwrap();
        let bound = LocalPackStoreOptions {
            pack_target_bytes: 256,
            identity: Some([7u8; 32]),
            ..LocalPackStoreOptions::default()
        };
        let _owner = LocalPackStore::open_with_options(dir.path(), bound).unwrap();

        // Root is live and bound to identity 7; a same-process open claiming a
        // different identity must NOT silently alias it.
        let foreign = LocalPackStoreOptions {
            pack_target_bytes: 256,
            identity: Some([8u8; 32]),
            ..LocalPackStoreOptions::default()
        };
        let err = LocalPackStore::open_with_options(dir.path(), foreign).unwrap_err();
        assert_eq!(err.storage_kind(), Some(StorageErrorKind::Corruption));
    }

    #[tokio::test]
    async fn read_only_put_stream_refuses_before_consuming_input() {
        let (dir, _owner) = open_temp(256);
        let inspector = LocalPackStore::open_read_only(dir.path()).unwrap();

        // A reader that panics if polled proves the gate fires before the
        // stream is consumed.
        struct Unpollable;
        impl tokio::io::AsyncRead for Unpollable {
            fn poll_read(
                self: std::pin::Pin<&mut Self>,
                _: &mut std::task::Context<'_>,
                _: &mut tokio::io::ReadBuf<'_>,
            ) -> std::task::Poll<std::io::Result<()>> {
                panic!("read-only put_stream must refuse before reading input");
            }
        }
        let err = inspector
            .put_stream(Box::new(Unpollable))
            .await
            .unwrap_err();
        assert_eq!(err.storage_kind(), Some(StorageErrorKind::Busy));
    }

    #[tokio::test]
    async fn crash_index_unknown_tag_torn_at_eof_still_fails_closed() {
        let (dir, store) = open_temp(4096);
        store.put(Bytes::from_static(b"fine")).await.unwrap();
        drop(store);

        // Unknown tag followed by only a partial hash: EOF-torn, but the tag
        // itself is garbage — corruption, never a healable torn tail.
        let index_path = dir.path().join("index.log");
        let mut file = OpenOptions::new().append(true).open(&index_path).unwrap();
        file.write_all(&[9u8]).unwrap();
        file.write_all(&[0u8; 10]).unwrap();
        file.sync_data().unwrap();

        let err = match LocalPackStore::open_with_pack_target(dir.path(), 4096) {
            Ok(_) => panic!("unknown tag torn at EOF must fail closed"),
            Err(err) => err,
        };
        assert_eq!(err.storage_kind(), Some(StorageErrorKind::Corruption));
    }

    #[tokio::test]
    async fn read_only_refuses_unowned_data_bearing_root() {
        // Data without a marker: unowned/foreign — inspection refuses.
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("packs")).unwrap();
        let err = LocalPackStore::open_read_only(dir.path()).unwrap_err();
        assert_eq!(err.storage_kind(), Some(StorageErrorKind::Corruption));

        // An empty root inspects as an empty store.
        let empty = tempfile::tempdir().unwrap();
        let inspector = LocalPackStore::open_read_only(empty.path()).unwrap();
        assert_eq!(inspector.len().unwrap(), 0);
    }

    // ---- RFS3: durable commit-point writes and crash windows ----

    #[tokio::test]
    async fn durable_write_fsync_order() {
        let (dir, store) = open_temp(4096);
        let recorder = Arc::new(RecordingSyncObserver::new());
        store.set_sync_observer(recorder.clone());

        store
            .put(Bytes::from_static(b"ordered durability"))
            .await
            .unwrap();

        // The store canonicalizes its root (macOS /var -> /private/var), so
        // compare against canonical paths.
        let canonical = dir.path().canonicalize().unwrap();
        let packs_dir = canonical.join("packs");
        let index_path = canonical.join("index.log");
        let pack_sync = recorder.index_where(
            |e| matches!(e, SyncEvent::FileSync(path) if path.starts_with(&packs_dir)),
        );
        let index_sync =
            recorder.index_where(|e| matches!(e, SyncEvent::FileSync(path) if path == &index_path));
        assert!(
            pack_sync < index_sync,
            "pack bytes must be durable before the index record is published: {:?}",
            recorder.events()
        );
    }

    #[tokio::test]
    async fn crash_bytes_written_index_missing() {
        let (dir, store) = open_temp(4096);
        let visible = store
            .put(Bytes::from_static(b"acknowledged"))
            .await
            .unwrap();
        drop(store);

        // Simulate the crash window: pack record fully written and synced,
        // index record never published.
        let orphan_body = b"never acknowledged";
        let orphan_hash = BlobHash::of(orphan_body);
        let path = pack_path(&dir.path().join("packs"), 0);
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(RECORD_MAGIC).unwrap();
        file.write_all(orphan_hash.as_bytes()).unwrap();
        file.write_all(&(orphan_body.len() as u64).to_le_bytes())
            .unwrap();
        file.write_all(orphan_body).unwrap();
        file.sync_data().unwrap();

        let reopened = LocalPackStore::open_with_pack_target(dir.path(), 4096).unwrap();
        assert!(
            !reopened.has(&orphan_hash).await.unwrap(),
            "an unindexed orphan record is invisible"
        );
        assert_eq!(
            reopened.get(&visible).await.unwrap(),
            Bytes::from_static(b"acknowledged")
        );
        // The store keeps working; a fresh put lands after the orphan bytes.
        let next = reopened
            .put(Bytes::from_static(b"after crash"))
            .await
            .unwrap();
        assert_eq!(
            reopened.get(&next).await.unwrap(),
            Bytes::from_static(b"after crash")
        );
    }

    #[tokio::test]
    async fn crash_index_partially_written() {
        let (dir, store) = open_temp(4096);
        let keep = store.put(Bytes::from_static(b"kept")).await.unwrap();
        let torn = store.put(Bytes::from_static(b"torn away")).await.unwrap();
        drop(store);

        // Tear the tail: cut 3 bytes out of the last index record.
        let index_path = dir.path().join("index.log");
        let full_len = fs::metadata(&index_path).unwrap().len();
        let file = OpenOptions::new().write(true).open(&index_path).unwrap();
        file.set_len(full_len - 3).unwrap();
        file.sync_data().unwrap();

        let reopened = LocalPackStore::open_with_pack_target(dir.path(), 4096).unwrap();
        assert!(reopened.has(&keep).await.unwrap(), "whole records survive");
        assert!(
            !reopened.has(&torn).await.unwrap(),
            "the torn record was never acknowledged and is dropped"
        );
        // A PUT record is 1 (tag) + 32 (hash) + 4*8 (fields) = 65 bytes; the
        // tear removed 3, so the heal truncates the remaining 62.
        assert_eq!(
            reopened.open_report().unwrap().torn_index_bytes_truncated,
            62
        );
        assert_eq!(
            fs::metadata(&index_path).unwrap().len(),
            full_len - 65,
            "the index is truncated back to the last whole record"
        );

        // The heal is durable: the next open sees a clean index.
        drop(reopened);
        let again = LocalPackStore::open_with_pack_target(dir.path(), 4096).unwrap();
        assert_eq!(again.open_report().unwrap().torn_index_bytes_truncated, 0);
        // And the store accepts the blob again.
        let rewritten = again.put(Bytes::from_static(b"torn away")).await.unwrap();
        assert_eq!(rewritten, torn);
        assert_eq!(
            again.get(&rewritten).await.unwrap(),
            Bytes::from_static(b"torn away")
        );
    }

    #[tokio::test]
    async fn crash_active_pack_truncated() {
        let (dir, store) = open_temp(64 * 1024);
        let victim_body: Vec<u8> = (0..2048usize).map(|i| (i % 251) as u8).collect();
        let victim = store.put(Bytes::from(victim_body)).await.unwrap();
        drop(store);

        // Truncate the pack mid-record-body.
        let path = pack_path(&dir.path().join("packs"), 0);
        let full_len = fs::metadata(&path).unwrap().len();
        let file = OpenOptions::new().write(true).open(&path).unwrap();
        file.set_len(full_len - 100).unwrap();
        file.sync_data().unwrap();

        // The store still opens; the truncated blob fails closed on read.
        let reopened = LocalPackStore::open_with_pack_target(dir.path(), 64 * 1024).unwrap();
        let err = reopened.get(&victim).await.unwrap_err();
        assert!(
            matches!(
                err.storage_kind(),
                Some(StorageErrorKind::Io) | Some(StorageErrorKind::Corruption)
            ),
            "no partial bytes are ever served: {err}"
        );

        // New writes still work.
        let fresh = reopened.put(Bytes::from_static(b"fresh")).await.unwrap();
        assert_eq!(
            reopened.get(&fresh).await.unwrap(),
            Bytes::from_static(b"fresh")
        );
    }

    #[tokio::test]
    async fn crash_temp_file_left_behind() {
        let (dir, store) = open_temp(256);
        drop(store);
        let leftover = dir.path().join(format!("{}crashed", disk::TMP_PREFIX));
        fs::write(&leftover, b"half-written marker").unwrap();

        let reopened = LocalPackStore::open_with_pack_target(dir.path(), 256).unwrap();
        assert!(!leftover.exists(), "crash leftovers are swept at open");
        assert_eq!(reopened.open_report().unwrap().stale_temp_files_removed, 1);
        let hash = reopened
            .put(Bytes::from_static(b"back to work"))
            .await
            .unwrap();
        assert_eq!(
            reopened.get(&hash).await.unwrap(),
            Bytes::from_static(b"back to work")
        );
    }

    #[tokio::test]
    async fn crash_index_points_at_corrupt_bytes() {
        let (dir, store) = open_temp(256);
        let hash = store.put(Bytes::from_static(b"authentic")).await.unwrap();
        let entry = lock(&store.state)
            .unwrap()
            .index
            .get(&hash)
            .copied()
            .unwrap();
        drop(store);

        let path = pack_path(&dir.path().join("packs"), entry.pack_id);
        let mut file = OpenOptions::new().write(true).open(path).unwrap();
        let body_offset =
            entry.offset + RECORD_MAGIC.len() as u64 + crate::BLAKE3_HASH_LEN as u64 + 8;
        file.seek(SeekFrom::Start(body_offset)).unwrap();
        file.write_all(b"X").unwrap();
        file.sync_data().unwrap();

        let reopened = LocalPackStore::open_with_pack_target(dir.path(), 256).unwrap();
        let err = reopened.get(&hash).await.unwrap_err();
        assert_eq!(
            err.storage_kind(),
            Some(StorageErrorKind::Corruption),
            "a content-address mismatch fails closed; no partial bytes"
        );
    }
    #[tokio::test]
    async fn crash_index_torn_release_tail_truncated() {
        let (dir, store) = open_temp(4096);
        let hash = store.put(Bytes::from_static(b"released?")).await.unwrap();
        store.release(&hash).await.unwrap();
        drop(store);

        // Tear the trailing RELEASE record (1 tag + 32 hash = 33 bytes).
        let index_path = dir.path().join("index.log");
        let full_len = fs::metadata(&index_path).unwrap().len();
        let file = OpenOptions::new().write(true).open(&index_path).unwrap();
        file.set_len(full_len - 3).unwrap();
        file.sync_data().unwrap();

        let reopened = LocalPackStore::open_with_pack_target(dir.path(), 4096).unwrap();
        assert!(
            reopened.has(&hash).await.unwrap(),
            "a torn release was never acknowledged, so the blob stays live"
        );
        assert_eq!(
            reopened.open_report().unwrap().torn_index_bytes_truncated,
            30
        );
    }

    #[tokio::test]
    async fn crash_index_unknown_tag_fails_closed() {
        let (dir, store) = open_temp(4096);
        store.put(Bytes::from_static(b"fine")).await.unwrap();
        drop(store);

        // Append a structurally complete record with an unknown tag: this is
        // not a torn tail, it is corruption, and the open must refuse.
        let index_path = dir.path().join("index.log");
        let mut file = OpenOptions::new().append(true).open(&index_path).unwrap();
        file.write_all(&[9u8]).unwrap();
        file.write_all(&[0u8; crate::BLAKE3_HASH_LEN]).unwrap();
        file.sync_data().unwrap();

        let err = match LocalPackStore::open_with_pack_target(dir.path(), 4096) {
            Ok(_) => panic!("unknown index tag must fail closed"),
            Err(err) => err,
        };
        assert_eq!(err.storage_kind(), Some(StorageErrorKind::Corruption));
    }

    #[tokio::test]
    async fn crash_release_replay_order_preserved() {
        let (dir, store) = open_temp(4096);
        let hash = store.put(Bytes::from_static(b"cycled")).await.unwrap();
        store.release(&hash).await.unwrap();
        let again = store.put(Bytes::from_static(b"cycled")).await.unwrap();
        assert_eq!(hash, again);
        drop(store);

        // Replay must apply PUT / RELEASE / PUT in log order: live at the end.
        let reopened = LocalPackStore::open_with_pack_target(dir.path(), 4096).unwrap();
        assert!(reopened.has(&hash).await.unwrap());
        assert_eq!(
            reopened.get(&hash).await.unwrap(),
            Bytes::from_static(b"cycled")
        );
        assert_eq!(
            reopened.len().unwrap(),
            1,
            "duplicate PUTs collapse to one entry"
        );
    }

    #[tokio::test]
    async fn compaction_crash_replay_prefers_rewritten_records() {
        let (dir, store) = open_temp(4096);
        let keep = store.put(Bytes::from_static(b"survivor")).await.unwrap();
        let dead = store.put(Bytes::from_static(b"garbage")).await.unwrap();
        store.release(&dead).await.unwrap();
        store.compact().await.unwrap();
        drop(store);

        // Simulate a crash where an old pack's delete never persisted: the
        // orphan pack reappears next to the rewritten one.
        let packs_dir = dir.path().join("packs");
        let orphan = pack_path(&packs_dir, 0);
        assert!(!orphan.exists(), "compaction removed the original pack");
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&orphan)
            .unwrap();
        file.write_all(PACK_MAGIC).unwrap();
        file.sync_data().unwrap();

        // Replay resolves the survivor to the rewritten (last-wins) record,
        // and the resurrected orphan is inert.
        let reopened = LocalPackStore::open_with_pack_target(dir.path(), 4096).unwrap();
        assert_eq!(
            reopened.get(&keep).await.unwrap(),
            Bytes::from_static(b"survivor")
        );
        assert!(!reopened.has(&dead).await.unwrap());

        // The next compaction removes the orphan pack.
        reopened.compact().await.unwrap();
        assert!(!orphan.exists(), "orphan pack is reclaimed by compaction");
        assert_eq!(
            reopened.get(&keep).await.unwrap(),
            Bytes::from_static(b"survivor")
        );
    }

    #[tokio::test]
    async fn local_pack_concurrent_same_hash_dedups_under_mutex() {
        let (_dir, store) = open_temp(64 * 1024);
        let mut handles = Vec::new();
        for _ in 0..8 {
            let store = store.clone();
            handles.push(tokio::spawn(async move {
                store.put(Bytes::from_static(b"same payload")).await
            }));
        }
        let mut hashes = Vec::new();
        for handle in handles {
            hashes.push(handle.await.unwrap().unwrap());
        }
        assert!(hashes.windows(2).all(|w| w[0] == w[1]));
        assert_eq!(store.len().unwrap(), 1, "concurrent identical puts dedup");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn durable_write_fsync_error_poisons_store() {
        use std::os::unix::fs::PermissionsExt;

        let (dir, store) = open_temp(4096);
        let hash = store
            .put(Bytes::from_static(b"before failure"))
            .await
            .unwrap();

        // Make the index unwritable so the next index append fails.
        let index_path = dir.path().join("index.log");
        fs::set_permissions(&index_path, fs::Permissions::from_mode(0o400)).unwrap();
        let err = store.release(&hash).await.unwrap_err();
        assert!(
            matches!(err.storage_kind(), Some(StorageErrorKind::Io)),
            "the failing write surfaces as Io: {err}"
        );

        // Fail-stop: even a write that would touch different files is refused.
        fs::set_permissions(&index_path, fs::Permissions::from_mode(0o644)).unwrap();
        let err = store
            .put(Bytes::from_static(b"after failure"))
            .await
            .unwrap_err();
        assert_eq!(
            err.storage_kind(),
            Some(StorageErrorKind::Unavailable),
            "a poisoned store refuses further mutations until reopened"
        );

        // Reads stay available (content-verified).
        assert_eq!(
            store.get(&hash).await.unwrap(),
            Bytes::from_static(b"before failure")
        );

        // Reopening recovers: state is revalidated from disk.
        drop(store);
        let reopened = LocalPackStore::open_with_pack_target(dir.path(), 4096).unwrap();
        assert!(reopened.has(&hash).await.unwrap(), "release never landed");
        let fresh = reopened
            .put(Bytes::from_static(b"after reopen"))
            .await
            .unwrap();
        assert_eq!(
            reopened.get(&fresh).await.unwrap(),
            Bytes::from_static(b"after reopen")
        );
    }
}
