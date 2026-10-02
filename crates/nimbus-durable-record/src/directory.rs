//! Directory entry durability.

use std::io;
use std::path::Path;

/// Make every completed entry change in `path` durable.
///
/// A file `fsync` does not persist the directory entry that names the file.
/// Call this after a create, rename, link, or unlink whose survival matters.
///
/// On Unix this opens the directory and calls `fsync`. Other platforms have
/// no portable directory sync, so this is a no-op there. On Windows the commit
/// step of [`crate::StagedWrite`] uses a write-through move instead.
#[cfg(unix)]
pub fn sync_directory(path: &Path) -> io::Result<()> {
    std::fs::File::open(path)?.sync_all()
}

/// Make every completed entry change in `path` durable.
///
/// Other platforms have no portable directory sync, so this is a no-op.
#[cfg(not(unix))]
pub fn sync_directory(_path: &Path) -> io::Result<()> {
    Ok(())
}
