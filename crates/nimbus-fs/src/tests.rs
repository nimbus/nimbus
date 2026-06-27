use std::borrow::Cow;
use std::collections::BTreeSet;
use std::io;
use std::ops::Range;
use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::Mutex;

use bytes::Bytes;
use deno_fs::sync::MaybeArc;
use deno_fs::{FileSystem, FsDirEntry, FsFileType, FsReadDirRc, OpenOptions};
use deno_io::fs::{File, FsError, FsResult, FsStat, FsStatFs};
use deno_permissions::{CheckedPath, CheckedPathBuf};
use nimbus_blob::{BlobHash, BlobStore, ByteStream, LocalPackStore};
use nimbus_core::Result as NimbusResult;

use super::{
    BackendRegistry, CacheLookup, CasBlobChunk, CasReadOnlyBackend, CasReadOnlyManifest,
    ChunkCache, DirPerms, FilePerms, FsCaps, FsMountCaps, MemFsBackend, MountResolver, MountTable,
    NimbusFs, ObjectRwBackend, ObjectUnsupportedOperation, PassthroughBackend, PersistenceMode,
    ResolvedAccess, WasiPreopenBuilder,
};

fn checked(path: &Path) -> CheckedPath<'_> {
    CheckedPath::unsafe_new(Cow::Borrowed(path))
}

fn checked_buf(path: impl Into<PathBuf>) -> CheckedPathBuf {
    CheckedPathBuf::unsafe_new(path.into())
}

#[test]
fn passthrough_round_trip_matches_realfs_for_common_operations() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("file.txt");
    let renamed = dir.path().join("renamed.txt");
    let nested = dir.path().join("nested");

    let fs = NimbusFs::with_cwd(PassthroughBackend::new(), dir.path());
    fs.write_file_sync(
        &checked(&file),
        OpenOptions::write(true, false, false, None),
        b"hello",
    )
    .unwrap();
    assert_eq!(
        fs.read_file_sync(&checked(&file), OpenOptions::read())
            .unwrap()
            .as_ref(),
        b"hello"
    );
    assert!(fs.stat_sync(&checked(&file)).unwrap().is_file);
    fs.mkdir_sync(&checked(&nested), false, None).unwrap();
    assert!(fs.stat_sync(&checked(&nested)).unwrap().is_directory);
    fs.rename_sync(&checked(&file), &checked(&renamed)).unwrap();
    fs.truncate_sync(&checked(&renamed), 2).unwrap();
    assert_eq!(
        fs.read_file_sync(&checked(&renamed), OpenOptions::read())
            .unwrap()
            .as_ref(),
        b"he"
    );
    fs.remove_sync(&checked(&renamed), false).unwrap();
    assert!(!fs.exists_sync(&checked(&renamed)));
}

#[test]
fn chdir_is_instance_local_and_does_not_touch_process_cwd() {
    let original = std::env::current_dir().unwrap();
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let fs_a = NimbusFs::with_cwd(PassthroughBackend::new(), a.path());
    let fs_b = NimbusFs::with_cwd(PassthroughBackend::new(), b.path());

    let child = a.path().join("child");
    std::fs::create_dir(&child).unwrap();
    fs_a.chdir(&checked(Path::new("child"))).unwrap();

    assert_eq!(fs_a.cwd().unwrap(), child);
    assert_eq!(fs_b.cwd().unwrap(), b.path());
    assert_eq!(std::env::current_dir().unwrap(), original);
}

fn memfs_rc() -> deno_fs::FileSystemRc {
    MaybeArc::new(MemFsBackend::new())
}

fn fs_with_mounts(table: MountTable) -> NimbusFs {
    NimbusFs::with_mount_table(table, "/")
}

fn expect_stat_error(result: FsResult<FsStat>, message: &str) -> FsError {
    match result {
        Ok(_) => panic!("{message}"),
        Err(error) => error,
    }
}

#[test]
fn resolver_uses_longest_prefix_and_rejects_mount_root_escape() {
    let mut table = MountTable::new(memfs_rc());
    table.mount("/app", memfs_rc()).unwrap();
    table.mount("/app/cache", memfs_rc()).unwrap();
    let resolver = MountResolver::new(table);

    let resolved = resolver
        .resolve(Path::new("/"), Path::new("/app/cache/file.txt"))
        .unwrap();
    assert_eq!(resolved.mount_prefix, Path::new("/app/cache"));
    assert_eq!(resolved.backend_path, Path::new("/file.txt"));

    let error = resolver
        .resolve(Path::new("/"), Path::new("/app/../host"))
        .expect_err("parent traversal out of a mount root must be denied");
    assert!(
        error.to_string().contains("mount root"),
        "unexpected error: {error}"
    );
}

#[test]
fn masked_and_readonly_overlays_are_mount_table_entries() {
    let mut table = MountTable::new(memfs_rc());
    table.mount("/scratch", memfs_rc()).unwrap();
    table.mount_readonly("/scratch/ro", memfs_rc()).unwrap();
    table.mount_masked("/scratch/secret").unwrap();
    let fs = fs_with_mounts(table);

    fs.write_file_sync(
        &checked(Path::new("/scratch/file.txt")),
        OpenOptions::write(true, false, false, None),
        b"ok",
    )
    .unwrap();
    assert_eq!(
        fs.read_file_sync(
            &checked(Path::new("/scratch/file.txt")),
            OpenOptions::read()
        )
        .unwrap()
        .as_ref(),
        b"ok"
    );

    let readonly = fs
        .write_file_sync(
            &checked(Path::new("/scratch/ro/file.txt")),
            OpenOptions::write(true, false, false, None),
            b"denied",
        )
        .expect_err("readonly overlay must reject writes before backend dispatch");
    assert!(
        readonly.to_string().contains("EROFS"),
        "unexpected readonly error: {readonly}"
    );

    assert!(!fs.exists_sync(&checked(Path::new("/scratch/secret"))));
    let masked = expect_stat_error(
        fs.stat_sync(&checked(Path::new("/scratch/secret"))),
        "masked overlay should be opaque",
    );
    assert!(
        masked.to_string().contains("masked"),
        "unexpected masked error: {masked}"
    );
}

#[test]
fn memfs_round_trip_and_teardown_are_backend_local() {
    let backend = MemFsBackend::new();
    let mut table = MountTable::new(memfs_rc());
    table.mount("/mem", MaybeArc::new(backend.clone())).unwrap();
    let fs = fs_with_mounts(table);

    fs.mkdir_sync(&checked(Path::new("/mem/dir")), false, None)
        .unwrap();
    fs.write_file_sync(
        &checked(Path::new("/mem/dir/file.txt")),
        OpenOptions::write(true, false, false, None),
        b"session",
    )
    .unwrap();
    assert_eq!(
        fs.read_file_sync(
            &checked(Path::new("/mem/dir/file.txt")),
            OpenOptions::read()
        )
        .unwrap()
        .as_ref(),
        b"session"
    );
    assert!(backend.total_bytes() >= 7);

    let mut fresh = MountTable::new(memfs_rc());
    fresh.mount("/mem", memfs_rc()).unwrap();
    let fresh = fs_with_mounts(fresh);
    assert!(!fresh.exists_sync(&checked(Path::new("/mem/dir/file.txt"))));
}

#[test]
fn cross_mount_rename_copy_and_link_fail_explicitly() {
    let mut table = MountTable::new(memfs_rc());
    table.mount("/a", memfs_rc()).unwrap();
    table.mount("/b", memfs_rc()).unwrap();
    let fs = fs_with_mounts(table);

    fs.write_file_sync(
        &checked(Path::new("/a/file.txt")),
        OpenOptions::write(true, false, false, None),
        b"x",
    )
    .unwrap();

    for (label, result) in [
        (
            "rename",
            fs.rename_sync(
                &checked(Path::new("/a/file.txt")),
                &checked(Path::new("/b/file.txt")),
            ),
        ),
        (
            "copy",
            fs.copy_file_sync(
                &checked(Path::new("/a/file.txt")),
                &checked(Path::new("/b/file.txt")),
            ),
        ),
        (
            "link",
            fs.link_sync(
                &checked(Path::new("/a/file.txt")),
                &checked(Path::new("/b/file.txt")),
            ),
        ),
    ] {
        let error = result.expect_err("cross-mount operation must fail");
        assert!(
            error.to_string().contains(&format!("cross-mount {label}")),
            "unexpected {label} error: {error}"
        );
    }
}

#[test]
fn symlink_targets_and_realpath_stay_inside_virtual_mount() {
    let mut table = MountTable::new(memfs_rc());
    table.mount("/mem", memfs_rc()).unwrap();
    let fs = fs_with_mounts(table);

    fs.mkdir_sync(&checked(Path::new("/mem/dir")), false, None)
        .unwrap();
    fs.write_file_sync(
        &checked(Path::new("/mem/dir/file.txt")),
        OpenOptions::write(true, false, false, None),
        b"x",
    )
    .unwrap();
    assert_eq!(
        fs.realpath_sync(&checked(Path::new("/mem/dir/file.txt")))
            .unwrap(),
        Path::new("/mem/dir/file.txt")
    );

    fs.symlink_sync(
        &checked(Path::new("/host/root")),
        &checked(Path::new("/mem/abs-link")),
        None,
    )
    .unwrap();
    let absolute = expect_stat_error(
        fs.stat_sync(&checked(Path::new("/mem/abs-link"))),
        "absolute symlink targets must be denied on access",
    );
    assert!(
        absolute.to_string().contains("absolute symlink"),
        "unexpected absolute symlink error: {absolute}"
    );

    fs.symlink_sync(
        &checked(Path::new("loop-b")),
        &checked(Path::new("/mem/loop-a")),
        None,
    )
    .unwrap();
    fs.symlink_sync(
        &checked(Path::new("loop-a")),
        &checked(Path::new("/mem/loop-b")),
        None,
    )
    .unwrap();
    let loop_error = expect_stat_error(
        fs.stat_sync(&checked(Path::new("/mem/loop-a"))),
        "symlink loops must be denied on access",
    );
    assert!(
        loop_error.to_string().contains("loop"),
        "unexpected symlink loop error: {loop_error}"
    );

    fs.symlink_sync(
        &checked(Path::new("/host/root")),
        &checked(Path::new("/mem/parent")),
        None,
    )
    .unwrap();
    let parent_escape = expect_stat_error(
        fs.stat_sync(&checked(Path::new("/mem/parent/file.txt"))),
        "pre-seeded symlink parents cannot escape roots",
    );
    assert!(
        parent_escape.to_string().contains("absolute symlink"),
        "unexpected parent symlink error: {parent_escape}"
    );
}

#[derive(Default)]
struct TrackingBlobStore {
    inner: LocalPackStore,
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

fn table_with_mem_mount(prefix: &str, backend: MemFsBackend) -> MountTable {
    let mut table = MountTable::new(memfs_rc());
    table.mount(prefix, MaybeArc::new(backend)).unwrap();
    table
}

#[test]
fn fscaps_ungranted_mount_is_invisible_without_passthrough_fallthrough() {
    let allowed = MemFsBackend::new();
    let secret = MemFsBackend::new();
    secret
        .write_file_sync(
            &checked(Path::new("/leak.txt")),
            OpenOptions::write(true, false, false, None),
            b"secret",
        )
        .unwrap();
    let mut table = MountTable::new(memfs_rc());
    table.mount("/allowed", MaybeArc::new(allowed)).unwrap();
    table.mount("/secret", MaybeArc::new(secret)).unwrap();

    let gated = FsCaps::new()
        .grant("/allowed", FsMountCaps::read_write())
        .apply_to_mount_table(&table);
    let fs = fs_with_mounts(gated);

    fs.write_file_sync(
        &checked(Path::new("/allowed/file.txt")),
        OpenOptions::write(true, false, false, None),
        b"ok",
    )
    .unwrap();
    assert!(!fs.exists_sync(&checked(Path::new("/secret/leak.txt"))));
    let hidden = expect_stat_error(
        fs.stat_sync(&checked(Path::new("/secret/leak.txt"))),
        "ungranted mount must be hidden",
    );
    assert_eq!(hidden.kind(), io::ErrorKind::NotFound);
}

#[test]
fn fscaps_readonly_and_write_size_quota_are_enforced() {
    let backend = MemFsBackend::new();
    let gated = FsCaps::new()
        .grant("/data", FsMountCaps::read_write().with_write_size_limit(4))
        .apply_to_mount_table(&table_with_mem_mount("/data", backend));
    let fs = fs_with_mounts(gated);

    fs.write_file_sync(
        &checked(Path::new("/data/small.txt")),
        OpenOptions::write(true, false, false, None),
        b"1234",
    )
    .unwrap();
    let quota = fs
        .write_file_sync(
            &checked(Path::new("/data/large.txt")),
            OpenOptions::write(true, false, false, None),
            b"12345",
        )
        .expect_err("write-size quota must reject oversized writes");
    assert_eq!(quota.kind(), io::ErrorKind::StorageFull);

    let readonly_backend = MemFsBackend::new();
    let readonly = FsCaps::new()
        .grant("/ro", FsMountCaps::read_only())
        .apply_to_mount_table(&table_with_mem_mount("/ro", readonly_backend));
    let fs = fs_with_mounts(readonly);
    let error = fs
        .write_file_sync(
            &checked(Path::new("/ro/file.txt")),
            OpenOptions::write(true, false, false, None),
            b"x",
        )
        .expect_err("readonly grant rejects writes");
    assert!(
        error.to_string().contains("EROFS"),
        "unexpected readonly error: {error}"
    );
}

#[test]
fn fscaps_open_and_mutation_matrix_is_fail_closed() {
    let read = FsCaps::open_requires(OpenOptions::read());
    assert!(read.file_read);
    assert!(!read.file_write);
    assert!(!read.directory_mutate);

    let create = FsCaps::open_requires(OpenOptions::write(true, false, false, None));
    assert!(create.file_write);
    assert!(create.directory_mutate);
    assert!(create.create);
    assert!(create.truncate);

    let append = FsCaps::open_requires(OpenOptions::write(false, true, false, None));
    assert!(append.file_write);
    assert!(append.append);

    let mut no_metadata = FsMountCaps::read_write();
    no_metadata.metadata_mutate = false;
    let backend = MemFsBackend::new();
    backend
        .write_file_sync(
            &checked(Path::new("/file.txt")),
            OpenOptions::write(true, false, false, None),
            b"x",
        )
        .unwrap();
    let fs = fs_with_mounts(
        FsCaps::new()
            .grant("/data", no_metadata)
            .apply_to_mount_table(&table_with_mem_mount("/data", backend)),
    );
    let chmod = fs
        .chmod_sync(&checked(Path::new("/data/file.txt")), 0o600)
        .expect_err("chmod requires metadata-mutate");
    assert!(chmod.to_string().contains("metadata-mutate"));
    let chown = fs
        .chown_sync(&checked(Path::new("/data/file.txt")), Some(1), Some(1))
        .expect_err("chown requires metadata-mutate");
    assert!(chown.to_string().contains("metadata-mutate"));
    let utime = fs
        .utime_sync(&checked(Path::new("/data/file.txt")), 1, 0, 1, 0)
        .expect_err("utime requires metadata-mutate");
    assert!(utime.to_string().contains("metadata-mutate"));

    let mut no_dir = FsMountCaps::read_write();
    no_dir.directory_mutate = false;
    let fs = fs_with_mounts(
        FsCaps::new()
            .grant("/data", no_dir)
            .apply_to_mount_table(&table_with_mem_mount("/data", MemFsBackend::new())),
    );
    let create_denied = fs
        .write_file_sync(
            &checked(Path::new("/data/new.txt")),
            OpenOptions::write(true, false, false, None),
            b"x",
        )
        .expect_err("create requires directory-mutate");
    assert!(create_denied.to_string().contains("EROFS"));
    let rename = fs
        .rename_sync(
            &checked(Path::new("/data/a.txt")),
            &checked(Path::new("/data/b.txt")),
        )
        .expect_err("rename requires directory-mutate");
    assert!(rename.to_string().contains("EROFS"));

    let mut no_link = FsMountCaps::read_write();
    no_link.link_create = false;
    let fs = fs_with_mounts(
        FsCaps::new()
            .grant("/data", no_link)
            .apply_to_mount_table(&table_with_mem_mount("/data", MemFsBackend::new())),
    );
    let link = fs
        .link_sync(
            &checked(Path::new("/data/a.txt")),
            &checked(Path::new("/data/b.txt")),
        )
        .expect_err("link requires link-create");
    assert!(link.to_string().contains("link-create"));
    let symlink = fs
        .symlink_sync(
            &checked(Path::new("a.txt")),
            &checked(Path::new("/data/link.txt")),
            None,
        )
        .expect_err("symlink creation requires link-create");
    assert!(symlink.to_string().contains("link-create"));
}

#[test]
fn wasi_preopen_builder_maps_dir_and_file_permissions() {
    let mut table = MountTable::new(memfs_rc());
    table.mount("/rw", memfs_rc()).unwrap();
    table.mount_readonly("/ro", memfs_rc()).unwrap();
    table.mount("/read", memfs_rc()).unwrap();
    let mut read_only_files = FsMountCaps::read_write();
    read_only_files.file_write = false;
    read_only_files.directory_mutate = false;
    read_only_files.readonly = true;
    let caps = FsCaps::new()
        .grant("/rw", FsMountCaps::read_write())
        .grant("/ro", FsMountCaps::read_write())
        .grant("/read", read_only_files);

    let builder = WasiPreopenBuilder::from_caps(&table, &caps);

    assert_eq!(builder.descriptors().len(), 3);
    let rw = builder.descriptor_for_path(Path::new("/rw/app")).unwrap();
    assert!(rw.dir_perms.contains(DirPerms::READ));
    assert!(rw.dir_perms.contains(DirPerms::MUTATE));
    assert!(rw.file_perms.contains(FilePerms::READ));
    assert!(rw.file_perms.contains(FilePerms::WRITE));

    let ro = builder.descriptor_for_path(Path::new("/ro/app")).unwrap();
    assert!(ro.dir_perms.contains(DirPerms::READ));
    assert!(!ro.dir_perms.contains(DirPerms::MUTATE));
    assert!(ro.file_perms.contains(FilePerms::READ));
    assert!(!ro.file_perms.contains(FilePerms::WRITE));

    let read = builder.descriptor_for_path(Path::new("/read/app")).unwrap();
    assert!(read.dir_perms.contains(DirPerms::READ));
    assert!(!read.dir_perms.contains(DirPerms::MUTATE));
    assert!(read.file_perms.contains(FilePerms::READ));
    assert!(!read.file_perms.contains(FilePerms::WRITE));
}

#[test]
fn wasi_preopen_builder_omits_denied_and_masked_mounts() {
    let mut table = MountTable::new(memfs_rc());
    table.mount("/visible", memfs_rc()).unwrap();
    table.mount("/denied", memfs_rc()).unwrap();
    table.mount_masked("/masked").unwrap();
    let caps = FsCaps::new()
        .grant("/visible", FsMountCaps::read_write())
        .grant("/masked", FsMountCaps::read_write());

    let builder = WasiPreopenBuilder::from_caps(&table, &caps);

    assert!(
        builder
            .descriptor_for_path(Path::new("/visible/file"))
            .is_some()
    );
    assert!(
        builder
            .descriptor_for_path(Path::new("/denied/file"))
            .is_none()
    );
    assert!(
        builder
            .descriptor_for_path(Path::new("/masked/file"))
            .is_none()
    );
}

#[test]
fn wasi_and_v8_binders_resolve_the_same_gated_mount_and_rights() {
    let mut table = MountTable::new(memfs_rc());
    table.mount("/rw", memfs_rc()).unwrap();
    table.mount_readonly("/ro", memfs_rc()).unwrap();
    let caps = FsCaps::new()
        .grant("/rw", FsMountCaps::read_write())
        .grant("/ro", FsMountCaps::read_write());
    let gated = caps.apply_to_mount_table(&table);
    let resolver = MountResolver::new(gated);
    let builder = WasiPreopenBuilder::from_caps(&table, &caps);

    let rw = builder
        .cross_binder_resolution(&resolver, Path::new("/"), Path::new("/rw/file.txt"))
        .unwrap();
    assert_eq!(rw.v8_mount_prefix, Path::new("/rw"));
    assert_eq!(rw.wasi_preopen_path, Path::new("/rw"));
    assert_eq!(rw.v8_access, ResolvedAccess::ReadWrite);
    assert_eq!(rw.wasi_rights, FsMountCaps::read_write());

    let ro = builder
        .cross_binder_resolution(&resolver, Path::new("/"), Path::new("/ro/file.txt"))
        .unwrap();
    assert_eq!(ro.v8_mount_prefix, Path::new("/ro"));
    assert_eq!(ro.wasi_preopen_path, Path::new("/ro"));
    assert_eq!(ro.v8_access, ResolvedAccess::ReadOnly);
    assert!(ro.wasi_rights.readonly);
}

#[test]
fn backend_registry_registers_stub_and_serves_through_mount_table() {
    let backend = MemFsBackend::new();
    let mut registry = BackendRegistry::new();
    registry
        .register(
            "stub",
            backend,
            FsMountCaps::read_write(),
            PersistenceMode::DurableExternal {
                sync_required: true,
            },
        )
        .unwrap();
    assert!(
        registry.get("stub").unwrap().requires_explicit_sync(),
        "external backend persistence must make sync semantics explicit"
    );

    let mut table = MountTable::new(memfs_rc());
    registry
        .mount_registered(&mut table, "/external", "stub")
        .unwrap();
    let fs = fs_with_mounts(table);

    fs.write_file_sync(
        &checked(Path::new("/external/file.txt")),
        OpenOptions::write(true, false, false, None),
        b"registered",
    )
    .unwrap();
    assert_eq!(
        fs.read_file_sync(
            &checked(Path::new("/external/file.txt")),
            OpenOptions::read()
        )
        .unwrap()
        .as_ref(),
        b"registered"
    );
}

#[test]
fn backend_registry_rejects_invalid_fscaps_contract() {
    let mut caps = FsMountCaps::read_only();
    caps.file_write = true;
    let error = BackendRegistry::new()
        .register("bad", MemFsBackend::new(), caps, PersistenceMode::Ephemeral)
        .expect_err("readonly backend cannot advertise write authority");
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn object_rw_backend_slot_rejects_unsupported_posix_operations() {
    assert_eq!(
        ObjectRwBackend::unsupported_operations(),
        &[
            ObjectUnsupportedOperation::RandomWrite,
            ObjectUnsupportedOperation::Hardlink,
            ObjectUnsupportedOperation::Symlink,
            ObjectUnsupportedOperation::MutableOwnership,
            ObjectUnsupportedOperation::DirectoryRename,
        ]
    );

    for operation in ObjectRwBackend::unsupported_operations() {
        let error = ObjectRwBackend::reject_unsupported(*operation)
            .expect_err("object slot must fail unsupported POSIX operation early");
        assert_eq!(error.kind(), io::ErrorKind::Unsupported);
        assert!(
            error.to_string().contains("unsupported POSIX"),
            "unexpected object slot error: {error}"
        );
    }
}

#[test]
fn cache_hit_avoids_refetch_and_eviction_respects_capacity() {
    let mut cache = ChunkCache::new(2);
    let mut fetches = 0;

    let (a, first) = cache.get_or_insert_with("a", |_| {
        fetches += 1;
        "alpha".to_string()
    });
    assert_eq!(a, "alpha");
    assert_eq!(first, CacheLookup::Miss);

    let (a, second) = cache.get_or_insert_with("a", |_| {
        fetches += 1;
        "new-alpha".to_string()
    });
    assert_eq!(a, "alpha");
    assert_eq!(second, CacheLookup::Hit);
    assert_eq!(fetches, 1, "cache hit avoids re-fetch");

    cache.get_or_insert_with("b", |_| {
        fetches += 1;
        "bravo".to_string()
    });
    let (_, hot) = cache.get_or_insert_with("a", |_| {
        fetches += 1;
        "new-alpha".to_string()
    });
    assert_eq!(hot, CacheLookup::Hit);
    cache.get_or_insert_with("c", |_| {
        fetches += 1;
        "charlie".to_string()
    });
    assert_eq!(cache.len(), 2);
    assert!(cache.contains_key(&"a"), "recent cache hit keeps a hot");
    assert!(!cache.contains_key(&"b"), "oldest cold key is evicted");
    assert!(cache.contains_key(&"c"));
}

#[derive(Debug, Clone, Default)]
struct SpyBackend {
    calls: Arc<Mutex<Vec<&'static str>>>,
}

impl SpyBackend {
    fn record_err<T>(&self, method: &'static str) -> FsResult<T> {
        self.calls.lock().unwrap().push(method);
        Err(io::Error::other(method).into())
    }

    fn record_bool(&self, method: &'static str) -> bool {
        self.calls.lock().unwrap().push(method);
        false
    }

    fn record_ok<T>(&self, method: &'static str, value: T) -> FsResult<T> {
        self.calls.lock().unwrap().push(method);
        Ok(value)
    }

    fn call_set(&self) -> BTreeSet<&'static str> {
        self.calls.lock().unwrap().iter().copied().collect()
    }
}

#[async_trait::async_trait(?Send)]
impl FileSystem for SpyBackend {
    fn cwd(&self) -> FsResult<PathBuf> {
        self.record_err("cwd")
    }

    fn tmp_dir(&self) -> FsResult<PathBuf> {
        self.record_err("tmp_dir")
    }

    fn chdir(&self, _path: &CheckedPath<'_>) -> FsResult<()> {
        self.record_err("chdir")
    }

    fn umask(&self, _mask: Option<u32>) -> FsResult<u32> {
        self.record_err("umask")
    }

    fn open_sync(&self, _path: &CheckedPath<'_>, _options: OpenOptions) -> FsResult<Rc<dyn File>> {
        self.record_err("open_sync")
    }

    async fn open_async<'a>(
        &'a self,
        _path: CheckedPathBuf,
        _options: OpenOptions,
    ) -> FsResult<Rc<dyn File>> {
        self.record_err("open_async")
    }

    fn mkdir_sync(
        &self,
        _path: &CheckedPath<'_>,
        _recursive: bool,
        _mode: Option<u32>,
    ) -> FsResult<()> {
        self.record_err("mkdir_sync")
    }

    async fn mkdir_async(
        &self,
        _path: CheckedPathBuf,
        _recursive: bool,
        _mode: Option<u32>,
    ) -> FsResult<()> {
        self.record_err("mkdir_async")
    }

    #[cfg(unix)]
    fn chmod_sync(&self, _path: &CheckedPath<'_>, _mode: u32) -> FsResult<()> {
        self.record_err("chmod_sync")
    }

    #[cfg(not(unix))]
    fn chmod_sync(&self, _path: &CheckedPath<'_>, _mode: i32) -> FsResult<()> {
        self.record_err("chmod_sync")
    }

    #[cfg(unix)]
    async fn chmod_async(&self, _path: CheckedPathBuf, _mode: u32) -> FsResult<()> {
        self.record_err("chmod_async")
    }

    #[cfg(not(unix))]
    async fn chmod_async(&self, _path: CheckedPathBuf, _mode: i32) -> FsResult<()> {
        self.record_err("chmod_async")
    }

    fn chown_sync(
        &self,
        _path: &CheckedPath<'_>,
        _uid: Option<u32>,
        _gid: Option<u32>,
    ) -> FsResult<()> {
        self.record_err("chown_sync")
    }

    async fn chown_async(
        &self,
        _path: CheckedPathBuf,
        _uid: Option<u32>,
        _gid: Option<u32>,
    ) -> FsResult<()> {
        self.record_err("chown_async")
    }

    fn lchmod_sync(&self, _path: &CheckedPath<'_>, _mode: u32) -> FsResult<()> {
        self.record_err("lchmod_sync")
    }

    async fn lchmod_async(&self, _path: CheckedPathBuf, _mode: u32) -> FsResult<()> {
        self.record_err("lchmod_async")
    }

    fn lchown_sync(
        &self,
        _path: &CheckedPath<'_>,
        _uid: Option<u32>,
        _gid: Option<u32>,
    ) -> FsResult<()> {
        self.record_err("lchown_sync")
    }

    async fn lchown_async(
        &self,
        _path: CheckedPathBuf,
        _uid: Option<u32>,
        _gid: Option<u32>,
    ) -> FsResult<()> {
        self.record_err("lchown_async")
    }

    fn remove_sync(&self, _path: &CheckedPath<'_>, _recursive: bool) -> FsResult<()> {
        self.record_err("remove_sync")
    }

    async fn remove_async(&self, _path: CheckedPathBuf, _recursive: bool) -> FsResult<()> {
        self.record_err("remove_async")
    }

    fn copy_file_sync(
        &self,
        _oldpath: &CheckedPath<'_>,
        _newpath: &CheckedPath<'_>,
    ) -> FsResult<()> {
        self.record_err("copy_file_sync")
    }

    async fn copy_file_async(
        &self,
        _oldpath: CheckedPathBuf,
        _newpath: CheckedPathBuf,
    ) -> FsResult<()> {
        self.record_err("copy_file_async")
    }

    fn cp_sync(&self, _path: &CheckedPath<'_>, _new_path: &CheckedPath<'_>) -> FsResult<()> {
        self.record_err("cp_sync")
    }

    async fn cp_async(&self, _path: CheckedPathBuf, _new_path: CheckedPathBuf) -> FsResult<()> {
        self.record_err("cp_async")
    }

    fn stat_sync(&self, _path: &CheckedPath<'_>) -> FsResult<FsStat> {
        self.record_err("stat_sync")
    }

    async fn stat_async(&self, _path: CheckedPathBuf) -> FsResult<FsStat> {
        self.record_err("stat_async")
    }

    fn lstat_sync(&self, _path: &CheckedPath<'_>) -> FsResult<FsStat> {
        self.record_err("lstat_sync")
    }

    async fn lstat_async(&self, _path: CheckedPathBuf) -> FsResult<FsStat> {
        self.record_err("lstat_async")
    }

    fn statfs_sync(&self, _path: &CheckedPath<'_>, _bigint: bool) -> FsResult<FsStatFs> {
        self.record_err("statfs_sync")
    }

    async fn statfs_async(&self, _path: CheckedPathBuf, _bigint: bool) -> FsResult<FsStatFs> {
        self.record_err("statfs_async")
    }

    fn realpath_sync(&self, _path: &CheckedPath<'_>) -> FsResult<PathBuf> {
        self.record_err("realpath_sync")
    }

    async fn realpath_async(&self, _path: CheckedPathBuf) -> FsResult<PathBuf> {
        self.record_err("realpath_async")
    }

    fn read_dir_sync(&self, _path: &CheckedPath<'_>) -> FsResult<Vec<FsDirEntry>> {
        self.record_err("read_dir_sync")
    }

    async fn read_dir_async(&self, _path: CheckedPathBuf) -> FsResult<FsReadDirRc> {
        self.record_err("read_dir_async")
    }

    fn rename_sync(&self, _oldpath: &CheckedPath<'_>, _newpath: &CheckedPath<'_>) -> FsResult<()> {
        self.record_err("rename_sync")
    }

    async fn rename_async(
        &self,
        _oldpath: CheckedPathBuf,
        _newpath: CheckedPathBuf,
    ) -> FsResult<()> {
        self.record_err("rename_async")
    }

    fn rmdir_sync(&self, _path: &CheckedPath<'_>) -> FsResult<()> {
        self.record_err("rmdir_sync")
    }

    async fn rmdir_async(&self, _path: CheckedPathBuf) -> FsResult<()> {
        self.record_err("rmdir_async")
    }

    fn link_sync(&self, _oldpath: &CheckedPath<'_>, _newpath: &CheckedPath<'_>) -> FsResult<()> {
        self.record_err("link_sync")
    }

    async fn link_async(&self, _oldpath: CheckedPathBuf, _newpath: CheckedPathBuf) -> FsResult<()> {
        self.record_err("link_async")
    }

    fn symlink_sync(
        &self,
        _oldpath: &CheckedPath<'_>,
        _newpath: &CheckedPath<'_>,
        _file_type: Option<FsFileType>,
    ) -> FsResult<()> {
        self.record_err("symlink_sync")
    }

    async fn symlink_async(
        &self,
        _oldpath: CheckedPathBuf,
        _newpath: CheckedPathBuf,
        _file_type: Option<FsFileType>,
    ) -> FsResult<()> {
        self.record_err("symlink_async")
    }

    fn read_link_sync(&self, _path: &CheckedPath<'_>) -> FsResult<PathBuf> {
        self.record_err("read_link_sync")
    }

    async fn read_link_async(&self, _path: CheckedPathBuf) -> FsResult<PathBuf> {
        self.record_err("read_link_async")
    }

    fn truncate_sync(&self, _path: &CheckedPath<'_>, _len: u64) -> FsResult<()> {
        self.record_err("truncate_sync")
    }

    async fn truncate_async(&self, _path: CheckedPathBuf, _len: u64) -> FsResult<()> {
        self.record_err("truncate_async")
    }

    fn utime_sync(
        &self,
        _path: &CheckedPath<'_>,
        _atime_secs: i64,
        _atime_nanos: u32,
        _mtime_secs: i64,
        _mtime_nanos: u32,
    ) -> FsResult<()> {
        self.record_err("utime_sync")
    }

    async fn utime_async(
        &self,
        _path: CheckedPathBuf,
        _atime_secs: i64,
        _atime_nanos: u32,
        _mtime_secs: i64,
        _mtime_nanos: u32,
    ) -> FsResult<()> {
        self.record_err("utime_async")
    }

    fn lutime_sync(
        &self,
        _path: &CheckedPath<'_>,
        _atime_secs: i64,
        _atime_nanos: u32,
        _mtime_secs: i64,
        _mtime_nanos: u32,
    ) -> FsResult<()> {
        self.record_err("lutime_sync")
    }

    async fn lutime_async(
        &self,
        _path: CheckedPathBuf,
        _atime_secs: i64,
        _atime_nanos: u32,
        _mtime_secs: i64,
        _mtime_nanos: u32,
    ) -> FsResult<()> {
        self.record_err("lutime_async")
    }

    fn exists_sync(&self, _path: &CheckedPath<'_>) -> bool {
        self.record_bool("exists_sync")
    }

    async fn exists_async(&self, _path: CheckedPathBuf) -> FsResult<bool> {
        self.record_ok("exists_async", false)
    }
}

#[test]
fn delegates_filesystem_trait_methods_to_backend_or_composes_them() {
    let root = tempfile::tempdir().unwrap();
    let spy = SpyBackend::default();
    let fs = NimbusFs::with_cwd(spy.clone(), root.path());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let path = checked(Path::new("a"));
    let other = checked(Path::new("b"));

    assert_eq!(fs.cwd().unwrap(), root.path());
    let _ = fs.chdir(&path);
    let _ = fs.tmp_dir();
    let _ = fs.umask(None);
    let _ = fs.open_sync(&path, OpenOptions::read());
    let _ = fs.mkdir_sync(&path, false, None);
    let _ = fs.chmod_sync(&path, 0o600);
    let _ = fs.chown_sync(&path, Some(1), Some(2));
    let _ = fs.lchmod_sync(&path, 0o600);
    let _ = fs.lchown_sync(&path, Some(1), Some(2));
    let _ = fs.remove_sync(&path, false);
    let _ = fs.copy_file_sync(&path, &other);
    let _ = fs.cp_sync(&path, &other);
    let _ = fs.stat_sync(&path);
    let _ = fs.lstat_sync(&path);
    let _ = fs.statfs_sync(&path, false);
    let _ = fs.realpath_sync(&path);
    let _ = fs.read_dir_sync(&path);
    let _ = fs.rename_sync(&path, &other);
    let _ = fs.rmdir_sync(&path);
    let _ = fs.link_sync(&path, &other);
    let _ = fs.symlink_sync(&path, &other, None);
    let _ = fs.read_link_sync(&path);
    let _ = fs.truncate_sync(&path, 1);
    let _ = fs.utime_sync(&path, 1, 2, 3, 4);
    let _ = fs.lutime_sync(&path, 1, 2, 3, 4);
    assert!(!fs.exists_sync(&path));
    let _ = fs.write_file_sync(&path, OpenOptions::write(true, false, false, None), b"x");
    let _ = fs.read_file_sync(&path, OpenOptions::read());
    assert!(!fs.is_file_sync(&path));
    assert!(!fs.is_dir_sync(&path));
    let _ = fs.read_text_file_lossy_sync(&path);

    runtime.block_on(async {
        let _ = fs.open_async(checked_buf("a"), OpenOptions::read()).await;
        let _ = fs.mkdir_async(checked_buf("a"), false, None).await;
        let _ = fs.chmod_async(checked_buf("a"), 0o600).await;
        let _ = fs.chown_async(checked_buf("a"), Some(1), Some(2)).await;
        let _ = fs.lchmod_async(checked_buf("a"), 0o600).await;
        let _ = fs.lchown_async(checked_buf("a"), Some(1), Some(2)).await;
        let _ = fs.remove_async(checked_buf("a"), false).await;
        let _ = fs.copy_file_async(checked_buf("a"), checked_buf("b")).await;
        let _ = fs.cp_async(checked_buf("a"), checked_buf("b")).await;
        let _ = fs.stat_async(checked_buf("a")).await;
        let _ = fs.lstat_async(checked_buf("a")).await;
        let _ = fs.statfs_async(checked_buf("a"), false).await;
        let _ = fs.realpath_async(checked_buf("a")).await;
        let _ = fs.read_dir_async(checked_buf("a")).await;
        let _ = fs.rename_async(checked_buf("a"), checked_buf("b")).await;
        let _ = fs.rmdir_async(checked_buf("a")).await;
        let _ = fs.link_async(checked_buf("a"), checked_buf("b")).await;
        let _ = fs
            .symlink_async(checked_buf("a"), checked_buf("b"), None)
            .await;
        let _ = fs.read_link_async(checked_buf("a")).await;
        let _ = fs.truncate_async(checked_buf("a"), 1).await;
        let _ = fs.utime_async(checked_buf("a"), 1, 2, 3, 4).await;
        let _ = fs.lutime_async(checked_buf("a"), 1, 2, 3, 4).await;
        let _ = fs.exists_async(checked_buf("a")).await;
        let _ = fs
            .write_file_async(
                checked_buf("a"),
                OpenOptions::write(true, false, false, None),
                Box::from(*b"x"),
            )
            .await;
        let _ = fs
            .read_file_async(checked_buf("a"), OpenOptions::read())
            .await;
        let _ = fs.read_text_file_lossy_async(checked_buf("a")).await;
    });

    let calls = spy.call_set();
    let required = BTreeSet::from([
        "tmp_dir",
        "umask",
        "open_sync",
        "open_async",
        "mkdir_sync",
        "mkdir_async",
        "chmod_sync",
        "chmod_async",
        "chown_sync",
        "chown_async",
        "lchmod_sync",
        "lchmod_async",
        "lchown_sync",
        "lchown_async",
        "remove_sync",
        "remove_async",
        "copy_file_sync",
        "copy_file_async",
        "cp_sync",
        "cp_async",
        "stat_sync",
        "stat_async",
        "lstat_sync",
        "lstat_async",
        "statfs_sync",
        "statfs_async",
        "realpath_sync",
        "realpath_async",
        "read_dir_sync",
        "read_dir_async",
        "rename_sync",
        "rename_async",
        "rmdir_sync",
        "rmdir_async",
        "link_sync",
        "link_async",
        "symlink_sync",
        "symlink_async",
        "read_link_sync",
        "read_link_async",
        "truncate_sync",
        "truncate_async",
        "utime_sync",
        "utime_async",
        "lutime_sync",
        "lutime_async",
        "exists_sync",
        "exists_async",
    ]);
    assert_eq!(&required - &calls, BTreeSet::new());
    assert!(
        !calls.contains("cwd"),
        "NimbusFS owns cwd instead of delegating to backend process cwd"
    );
    assert!(
        !calls.contains("chdir"),
        "NimbusFS chdir must compose over stat without mutating backend process cwd"
    );

    let backend_rc: deno_fs::FileSystemRc = MaybeArc::new(spy);
    let _ = NimbusFs::with_backend_rc(backend_rc, root.path());
}
