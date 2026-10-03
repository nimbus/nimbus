//! Cross-process exclusion for the writers of one record.

use std::fmt;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

const LOCK_RETRY: Duration = Duration::from_millis(10);

/// A failure on a lock or stage entry of a record.
#[derive(Debug)]
pub enum EntryError {
    /// The entry exists and is not a regular file. A writer never creates
    /// such an entry, so the caller must keep it for inspection.
    NotRegular { path: PathBuf },
    Io {
        operation: &'static str,
        path: PathBuf,
        source: io::Error,
    },
    /// Another holder kept the lock for the whole timeout.
    LockTimeout { path: PathBuf, timeout: Duration },
}

impl EntryError {
    pub(crate) fn io(operation: &'static str, path: &Path) -> impl FnOnce(io::Error) -> Self {
        move |source| Self::Io {
            operation,
            path: path.to_path_buf(),
            source,
        }
    }
}

impl fmt::Display for EntryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotRegular { path } => {
                write!(formatter, "{} is not a regular file", path.display())
            }
            Self::Io {
                operation,
                path,
                source,
            } => write!(
                formatter,
                "failed to {operation} {}: {source}",
                path.display()
            ),
            Self::LockTimeout { path, timeout } => write!(
                formatter,
                "timed out after {timeout:?} acquiring lock {}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for EntryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// An exclusive advisory lock on a record's lock file. Drop releases it.
#[derive(Debug)]
pub struct RecordLock {
    _file: File,
}

impl RecordLock {
    /// Lock the regular file at `path`, and create it when it is absent.
    ///
    /// A held lock is retried until `timeout` passes. An entry that is not a
    /// regular file, a symlink included, is refused before it is opened.
    pub fn acquire(path: &Path, timeout: Duration) -> Result<Self, EntryError> {
        match fs::symlink_metadata(path) {
            Ok(metadata) if !metadata.file_type().is_file() => {
                return Err(EntryError::NotRegular {
                    path: path.to_path_buf(),
                });
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(EntryError::io("inspect lock", path)(error)),
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(EntryError::io("open lock", path))?;
        if !file
            .metadata()
            .map_err(EntryError::io("inspect opened lock", path))?
            .is_file()
        {
            return Err(EntryError::NotRegular {
                path: path.to_path_buf(),
            });
        }
        let deadline = Instant::now() + timeout;
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(Self { _file: file }),
                Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                    thread::sleep(LOCK_RETRY);
                }
                Err(TryLockError::WouldBlock) => {
                    return Err(EntryError::LockTimeout {
                        path: path.to_path_buf(),
                        timeout,
                    });
                }
                Err(TryLockError::Error(source)) => {
                    return Err(EntryError::io("acquire lock", path)(source));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn held_lock_times_out_and_is_reusable_after_release() {
        let root = tempfile::tempdir().expect("temporary directory should exist");
        let path = root.path().join("record.lock");
        let held = RecordLock::acquire(&path, Duration::ZERO).expect("free lock should acquire");

        let contender = path.clone();
        let error =
            thread::spawn(move || RecordLock::acquire(&contender, Duration::from_millis(30)))
                .join()
                .expect("contender should not panic")
                .expect_err("held lock should time out");
        assert!(matches!(error, EntryError::LockTimeout { .. }), "{error:?}");

        drop(held);
        RecordLock::acquire(&path, Duration::ZERO).expect("released lock should acquire");
    }

    #[test]
    fn non_regular_lock_entry_is_refused() {
        let root = tempfile::tempdir().expect("temporary directory should exist");
        let path = root.path().join("record.lock");
        fs::create_dir(&path).expect("directory entry should exist");

        let error = RecordLock::acquire(&path, Duration::ZERO).expect_err("directory must fail");
        assert!(matches!(error, EntryError::NotRegular { .. }), "{error:?}");
        assert!(
            path.is_dir(),
            "the refused entry must remain for inspection"
        );
    }
}
