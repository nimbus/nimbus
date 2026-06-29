use std::path::Path;

use deno_fs::sync::MaybeArc;
use deno_fs::{FileSystem, OpenOptions};

use super::{checked, expect_stat_error, fs_with_mounts, memfs_rc};
use crate::{MemFsBackend, MountResolver, MountTable};

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
