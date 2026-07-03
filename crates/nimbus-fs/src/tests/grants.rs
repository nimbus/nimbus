//! Coverage for the grant-resolved `FileSystemRc` constructors (FCW1):
//! `file_system_for_grants` and `deny_file_system`. These replace the old
//! ungated `default_file_system()` convenience.

use std::io;

use deno_fs::OpenOptions;

use super::{checked, expect_stat_error};
use crate::{FsCaps, FsMountCaps};

#[test]
fn ungranted_substrate_gets_deny_filesystem() {
    // 1. The policy choice for "no grant resolved" (`nimbus-server`'s
    //    `resolve_fs_grants() -> None`) is `deny_file_system()`: every
    //    operation must be rejected, and rejected for an authority reason,
    //    not because the path happens not to exist on host disk.
    let deny = crate::deny_file_system();
    let dir = tempfile::tempdir().unwrap();
    let existing = dir.path().join("exists.txt");
    std::fs::write(&existing, b"on-disk").unwrap();

    let open_error = match deny.open_sync(&checked(&existing), OpenOptions::read()) {
        Ok(_) => {
            panic!("deny filesystem must reject open even for a file that exists on host disk")
        }
        Err(error) => error,
    };
    assert_eq!(open_error.kind(), io::ErrorKind::PermissionDenied);

    let stat_error = expect_stat_error(
        deny.stat_sync(&checked(&existing)),
        "deny filesystem must reject stat",
    );
    assert_eq!(stat_error.kind(), io::ErrorKind::PermissionDenied);

    let write_error = deny
        .write_file_sync(
            &checked(&dir.path().join("new.txt")),
            OpenOptions::write(true, false, false, None),
            b"nope",
        )
        .expect_err("deny filesystem must reject writes");
    assert_eq!(write_error.kind(), io::ErrorKind::PermissionDenied);
    assert!(
        !dir.path().join("new.txt").exists(),
        "deny filesystem must never reach host disk"
    );

    // 2. `file_system_for_grants` with an empty `FsCaps` must also deny
    //    everything (masked root mount) rather than falling through to the
    //    passthrough backend underneath it.
    let empty_grants = FsCaps::new();
    let gated = crate::file_system_for_grants(&empty_grants).unwrap();
    let masked_stat = expect_stat_error(
        gated.stat_sync(&checked(&existing)),
        "empty FsCaps must mask the root mount, not fall through to passthrough",
    );
    assert_eq!(masked_stat.kind(), io::ErrorKind::NotFound);
    assert!(
        masked_stat.to_string().contains("ungranted"),
        "masked-mount denial must be distinguishable from a real host-disk ENOENT, got: {masked_stat}"
    );
    assert!(
        !gated.exists_sync(&checked(&existing)),
        "an ungranted substrate must never observe a file that is actually on host disk"
    );
}

#[test]
fn read_only_root_grant_is_enforced() {
    let dir = tempfile::tempdir().unwrap();
    let existing = dir.path().join("readable.txt");
    std::fs::write(&existing, b"seed").unwrap();

    let grants = FsCaps::new().grant("/", FsMountCaps::read_only());
    let fs = crate::file_system_for_grants(&grants).unwrap();

    assert_eq!(
        fs.read_file_sync(&checked(&existing), OpenOptions::read())
            .unwrap()
            .as_ref(),
        b"seed",
        "read-only grant must still permit reads"
    );

    let write_error = fs
        .write_file_sync(
            &checked(&existing),
            OpenOptions::write(false, false, false, None),
            b"mutated",
        )
        .expect_err("read-only grant must reject writes");
    assert!(
        write_error.to_string().contains("EROFS"),
        "unexpected read-only write error surface: {write_error}"
    );

    let create_error = fs
        .write_file_sync(
            &checked(&dir.path().join("new.txt")),
            OpenOptions::write(true, false, false, None),
            b"created",
        )
        .expect_err("read-only grant must reject file creation");
    assert!(
        create_error.to_string().contains("EROFS"),
        "unexpected read-only create error surface: {create_error}"
    );
    assert!(!dir.path().join("new.txt").exists());

    let remove_error = fs
        .remove_sync(&checked(&existing), false)
        .expect_err("read-only grant must reject removal");
    assert!(
        remove_error.to_string().contains("EROFS"),
        "unexpected read-only remove error surface: {remove_error}"
    );
    assert!(existing.exists(), "read-only grant must not allow removal");
}

#[test]
fn launch_default_grants_match_passthrough() {
    // No-regression proof: an explicit read-write "/" grant (the launch
    // default `nimbus-server` resolves via `launch_default_grants`) must
    // behave identically to the old ungated `default_file_system()` for the
    // same round trip exercised in `delegation::passthrough_round_trip_matches_realfs_for_common_operations`.
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("file.txt");
    let renamed = dir.path().join("renamed.txt");
    let nested = dir.path().join("nested");

    let launch_default_grant = FsCaps::new().grant("/", FsMountCaps::read_write());
    let fs = crate::file_system_for_grants(&launch_default_grant).unwrap();

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
