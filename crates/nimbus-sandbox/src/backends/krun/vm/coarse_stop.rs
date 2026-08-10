//! Legacy coarse Krun stop retained until the NNC6.5g caller cutover.
//!
//! This owner remains intentionally separate from exact execution teardown:
//! coarse stop releases network and launch authority, while execution stop
//! must retain both for the later workload-saga release phase.

use super::lifecycle::NetworkArtifactTeardownMode;
use super::readiness::synchronize_handle_status;
use super::*;

impl KrunSandboxBackend {
    pub(super) fn stop_sync(&self, id: &SandboxId) -> Result<()> {
        let Some(observed) = self.read_manifest(id)? else {
            return Err(SandboxError::NotFound {
                sandbox_id: id.as_str().to_owned(),
            });
        };
        let _lifecycle = self.lock_launch_lifecycle(&observed)?;
        let Some(mut manifest) = self.read_manifest(id)? else {
            return Err(SandboxError::NotFound {
                sandbox_id: id.as_str().to_owned(),
            });
        };
        manifest.require_execution_admission_open("coarse Krun stop")?;

        match self.config.start_mode {
            KrunStartMode::PlanOnly => {
                manifest.shutdown_requested = true;
                manifest.last_exit_code = Some(0);
                manifest.status = SandboxStatus::Stopped;
                manifest.handle.status = SandboxStatus::Stopped;
                self.cleanup_manifest_launch_artifacts(&manifest)?;
                manifest.launch_artifact = None;
                self.write_manifest(&manifest)
            }
            KrunStartMode::Execute => {
                self.reconcile_pending_creator_before_cleanup(&mut manifest)?;
                if manifest.provider_failure_cleanup.is_active() {
                    return self.resume_provider_failure_cleanup(&mut manifest);
                }
                match &manifest.launch_authority {
                    KrunLaunchAuthority::Reserved { .. } => {
                        self.stop_reserved_launch(&mut manifest)
                    }
                    KrunLaunchAuthority::Adopting { .. } => {
                        self.stop_adopting_launch(&mut manifest)
                    }
                    KrunLaunchAuthority::Adopted { .. }
                        if manifest.creator_handoff == KrunCreatorHandoffState::NotSpawned =>
                    {
                        match manifest.conmon_layout.exit_status_file.try_exists() {
                            Ok(true) => self.execute_stop(&mut manifest),
                            Ok(false) => self.stop_adopted_never_spawned(&mut manifest),
                            Err(error) => Err(SandboxError::OperationFailed {
                                message: format!(
                                    "failed to inspect krun exit receipt {} before explicit stop: \
                                     {error}",
                                    manifest.conmon_layout.exit_status_file.display()
                                ),
                            }),
                        }
                    }
                    KrunLaunchAuthority::Adopted { .. } | KrunLaunchAuthority::ProviderOwned => {
                        self.execute_stop(&mut manifest)
                    }
                    KrunLaunchAuthority::Released => {
                        manifest.shutdown_requested = true;
                        let terminal_status = if manifest.status == SandboxStatus::Failed {
                            SandboxStatus::Failed
                        } else {
                            SandboxStatus::Stopped
                        };
                        synchronize_handle_status(&mut manifest, terminal_status);
                        self.persist_effect_barrier(&manifest, "released krun stop completion")
                    }
                    KrunLaunchAuthority::PlanOnly => Err(SandboxError::OperationFailed {
                        message: format!(
                            "execute-mode krun workload {} carries plan-only launch authority",
                            manifest.handle.id
                        ),
                    }),
                }
            }
        }
    }

    pub(super) fn stop_reserved_launch(&self, manifest: &mut KrunSandboxManifest) -> Result<()> {
        manifest.shutdown_requested = true;
        synchronize_handle_status(manifest, SandboxStatus::Stopping);
        self.persist_effect_barrier(manifest, "reserved krun stop intent")?;
        self.release_reserved_launch(manifest)?;
        self.cleanup_manifest_launch_artifacts(manifest)?;
        manifest.launch_artifact = None;
        manifest.launch_authority = KrunLaunchAuthority::Released;
        synchronize_handle_status(manifest, SandboxStatus::Stopped);
        self.persist_effect_barrier(manifest, "reserved krun stop completion")
    }

    fn stop_adopted_never_spawned(&self, manifest: &mut KrunSandboxManifest) -> Result<()> {
        manifest.shutdown_requested = true;
        synchronize_handle_status(manifest, SandboxStatus::Stopping);
        manifest.provider_failure_cleanup = KrunProviderFailureCleanupState::Requested;
        self.persist_effect_barrier(manifest, "adopted never-spawned krun stop intent")?;
        self.resume_provider_failure_cleanup(manifest)
    }

    fn execute_stop(&self, manifest: &mut KrunSandboxManifest) -> Result<()> {
        manifest.shutdown_requested = true;
        synchronize_handle_status(manifest, SandboxStatus::Stopping);
        self.persist_effect_barrier(manifest, "explicit krun stop intent")?;

        if manifest.conmon_layout.exit_status_file.exists() {
            manifest.last_exit_code =
                Some(read_exit_code(&manifest.conmon_layout.exit_status_file)?);
            self.release_network_artifacts(manifest, NetworkArtifactTeardownMode::Final)?;
            self.cleanup_manifest_launch_artifacts(manifest)?;
            manifest.launch_artifact = None;
            manifest.launch_authority = KrunLaunchAuthority::Released;
            synchronize_handle_status(manifest, SandboxStatus::Stopped);
            return self.persist_effect_barrier(manifest, "explicit krun stop completion");
        }

        let pid = read_pid(&manifest.conmon_layout.pidfile)?;
        let stop_signal = configured_stop_signal(manifest.image_metadata.stop_signal.as_deref());
        signal_process(&stop_signal, pid)?;
        let stop_timeout = configured_stop_timeout(&manifest.spec, self.config.stop_timeout);
        if !wait_for_path(&manifest.conmon_layout.exit_status_file, stop_timeout) {
            signal_process("KILL", pid)?;
            if !wait_for_path(&manifest.conmon_layout.exit_status_file, stop_timeout) {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "sandbox {} did not write an exit file after TERM/KILL",
                        manifest.handle.id
                    ),
                });
            }
        }

        manifest.last_exit_code = Some(read_exit_code(&manifest.conmon_layout.exit_status_file)?);
        self.release_network_artifacts(manifest, NetworkArtifactTeardownMode::Final)?;
        self.cleanup_manifest_launch_artifacts(manifest)?;
        manifest.launch_artifact = None;
        manifest.launch_authority = KrunLaunchAuthority::Released;
        synchronize_handle_status(manifest, SandboxStatus::Stopped);
        self.persist_effect_barrier(manifest, "explicit krun stop completion")
    }

    #[cfg(test)]
    pub(super) fn execute_stop_for_test(&self, manifest: &mut KrunSandboxManifest) -> Result<()> {
        self.execute_stop(manifest)
    }
}
