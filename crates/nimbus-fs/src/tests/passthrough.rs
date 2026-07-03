use std::io;
use std::path::{Path, PathBuf};

use deno_fs::{FileSystem, OpenOptions};

use super::checked;
use crate::{NimbusFs, PassthroughBackend};

fn passthrough_backend() -> PassthroughBackend {
    PassthroughBackend::new().expect("passthrough root should open in tests")
}

fn rooted_passthrough_backend(root: impl AsRef<Path>) -> PassthroughBackend {
    PassthroughBackend::rooted(root.as_ref()).expect("test passthrough root should open")
}

#[test]
fn passthrough_round_trip_matches_realfs_for_common_operations() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("file.txt");
    let renamed = dir.path().join("renamed.txt");
    let nested = dir.path().join("nested");

    let fs = NimbusFs::with_cwd(passthrough_backend(), dir.path());
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
fn default_nimbusfs_cwd_is_configured_root_not_process_cwd() {
    let fs = NimbusFs::new(passthrough_backend());

    assert_eq!(fs.cwd().unwrap(), PathBuf::from("/"));
}

#[test]
fn chdir_is_instance_local_and_does_not_touch_process_cwd() {
    let original = std::env::current_dir().unwrap();
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let fs_a = NimbusFs::with_cwd(passthrough_backend(), a.path());
    let fs_b = NimbusFs::with_cwd(passthrough_backend(), b.path());

    let child = a.path().join("child");
    std::fs::create_dir(&child).unwrap();
    fs_a.chdir(&checked(Path::new("child"))).unwrap();

    assert_eq!(fs_a.cwd().unwrap(), child);
    assert_eq!(fs_b.cwd().unwrap(), b.path());
    assert_eq!(std::env::current_dir().unwrap(), original);
}

#[test]
fn raw_passthrough_chdir_does_not_touch_process_cwd() {
    let original = std::env::current_dir().unwrap();
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("child")).unwrap();
    let backend = rooted_passthrough_backend(root.path());

    backend.chdir(&checked(Path::new("/child"))).unwrap();

    assert_eq!(std::env::current_dir().unwrap(), original);
    let error = backend
        .chdir(&checked(Path::new("/missing")))
        .expect_err("missing target should not be admitted");
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
    assert_eq!(std::env::current_dir().unwrap(), original);
}

#[test]
fn rooted_passthrough_stays_under_configured_root() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret.txt"), b"secret").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(outside.path(), root.path().join("outside")).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(outside.path(), root.path().join("outside")).unwrap();

    let fs = NimbusFs::with_cwd(rooted_passthrough_backend(root.path()), "/");
    fs.write_file_sync(
        &checked(Path::new("/inside.txt")),
        OpenOptions::write(true, false, false, None),
        b"inside",
    )
    .unwrap();
    assert_eq!(
        std::fs::read(root.path().join("inside.txt")).unwrap(),
        b"inside"
    );
    assert_eq!(
        fs.realpath_sync(&checked(Path::new("/inside.txt")))
            .unwrap(),
        PathBuf::from("/inside.txt")
    );

    let escape = fs
        .read_file_sync(
            &checked(Path::new("/outside/secret.txt")),
            OpenOptions::read(),
        )
        .expect_err("rooted passthrough must reject symlink escape");
    assert_eq!(escape.kind(), io::ErrorKind::PermissionDenied);
}

#[test]
fn rooted_passthrough_rejects_parent_escape_before_create() {
    let root = tempfile::tempdir().unwrap();
    let backend = rooted_passthrough_backend(root.path());
    let escape_name = format!(
        "{}-escape.txt",
        root.path()
            .file_name()
            .expect("tempdir should have basename")
            .to_string_lossy()
    );
    let outside_target = root
        .path()
        .parent()
        .expect("tempdir should have parent")
        .join(&escape_name);
    assert!(!outside_target.exists());

    let escape = match backend.open_sync(
        &checked(&Path::new("/..").join(&escape_name)),
        OpenOptions::write(true, false, false, None),
    ) {
        Ok(_) => panic!("parent traversal must fail before file creation"),
        Err(error) => error,
    };

    assert_eq!(escape.kind(), io::ErrorKind::PermissionDenied);
    assert!(
        !outside_target.exists(),
        "escape target outside the root must not be created"
    );
}

#[test]
fn rooted_passthrough_rejects_absolute_symlink_targets() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let backend = rooted_passthrough_backend(root.path());

    let error = backend
        .symlink_sync(
            &checked(&outside.path().join("secret.txt")),
            &checked(Path::new("/link")),
            None,
        )
        .expect_err("absolute symlink targets must not be admitted");

    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert!(!root.path().join("link").exists());
}

#[cfg(unix)]
#[test]
fn symlink_parent_swap_toctou_never_leaks_outside_content() {
    // In-process analog of runc CVE-2026-41579 / crun CVE-2026-47766: a
    // parent directory that was legitimate at the time a caller checked it
    // is atomically replaced with a symlink pointing outside the sandboxed
    // root before the caller actually uses the path. cap-std's directory
    // handles are opened once (by fd) and every subsequent lookup is
    // resolved relative to that fd, so a later swap of a path *component*
    // must never let a lookup land outside the root the fd was opened
    // against.
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();

    // Legitimate nested structure inside the root: R/a/b/leaf.txt.
    std::fs::create_dir_all(root.path().join("a/b")).unwrap();
    std::fs::write(root.path().join("a/b/leaf.txt"), b"inside-content").unwrap();

    // A different nested structure living entirely outside the root, with
    // different content at the same relative path.
    std::fs::create_dir_all(outside.path().join("b")).unwrap();
    std::fs::write(outside.path().join("b/leaf.txt"), b"outside-content").unwrap();
    let outside_leaf = outside.path().join("b/leaf.txt");
    let outside_leaf_before = std::fs::read(&outside_leaf).unwrap();
    let outside_mtime_before = std::fs::metadata(&outside_leaf)
        .unwrap()
        .modified()
        .unwrap();

    let backend = rooted_passthrough_backend(root.path());

    // The swap: replace R/a with a symlink to the outside tempdir, between
    // whatever earlier check admitted "/a" and this access.
    std::fs::remove_dir_all(root.path().join("a")).unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join("a")).unwrap();

    // Reading through the swapped parent must never return the outside
    // file's content. cap-std is expected to refuse the traversal outright;
    // at an absolute minimum the outside bytes must never come back.
    match backend.read_file_sync(&checked(Path::new("/a/b/leaf.txt")), OpenOptions::read()) {
        Err(_) => {}
        Ok(bytes) => assert_ne!(
            bytes.as_ref(),
            b"outside-content",
            "read through a swapped parent must never leak the outside file's content"
        ),
    }

    // Creating/writing through the swapped parent must not land outside R
    // either.
    let write_result = backend.write_file_sync(
        &checked(Path::new("/a/newfile.txt")),
        OpenOptions::write(true, false, false, None),
        b"escaped",
    );
    assert!(
        write_result.is_err(),
        "write through a swapped parent must not be admitted"
    );
    assert!(
        !outside.path().join("newfile.txt").exists(),
        "write through a swapped parent must not create a file outside the root"
    );

    // The pre-existing outside file must be untouched by any of the above.
    assert_eq!(
        std::fs::read(&outside_leaf).unwrap(),
        outside_leaf_before,
        "outside file content must be unchanged by access through the swapped parent"
    );
    assert_eq!(
        std::fs::metadata(&outside_leaf)
            .unwrap()
            .modified()
            .unwrap(),
        outside_mtime_before,
        "outside file mtime must be unchanged by access through the swapped parent"
    );
}

#[cfg(unix)]
#[test]
fn rooted_passthrough_cp_rejects_absolute_symlink_targets() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let source_dir = root.path().join("source");
    std::fs::create_dir(&source_dir).unwrap();
    std::os::unix::fs::symlink(outside.path().join("secret.txt"), source_dir.join("link")).unwrap();

    let backend = rooted_passthrough_backend(root.path());
    let error = backend
        .cp_sync(
            &checked(Path::new("/source")),
            &checked(Path::new("/destination")),
        )
        .expect_err("recursive copy must not admit absolute symlink targets");

    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert!(
        !root.path().join("destination/link").exists(),
        "copy must fail before creating an escaped symlink in the destination tree"
    );
}

#[cfg(unix)]
#[test]
fn ambient_root_passthrough_follows_absolute_symlinks() {
    // The live launch-default grant (RW "/") has no boundary to protect: the
    // cap-std sandbox is vacuous at root "/", so this backend must behave
    // exactly like RealFs, including following a pre-existing absolute-target
    // symlink on read and stat. Before the ambient-root carve-out, this
    // failed with "a path led outside of the filesystem" even though the
    // link and its target were both real, ordinary files.
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("real-target.txt");
    std::fs::write(&target, b"through-link").unwrap();
    let link = dir.path().join("abs-link.txt");
    std::os::unix::fs::symlink(&target, &link).unwrap();

    let grants = crate::FsCaps::new().grant("/", crate::FsMountCaps::read_write());
    let fs = crate::file_system_for_grants(&grants).unwrap();

    let bytes = fs
        .read_file_sync(&checked(&link), deno_fs::OpenOptions::read())
        .expect("ambient root must follow a pre-existing absolute symlink on read");
    assert_eq!(bytes.as_ref(), b"through-link");

    let stat = fs
        .stat_sync(&checked(&link))
        .expect("ambient root must follow a pre-existing absolute symlink on stat");
    assert!(stat.is_file);
}

#[cfg(unix)]
#[test]
fn ambient_root_passthrough_creates_absolute_symlink_targets() {
    // RealFs parity for symlink creation, matching the pattern used by the
    // 11 gated Node-compat fixtures that create absolute-target symlinks
    // (e.g. test/parallel/test-fs-symlink.js via fixtures.path(...)).
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("real-target.txt");
    std::fs::write(&target, b"absolute-target").unwrap();
    let link = dir.path().join("abs-link.txt");

    let grants = crate::FsCaps::new().grant("/", crate::FsMountCaps::read_write());
    let fs = crate::file_system_for_grants(&grants).unwrap();

    fs.symlink_sync(&checked(&target), &checked(&link), None)
        .expect("ambient root must create an absolute-target symlink like RealFs");

    let resolved = fs
        .read_link_sync(&checked(&link))
        .expect("ambient root must read back the absolute symlink target");
    assert_eq!(resolved, target);

    let bytes = fs
        .read_file_sync(&checked(&link), deno_fs::OpenOptions::read())
        .expect("the newly created absolute symlink must be traversable");
    assert_eq!(bytes.as_ref(), b"absolute-target");
}
