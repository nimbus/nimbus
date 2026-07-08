// Copyright 2024 RustFS Team
// Copyright 2026 Nimbus contributors
//
// Adapted from rustfs/rustfs@bd5d3c5d92a0aa70a7d92da3e48761d6e61f0dc9
// (crates/ecstore/src/disk/os.rs directory-fsync helpers and the `SyncMode`
// durable-write recipe from crates/ecstore/src/disk/local.rs) by Nimbus
// contributors. See THIRD_PARTY.md at the crate root.
//
// Licensed under the Apache License, Version 2.0 (the "License"); you may not
// use this file except in compliance with the License. You may obtain a copy
// of the License at http://www.apache.org/licenses/LICENSE-2.0.

//! Durable local-disk primitives for the pack store's commit points.
//!
//! Everything here is synchronous std I/O; callers run it under
//! `spawn_blocking` (as [`crate::LocalPackStore`] already does for every
//! disk-touching operation).
//!
//! ## The durable-replace recipe
//!
//! A file that must atomically replace (or first-create) a commit-point path
//! is written as: temp file in the destination directory → `fdatasync` the
//! temp contents → rename over the destination → `fsync` the destination's
//! parent directory. A crash before the rename leaves only a stale temp file
//! (cleaned at the next open); a crash after the rename but before the
//! directory fsync can lose the *rename* on power loss, which is equivalent
//! to crashing before it — never a torn destination file.
//!
//! ## Observability
//!
//! Durability regressions are invisible to ordinary behavior tests — the data
//! lands on disk either way — so every sync/rename performed here (and by the
//! pack store's append paths) is reported to a [`SyncObserver`]. Production
//! uses the no-op observer; tests install a recorder and assert the *order*
//! of events, not merely that the operation succeeded.

use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

/// Prefix for temp files created by [`write_replace_durable`]. Files with
/// this prefix found at open time are crash leftovers and are safe to delete.
pub(crate) const TMP_PREFIX: &str = ".nbl-tmp-";

/// Returns whether `file_name` is a crash-leftover temp file.
pub(crate) fn is_stale_temp(file_name: &str) -> bool {
    file_name.starts_with(TMP_PREFIX)
}

// RustFS's recipe carries a three-way `SyncMode` (None / FileOnly /
// FileAndDir); here the replace helper always runs the full
// file-sync→rename→dir-sync sequence. The weaker modes exist upstream for
// staged files a caller renames away itself — Nimbus has no such caller yet,
// and the append paths (pack records, index records) own their sync
// discipline inline. Reintroduce a mode enum when a real staged-write caller
// exists, not before.

/// A sync/rename the disk layer performed, in the order it was performed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SyncEvent {
    /// `fdatasync` of a file's contents.
    FileSync(PathBuf),
    /// `fsync` of a directory (persists create/rename/unlink entries).
    DirSync(PathBuf),
    /// A commit-point rename.
    Rename { from: PathBuf, to: PathBuf },
}

/// Receives every durability-relevant disk event, in order.
pub(crate) trait SyncObserver: Send + Sync {
    fn record(&self, event: SyncEvent);
}

/// Production observer: does nothing.
pub(crate) struct NoopSyncObserver;

impl SyncObserver for NoopSyncObserver {
    fn record(&self, _event: SyncEvent) {}
}

/// `fdatasync` a file's contents and report it.
pub(crate) fn sync_file_data(
    file: &File,
    path: &Path,
    observer: &dyn SyncObserver,
) -> io::Result<()> {
    file.sync_data()?;
    observer.record(SyncEvent::FileSync(path.to_path_buf()));
    Ok(())
}

/// Fsync a directory so recently created, renamed, or removed entries survive
/// power loss. No-op (but still recorded, so ordering tests stay meaningful)
/// on non-Unix platforms where directories cannot be opened for syncing.
pub(crate) fn fsync_dir(dir: &Path, observer: &dyn SyncObserver) -> io::Result<()> {
    #[cfg(unix)]
    {
        File::open(dir)?.sync_all()?;
    }
    observer.record(SyncEvent::DirSync(dir.to_path_buf()));
    Ok(())
}

/// `create_dir_all` with durable directory entries: after creating any
/// missing levels, fsyncs the parent of every newly created directory so the
/// new entries survive power loss. Without this, an acknowledged first write
/// into a brand-new root could lose the entire root directory on crash.
pub(crate) fn create_dir_all_durable(path: &Path, observer: &dyn SyncObserver) -> io::Result<()> {
    // Absolutize first: a relative path's top component has an empty
    // `parent()`, which would silently skip the fsync of the directory entry
    // that names the new root.
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let path = path.as_path();
    // Find the deepest ancestor that already exists.
    let mut missing: Vec<PathBuf> = Vec::new();
    let mut probe = path.to_path_buf();
    while !probe.exists() {
        missing.push(probe.clone());
        match probe.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => probe = parent.to_path_buf(),
            _ => break,
        }
    }
    if missing.is_empty() {
        return Ok(());
    }
    std::fs::create_dir_all(path)?;
    // Fsync the parent of each created level (deepest last) so every new
    // directory entry is durable.
    for dir in missing.iter().rev() {
        if let Some(parent) = dir.parent() {
            if !parent.as_os_str().is_empty() {
                fsync_dir(parent, observer)?;
            }
        }
    }
    Ok(())
}

/// Whether a failed commit-point rename should be retried.
///
/// Only the first failure is retried, and `NotFound` is never retried: the
/// retry does not recreate the missing source or parent directory, so a
/// second attempt is guaranteed to fail identically. Genuine transient errors
/// (e.g. `PermissionDenied` from a concurrent scanner on some platforms) get
/// exactly one retry.
pub(crate) fn should_retry_rename(err: &io::Error, attempt: usize) -> bool {
    attempt == 0 && err.kind() != io::ErrorKind::NotFound
}

/// Writes `bytes` to `final_path` through the durable-replace recipe:
/// temp-in-same-directory → fdatasync → rename → fsync the parent directory.
///
/// The temp file is created in `final_path`'s parent directory (same
/// filesystem, so the rename is atomic) with the [`TMP_PREFIX`] name marker.
pub(crate) fn write_replace_durable(
    final_path: &Path,
    bytes: &[u8],
    observer: &dyn SyncObserver,
) -> io::Result<()> {
    let parent = final_path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "durable write target {} has no parent",
                final_path.display()
            ),
        )
    })?;

    let mut tmp = tempfile::Builder::new()
        .prefix(TMP_PREFIX)
        .tempfile_in(parent)?;
    io::Write::write_all(&mut tmp, bytes)?;
    sync_file_data(tmp.as_file(), tmp.path(), observer)?;

    let tmp_path = tmp.path().to_path_buf();
    let mut attempt = 0usize;
    let mut pending = tmp;
    loop {
        match pending.persist(final_path) {
            Ok(_file) => break,
            Err(err) if should_retry_rename(&err.error, attempt) => {
                attempt += 1;
                pending = err.file;
            }
            Err(err) => return Err(err.error),
        }
    }
    observer.record(SyncEvent::Rename {
        from: tmp_path,
        to: final_path.to_path_buf(),
    });

    fsync_dir(parent, observer)?;
    Ok(())
}

#[cfg(test)]
pub(crate) mod recorder {
    use std::sync::{Arc, Mutex};

    use super::{SyncEvent, SyncObserver};

    /// Test observer capturing every event in order.
    #[derive(Clone, Default)]
    pub(crate) struct RecordingSyncObserver {
        events: Arc<Mutex<Vec<SyncEvent>>>,
    }

    impl RecordingSyncObserver {
        pub(crate) fn new() -> Self {
            Self::default()
        }

        pub(crate) fn events(&self) -> Vec<SyncEvent> {
            self.events.lock().expect("sync recorder poisoned").clone()
        }

        /// Index of the first event matching `pred`, or a panic naming the
        /// recorded sequence — order assertions read better than `Option`s.
        pub(crate) fn index_where(&self, pred: impl Fn(&SyncEvent) -> bool) -> usize {
            let events = self.events();
            events
                .iter()
                .position(pred)
                .unwrap_or_else(|| panic!("no matching sync event in {events:?}"))
        }
    }

    impl SyncObserver for RecordingSyncObserver {
        fn record(&self, event: SyncEvent) {
            self.events
                .lock()
                .expect("sync recorder poisoned")
                .push(event);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::recorder::RecordingSyncObserver;
    use super::*;

    // Adapted from rustfs/rustfs@bd5d3c5d crates/ecstore/src/disk/os.rs tests:
    // NotFound is terminal for the retry loop; other errors get one retry.
    #[test]
    fn rename_retry_never_retries_not_found() {
        let not_found = io::Error::new(io::ErrorKind::NotFound, "missing");
        assert!(!should_retry_rename(&not_found, 0));
        assert!(!should_retry_rename(&not_found, 1));
    }

    #[test]
    fn rename_retry_allows_single_retry_for_other_errors() {
        let denied = io::Error::new(io::ErrorKind::PermissionDenied, "denied");
        assert!(should_retry_rename(&denied, 0));
        assert!(!should_retry_rename(&denied, 1));
    }

    #[test]
    fn fsync_dir_succeeds_on_directory() {
        let dir = tempfile::tempdir().unwrap();
        fsync_dir(dir.path(), &NoopSyncObserver).unwrap();
    }

    #[test]
    fn fsync_dir_missing_dir_fails() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing");
        let err = fsync_dir(&missing, &NoopSyncObserver).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn write_replace_orders_file_sync_before_rename_before_dir_sync() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("marker");
        let recorder = RecordingSyncObserver::new();

        write_replace_durable(&target, b"payload", &recorder).unwrap();

        assert_eq!(std::fs::read(&target).unwrap(), b"payload");
        let file_sync = recorder.index_where(|e| matches!(e, SyncEvent::FileSync(_)));
        let rename = recorder.index_where(|e| matches!(e, SyncEvent::Rename { .. }));
        let dir_sync =
            recorder.index_where(|e| matches!(e, SyncEvent::DirSync(path) if path == dir.path()));
        assert!(
            file_sync < rename && rename < dir_sync,
            "durable replace must sync contents, then rename, then sync the parent dir: {:?}",
            recorder.events()
        );
        // No temp leftovers after a successful replace.
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| is_stale_temp(&e.file_name().to_string_lossy()))
            .collect();
        assert!(leftovers.is_empty(), "no stale temp files: {leftovers:?}");
    }

    #[test]
    fn write_replace_overwrites_existing_destination() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("marker");
        std::fs::write(&target, b"old").unwrap();

        write_replace_durable(&target, b"new", &NoopSyncObserver).unwrap();

        assert_eq!(std::fs::read(&target).unwrap(), b"new");
    }
}
