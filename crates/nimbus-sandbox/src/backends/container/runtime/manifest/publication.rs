//! Crash-safe publication for one container manifest.
//!
//! `manifest.json` is the sole commit point. Stage bytes are never promoted by
//! reconciliation: an interrupted writer is either followed by the previous
//! canonical manifest or by a later complete publication.

use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use crate::error::{Result, SandboxError};
use fs2::FileExt;

pub(in crate::backends::container::runtime) const MANIFEST_PUBLICATION_LOCK_FILE: &str =
    ".nimbus-container-manifest.lock";
pub(in crate::backends::container::runtime) const MANIFEST_PUBLICATION_STAGE_FILE: &str =
    ".nimbus-container-manifest.stage";
#[cfg(not(test))]
const MANIFEST_PUBLICATION_LOCK_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(test)]
const MANIFEST_PUBLICATION_LOCK_TIMEOUT: Duration = Duration::from_millis(100);
const MANIFEST_PUBLICATION_LOCK_RETRY: Duration = Duration::from_millis(10);

pub(in crate::backends::container::runtime) fn reconcile_startup_manifest_publications(
    state_root: &Path,
) -> Result<()> {
    let container_state_dirs = crate::artifact_paths::all_container_state_dirs(state_root)
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to enumerate container manifest publication state under {}: {error}",
                state_root.display()
            ),
        })?;
    let mut failures = Vec::new();
    for container_state_dir in container_state_dirs {
        let reconciliation = (|| -> Result<()> {
            if has_exact_stage_candidate(&container_state_dir)? {
                let _guard = lock_publication(&container_state_dir)?;
                reconcile_exact_stage_files(&container_state_dir)?;
            }
            Ok(())
        })();
        if let Err(error) = reconciliation {
            failures.push(format!("{}: {error}", container_state_dir.display()));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(SandboxError::OperationFailed {
            message: format!(
                "container manifest startup reconciliation failed for {} independent state \
                 director{}: {}",
                failures.len(),
                if failures.len() == 1 { "y" } else { "ies" },
                failures.join("; ")
            ),
        })
    }
}

pub(super) fn publish(
    state_root: &Path,
    container_state_dir: &Path,
    manifest_path: &Path,
    rendered: &[u8],
) -> Result<()> {
    publish_with_directory_sync(
        state_root,
        container_state_dir,
        manifest_path,
        rendered,
        sync_directory,
    )
}

pub(in crate::backends::container::runtime) fn publish_with_directory_sync<F>(
    state_root: &Path,
    container_state_dir: &Path,
    manifest_path: &Path,
    rendered: &[u8],
    mut directory_sync: F,
) -> Result<()>
where
    F: FnMut(&Path) -> std::io::Result<()>,
{
    if manifest_path.parent() != Some(container_state_dir) {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "container manifest path {} is not directly owned by publication directory {}; \
                 publication remains fenced",
                manifest_path.display(),
                container_state_dir.display()
            ),
        });
    }
    establish_durable_manifest_directory_chain_with(
        state_root,
        container_state_dir,
        &mut directory_sync,
    )?;
    let _guard = lock_publication(container_state_dir)?;
    reconcile_exact_stage_files(container_state_dir)?;

    let staged_path = container_state_dir.join(MANIFEST_PUBLICATION_STAGE_FILE);
    let publication = (|| -> Result<()> {
        let mut staged = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staged_path)
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to create staged sandbox manifest {}: {error}",
                    staged_path.display()
                ),
            })?;
        staged
            .write_all(rendered)
            .and_then(|()| staged.sync_all())
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to durably stage sandbox manifest {}: {error}",
                    staged_path.display()
                ),
            })?;
        fs::rename(&staged_path, manifest_path).map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to atomically publish sandbox manifest {}: {error}",
                manifest_path.display()
            ),
        })?;
        directory_sync(container_state_dir).map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "sandbox manifest {} reached its commit point but the parent-directory sync \
                     failed; publication outcome is ambiguous: {error}",
                manifest_path.display()
            ),
        })
    })();

    if let Err(primary) = publication {
        let cleanup = remove_regular_stage_if_present(&staged_path).and_then(|removed| {
            if removed {
                directory_sync(container_state_dir).map_err(|error| SandboxError::OperationFailed {
                    message: format!(
                        "failed to durably remove staged sandbox manifest {}: {error}",
                        staged_path.display()
                    ),
                })
            } else {
                Ok(())
            }
        });
        return match cleanup {
            Ok(_) => Err(primary),
            Err(cleanup) => Err(SandboxError::OperationFailed {
                message: format!(
                    "{primary}; staged sandbox manifest cleanup also failed: {cleanup}"
                ),
            }),
        };
    }
    Ok(())
}

pub(in crate::backends::container::runtime) fn establish_durable_manifest_directory_chain_with<F>(
    state_root: &Path,
    container_state_dir: &Path,
    directory_sync: F,
) -> Result<()>
where
    F: FnMut(&Path) -> std::io::Result<()>,
{
    crate::backends::oci::durable_directory::establish_durable_directory_chain_with(
        state_root,
        container_state_dir,
        "container manifest",
        directory_sync,
    )
}

fn lock_publication(container_state_dir: &Path) -> Result<ManifestPublicationGuard> {
    let lock_path = container_state_dir.join(MANIFEST_PUBLICATION_LOCK_FILE);
    match fs::symlink_metadata(&lock_path) {
        Ok(metadata) if !metadata.file_type().is_file() => {
            return Err(non_regular_publication_entry(&lock_path));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "failed to inspect container manifest publication lock {}: {error}",
                    lock_path.display()
                ),
            });
        }
    }
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to open container manifest publication lock {}: {error}",
                lock_path.display()
            ),
        })?;
    if !lock
        .metadata()
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to inspect opened container manifest publication lock {}: {error}",
                lock_path.display()
            ),
        })?
        .is_file()
    {
        return Err(non_regular_publication_entry(&lock_path));
    }

    let deadline = Instant::now() + MANIFEST_PUBLICATION_LOCK_TIMEOUT;
    loop {
        match FileExt::try_lock_exclusive(&lock) {
            Ok(()) => return Ok(ManifestPublicationGuard { _lock: lock }),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "timed out acquiring container manifest publication lock {}; \
                             canonical manifest state remains unchanged",
                            lock_path.display()
                        ),
                    });
                }
                thread::sleep(MANIFEST_PUBLICATION_LOCK_RETRY);
            }
            Err(error) => {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "failed to acquire container manifest publication lock {}: {error}",
                        lock_path.display()
                    ),
                });
            }
        }
    }
}

fn has_exact_stage_candidate(container_state_dir: &Path) -> Result<bool> {
    for entry in
        fs::read_dir(container_state_dir).map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to inspect container manifest publication directory {}: {error}",
                container_state_dir.display()
            ),
        })?
    {
        let entry = entry.map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to inspect an entry under container manifest publication directory {}: \
                 {error}",
                container_state_dir.display()
            ),
        })?;
        if is_exact_stage_name(&entry.file_name()) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn reconcile_exact_stage_files(container_state_dir: &Path) -> Result<()> {
    let mut stages = Vec::new();
    for entry in
        fs::read_dir(container_state_dir).map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to reconcile container manifest publication directory {}: {error}",
                container_state_dir.display()
            ),
        })?
    {
        let entry = entry.map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to inspect an entry while reconciling container manifest publication \
                 directory {}: {error}",
                container_state_dir.display()
            ),
        })?;
        if is_exact_stage_name(&entry.file_name()) {
            stages.push(entry.path());
        }
    }
    stages.sort();

    let mut removed = false;
    let mut failures = Vec::new();
    for stage in stages {
        match remove_regular_stage_if_present(&stage) {
            Ok(stage_removed) => removed |= stage_removed,
            Err(error) => failures.push(error.to_string()),
        }
    }
    if removed && let Err(error) = sync_directory(container_state_dir) {
        failures.push(format!(
            "failed to durably reconcile container manifest publication directory {}: {error}",
            container_state_dir.display()
        ));
    }
    if !failures.is_empty() {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "container manifest publication reconciliation failed under {}: {}",
                container_state_dir.display(),
                failures.join("; ")
            ),
        });
    }
    Ok(())
}

fn is_exact_stage_name(name: &std::ffi::OsStr) -> bool {
    name == std::ffi::OsStr::new(MANIFEST_PUBLICATION_STAGE_FILE)
}

fn remove_regular_stage_if_present(stage_path: &Path) -> Result<bool> {
    match fs::symlink_metadata(stage_path) {
        Ok(metadata) if !metadata.file_type().is_file() => {
            Err(non_regular_publication_entry(stage_path))
        }
        Ok(_) => {
            fs::remove_file(stage_path).map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to remove staged sandbox manifest {}: {error}",
                    stage_path.display()
                ),
            })?;
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(SandboxError::OperationFailed {
            message: format!(
                "failed to inspect staged sandbox manifest {}: {error}",
                stage_path.display()
            ),
        }),
    }
}

fn non_regular_publication_entry(path: &Path) -> SandboxError {
    SandboxError::OperationFailed {
        message: format!(
            "container manifest publication entry {} is not a regular file; publication remains \
             fenced",
            path.display()
        ),
    }
}

fn sync_directory(path: &Path) -> std::io::Result<()> {
    File::open(path)?.sync_all()
}

struct ManifestPublicationGuard {
    _lock: File,
}
