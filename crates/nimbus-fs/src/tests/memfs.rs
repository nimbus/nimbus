use std::path::Path;

use deno_fs::{FileSystem, OpenOptions};

use super::checked;
use crate::MemFsBackend;

#[test]
fn memfs_preserves_pre_epoch_file_timestamps() {
    let fs = MemFsBackend::new();
    let path = checked(Path::new("/pre-epoch.txt"));
    fs.write_file_sync(
        &path,
        OpenOptions::write(true, true, false, None),
        b"payload",
    )
    .unwrap();

    fs.utime_sync(&path, -2, 500_000_000, -1, 250_000_000)
        .unwrap();

    let stat = fs.stat_sync(&path).unwrap();
    assert_eq!(stat.atime, Some(-1_500));
    assert_eq!(stat.mtime, Some(-750));
}
