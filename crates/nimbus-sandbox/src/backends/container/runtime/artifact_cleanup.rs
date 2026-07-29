//! Container launch-artifact cleanup.
//!
//! Plan-only runner handoff has no process-local PEP registration, so its
//! materialized trust anchor is cleaned here alongside rootfs artifacts.

use super::*;
use crate::backends::oci::buildah::BuildahCli;
use crate::backends::oci::egress::remove_unactivated_egress_trust_anchor;

impl ContainerSandboxBackend {
    /// Release every launch resource after a failure proven to precede all
    /// provider effects.
    pub(super) fn release_unstarted_launch_artifacts(
        &self,
        manifest: &mut ContainerSandboxManifest,
    ) -> Result<()> {
        self.validate_manifest_execution_context(manifest)?;
        let mut errors = Vec::new();
        let mut reservation_released = false;
        if let Err(error) = self.remove_runner_manifest_pointer(manifest) {
            errors.push(error.to_string());
        }
        match manifest.launch_reservation_claim.as_ref() {
            Some(reservation_claim) => {
                match self.release_reserved_launch(manifest, reservation_claim) {
                    Ok(()) => reservation_released = true,
                    Err(error) => errors.push(error.to_string()),
                }
            }
            None => errors.push(
                "missing exact launch coordinator claim for pre-provider compensation".to_owned(),
            ),
        }
        match self.cleanup_manifest_launch_artifacts(manifest) {
            Ok(()) => manifest.launch_artifact = None,
            Err(error) => errors.push(error.to_string()),
        }
        if errors.is_empty() {
            if reservation_released {
                manifest.launch_reservation_claim = None;
            }
            manifest.network_cleanup_complete = true;
            Ok(())
        } else {
            Err(SandboxError::OperationFailed {
                message: format!(
                    "failed to release unstarted container launch artifacts for {}: {}",
                    manifest.handle.id,
                    errors.join("; ")
                ),
            })
        }
    }

    pub(super) fn release_plan_only_execution_artifacts(
        &self,
        manifest: &mut ContainerSandboxManifest,
    ) -> Result<()> {
        self.validate_manifest_execution_context(manifest)?;
        let mut errors = Vec::new();
        if let Err(error) = self.remove_runner_manifest_pointer(manifest) {
            errors.push(error.to_string());
        }
        if let Some(reservation_claim) = manifest.launch_reservation_claim.as_ref() {
            match self.release_reserved_launch(manifest, reservation_claim) {
                Ok(()) => manifest.launch_reservation_claim = None,
                Err(error) => errors.push(error.to_string()),
            }
        }
        match self.cleanup_manifest_launch_artifacts(manifest) {
            Ok(()) => manifest.launch_artifact = None,
            Err(error) => errors.push(error.to_string()),
        }
        if errors.is_empty() {
            manifest.network_cleanup_complete = true;
            Ok(())
        } else {
            Err(SandboxError::OperationFailed {
                message: format!(
                    "failed to release prepared container runner artifacts for {}: {}",
                    manifest.handle.id,
                    errors.join("; ")
                ),
            })
        }
    }

    pub(super) fn remove_runner_manifest_pointer(
        &self,
        manifest: &ContainerSandboxManifest,
    ) -> Result<()> {
        let pointer_path = manifest
            .bundle_layout
            .bundle_dir
            .join(super::runner::RUNNER_MANIFEST_POINTER_FILE);
        match std::fs::remove_file(&pointer_path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(SandboxError::OperationFailed {
                message: format!(
                    "failed to remove container runner manifest pointer {}: {error}",
                    pointer_path.display()
                ),
            }),
        }
    }

    pub(super) fn cleanup_manifest_launch_artifacts(
        &self,
        manifest: &ContainerSandboxManifest,
    ) -> Result<()> {
        self.validate_manifest_execution_context(manifest)?;
        let mut errors = Vec::new();
        // Active PEP teardown removes this anchor first. A launch that fails
        // before PEP adoption has no registry entry to own that step, so the
        // same idempotent removal must also run for execute-mode compensation.
        if let Err(error) = remove_unactivated_egress_trust_anchor(
            &manifest.runner_config.workload_state_root,
            &manifest.spec.tenant_id,
            &manifest.handle.id,
        ) {
            errors.push(error.to_string());
        }
        if let Some(artifact) = manifest.launch_artifact.as_ref() {
            let cleanup = match artifact {
                ContainerLaunchArtifact::MountedRootfs(session) => {
                    BuildahCli::new(&manifest.runner_config.buildah_path)
                        .with_unshare(manifest.runner_config.use_buildah_unshare)
                        .cleanup_rootfs_session(&session.session_name)
                }
                ContainerLaunchArtifact::Rootfs(rootfs) => {
                    if rootfs.rootfs_path.exists() {
                        std::fs::remove_dir_all(&rootfs.rootfs_path).map_err(|error| {
                            SandboxError::OperationFailed {
                                message: format!(
                                    "failed to remove materialized rootfs {}: {error}",
                                    rootfs.rootfs_path.display()
                                ),
                            }
                        })
                    } else {
                        Ok(())
                    }
                }
            };
            if let Err(error) = cleanup {
                errors.push(error.to_string());
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(SandboxError::OperationFailed {
                message: format!(
                    "failed to clean container launch artifacts for {}: {}",
                    manifest.handle.id,
                    errors.join("; ")
                ),
            })
        }
    }
}
