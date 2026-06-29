//! [`LocalPackStore`] - durable append-only local [`BlobStore`] implementation.
//!
//! The pack store keeps one tenant's immutable blobs in a small set of pack
//! files plus an append-only binary index. Pack bytes are persisted before the
//! index record is published, so a crash may leave an unindexed orphaned record
//! but not a visible blob whose bytes were never written.

use std::collections::{BTreeSet, HashMap};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use async_trait::async_trait;
use bytes::Bytes;
use nimbus_core::{Error, Result, StorageErrorKind};
use tokio::io::AsyncReadExt;

use crate::hash::BlobHash;
use crate::store::{BlobStore, ByteStream};

const PACK_MAGIC: &[u8] = b"NBLPACK1\n";
const RECORD_MAGIC: &[u8] = b"NBLR";
const INDEX_MAGIC: &[u8] = b"NBLIDX1\n";
const INDEX_PUT: u8 = 1;
const INDEX_RELEASE: u8 = 2;
const DEFAULT_PACK_TARGET_BYTES: u64 = 128 * 1024 * 1024;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PackEntry {
    pack_id: u64,
    offset: u64,
    len: u64,
}

struct LocalPackState {
    packs_dir: PathBuf,
    index_path: PathBuf,
    pack_target_bytes: u64,
    active_pack_id: u64,
    active_pack_bytes: u64,
    index: HashMap<BlobHash, PackEntry>,
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
}

impl LocalPackStore {
    /// Opens or creates a local pack store rooted at `root`.
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        Self::open_with_pack_target(root, DEFAULT_PACK_TARGET_BYTES)
    }

    /// Opens or creates a local pack store with a custom pack target.
    ///
    /// This is public so operators/tests can choose smaller stores, but the
    /// hard cap preserves the plan's 512 MB launch bound.
    pub fn open_with_pack_target(root: impl AsRef<Path>, pack_target_bytes: u64) -> Result<Self> {
        if pack_target_bytes == 0 || pack_target_bytes > MAX_PACK_TARGET_BYTES {
            return Err(Error::InvalidInput(format!(
                "local pack target must be 1..={MAX_PACK_TARGET_BYTES} bytes, got {pack_target_bytes}"
            )));
        }

        let root = root.as_ref().to_path_buf();
        let packs_dir = root.join("packs");
        let index_path = root.join("index.log");
        fs::create_dir_all(&packs_dir).map_err(|err| {
            io_error(
                err,
                format!("create local pack directory {}", packs_dir.display()),
            )
        })?;
        ensure_index_file(&index_path)?;

        let index = load_index(&index_path)?;
        let active_pack_id = index.values().map(|entry| entry.pack_id).max().unwrap_or(0);
        let mut active_pack_bytes = ensure_pack_file(&packs_dir, active_pack_id)?;
        if active_pack_bytes >= pack_target_bytes && active_pack_bytes > PACK_MAGIC.len() as u64 {
            let next_pack_id = active_pack_id.saturating_add(1);
            active_pack_bytes = ensure_pack_file(&packs_dir, next_pack_id)?;
            return Ok(Self {
                state: Arc::new(Mutex::new(LocalPackState {
                    packs_dir,
                    index_path,
                    pack_target_bytes,
                    active_pack_id: next_pack_id,
                    active_pack_bytes,
                    index,
                })),
            });
        }

        Ok(Self {
            state: Arc::new(Mutex::new(LocalPackState {
                packs_dir,
                index_path,
                pack_target_bytes,
                active_pack_id,
                active_pack_bytes,
                index,
            })),
        })
    }

    /// Number of live blobs in the local index.
    pub fn len(&self) -> Result<usize> {
        Ok(lock(&self.state)?.index.len())
    }

    /// Whether the local index contains no live blobs.
    pub fn is_empty(&self) -> Result<bool> {
        Ok(self.len()? == 0)
    }

    /// Rewrites live blobs into fresh packs and removes packs no live index entry
    /// references afterward.
    pub async fn compact(&self) -> Result<CompactionStats> {
        self.blocking(|mut state| compact_locked(&mut state)).await
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
}

#[async_trait]
impl BlobStore for LocalPackStore {
    async fn put(&self, bytes: Bytes) -> Result<BlobHash> {
        self.blocking(move |mut state| put_locked(&mut state, bytes))
            .await
    }

    async fn put_stream(&self, mut src: ByteStream) -> Result<BlobHash> {
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
        let bytes = self.get(hash).await?;
        Ok(Box::new(std::io::Cursor::new(bytes)))
    }

    async fn get_range(&self, hash: &BlobHash, range: Range<u64>) -> Result<Bytes> {
        let bytes = self.get(hash).await?;
        let len = bytes.len() as u64;
        if range.start > range.end || range.end > len {
            return Err(Error::InvalidInput(format!(
                "range {}..{} out of bounds for blob of {len} bytes",
                range.start, range.end
            )));
        }
        Ok(bytes.slice(range.start as usize..range.end as usize))
    }

    async fn has(&self, hash: &BlobHash) -> Result<bool> {
        let hash = *hash;
        self.blocking(move |state| Ok(state.index.contains_key(&hash)))
            .await
    }

    async fn release(&self, hash: &BlobHash) -> Result<()> {
        let hash = *hash;
        self.blocking(move |mut state| {
            if state.index.remove(&hash).is_some() {
                append_release_index_record(&state.index_path, &hash)?;
            }
            Ok(())
        })
        .await
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

fn ensure_index_file(index_path: &Path) -> Result<()> {
    if index_path.exists() {
        return Ok(());
    }
    if let Some(parent) = index_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| io_error(err, format!("create index parent {}", parent.display())))?;
    }
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(index_path)
        .map_err(|err| io_error(err, format!("create index {}", index_path.display())))?;
    file.write_all(INDEX_MAGIC)
        .map_err(|err| io_error(err, format!("write index header {}", index_path.display())))?;
    file.sync_data()
        .map_err(|err| io_error(err, format!("sync index {}", index_path.display())))
}

fn ensure_pack_file(packs_dir: &Path, pack_id: u64) -> Result<u64> {
    fs::create_dir_all(packs_dir)
        .map_err(|err| io_error(err, format!("create packs dir {}", packs_dir.display())))?;
    let path = pack_path(packs_dir, pack_id);
    if !path.exists() {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .map_err(|err| io_error(err, format!("create pack {}", path.display())))?;
        file.write_all(PACK_MAGIC)
            .map_err(|err| io_error(err, format!("write pack header {}", path.display())))?;
        file.sync_data()
            .map_err(|err| io_error(err, format!("sync pack {}", path.display())))?;
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

fn load_index(index_path: &Path) -> Result<HashMap<BlobHash, PackEntry>> {
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
    while cursor < bytes.len() {
        let tag = bytes[cursor];
        cursor += 1;
        let hash = read_hash(index_path, &bytes, &mut cursor)?;
        match tag {
            INDEX_PUT => {
                let pack_id = read_u64(index_path, &bytes, &mut cursor)?;
                let offset = read_u64(index_path, &bytes, &mut cursor)?;
                let len = read_u64(index_path, &bytes, &mut cursor)?;
                index.insert(
                    hash,
                    PackEntry {
                        pack_id,
                        offset,
                        len,
                    },
                );
            }
            INDEX_RELEASE => {
                index.remove(&hash);
            }
            other => {
                return Err(corruption(format!(
                    "index {} has unknown record tag {other}",
                    index_path.display()
                )));
            }
        }
    }
    Ok(index)
}

fn read_hash(path: &Path, bytes: &[u8], cursor: &mut usize) -> Result<BlobHash> {
    if bytes.len().saturating_sub(*cursor) < crate::BLAKE3_HASH_LEN {
        return Err(corruption(format!(
            "index {} ended mid hash record",
            path.display()
        )));
    }
    let mut hash = [0u8; crate::BLAKE3_HASH_LEN];
    hash.copy_from_slice(&bytes[*cursor..*cursor + crate::BLAKE3_HASH_LEN]);
    *cursor += crate::BLAKE3_HASH_LEN;
    Ok(BlobHash::from_bytes(hash))
}

fn read_u64(path: &Path, bytes: &[u8], cursor: &mut usize) -> Result<u64> {
    if bytes.len().saturating_sub(*cursor) < 8 {
        return Err(corruption(format!(
            "index {} ended mid u64 record",
            path.display()
        )));
    }
    let mut raw = [0u8; 8];
    raw.copy_from_slice(&bytes[*cursor..*cursor + 8]);
    *cursor += 8;
    Ok(u64::from_le_bytes(raw))
}

fn put_locked(state: &mut LocalPackState, bytes: Bytes) -> Result<BlobHash> {
    let hash = BlobHash::of(&bytes);
    if state.index.contains_key(&hash) {
        return Ok(hash);
    }

    let entry = append_pack_record(state, &hash, &bytes)?;
    append_put_index_record(&state.index_path, &hash, entry)?;
    state.index.insert(hash, entry);
    Ok(hash)
}

fn append_pack_record(
    state: &mut LocalPackState,
    hash: &BlobHash,
    bytes: &[u8],
) -> Result<PackEntry> {
    let record_len =
        RECORD_MAGIC.len() as u64 + crate::BLAKE3_HASH_LEN as u64 + 8 + bytes.len() as u64;
    if state.active_pack_bytes > PACK_MAGIC.len() as u64
        && state.active_pack_bytes.saturating_add(record_len) > state.pack_target_bytes
    {
        state.active_pack_id = state.active_pack_id.saturating_add(1);
        state.active_pack_bytes = ensure_pack_file(&state.packs_dir, state.active_pack_id)?;
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
    file.sync_data()
        .map_err(|err| io_error(err, format!("sync pack {}", path.display())))?;
    state.active_pack_bytes = offset.saturating_add(record_len);
    Ok(PackEntry {
        pack_id: state.active_pack_id,
        offset,
        len: bytes.len() as u64,
    })
}

fn append_put_index_record(index_path: &Path, hash: &BlobHash, entry: PackEntry) -> Result<()> {
    append_index_record(index_path, |file| {
        file.write_all(&[INDEX_PUT])?;
        file.write_all(hash.as_bytes())?;
        file.write_all(&entry.pack_id.to_le_bytes())?;
        file.write_all(&entry.offset.to_le_bytes())?;
        file.write_all(&entry.len.to_le_bytes())?;
        Ok(())
    })
}

fn append_release_index_record(index_path: &Path, hash: &BlobHash) -> Result<()> {
    append_index_record(index_path, |file| {
        file.write_all(&[INDEX_RELEASE])?;
        file.write_all(hash.as_bytes())?;
        Ok(())
    })
}

fn append_index_record(
    index_path: &Path,
    write: impl FnOnce(&mut File) -> std::io::Result<()>,
) -> Result<()> {
    let mut file = OpenOptions::new()
        .append(true)
        .open(index_path)
        .map_err(|err| io_error(err, format!("open index {}", index_path.display())))?;
    write(&mut file)
        .map_err(|err| io_error(err, format!("append index {}", index_path.display())))?;
    file.sync_data()
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

fn compact_locked(state: &mut LocalPackState) -> Result<CompactionStats> {
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
        state.active_pack_bytes = ensure_pack_file(&state.packs_dir, state.active_pack_id)?;
        return Ok(stats);
    }

    let live: Vec<(BlobHash, Bytes)> = state
        .index
        .iter()
        .map(|(hash, entry)| {
            read_pack_entry(&state.packs_dir, hash, *entry).map(|bytes| (*hash, bytes))
        })
        .collect::<Result<Vec<_>>>()?;

    state.active_pack_id = state.active_pack_id.saturating_add(1);
    state.active_pack_bytes = ensure_pack_file(&state.packs_dir, state.active_pack_id)?;

    let mut stats = CompactionStats::default();
    for (hash, bytes) in live {
        let entry = append_pack_record(state, &hash, &bytes)?;
        append_put_index_record(&state.index_path, &hash, entry)?;
        state.index.insert(hash, entry);
        stats.blobs_rewritten += 1;
        stats.bytes_rewritten += bytes.len() as u64;
    }

    let referenced_packs: BTreeSet<u64> = state.index.values().map(|entry| entry.pack_id).collect();
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
        }
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
    async fn release_removes_index_entry_without_deleting_other_blobs() {
        let (_dir, store) = open_temp(128);
        let keep = store.put(Bytes::from_static(b"keep")).await.unwrap();
        let drop_hash = store.put(Bytes::from_static(b"drop")).await.unwrap();

        store.release(&drop_hash).await.unwrap();

        assert!(!store.has(&drop_hash).await.unwrap());
        assert_eq!(store.get(&keep).await.unwrap(), Bytes::from_static(b"keep"));
    }

    #[tokio::test]
    async fn read_detects_pack_corruption() {
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
        assert_eq!(err.storage_kind(), Some(StorageErrorKind::Corruption));
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
}
