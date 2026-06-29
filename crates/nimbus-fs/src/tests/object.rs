use std::path::Path;
use std::sync::Arc;

use deno_fs::sync::MaybeArc;
use deno_fs::{FileSystem, OpenOptions};
use nimbus_blob::{BlobStore, MemoryBlobStore};
use nimbus_storage::{ObjectBlobLayout, ObjectMetaStore, TenantStore};

use super::{checked, fs_with_mounts, memfs_rc};
use crate::{ExternalFuseObjectMount, MountTable, ObjectRwBackend};

const BUCKET: &str = "launch-bucket";

fn object_backend() -> (ObjectRwBackend, Arc<MemoryBlobStore>, Arc<TenantStore>) {
    let blobs = Arc::new(MemoryBlobStore::new());
    let manifests = Arc::new(TenantStore::create_in_memory().expect("tenant store should open"));
    let blob_store: Arc<dyn BlobStore> = blobs.clone();
    let meta_store: Arc<dyn ObjectMetaStore + Send + Sync> = manifests.clone();
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
        .get_object_manifest(BUCKET, "reports/launch.txt")
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
            .get_object_manifest(BUCKET, "pending/data.bin")
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
            .get_object_manifest(BUCKET, "served/write.txt")
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
