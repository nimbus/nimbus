//! Crash-safe first publication of krun launch authority.
//!
//! The no-replace manifest link is the first durable owner record. Every
//! ancestor must be durable before attachment or port reservation can follow.

use std::path::Path;

use nimbus_durable_record::{Commit, StagedWrite, WriteError, WriteFailure, WriteStep};
use ulid::Ulid;

use super::{KrunSandboxBackend, KrunSandboxManifest};
use crate::error::{Result, SandboxError};

impl KrunSandboxBackend {
    pub(super) fn create_manifest(&self, manifest: &KrunSandboxManifest) -> Result<()> {
        self.create_manifest_with_directory_sync_inner(
            manifest,
            nimbus_durable_record::sync_directory,
        )
    }

    #[cfg(test)]
    pub(super) fn create_manifest_with_directory_sync<F>(
        &self,
        manifest: &KrunSandboxManifest,
        directory_sync: F,
    ) -> Result<()>
    where
        F: FnMut(&Path) -> std::io::Result<()>,
    {
        self.create_manifest_with_directory_sync_inner(manifest, directory_sync)
    }

    fn create_manifest_with_directory_sync_inner<F>(
        &self,
        manifest: &KrunSandboxManifest,
        mut directory_sync: F,
    ) -> Result<()>
    where
        F: FnMut(&Path) -> std::io::Result<()>,
    {
        crate::backends::oci::durable_directory::establish_durable_directory_chain_with(
            &self.config.workload_state_root,
            &manifest.conmon_layout.container_state_dir,
            "krun manifest",
            &mut directory_sync,
        )?;
        let mut rendered =
            serde_json::to_vec_pretty(manifest).map_err(|error| SandboxError::OperationFailed {
                message: format!("failed to serialize sandbox manifest: {error}"),
            })?;
        rendered.push(b'\n');
        let staged_path = manifest.conmon_layout.container_state_dir.join(format!(
            ".nimbus-krun-manifest.{}.create",
            Ulid::new().to_string().to_ascii_lowercase()
        ));
        let publish = StagedWrite::new(&staged_path, &manifest.conmon_layout.manifest_path)
            .commit(Commit::CreateNew)
            .directory_sync(&mut directory_sync)
            .write(&rendered)
            .map_err(|error| {
                publication_error(&staged_path, &manifest.conmon_layout.manifest_path, error)
            });
        match publish {
            Ok(()) => Ok(()),
            Err(primary) => {
                let observed = self.read_manifest(&manifest.handle.id);
                match observed {
                    Ok(Some(candidate)) if candidate == *manifest => self
                        .sync_manifest_parent(manifest)
                        .map_err(|retry| SandboxError::OperationFailed {
                            message: format!(
                                "initial krun manifest publication became observable but its \
                                 durability acknowledgement remains ambiguous: {primary}; \
                                 parent-directory sync retry failed: {retry}"
                            ),
                        }),
                    Ok(_) => Err(primary),
                    Err(observe) => Err(SandboxError::OperationFailed {
                        message: format!(
                            "initial krun manifest publication failed and its authority outcome \
                             could not be inspected: {primary}; readback failed: {observe}"
                        ),
                    }),
                }
            }
        }
    }
}

fn publication_error(staged_path: &Path, manifest_path: &Path, error: WriteError) -> SandboxError {
    // The stage name carries no authority, so a failed stage cleanup leaves
    // only an inert file and does not change the outcome.
    let message = match error.failure {
        WriteFailure::Io {
            step: WriteStep::CreateStage,
            source,
            ..
        } => format!(
            "failed to create staged krun manifest {}: {source}",
            staged_path.display()
        ),
        WriteFailure::Io {
            step: WriteStep::WriteStage | WriteStep::SyncStage,
            source,
            ..
        } => format!(
            "failed to durably stage krun manifest {}: {source}",
            staged_path.display()
        ),
        WriteFailure::Io {
            step: WriteStep::Commit,
            source,
            ..
        } if source.kind() == std::io::ErrorKind::AlreadyExists => format!(
            "durable krun launch manifest {} already exists; refusing to replace another launch \
             owner",
            manifest_path.display()
        ),
        WriteFailure::Io {
            step: WriteStep::Commit,
            source,
            ..
        } => format!(
            "failed to publish initial krun manifest {} without replacement: {source}",
            manifest_path.display()
        ),
        WriteFailure::Io {
            step: WriteStep::SyncDirectory,
            source,
            ..
        } => format!(
            "failed to durably publish initial krun manifest {}: {source}",
            manifest_path.display()
        ),
        WriteFailure::Observer { error, .. } => match error {},
    };
    SandboxError::OperationFailed { message }
}
