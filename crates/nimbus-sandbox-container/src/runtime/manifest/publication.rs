//! Crash-safe publication for one container manifest.
//!
//! `manifest.json` is the sole commit point. Stage bytes are never promoted by
//! reconciliation: an interrupted writer is either followed by the previous
//! canonical manifest or by a later complete publication.

use std::fs;
use std::path::Path;
use std::time::Duration;

use nimbus_durable_record::{
    EntryError, RecordLock, StagedWrite, remove_stale_stage, sync_directory,
};

use nimbus_sandbox::{Result, SandboxError};

pub(in crate::runtime) const MANIFEST_PUBLICATION_LOCK_FILE: &str =
    ".nimbus-container-manifest.lock";
pub(in crate::runtime) const MANIFEST_PUBLICATION_STAGE_FILE: &str =
    ".nimbus-container-manifest.stage";
#[cfg(not(test))]
const MANIFEST_PUBLICATION_LOCK_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(test)]
const MANIFEST_PUBLICATION_LOCK_TIMEOUT: Duration = Duration::from_millis(100);

pub(in crate::runtime) fn reconcile_startup_manifest_publications(state_root: &Path) -> Result<()> {
    let container_state_dirs = nimbus_sandbox::artifact_paths::all_container_state_dirs(state_root)
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to enumerate container manifest publication state under {}: {error}",
                state_root.display()
            ),
        })?;
    let mut failures = Vec::new();
    for container_state_dir in container_state_dirs {
        // A directory without a stage needs no lock, so startup creates none.
        let stage = container_state_dir.join(MANIFEST_PUBLICATION_STAGE_FILE);
        let reconciliation = match fs::symlink_metadata(&stage) {
            Ok(_) => reconcile_stage(&container_state_dir).map(drop),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(entry_error(EntryError::Io {
                operation: "inspect stage",
                path: stage,
                source: error,
            })),
        };
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

pub(in crate::runtime) fn publish_with_directory_sync<F>(
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
    let _guard = reconcile_stage(container_state_dir)?;
    StagedWrite::new(
        &container_state_dir.join(MANIFEST_PUBLICATION_STAGE_FILE),
        manifest_path,
    )
    .directory_sync(&mut directory_sync)
    .write(rendered)
    .map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "failed to publish sandbox manifest {}: {error}",
            manifest_path.display()
        ),
    })
}

pub(in crate::runtime) fn establish_durable_manifest_directory_chain_with<F>(
    state_root: &Path,
    container_state_dir: &Path,
    directory_sync: F,
) -> Result<()>
where
    F: FnMut(&Path) -> std::io::Result<()>,
{
    nimbus_sandbox::durable_directory::establish_durable_directory_chain_with(
        state_root,
        container_state_dir,
        "container manifest",
        directory_sync,
    )
}

/// Take the publication lock and discard an abandoned stage without promoting it.
fn reconcile_stage(container_state_dir: &Path) -> Result<RecordLock> {
    let guard = RecordLock::acquire(
        &container_state_dir.join(MANIFEST_PUBLICATION_LOCK_FILE),
        MANIFEST_PUBLICATION_LOCK_TIMEOUT,
    )
    .map_err(entry_error)?;
    remove_stale_stage(&container_state_dir.join(MANIFEST_PUBLICATION_STAGE_FILE))
        .map_err(entry_error)?;
    Ok(guard)
}

fn entry_error(error: EntryError) -> SandboxError {
    let message = match &error {
        EntryError::LockTimeout { path, .. } => format!(
            "timed out acquiring container manifest publication lock {}; canonical manifest \
             state remains unchanged",
            path.display()
        ),
        EntryError::NotRegular { path } => format!(
            "container manifest publication entry {} is not a regular file; publication remains \
             fenced",
            path.display()
        ),
        EntryError::Io { .. } => format!("container manifest publication failed: {error}"),
    };
    SandboxError::OperationFailed { message }
}
