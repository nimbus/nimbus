//! Persistent Linux network namespace lifecycle.

use std::path::Path;

#[cfg(target_os = "linux")]
use std::ffi::CString;
#[cfg(target_os = "linux")]
use std::fs;
#[cfg(target_os = "linux")]
use std::fs::File;
#[cfg(target_os = "linux")]
use std::os::unix::ffi::OsStrExt;
#[cfg(target_os = "linux")]
use std::thread;

use crate::error::{Result, SandboxError};

pub(super) fn create_persistent_network_namespace(path: &Path) -> Result<()> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = path;
        Err(SandboxError::BackendUnavailable {
            message: "persistent OCI network namespaces require Linux".to_owned(),
        })
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to create network-namespace parent {}: {error}",
                    parent.display()
                ),
            })?;
        }
        if path.exists() {
            remove_persistent_network_namespace(path)?;
        }
        File::create(path).map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to create network-namespace file {}: {error}",
                path.display()
            ),
        })?;

        let target = path.to_path_buf();
        let join = thread::spawn(move || -> Result<()> {
            let target_c = cstring_path(&target)?;
            let source = CString::new("/proc/thread-self/ns/net").map_err(|error| {
                SandboxError::OperationFailed {
                    message: format!("failed to encode network-namespace source path: {error}"),
                }
            })?;
            // SAFETY: unshare and mount are called with validated constant flags and
            // NUL-terminated C strings owned for the duration of the calls.
            unsafe {
                if libc::unshare(libc::CLONE_NEWNET) != 0 {
                    return Err(last_os_error("failed to unshare network namespace"));
                }
                if libc::mount(
                    source.as_ptr(),
                    target_c.as_ptr(),
                    std::ptr::null(),
                    libc::MS_BIND as libc::c_ulong,
                    std::ptr::null(),
                ) != 0
                {
                    return Err(last_os_error("failed to persist network namespace"));
                }
            }
            Ok(())
        });
        join.join().map_err(|_| SandboxError::OperationFailed {
            message: format!(
                "network-namespace setup thread panicked for {}",
                path.display()
            ),
        })?
    }
}

pub(super) fn remove_persistent_network_namespace(path: &Path) -> Result<()> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = path;
        Ok(())
    }

    #[cfg(target_os = "linux")]
    {
        if !path.exists() {
            return Ok(());
        }
        let target_c = cstring_path(path)?;
        // SAFETY: umount2 is called with a valid filesystem path encoded as a
        // NUL-terminated C string owned for the duration of the call.
        unsafe {
            if libc::umount2(target_c.as_ptr(), libc::MNT_DETACH) != 0 {
                let error = std::io::Error::last_os_error();
                if !matches!(
                    error.raw_os_error(),
                    Some(libc::EINVAL) | Some(libc::ENOENT)
                ) {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "failed to remove network namespace {}: {error}",
                            path.display()
                        ),
                    });
                }
            }
        }
        fs::remove_file(path)
            .or_else(ignore_not_found)
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to delete network-namespace file {}: {error}",
                    path.display()
                ),
            })?;
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn cstring_path(path: &Path) -> Result<CString> {
    CString::new(path.as_os_str().as_bytes()).map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "failed to encode filesystem path {}: {error}",
            path.display()
        ),
    })
}

#[cfg(target_os = "linux")]
fn last_os_error(context: &str) -> SandboxError {
    SandboxError::OperationFailed {
        message: format!("{context}: {}", std::io::Error::last_os_error()),
    }
}

#[cfg(target_os = "linux")]
fn ignore_not_found(error: std::io::Error) -> std::io::Result<()> {
    if error.kind() == std::io::ErrorKind::NotFound {
        Ok(())
    } else {
        Err(error)
    }
}
