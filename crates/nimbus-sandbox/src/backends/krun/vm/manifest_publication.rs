//! Crash-safe first publication of krun launch authority.
//!
//! The no-replace manifest link is the first durable owner record. Every
//! ancestor must be durable before attachment or port reservation can follow.

use std::io::Write as _;
use std::path::Path;

use ulid::Ulid;

use super::{KrunSandboxBackend, KrunSandboxManifest};
use crate::error::{Result, SandboxError};

impl KrunSandboxBackend {
    pub(super) fn create_manifest(&self, manifest: &KrunSandboxManifest) -> Result<()> {
        self.create_manifest_with_directory_sync_inner(manifest, sync_directory)
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
            &self.config.state_root,
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
        let publish = (|| -> Result<()> {
            let mut staged = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&staged_path)
                .map_err(|error| SandboxError::OperationFailed {
                    message: format!(
                        "failed to create staged krun manifest {}: {error}",
                        staged_path.display()
                    ),
                })?;
            staged
                .write_all(&rendered)
                .and_then(|()| staged.sync_all())
                .map_err(|error| SandboxError::OperationFailed {
                    message: format!(
                        "failed to durably stage krun manifest {}: {error}",
                        staged_path.display()
                    ),
                })?;
            std::fs::hard_link(&staged_path, &manifest.conmon_layout.manifest_path).map_err(
                |error| SandboxError::OperationFailed {
                    message: if error.kind() == std::io::ErrorKind::AlreadyExists {
                        format!(
                            "durable krun launch manifest {} already exists; refusing to replace \
                             another launch owner",
                            manifest.conmon_layout.manifest_path.display()
                        )
                    } else {
                        format!(
                            "failed to publish initial krun manifest {} without replacement: \
                             {error}",
                            manifest.conmon_layout.manifest_path.display()
                        )
                    },
                },
            )?;
            directory_sync(&manifest.conmon_layout.container_state_dir).map_err(|error| {
                SandboxError::OperationFailed {
                    message: format!(
                        "failed to durably publish initial krun manifest {}: {error}",
                        manifest.conmon_layout.manifest_path.display()
                    ),
                }
            })
        })();
        let _ = std::fs::remove_file(&staged_path);
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

fn sync_directory(path: &Path) -> std::io::Result<()> {
    std::fs::File::open(path)?.sync_all()
}
