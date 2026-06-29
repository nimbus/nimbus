use std::io;
use std::ops::Range;
use std::path::Path;
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use deno_fs::{FileSystem, OpenOptions};
use nimbus_blob::{BlobHash, BlobStore, ByteStream, MemoryBlobStore};
use nimbus_core::Result as NimbusResult;

use super::checked;
use crate::{CasBlobChunk, CasReadOnlyBackend, CasReadOnlyManifest};

#[derive(Default)]
struct TrackingBlobStore {
    inner: MemoryBlobStore,
    get_stream_calls: Arc<Mutex<Vec<BlobHash>>>,
    get_calls: Arc<Mutex<usize>>,
}

impl TrackingBlobStore {
    fn stream_calls(&self) -> Vec<BlobHash> {
        self.get_stream_calls.lock().unwrap().clone()
    }

    fn clear_stream_calls(&self) {
        self.get_stream_calls.lock().unwrap().clear();
    }

    fn get_call_count(&self) -> usize {
        *self.get_calls.lock().unwrap()
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
        self.inner.get_range(hash, range).await
    }

    async fn has(&self, hash: &BlobHash) -> NimbusResult<bool> {
        self.inner.has(hash).await
    }

    async fn release(&self, hash: &BlobHash) -> NimbusResult<()> {
        self.inner.release(hash).await
    }
}

fn put_test_blob(store: &TrackingBlobStore, bytes: &'static [u8]) -> BlobHash {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(store.put(Bytes::from_static(bytes)))
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
fn cas_ro_reads_multi_blob_file_from_get_stream() {
    let (store, backend, first, second) = cas_ro_fixture();

    let data = backend
        .read_file_sync(&checked(Path::new("/bundle/app.txt")), OpenOptions::read())
        .unwrap();

    assert_eq!(data.as_ref(), b"hello world");
    assert_eq!(store.stream_calls(), vec![first, second]);
    assert_eq!(
        store.get_call_count(),
        0,
        "CAS-RO must not use BlobStore::get"
    );
}

#[test]
fn cas_ro_partial_read_streams_only_overlapping_blob() {
    let (store, backend, _first, second) = cas_ro_fixture();
    store.clear_stream_calls();
    let file = backend
        .open_sync(&checked(Path::new("/bundle/app.txt")), OpenOptions::read())
        .unwrap();
    let mut buf = [0_u8; 3];

    let nread = file.read_at_sync(&mut buf, 6).unwrap();

    assert_eq!(nread, 3);
    assert_eq!(&buf, b"wor");
    assert_eq!(
        store.stream_calls(),
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
