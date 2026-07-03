use std::io;
use std::ops::Range;
use std::path::Path;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use bytes::Bytes;
use deno_fs::{FileSystem, OpenOptions};
use nimbus_blob::{BlobHash, BlobStore, ByteStream, MemoryBlobStore};
use nimbus_core::{Error, Result as NimbusResult, StorageErrorKind};
use tokio::io::{AsyncRead, ReadBuf};

use super::checked;
use crate::{CasBlobChunk, CasReadOnlyBackend, CasReadOnlyManifest};

type RangeCallLog = Arc<Mutex<Vec<(BlobHash, Range<u64>)>>>;

#[derive(Default)]
struct TrackingBlobStore {
    inner: MemoryBlobStore,
    get_stream_calls: Arc<Mutex<Vec<BlobHash>>>,
    get_calls: Arc<Mutex<usize>>,
    get_range_calls: RangeCallLog,
}

struct AccountingBlobStore {
    hash: BlobHash,
    bytes: Bytes,
    stream_calls: AtomicUsize,
    get_calls: AtomicUsize,
    range_calls: AtomicUsize,
    bytes_read: Arc<AtomicUsize>,
}

struct AccountingStream {
    bytes: Bytes,
    position: usize,
    bytes_read: Arc<AtomicUsize>,
}

impl TrackingBlobStore {
    fn stream_calls(&self) -> Vec<BlobHash> {
        self.get_stream_calls.lock().unwrap().clone()
    }

    fn get_call_count(&self) -> usize {
        *self.get_calls.lock().unwrap()
    }

    fn range_calls(&self) -> Vec<(BlobHash, Range<u64>)> {
        self.get_range_calls.lock().unwrap().clone()
    }

    fn range_bytes_requested(&self) -> u64 {
        self.get_range_calls
            .lock()
            .unwrap()
            .iter()
            .map(|(_, range)| range.end - range.start)
            .sum()
    }
}

impl AccountingBlobStore {
    fn new(bytes: Bytes) -> Self {
        Self {
            hash: BlobHash::of(bytes.as_ref()),
            bytes,
            stream_calls: AtomicUsize::new(0),
            get_calls: AtomicUsize::new(0),
            range_calls: AtomicUsize::new(0),
            bytes_read: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn hash(&self) -> BlobHash {
        self.hash
    }

    fn stream_calls(&self) -> usize {
        self.stream_calls.load(Ordering::SeqCst)
    }

    fn get_calls(&self) -> usize {
        self.get_calls.load(Ordering::SeqCst)
    }

    fn range_calls(&self) -> usize {
        self.range_calls.load(Ordering::SeqCst)
    }

    fn bytes_read(&self) -> usize {
        self.bytes_read.load(Ordering::SeqCst)
    }
}

impl AsyncRead for AccountingStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.position >= self.bytes.len() || buf.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        let len = (self.bytes.len() - self.position).min(buf.remaining());
        let end = self.position + len;
        buf.put_slice(&self.bytes[self.position..end]);
        self.position = end;
        self.bytes_read.fetch_add(len, Ordering::SeqCst);
        Poll::Ready(Ok(()))
    }
}

#[async_trait::async_trait]
impl BlobStore for TrackingBlobStore {
    async fn put(&self, bytes: Bytes) -> NimbusResult<BlobHash> {
        self.inner.put(bytes).await
    }

    async fn put_stream(&self, src: ByteStream) -> NimbusResult<BlobHash> {
        self.inner.put_stream(src).await
    }

    async fn get(&self, hash: &BlobHash) -> NimbusResult<Bytes> {
        *self.get_calls.lock().unwrap() += 1;
        self.inner.get(hash).await
    }

    async fn get_stream(&self, hash: &BlobHash) -> NimbusResult<ByteStream> {
        self.get_stream_calls.lock().unwrap().push(*hash);
        self.inner.get_stream(hash).await
    }

    async fn get_range(&self, hash: &BlobHash, range: Range<u64>) -> NimbusResult<Bytes> {
        self.get_range_calls
            .lock()
            .unwrap()
            .push((*hash, range.clone()));
        self.inner.get_range(hash, range).await
    }

    async fn has(&self, hash: &BlobHash) -> NimbusResult<bool> {
        self.inner.has(hash).await
    }

    async fn release(&self, hash: &BlobHash) -> NimbusResult<()> {
        self.inner.release(hash).await
    }
}

#[async_trait::async_trait]
impl BlobStore for AccountingBlobStore {
    async fn put(&self, bytes: Bytes) -> NimbusResult<BlobHash> {
        Ok(BlobHash::of(bytes.as_ref()))
    }

    async fn put_stream(&self, _src: ByteStream) -> NimbusResult<BlobHash> {
        Err(Error::storage(
            StorageErrorKind::Unavailable,
            "accounting store does not accept writes",
        ))
    }

    async fn get(&self, hash: &BlobHash) -> NimbusResult<Bytes> {
        self.get_calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(*hash, self.hash);
        Ok(self.bytes.clone())
    }

    async fn get_stream(&self, hash: &BlobHash) -> NimbusResult<ByteStream> {
        self.stream_calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(*hash, self.hash);
        Ok(Box::new(AccountingStream {
            bytes: self.bytes.clone(),
            position: 0,
            bytes_read: self.bytes_read.clone(),
        }))
    }

    async fn get_range(&self, hash: &BlobHash, range: Range<u64>) -> NimbusResult<Bytes> {
        self.range_calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(*hash, self.hash);
        let slice = self.bytes.slice(range.start as usize..range.end as usize);
        self.bytes_read.fetch_add(slice.len(), Ordering::SeqCst);
        Ok(slice)
    }

    async fn has(&self, hash: &BlobHash) -> NimbusResult<bool> {
        Ok(*hash == self.hash)
    }

    async fn release(&self, _hash: &BlobHash) -> NimbusResult<()> {
        Ok(())
    }
}

fn put_test_blob(store: &TrackingBlobStore, bytes: &'static [u8]) -> BlobHash {
    put_test_blob_bytes(store, Bytes::from_static(bytes))
}

fn put_test_blob_bytes(store: &TrackingBlobStore, bytes: Bytes) -> BlobHash {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(store.put(bytes))
        .unwrap()
}

fn cas_ro_fixture() -> (
    Arc<TrackingBlobStore>,
    CasReadOnlyBackend,
    BlobHash,
    BlobHash,
) {
    let store = Arc::new(TrackingBlobStore::default());
    let first = put_test_blob(&store, b"hello ");
    let second = put_test_blob(&store, b"world");
    let manifest = CasReadOnlyManifest::new()
        .add_file(
            "/bundle/app.txt",
            vec![CasBlobChunk::new(first, 6), CasBlobChunk::new(second, 5)],
            0o444,
        )
        .unwrap();
    let backend = CasReadOnlyBackend::new(store.clone(), manifest);
    (store, backend, first, second)
}

#[test]
fn cas_ro_partial_read_does_not_drain_whole_blob_stream() {
    let payload = Bytes::from(vec![b'x'; 1024 * 1024]);
    let store = Arc::new(AccountingBlobStore::new(payload.clone()));
    let manifest = CasReadOnlyManifest::new()
        .add_file(
            "/large.bin",
            vec![CasBlobChunk::new(store.hash(), payload.len() as u64)],
            0o444,
        )
        .unwrap();
    let backend = CasReadOnlyBackend::new(store.clone(), manifest);
    let file = backend
        .open_sync(&checked(Path::new("/large.bin")), OpenOptions::read())
        .unwrap();
    let mut buf = [0_u8; 8];
    let position = 4096;

    let nread = file.read_at_sync(&mut buf, position).unwrap();

    assert_eq!(nread, buf.len());
    assert_eq!(&buf, b"xxxxxxxx");
    assert_eq!(
        store.range_calls(),
        1,
        "a single positional read is a single BlobStore::get_range call"
    );
    assert_eq!(
        store.stream_calls(),
        0,
        "CAS-RO must not fall back to BlobStore::get_stream"
    );
    assert_eq!(store.get_calls(), 0, "CAS-RO must not use BlobStore::get");
    assert_eq!(
        store.bytes_read(),
        buf.len(),
        "get_range must transfer only the requested window, not a skipped prefix"
    );
}

#[test]
fn cas_ro_reads_multi_blob_file_from_get_range() {
    let (store, backend, first, second) = cas_ro_fixture();

    let data = backend
        .read_file_sync(&checked(Path::new("/bundle/app.txt")), OpenOptions::read())
        .unwrap();

    assert_eq!(data.as_ref(), b"hello world");
    assert_eq!(
        store
            .range_calls()
            .into_iter()
            .map(|(hash, _)| hash)
            .collect::<Vec<_>>(),
        vec![first, second],
        "a whole-file read fetches each overlapping chunk once via get_range"
    );
    assert_eq!(
        store.stream_calls(),
        Vec::<BlobHash>::new(),
        "CAS-RO must not use BlobStore::get_stream"
    );
    assert_eq!(
        store.get_call_count(),
        0,
        "CAS-RO must not use BlobStore::get"
    );
}

#[test]
fn cas_ro_partial_read_streams_only_overlapping_blob() {
    let (store, backend, _first, second) = cas_ro_fixture();
    let file = backend
        .open_sync(&checked(Path::new("/bundle/app.txt")), OpenOptions::read())
        .unwrap();
    let mut buf = [0_u8; 3];

    let nread = file.read_at_sync(&mut buf, 6).unwrap();

    assert_eq!(nread, 3);
    assert_eq!(&buf, b"wor");
    assert_eq!(
        store
            .range_calls()
            .into_iter()
            .map(|(hash, _)| hash)
            .collect::<Vec<_>>(),
        vec![second],
        "partial reads fetch only overlapping blob chunks"
    );
}

#[test]
fn cas_ro_manifest_owns_directory_entries_and_stat_metadata() {
    let (_store, backend, _first, _second) = cas_ro_fixture();

    let stat = backend
        .stat_sync(&checked(Path::new("/bundle/app.txt")))
        .unwrap();
    assert!(stat.is_file);
    assert_eq!(stat.size, 11);
    assert_eq!(stat.mode, 0o444);

    let entries = backend
        .read_dir_sync(&checked(Path::new("/bundle")))
        .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "app.txt");
    assert!(entries[0].is_file);
}

#[test]
fn cas_ro_missing_blob_surfaces_enoent() {
    let store = Arc::new(TrackingBlobStore::default());
    let missing = BlobHash::of(b"missing");
    let manifest = CasReadOnlyManifest::new()
        .add_file(
            "/missing.txt",
            vec![CasBlobChunk::new(missing, b"missing".len() as u64)],
            0o444,
        )
        .unwrap();
    let backend = CasReadOnlyBackend::new(store, manifest);

    let error = backend
        .read_file_sync(&checked(Path::new("/missing.txt")), OpenOptions::read())
        .expect_err("missing CAS blob should fail the read");
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
}

#[test]
fn cas_ro_rejects_every_mutation_with_erofs() {
    let (_store, backend, _first, _second) = cas_ro_fixture();

    for (label, result) in [
        (
            "write",
            backend.write_file_sync(
                &checked(Path::new("/bundle/app.txt")),
                OpenOptions::write(false, false, false, None),
                b"x",
            ),
        ),
        (
            "mkdir",
            backend.mkdir_sync(&checked(Path::new("/bundle/new")), false, None),
        ),
        (
            "remove",
            backend.remove_sync(&checked(Path::new("/bundle/app.txt")), false),
        ),
        (
            "truncate",
            backend.truncate_sync(&checked(Path::new("/bundle/app.txt")), 0),
        ),
        (
            "symlink",
            backend.symlink_sync(
                &checked(Path::new("app.txt")),
                &checked(Path::new("/bundle/link")),
                None,
            ),
        ),
    ] {
        let error = result.expect_err("CAS-RO mutation should fail");
        assert!(
            error.to_string().contains("EROFS"),
            "unexpected {label} error: {error}"
        );
    }
}

#[test]
fn sequential_chunked_reads_transfer_o_of_requested_bytes() {
    // Five 64KiB chunks (320KiB total), each filled with a distinct byte so
    // reads across chunk boundaries can be verified for correctness, not
    // just cost.
    const CHUNK_LEN: usize = 64 * 1024;
    const CHUNK_COUNT: usize = 5;
    let chunks: Vec<Bytes> = (0..CHUNK_COUNT)
        .map(|i| Bytes::from(vec![i as u8; CHUNK_LEN]))
        .collect();

    let store = Arc::new(TrackingBlobStore::default());
    let mut manifest_chunks = Vec::new();
    for chunk in &chunks {
        let hash = put_test_blob_bytes(&store, chunk.clone());
        manifest_chunks.push(CasBlobChunk::new(hash, chunk.len() as u64));
    }
    let manifest = CasReadOnlyManifest::new()
        .add_file("/sequential.bin", manifest_chunks, 0o444)
        .unwrap();
    let backend = CasReadOnlyBackend::new(store.clone(), manifest);
    let file = backend
        .open_sync(&checked(Path::new("/sequential.bin")), OpenOptions::read())
        .unwrap();

    // 64 sequential reads of 4KiB each, covering the first 256KiB — crossing
    // a chunk boundary every 16 reads (64KiB / 4KiB).
    const WINDOW: usize = 4096;
    const READS: usize = 64;
    let mut buf = [0_u8; WINDOW];
    for i in 0..READS {
        let position = (i * WINDOW) as u64;
        let nread = file.clone().read_at_sync(&mut buf, position).unwrap();
        assert_eq!(nread, WINDOW);
        let expected_chunk = (i * WINDOW) / CHUNK_LEN;
        assert!(
            buf.iter().all(|byte| *byte == expected_chunk as u8),
            "read at position {position} crossed into unexpected chunk content"
        );
    }

    let requested_total = (READS * WINDOW) as u64;
    let bytes_transferred = store.range_bytes_requested();

    assert_eq!(
        bytes_transferred, requested_total,
        "get_range assembly must transfer exactly the requested bytes, not O(reads * offset)"
    );
    // The exact-equality assertion above is strictly stronger than this, but
    // this is the bound the FCW2 spec calls out explicitly.
    assert!(
        bytes_transferred < requested_total * 2,
        "bytes transferred must stay within 2x the requested total"
    );
    assert_eq!(
        store.get_call_count(),
        0,
        "sequential chunked reads must not use BlobStore::get"
    );
    assert_eq!(
        store.stream_calls(),
        Vec::<BlobHash>::new(),
        "sequential chunked reads must not use BlobStore::get_stream (the O(n^2) reopen+skip path)"
    );
}

#[test]
fn dropping_file_mid_sequence_does_not_poison_shared_bridge_runtime() {
    let (_store, backend, _first, _second) = cas_ro_fixture();
    {
        let file = backend
            .open_sync(&checked(Path::new("/bundle/app.txt")), OpenOptions::read())
            .unwrap();
        let mut buf = [0_u8; 3];
        let nread = file.read_at_sync(&mut buf, 0).unwrap();
        assert_eq!(nread, 3);
        assert_eq!(&buf, b"hel");
        // `file` (and the Rc it wraps) is dropped here, mid-sequence, before
        // the rest of "/bundle/app.txt" is ever read.
    }

    // A fresh handle proves the shared bridge runtime (a process-lifetime
    // `OnceLock` singleton) is still alive and serviceable after a `File` is
    // dropped mid-read: it does not require one runtime per `File`, and
    // dropping a `File` early does not poison or shut down the runtime that
    // earlier reads used.
    let file = backend
        .open_sync(&checked(Path::new("/bundle/app.txt")), OpenOptions::read())
        .unwrap();
    let data = file.read_all_sync().unwrap();
    assert_eq!(data.as_ref(), b"hello world");
}
