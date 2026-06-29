use std::io;
use std::path::Path;

use deno_fs::sync::MaybeArc;
use deno_fs::{FileSystem, OpenOptions};

use super::{checked, expect_stat_error, fs_with_mounts, memfs_rc};
use crate::{
    BackendRegistry, CacheLookup, ChunkCache, DirPerms, FilePerms, FsCaps, FsMountCaps,
    MemFsBackend, MountResolver, MountTable, ObjectRwBackend, ObjectUnsupportedOperation,
    PersistenceMode, ResolvedAccess, WasiPreopenBuilder,
};

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
fn fscaps_readonly_and_max_write_size_are_enforced() {
    let backend = MemFsBackend::new();
    let gated = FsCaps::new()
        .grant("/data", FsMountCaps::read_write().with_max_write_size(4))
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
        .expect_err("max write size must reject oversized writes");
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
fn backend_registry_mount_enforces_registered_caps() {
    let backend = MemFsBackend::new();
    backend
        .write_file_sync(
            &checked(Path::new("/seed.txt")),
            OpenOptions::write(true, false, false, None),
            b"seed",
        )
        .unwrap();
    let mut registry = BackendRegistry::new();
    registry
        .register(
            "readonly",
            backend,
            FsMountCaps::read_only(),
            PersistenceMode::Ephemeral,
        )
        .unwrap();
    let mut table = MountTable::new(memfs_rc());
    registry
        .mount_registered(&mut table, "/data", "readonly")
        .unwrap();
    let fs = fs_with_mounts(table);

    assert_eq!(
        fs.read_file_sync(&checked(Path::new("/data/seed.txt")), OpenOptions::read())
            .unwrap()
            .as_ref(),
        b"seed"
    );
    let error = fs
        .write_file_sync(
            &checked(Path::new("/data/new.txt")),
            OpenOptions::write(true, false, false, None),
            b"nope",
        )
        .expect_err("registered read-only caps must gate writes after mounting");
    assert!(
        error.to_string().contains("EROFS"),
        "unexpected registry cap error: {error}"
    );
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
