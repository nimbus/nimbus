//! One OS-backed lifecycle lock for runner, provision, restart, and teardown.

use std::fs::{self, File, OpenOptions};
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use fs2::FileExt;

use super::*;

/// Process-scoped ownership of one lifecycle transition.
#[derive(Debug)]
pub(in crate::backends::container::runtime) struct RunnerHandoffGuard {
    _lock: File,
}

pub(in crate::backends::container::runtime) struct RunnerInspectionGuard {
    _lock: File,
}

/// Test seam for holding the same Execute lifecycle lock as production paths.
#[cfg(test)]
pub(in crate::backends::container::runtime) fn lock_execute_lifecycle(
    manifest: &ContainerSandboxManifest,
) -> Result<RunnerHandoffGuard> {
    lock_current_execute_lifecycle(manifest, None).map(|(handoff, _)| handoff)
}

/// Acquire the bounded Execute lifecycle lock and return the canonical
/// manifest authenticated under that lock.
pub(in crate::backends::container::runtime) fn lock_current_execute_lifecycle_for_backend(
    backend: &ContainerSandboxBackend,
    manifest: &ContainerSandboxManifest,
) -> Result<(RunnerHandoffGuard, ContainerSandboxManifest)> {
    lock_current_execute_lifecycle(manifest, Some(backend))
}

/// Lock one existing provision lifecycle and return its canonical manifest.
pub(in crate::backends::container::runtime) fn lock_current_provision_lifecycle_for_backend(
    backend: &ContainerSandboxBackend,
    manifest: &ContainerSandboxManifest,
) -> Result<(RunnerHandoffGuard, ContainerSandboxManifest)> {
    let handoff = lock_runner_handoff_with_deadline(
        manifest,
        Some(Instant::now() + RUNNER_HANDOFF_LOCK_TIMEOUT),
        Some(backend),
    )?;
    let persisted = read_runner_manifest(&manifest.conmon_layout.manifest_path)?;
    backend.validate_manifest_execution_context(&persisted)?;
    if persisted.handle.id != manifest.handle.id
        || persisted.spec.tenant_id != manifest.spec.tenant_id
        || persisted.conmon_layout.manifest_path != manifest.conmon_layout.manifest_path
    {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "container provision lifecycle crossed the canonical tenant-qualified manifest for {}",
                manifest.handle.id
            ),
        });
    }
    Ok((handoff, persisted))
}

/// Lock a tenant-qualified lifecycle before its first manifest publication.
pub(in crate::backends::container::runtime) fn lock_new_provision_lifecycle_for_backend(
    backend: &ContainerSandboxBackend,
    container_state_dir: &Path,
) -> Result<RunnerHandoffGuard> {
    fs::create_dir_all(container_state_dir).map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "failed to create container provision lifecycle directory {}: {error}",
            container_state_dir.display()
        ),
    })?;
    lock_runner_handoff_path_with_deadline(
        &container_state_dir.join(RUNNER_HANDOFF_LOCK_FILE),
        Some(Instant::now() + RUNNER_HANDOFF_LOCK_TIMEOUT),
        Some(backend),
    )
}

/// Acquire the Execute lifecycle lock and return the current durable manifest.
pub(in crate::backends::container::runtime) fn lock_execute_lifecycle_and_read_current_for_backend(
    backend: &ContainerSandboxBackend,
    manifest: &ContainerSandboxManifest,
) -> Result<(RunnerHandoffGuard, ContainerSandboxManifest)> {
    if manifest.start_mode != ContainerStartMode::Execute {
        return Err(SandboxError::InvalidSpec {
            message: "execute lifecycle lock requires an Execute manifest".to_owned(),
        });
    }
    let handoff = lock_runner_handoff_with_deadline(
        manifest,
        Some(Instant::now() + RUNNER_HANDOFF_LOCK_TIMEOUT),
        Some(backend),
    )?;
    let persisted = read_runner_manifest(&manifest.conmon_layout.manifest_path)?;
    Ok((handoff, persisted))
}

/// Acquire the existing lifecycle lock in shared mode for an inspection.
pub(in crate::backends::container::runtime) fn lock_current_inspection_for_backend(
    backend: &ContainerSandboxBackend,
    manifest: &ContainerSandboxManifest,
) -> Result<(RunnerInspectionGuard, ContainerSandboxManifest)> {
    lock_current_inspection_with_timeout(backend, manifest, RUNNER_HANDOFF_LOCK_TIMEOUT)
}

#[cfg(test)]
pub(in crate::backends::container::runtime) fn lock_current_inspection_for_backend_with_timeout_for_test(
    backend: &ContainerSandboxBackend,
    manifest: &ContainerSandboxManifest,
    timeout: Duration,
) -> Result<(RunnerInspectionGuard, ContainerSandboxManifest)> {
    lock_current_inspection_with_timeout(backend, manifest, timeout)
}

fn lock_current_inspection_with_timeout(
    backend: &ContainerSandboxBackend,
    manifest: &ContainerSandboxManifest,
    timeout: Duration,
) -> Result<(RunnerInspectionGuard, ContainerSandboxManifest)> {
    #[cfg(not(test))]
    let _ = backend;
    let lock_path = manifest
        .conmon_layout
        .container_state_dir
        .join(RUNNER_HANDOFF_LOCK_FILE);
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&lock_path)
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to open existing container inspection lock {}: {error}; inspection cannot create synchronization state",
                lock_path.display()
            ),
        })?;
    let deadline = Instant::now() + timeout;
    loop {
        match FileExt::try_lock_shared(&lock) {
            Ok(()) => break,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                #[cfg(test)]
                if let Some(probe) = backend.runner_lifecycle_lock_test_probe.as_ref() {
                    probe.record_contended()?;
                }
                if Instant::now() >= deadline {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "timed out acquiring existing container inspection lock {}; observation remains unknown",
                            lock_path.display()
                        ),
                    });
                }
                thread::sleep(RUNNER_HANDOFF_LOCK_RETRY);
            }
            Err(error) => {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "failed to acquire existing container inspection lock {}: {error}",
                        lock_path.display()
                    ),
                });
            }
        }
    }
    let persisted = read_runner_manifest(&manifest.conmon_layout.manifest_path)?;
    Ok((RunnerInspectionGuard { _lock: lock }, persisted))
}

fn lock_current_execute_lifecycle(
    manifest: &ContainerSandboxManifest,
    test_observer: Option<&ContainerSandboxBackend>,
) -> Result<(RunnerHandoffGuard, ContainerSandboxManifest)> {
    if manifest.start_mode != ContainerStartMode::Execute {
        return Err(SandboxError::InvalidSpec {
            message: "execute lifecycle lock requires an Execute manifest".to_owned(),
        });
    }
    let handoff = lock_runner_handoff_with_deadline(
        manifest,
        Some(Instant::now() + RUNNER_HANDOFF_LOCK_TIMEOUT),
        test_observer,
    )?;
    let persisted = read_runner_manifest(&manifest.conmon_layout.manifest_path)?;
    if persisted != *manifest {
        return Err(changed_runner_manifest_error(manifest));
    }
    Ok((handoff, persisted))
}

pub(in crate::backends::container::runtime) fn lock_runner_handoff(
    manifest: &ContainerSandboxManifest,
) -> Result<RunnerHandoffGuard> {
    lock_runner_handoff_with_deadline(
        manifest,
        Some(Instant::now() + RUNNER_HANDOFF_LOCK_TIMEOUT),
        None,
    )
}

/// Establish the command-side synchronization artifact before first publish.
pub(in crate::backends::container::runtime) fn ensure_runner_handoff_lock_artifact(
    manifest: &ContainerSandboxManifest,
) -> Result<()> {
    let lock_path = manifest
        .conmon_layout
        .container_state_dir
        .join(RUNNER_HANDOFF_LOCK_FILE);
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .map(|_| ())
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to establish container lifecycle lock {} before manifest publication: {error}",
                lock_path.display()
            ),
        })
}

pub(in crate::backends::container::runtime) fn converge_runner_lifecycle_lock(
    backend: &ContainerSandboxBackend,
    manifest: &ContainerSandboxManifest,
) -> Result<RunnerHandoffGuard> {
    lock_runner_handoff_with_deadline(
        manifest,
        Some(Instant::now() + RUNNER_HANDOFF_LOCK_TIMEOUT),
        Some(backend),
    )
}

#[cfg(test)]
pub(in crate::backends::container::runtime) fn converge_runner_lifecycle_lock_with_timeout_for_test(
    backend: &ContainerSandboxBackend,
    manifest: &ContainerSandboxManifest,
    timeout: Duration,
) -> Result<RunnerHandoffGuard> {
    lock_runner_handoff_with_deadline(manifest, Some(Instant::now() + timeout), Some(backend))
}

fn lock_runner_handoff_with_deadline(
    manifest: &ContainerSandboxManifest,
    deadline: Option<Instant>,
    test_observer: Option<&ContainerSandboxBackend>,
) -> Result<RunnerHandoffGuard> {
    let lock_path = manifest
        .conmon_layout
        .container_state_dir
        .join(RUNNER_HANDOFF_LOCK_FILE);
    lock_runner_handoff_path_with_deadline(&lock_path, deadline, test_observer)
}

fn lock_runner_handoff_path_with_deadline(
    lock_path: &Path,
    deadline: Option<Instant>,
    test_observer: Option<&ContainerSandboxBackend>,
) -> Result<RunnerHandoffGuard> {
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path)
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to open container runner handoff lock {}: {error}",
                lock_path.display()
            ),
        })?;
    loop {
        match FileExt::try_lock_exclusive(&lock) {
            Ok(()) => return Ok(RunnerHandoffGuard { _lock: lock }),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                #[cfg(test)]
                if let Some(probe) = test_observer
                    .and_then(|backend| backend.runner_lifecycle_lock_test_probe.as_ref())
                {
                    probe.record_contended()?;
                }
                #[cfg(not(test))]
                let _ = test_observer;
                if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "timed out acquiring container runner handoff lock {}; execution and cancellation remain fenced",
                            lock_path.display()
                        ),
                    });
                }
                thread::sleep(RUNNER_HANDOFF_LOCK_RETRY);
            }
            Err(error) => {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "failed to acquire container runner handoff lock {}: {error}",
                        lock_path.display()
                    ),
                });
            }
        }
    }
}
