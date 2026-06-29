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
