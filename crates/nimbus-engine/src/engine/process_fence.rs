//! Exclusive process ownership of local Engine persistence roots.
//!
//! Embedded providers can have their own file locks, but those locks do not
//! define one Engine-wide ownership domain. In particular, encrypted redb
//! uses Nimbus's custom storage backend and has no native redb lock. This
//! guard takes one advisory lock per distinct local root before any provider
//! opens and retains those locks for the full [`super::Engine`] lifetime.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use fs2::FileExt as _;
use nimbus_core::{Error, Result, StorageErrorKind};

const LOCK_FILE: &str = ".nimbus-engine.lock";
// fs2 0.4 returns the raw Win32 ERROR_LOCK_VIOLATION code on contention.
const WINDOWS_ERROR_LOCK_VIOLATION: i32 = 33;

/// Lifetime guard for all local roots owned by one Engine.
#[derive(Debug)]
pub(super) struct EngineProcessFence {
    // The files carry the OS lock. Retain their canonical roots in the same
    // value so diagnostics and a debugger can identify every owned domain.
    _roots: Vec<LockedRoot>,
}

#[derive(Debug)]
struct LockedRoot {
    _root: PathBuf,
    _file: File,
}

impl EngineProcessFence {
    /// Acquires every distinct root in canonical-path order.
    ///
    /// Sorting gives every multi-root bootstrap one deterministic acquisition
    /// order. Canonicalization also collapses relative and symlink aliases
    /// before the lock files are opened.
    pub(super) fn acquire(roots: impl IntoIterator<Item = PathBuf>) -> Result<Self> {
        let mut roots = roots
            .into_iter()
            .map(|root| canonical_root(&root))
            .collect::<Result<Vec<_>>>()?;
        roots.sort();
        roots.dedup();

        let mut locked = Vec::with_capacity(roots.len());
        for root in roots {
            locked.push(lock_root(root)?);
        }
        Ok(Self { _roots: locked })
    }
}

fn canonical_root(root: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(root)
        .map_err(|error| root_io_error("create local Engine root", root, error))?;
    std::fs::canonicalize(root)
        .map_err(|error| root_io_error("canonicalize local Engine root", root, error))
}

fn lock_root(root: PathBuf) -> Result<LockedRoot> {
    let lock_path = root.join(LOCK_FILE);
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .map_err(|error| root_io_error("open local Engine lock", &lock_path, error))?;
    match file.try_lock_exclusive() {
        Ok(()) => Ok(LockedRoot {
            _root: root,
            _file: file,
        }),
        Err(error) if is_lock_contended(&error) => Err(Error::storage(
            StorageErrorKind::Busy,
            format!(
                "local Engine root {} is exclusively owned by another live process; close that Engine before reopening the same persistence or control root",
                root.display()
            ),
        )),
        Err(error) => Err(root_io_error(
            "acquire local Engine lock",
            &lock_path,
            error,
        )),
    }
}

fn root_io_error(operation: &str, path: &Path, error: io::Error) -> Error {
    Error::storage(
        StorageErrorKind::Io,
        format!("{operation} {}: {error}", path.display()),
    )
}

fn is_lock_contended(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::WouldBlock
        || matches!(
            error.raw_os_error(),
            Some(11 | 35 | 36 | WINDOWS_ERROR_LOCK_VIOLATION)
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_live_guard_excludes_an_alias_and_releases_on_drop() {
        let parent = tempfile::tempdir().expect("process fence tempdir should build");
        let root = parent.path().join("root");
        let first = EngineProcessFence::acquire([root.clone()])
            .expect("first process fence should acquire");

        let alias = root.join("..").join("root");
        let error = EngineProcessFence::acquire([alias.clone()])
            .expect_err("a canonical alias must share the live root fence");
        assert_eq!(error.storage_kind(), Some(StorageErrorKind::Busy));

        drop(first);
        EngineProcessFence::acquire([alias]).expect("dropping the guard must release the root");
    }

    #[test]
    fn duplicate_root_names_acquire_one_lock_domain() {
        let parent = tempfile::tempdir().expect("process fence tempdir should build");
        let root = parent.path().join("root");
        let alias = root.join("..").join("root");
        let guard = EngineProcessFence::acquire([root, alias])
            .expect("aliases in one bootstrap should deduplicate");
        assert_eq!(guard._roots.len(), 1);
        assert!(guard._roots[0]._root.is_absolute());
    }
}
