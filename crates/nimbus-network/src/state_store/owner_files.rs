//! Owner-only authority files and directories.
//!
//! Every authority file and directory is private to the node user. This module
//! owns their creation, permission checks, synchronization, atomic replacement,
//! and the classification of advisory-lock contention.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::Path;

use super::NetworkStateStoreError;

#[cfg(unix)]
pub(super) const OWNER_DIRECTORY_MODE: u32 = 0o700;
#[cfg(unix)]
pub(super) const OWNER_FILE_MODE: u32 = 0o600;
// Also treat the raw Win32 `ERROR_LOCK_VIOLATION` from
// `LockFileEx(..., LOCKFILE_FAIL_IMMEDIATELY)` as lock contention.
const WINDOWS_ERROR_LOCK_VIOLATION: i32 = 33;

pub(crate) fn create_dir_all_owner_only(path: &Path) -> Result<(), NetworkStateStoreError> {
    let mut missing = Vec::new();
    let mut candidate = path;
    while !candidate.exists() {
        missing.push(candidate.to_path_buf());
        candidate = candidate
            .parent()
            .ok_or_else(|| NetworkStateStoreError::Io {
                operation: "resolve authority directory parent",
                path: path.to_path_buf(),
                source: io::Error::new(io::ErrorKind::InvalidInput, "path has no parent"),
            })?;
    }
    create_dir_all_with_owner_mode(path).map_err(|source| NetworkStateStoreError::Io {
        operation: "create authority directory",
        path: path.to_path_buf(),
        source,
    })?;
    for directory in missing.iter().rev() {
        set_owner_directory_permissions(directory)?;
        if let Some(parent) = directory.parent() {
            sync_directory(parent)?;
        }
    }
    set_owner_directory_permissions(path)
}

#[cfg(unix)]
fn create_dir_all_with_owner_mode(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;

    let mut builder = fs::DirBuilder::new();
    builder.recursive(true).mode(OWNER_DIRECTORY_MODE);
    builder.create(path)
}

#[cfg(not(unix))]
fn create_dir_all_with_owner_mode(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)
}

#[cfg(unix)]
fn set_owner_directory_permissions(path: &Path) -> Result<(), NetworkStateStoreError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(OWNER_DIRECTORY_MODE)).map_err(|source| {
        NetworkStateStoreError::Io {
            operation: "protect authority directory",
            path: path.to_path_buf(),
            source,
        }
    })
}

#[cfg(not(unix))]
fn set_owner_directory_permissions(_path: &Path) -> Result<(), NetworkStateStoreError> {
    Ok(())
}

pub(crate) fn open_owner_file(
    path: &Path,
    create_new: bool,
) -> Result<File, NetworkStateStoreError> {
    let mut options = OpenOptions::new();
    options.read(true).write(true);
    if create_new {
        options.create_new(true);
    } else {
        options.create(true).truncate(false);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(OWNER_FILE_MODE);
    }
    let file = options
        .open(path)
        .map_err(|source| NetworkStateStoreError::Io {
            operation: "open owner-only authority file",
            path: path.to_path_buf(),
            source,
        })?;
    set_owner_file_permissions(path, &file)?;
    Ok(file)
}

#[cfg(unix)]
fn set_owner_file_permissions(path: &Path, file: &File) -> Result<(), NetworkStateStoreError> {
    use std::os::unix::fs::PermissionsExt;
    file.set_permissions(fs::Permissions::from_mode(OWNER_FILE_MODE))
        .map_err(|source| NetworkStateStoreError::Io {
            operation: "protect authority file",
            path: path.to_path_buf(),
            source,
        })
}

#[cfg(not(unix))]
fn set_owner_file_permissions(_path: &Path, _file: &File) -> Result<(), NetworkStateStoreError> {
    Ok(())
}

#[cfg(unix)]
pub(super) fn validate_owner_file_permissions(
    path: &Path,
    file: &File,
) -> Result<(), NetworkStateStoreError> {
    use std::os::unix::fs::PermissionsExt;
    let mode = file
        .metadata()
        .map_err(|source| NetworkStateStoreError::Io {
            operation: "inspect authority file permissions",
            path: path.to_path_buf(),
            source,
        })?
        .permissions()
        .mode()
        & 0o777;
    if mode & 0o077 == 0 {
        Ok(())
    } else {
        Err(NetworkStateStoreError::InsecurePermissions {
            path: path.to_path_buf(),
            mode,
        })
    }
}

#[cfg(not(unix))]
pub(super) fn validate_owner_file_permissions(
    _path: &Path,
    _file: &File,
) -> Result<(), NetworkStateStoreError> {
    Ok(())
}

#[cfg(unix)]
pub(super) fn sync_directory(path: &Path) -> Result<(), NetworkStateStoreError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|source| NetworkStateStoreError::Io {
            operation: "sync authority directory",
            path: path.to_path_buf(),
            source,
        })
}

#[cfg(not(unix))]
pub(super) fn sync_directory(_path: &Path) -> Result<(), NetworkStateStoreError> {
    Ok(())
}

#[cfg(not(windows))]
pub(super) fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(windows)]
pub(super) fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let mut source_wide: Vec<u16> = source.as_os_str().encode_wide().collect();
    source_wide.push(0);
    let mut destination_wide: Vec<u16> = destination.as_os_str().encode_wide().collect();
    destination_wide.push(0);
    // SAFETY: both pointers address null-terminated buffers that remain alive
    // for the duration of the synchronous call.
    let result = unsafe {
        MoveFileExW(
            source_wide.as_ptr(),
            destination_wide.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

pub(crate) fn is_lock_contended(source: &io::Error) -> bool {
    source.kind() == io::ErrorKind::WouldBlock
        || matches!(
            source.raw_os_error(),
            Some(11 | 35 | 36 | WINDOWS_ERROR_LOCK_VIOLATION)
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_lock_violation_is_classified_as_bounded_contention() {
        let error = io::Error::from_raw_os_error(WINDOWS_ERROR_LOCK_VIOLATION);
        assert!(
            is_lock_contended(&error),
            "Windows ERROR_LOCK_VIOLATION must enter the bounded retry path"
        );
    }
}
