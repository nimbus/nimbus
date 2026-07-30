use std::collections::BTreeMap;
use std::ops::Range;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use bytes::Bytes;
use deno_fs::sync::MaybeArc;
use deno_fs::{FileSystem, OpenOptions};
use nimbus_blob::{BlobHash, BlobStore, ByteStream, MemoryBlobStore};
use nimbus_core::Result as NimbusResult;
use nimbus_storage::{ObjectBlobLayout, ObjectChunkRef, ObjectManifest, ObjectManifestAttributes};

use super::{checked, fs_with_mounts, memfs_rc};
use crate::{ExternalFuseObjectMount, MountTable, ObjectManifestStore, ObjectRwBackend};

const BUCKET: &str = "launch-bucket";

/// The manifest plane this suite mounts behind `ObjectRwBackend`.
///
/// `ObjectManifestStore` is the inverted seam: the backend declares the
/// capability it needs and whoever mounts it supplies an implementation.
/// Production wiring owes an engine-backed, committer-fenced one (see the
/// trait's fencing contract); a test owns its entire namespace with a single
/// writer, so this map-backed double is the honest stand-in and keeps the
/// filesystem suite independent of any storage provider.
#[derive(Default)]
struct MemoryObjectManifests {
    manifests: Mutex<BTreeMap<(String, String), ObjectManifest>>,
}

impl MemoryObjectManifests {
    fn entries(&self) -> MutexGuard<'_, BTreeMap<(String, String), ObjectManifest>> {
        self.manifests
            .lock()
            .expect("manifest map lock should not be poisoned")
    }
}

impl ObjectManifestStore for MemoryObjectManifests {
    fn get_manifest(&self, bucket: &str, key: &str) -> NimbusResult<Option<ObjectManifest>> {
        Ok(self
            .entries()
            .get(&(bucket.to_string(), key.to_string()))
            .cloned())
    }

    fn list_manifests(
        &self,
        bucket: &str,
        prefix: &str,
        limit: usize,
    ) -> NimbusResult<Vec<ObjectManifest>> {
        // Keyed `(bucket, key)`, so map order already yields ascending keys
        // within one bucket — the order the trait requires.
        Ok(self
            .entries()
            .iter()
            .filter(|((entry_bucket, key), _)| entry_bucket == bucket && key.starts_with(prefix))
            .map(|(_, manifest)| manifest.clone())
            .take(limit)
            .collect())
    }

    fn put_manifest(&self, manifest: &ObjectManifest) -> NimbusResult<()> {
        self.entries().insert(
            (manifest.bucket.clone(), manifest.key.clone()),
            manifest.clone(),
        );
        Ok(())
    }

    fn delete_manifest(&self, bucket: &str, key: &str) -> NimbusResult<()> {
        self.entries()
            .remove(&(bucket.to_string(), key.to_string()));
        Ok(())
    }
}

/// Wraps `MemoryBlobStore` and counts body bytes actually transferred
/// through `get` (whole-blob) and `get_range` (windowed), so tests can prove
/// the in-isolate reader never materializes more than it reads.
#[derive(Default)]
struct TrackingBlobStore {
    inner: MemoryBlobStore,
    get_calls: AtomicUsize,
    range_calls: AtomicUsize,
    get_bytes: AtomicUsize,
    range_bytes: AtomicUsize,
}

impl TrackingBlobStore {
    fn get_calls(&self) -> usize {
        self.get_calls.load(Ordering::SeqCst)
    }

    fn range_calls(&self) -> usize {
        self.range_calls.load(Ordering::SeqCst)
    }

    /// Total body bytes transferred across both whole-blob `get` and
    /// windowed `get_range` calls — the number that must be zero at open
    /// and must equal exactly the requested window on a bounded read.
    fn total_body_bytes(&self) -> usize {
        self.get_bytes.load(Ordering::SeqCst) + self.range_bytes.load(Ordering::SeqCst)
    }

    fn put_sync(&self, bytes: Bytes) -> BlobHash {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(self.inner.put(bytes))
            .expect("blob put should succeed")
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
        self.get_calls.fetch_add(1, Ordering::SeqCst);
        let bytes = self.inner.get(hash).await?;
        self.get_bytes.fetch_add(bytes.len(), Ordering::SeqCst);
        Ok(bytes)
    }

    async fn get_stream(&self, hash: &BlobHash) -> NimbusResult<ByteStream> {
        self.inner.get_stream(hash).await
    }

    async fn get_range(&self, hash: &BlobHash, range: Range<u64>) -> NimbusResult<Bytes> {
        self.range_calls.fetch_add(1, Ordering::SeqCst);
        let bytes = self.inner.get_range(hash, range).await?;
        self.range_bytes.fetch_add(bytes.len(), Ordering::SeqCst);
        Ok(bytes)
    }

    async fn has(&self, hash: &BlobHash) -> NimbusResult<bool> {
        self.inner.has(hash).await
    }

    async fn release(&self, hash: &BlobHash) -> NimbusResult<()> {
        self.inner.release(hash).await
    }
}

/// Builds a chunked object (three chunks: 4096, 4096, 2048 bytes — the last
/// chunk deliberately shorter to exercise a final partial chunk) directly
/// through the manifest plane, mirroring how the S3 multipart-complete path
/// (`nimbus-s3/src/service.rs`) constructs `ObjectBlobLayout::Chunked`.
/// Returns the reference (concatenated) bytes alongside the tracking store
/// and manifest plane so tests can assert both correctness and byte counts.
fn chunked_object(
    manifests: &MemoryObjectManifests,
    key: &str,
) -> (Arc<TrackingBlobStore>, Vec<u8>, ObjectManifest) {
    let store = Arc::new(TrackingBlobStore::default());
    let chunk_bytes: Vec<Bytes> = vec![
        Bytes::from(vec![b'A'; 4096]),
        Bytes::from(vec![b'B'; 4096]),
        Bytes::from(vec![b'C'; 2048]),
    ];
    let mut reference = Vec::new();
    let mut chunks = Vec::new();
    let mut offset = 0u64;
    for bytes in &chunk_bytes {
        let hash = store.put_sync(bytes.clone());
        reference.extend_from_slice(bytes);
        chunks.push(ObjectChunkRef {
            blob_hash: hash.to_hex(),
            offset,
            len: bytes.len() as u64,
        });
        offset += bytes.len() as u64;
    }
    let manifest = ObjectManifest::chunked(
        BUCKET,
        key,
        offset,
        chunks,
        ObjectManifestAttributes::new("\"chunked-etag\"", 1),
    )
    .expect("chunked manifest should validate");
    manifests
        .put_manifest(&manifest)
        .expect("manifest commit should succeed");
    // Reset counters: `put_sync` above goes through the tracking store's
    // `put`, which this suite does not count against the read-path budget.
    store.get_calls.store(0, Ordering::SeqCst);
    store.range_calls.store(0, Ordering::SeqCst);
    store.get_bytes.store(0, Ordering::SeqCst);
    store.range_bytes.store(0, Ordering::SeqCst);
    (store, reference, manifest)
}

fn object_backend() -> (
    ObjectRwBackend,
    Arc<MemoryBlobStore>,
    Arc<MemoryObjectManifests>,
) {
    let blobs = Arc::new(MemoryBlobStore::new());
    let manifests = Arc::new(MemoryObjectManifests::default());
    let blob_store: Arc<dyn BlobStore> = blobs.clone();
    let meta_store: Arc<dyn ObjectManifestStore> = manifests.clone();
    (
        ObjectRwBackend::new(BUCKET, blob_store, meta_store).expect("backend should build"),
        blobs,
        manifests,
    )
}

#[test]
fn object_rw_backend_write_file_commits_blob_manifest_and_reads_back() {
    let (backend, blobs, manifests) = object_backend();
    let mut table = MountTable::new(memfs_rc());
    table.mount("/objects", MaybeArc::new(backend)).unwrap();
    let fs = fs_with_mounts(table);

    fs.mkdir_sync(&checked(Path::new("/objects/reports")), true, None)
        .unwrap();
    fs.write_file_sync(
        &checked(Path::new("/objects/reports/launch.txt")),
        OpenOptions::write(true, false, false, None),
        b"ready for traffic",
    )
    .unwrap();

    assert_eq!(blobs.len(), 1, "object file write stores one content blob");
    let manifest = manifests
        .get_manifest(BUCKET, "reports/launch.txt")
        .expect("manifest lookup should succeed")
        .expect("manifest should be committed");
    assert_eq!(manifest.size, 17);
    assert!(matches!(
        manifest.blob_layout,
        ObjectBlobLayout::Whole { .. }
    ));
    assert_eq!(
        fs.read_file_sync(
            &checked(Path::new("/objects/reports/launch.txt")),
            OpenOptions::read()
        )
        .unwrap()
        .as_ref(),
        b"ready for traffic"
    );
}

#[test]
fn object_rw_backend_visibility_follows_manifest_commit() {
    let (backend, blobs, manifests) = object_backend();
    let mut session = backend
        .begin_agent_write("/pending/data.bin")
        .expect("write session should start");

    session.write_sequential(0, b"partial").unwrap();
    assert!(
        backend.read_path("/pending/data.bin").is_err(),
        "uncommitted object writes must not be visible through the mount"
    );
    assert_eq!(blobs.len(), 0, "staging does not admit blobs before commit");

    let manifest = session.commit().expect("commit should publish manifest");
    assert_eq!(manifest.key, "pending/data.bin");
    assert_eq!(blobs.len(), 1, "commit admits the blob exactly once");
    assert_eq!(
        manifests
            .get_manifest(BUCKET, "pending/data.bin")
            .unwrap()
            .unwrap()
            .size,
        7
    );
    assert_eq!(
        backend.read_path("/pending/data.bin").unwrap().as_ref(),
        b"partial"
    );
}

#[test]
fn external_fuse_mount_reads_and_rejects_non_sequential_write() {
    let (backend, _blobs, manifests) = object_backend();
    backend
        .commit_path(
            Path::new("/served/read.txt"),
            bytes::Bytes::from_static(b"external-read"),
        )
        .unwrap();
    let fuse = ExternalFuseObjectMount::new(backend.clone());

    assert_eq!(
        fuse.read("/served/read.txt", 3, 5).unwrap(),
        b"ernal".to_vec(),
        "external FUSE face serves committed object reads"
    );

    let mut write = fuse.begin_write("/served/write.txt").unwrap();
    write.write_at(0, b"abc").unwrap();
    let random = write
        .write_at(1, b"x")
        .expect_err("external FUSE writes must be sequential only");
    assert_eq!(random.kind(), std::io::ErrorKind::Unsupported);
    assert!(
        random.to_string().contains("random write"),
        "unexpected non-sequential write error: {random}"
    );
    write.write_at(3, b"def").unwrap();
    write.flush().unwrap();

    assert_eq!(
        manifests
            .get_manifest(BUCKET, "served/write.txt")
            .unwrap()
            .unwrap()
            .size,
        6
    );
    assert_eq!(
        backend.read_path("/served/write.txt").unwrap().as_ref(),
        b"abcdef"
    );
}

#[test]
fn object_rw_backend_lists_manifest_prefixes_as_directories() {
    let (backend, _blobs, _manifests) = object_backend();
    backend
        .commit_path(
            Path::new("/alpha/bravo.txt"),
            bytes::Bytes::from_static(b"bravo"),
        )
        .unwrap();
    backend
        .commit_path(
            Path::new("/alpha/charlie/delta.txt"),
            bytes::Bytes::from_static(b"delta"),
        )
        .unwrap();

    let mut entries = backend
        .read_dir_sync(&checked(Path::new("/alpha")))
        .unwrap()
        .into_iter()
        .map(|entry| (entry.name, entry.is_file, entry.is_directory))
        .collect::<Vec<_>>();
    entries.sort();

    assert_eq!(
        entries,
        vec![
            ("bravo.txt".to_string(), true, false),
            ("charlie".to_string(), false, true),
        ]
    );
}

/// Sets up a chunked object (`chunked_object`) behind a real `ObjectRwBackend`
/// whose byte plane is the tracking store, so `open_sync`/`File` reads flow
/// through the same in-isolate path production code uses (BRH3).
fn chunked_backend(key: &str) -> (ObjectRwBackend, Arc<TrackingBlobStore>, Vec<u8>) {
    let manifests = Arc::new(MemoryObjectManifests::default());
    let (store, reference, _manifest) = chunked_object(&manifests, key);
    let blob_store: Arc<dyn BlobStore> = store.clone();
    let meta_store: Arc<dyn ObjectManifestStore> = manifests;
    let backend =
        ObjectRwBackend::new(BUCKET, blob_store, meta_store).expect("backend should build");
    (backend, store, reference)
}

/// BRH3 decision: `Whole`-layout objects get no eager special case. The lazy
/// path (`read_manifest_range`) serves them through the exact same one
/// `get_range` call a `Chunked` single-chunk object would use; there is no
/// size threshold below which `ObjectFile` falls back to materializing at
/// open. Proven here rather than merely asserted in a doc comment.
#[test]
fn whole_layout_object_reads_lazily_through_get_range_no_eager_special_case() {
    let manifests = Arc::new(MemoryObjectManifests::default());
    let store = Arc::new(TrackingBlobStore::default());
    let blob_store: Arc<dyn BlobStore> = store.clone();
    let meta_store: Arc<dyn ObjectManifestStore> = manifests;
    let backend =
        ObjectRwBackend::new(BUCKET, blob_store, meta_store).expect("backend should build");

    backend
        .commit_path(
            Path::new("/whole/small.bin"),
            Bytes::from_static(b"tiny whole object"),
        )
        .expect("commit should succeed");
    // The commit above writes through `put`; reset counters so this test
    // measures only the read path that follows.
    store.get_calls.store(0, Ordering::SeqCst);
    store.range_calls.store(0, Ordering::SeqCst);
    store.get_bytes.store(0, Ordering::SeqCst);
    store.range_bytes.store(0, Ordering::SeqCst);

    let file = backend
        .open_sync(&checked(Path::new("/whole/small.bin")), OpenOptions::read())
        .expect("open should succeed");
    assert_eq!(
        store.total_body_bytes(),
        0,
        "opening a Whole-layout object must not transfer body bytes either"
    );

    let data = file.read_all_sync().expect("read_all_sync should succeed");
    assert_eq!(data.as_ref(), b"tiny whole object");
    assert_eq!(
        store.get_calls(),
        0,
        "Whole-layout reads go through get_range, never whole-blob get"
    );
    assert_eq!(store.range_calls(), 1);
}

#[test]
fn open_does_not_materialize_whole_object() {
    let (backend, store, _reference) = chunked_backend("chunked/untouched.bin");

    let file = backend
        .open_sync(
            &checked(Path::new("/chunked/untouched.bin")),
            OpenOptions::read(),
        )
        .expect("open should succeed");
    drop(file);

    assert_eq!(
        store.total_body_bytes(),
        0,
        "opening (and closing without reading) a chunked object must transfer zero body bytes"
    );
    assert_eq!(store.get_calls(), 0);
    assert_eq!(store.range_calls(), 0);
}

#[test]
fn object_file_reads_are_windowed() {
    let (backend, store, _reference) = chunked_backend("chunked/windowed.bin");

    let file = backend
        .open_sync(
            &checked(Path::new("/chunked/windowed.bin")),
            OpenOptions::read(),
        )
        .expect("open should succeed");
    assert_eq!(
        store.total_body_bytes(),
        0,
        "open itself must not transfer any body bytes"
    );

    // Chunk layout is 4096(A) + 4096(B) + 2048(C). A window of [2048, 6144)
    // spans the tail of chunk0 and the head of chunk1 only — 4096 bytes,
    // none of chunk2.
    let mut buf = [0_u8; 4096];
    let nread = file
        .read_at_sync(&mut buf, 2048)
        .expect("windowed read should succeed");
    assert_eq!(nread, 4096);
    let mut expected = vec![b'A'; 2048];
    expected.extend(vec![b'B'; 2048]);
    assert_eq!(buf.as_slice(), expected.as_slice());

    assert_eq!(
        store.total_body_bytes(),
        4096,
        "a 4KiB windowed read must transfer exactly the requested bytes, not the whole 10KiB object"
    );
    assert_eq!(
        store.get_calls(),
        0,
        "windowed reads must never use whole-blob get"
    );
    assert_eq!(
        store.range_calls(),
        2,
        "the window overlaps exactly two chunks"
    );
}

#[test]
fn lazy_sequential_read_matches_eager_reference_including_final_partial_chunk() {
    let (backend, store, reference) = chunked_backend("chunked/sequential.bin");
    assert_eq!(reference.len(), 4096 + 4096 + 2048);

    let file = backend
        .open_sync(
            &checked(Path::new("/chunked/sequential.bin")),
            OpenOptions::read(),
        )
        .expect("open should succeed");
    assert_eq!(store.total_body_bytes(), 0, "open must not materialize");

    // 3000-byte reads over a 10240-byte object: 3000, 3000, 3000, then a
    // final 1240-byte partial read at EOF that also lands inside the
    // shorter (2048-byte) final chunk.
    const WINDOW: usize = 3000;
    let mut collected = Vec::new();
    loop {
        let mut buf = [0_u8; WINDOW];
        let nread = file
            .clone()
            .read_sync(&mut buf)
            .expect("sequential read should succeed");
        if nread == 0 {
            break;
        }
        collected.extend_from_slice(&buf[..nread]);
        if nread < WINDOW {
            break;
        }
    }

    assert_eq!(
        collected, reference,
        "lazy sequential read must equal the eager reference byte-for-byte"
    );

    // Cross-check against the backend's own eager whole-object read path
    // (`read_path`, used by `read_file_sync`/`copy_file_sync`), proving the
    // lazy in-isolate reader and the eager path agree byte-for-byte.
    let eager = backend
        .read_path(Path::new("/chunked/sequential.bin"))
        .expect("eager read should succeed");
    assert_eq!(eager.as_ref(), reference.as_slice());
    assert_eq!(collected, eager.to_vec());
}
