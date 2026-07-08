//! Prepared plan-only workload runner entrypoint.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::backends::conmon::lifecycle::read_exit_code;
use crate::backends::poll::poll_until_deadline;
use crate::error::{Result, SandboxError};

use super::ContainerSandboxBackend;
use super::config::ContainerStartMode;
use super::manifest::ContainerSandboxManifest;

pub(super) const RUNNER_MANIFEST_POINTER_FILE: &str = ".nimbus-container-manifest";

pub fn run_prepared_container_service_workload(bundle_dir: impl AsRef<Path>) -> Result<()> {
    let bundle_dir = bundle_dir.as_ref();
    let manifest_path = read_runner_manifest_pointer(bundle_dir)?;
    let mut manifest = read_runner_manifest(&manifest_path)?;
    if manifest.start_mode != ContainerStartMode::PlanOnly {
        return Err(SandboxError::InvalidSpec {
            message: format!(
                "container runner expected a prepared plan-only workload manifest, got {:?}",
                manifest.start_mode
            ),
        });
    }
    if manifest.bundle_layout.bundle_dir != bundle_dir {
        return Err(SandboxError::InvalidSpec {
            message: format!(
                "container runner bundle {} does not match prepared manifest bundle {}",
                bundle_dir.display(),
                manifest.bundle_layout.bundle_dir.display()
            ),
        });
    }
    let backend = ContainerSandboxBackend::new(manifest.runner_config.to_backend_config());
    backend.launch_manifest(&mut manifest, true)?;
    let exit_code = wait_for_container_runner_exit(&manifest)?;
    if exit_code != 0 {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "container workload {} exited with status {exit_code}",
                manifest.handle.id
            ),
        });
    }
    Ok(())
}

fn wait_for_container_runner_exit(manifest: &ContainerSandboxManifest) -> Result<i32> {
    poll_until_deadline(None, Duration::from_millis(200), || {
        Ok(manifest
            .conmon_layout
            .exit_status_file
            .exists()
            .then_some(()))
    })?;
    read_exit_code(&manifest.conmon_layout.exit_status_file)
}

fn read_runner_manifest_pointer(bundle_dir: &Path) -> Result<PathBuf> {
    let pointer_path = bundle_dir.join(RUNNER_MANIFEST_POINTER_FILE);
    let contents =
        std::fs::read_to_string(&pointer_path).map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to read container runner manifest pointer {}: {error}",
                pointer_path.display()
            ),
        })?;
    let path = contents.trim();
    if path.is_empty() {
        return Err(SandboxError::InvalidSpec {
            message: format!(
                "container runner manifest pointer {} is empty",
                pointer_path.display()
            ),
        });
    }
    Ok(PathBuf::from(path))
}

fn read_runner_manifest(manifest_path: &Path) -> Result<ContainerSandboxManifest> {
    let contents = std::fs::read(manifest_path).map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "failed to read container runner manifest {}: {error}",
            manifest_path.display()
        ),
    })?;
    serde_json::from_slice(&contents).map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "failed to parse container runner manifest {}: {error}",
            manifest_path.display()
        ),
    })
}
