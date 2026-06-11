use super::launch::ensure_guest_user_helper_available;
use super::readiness::{running_status, synchronize_handle_status, visible_published_endpoints};
use super::*;

impl KrunSandboxBackend {
    pub(super) fn inspect_sync(&self, id: &SandboxId) -> Result<Option<SandboxHandle>> {
        let Some(mut manifest) = self.read_manifest(id)? else {
            return Ok(None);
        };

        manifest.status = match self.config.launch_mode {
            KrunLaunchMode::PlanOnly => manifest.status,
            KrunLaunchMode::Execute => {
                if self.maybe_restart_after_exit(&mut manifest)? {
                    manifest.status
                } else {
                    self.detect_runtime_status(&manifest)?
                }
            }
        };
        manifest.handle.status = manifest.status;
        manifest.handle.published_endpoints =
            visible_published_endpoints(manifest.launch_mode, &manifest.spec, manifest.status);
        self.write_manifest(&manifest)?;
        Ok(Some(manifest.handle))
    }

    pub(super) fn stop_sync(&self, id: &SandboxId) -> Result<()> {
        let Some(mut manifest) = self.read_manifest(id)? else {
            return Err(SandboxError::NotFound {
                sandbox_id: id.as_str().to_owned(),
            });
        };

        match self.config.launch_mode {
            KrunLaunchMode::PlanOnly => {
                manifest.shutdown_requested = true;
                manifest.last_exit_code = Some(0);
                manifest.status = SandboxStatus::Stopped;
                manifest.handle.status = SandboxStatus::Stopped;
                self.cleanup_manifest_launch_artifacts(&manifest)?;
                manifest.launch_artifact = None;
                self.write_manifest(&manifest)
            }
            KrunLaunchMode::Execute => self.execute_stop(&mut manifest),
        }
    }

    pub(super) fn execute_start(&self, launch_plan: &KrunLaunchPlan) -> Result<SandboxHandle> {
        ensure_linux_host("krun")?;
        let mut manifest = launch_plan.manifest.clone();
        self.launch_manifest(&mut manifest, true)?;
        Ok(manifest.handle)
    }

    fn execute_stop(&self, manifest: &mut KrunSandboxManifest) -> Result<()> {
        if manifest.conmon_layout.exit_status_file.exists() {
            manifest.shutdown_requested = true;
            manifest.next_restart_at_millis = None;
            manifest.last_exit_code =
                Some(read_exit_code(&manifest.conmon_layout.exit_status_file)?);
            synchronize_handle_status(manifest, SandboxStatus::Stopped);
            return self.write_manifest(manifest);
        }

        manifest.shutdown_requested = true;
        manifest.next_restart_at_millis = None;
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
        synchronize_handle_status(manifest, SandboxStatus::Stopped);
        self.cleanup_manifest_launch_artifacts(manifest)?;
        manifest.launch_artifact = None;
        self.write_manifest(manifest)
    }

    pub(super) fn detect_runtime_status(
        &self,
        manifest: &KrunSandboxManifest,
    ) -> Result<SandboxStatus> {
        detect_conmon_runtime_status(
            RuntimeStatusProbe {
                exit_status_file: &manifest.conmon_layout.exit_status_file,
                state_command: &manifest.conmon_launch.state_command,
                pidfile: &manifest.conmon_layout.pidfile,
                shutdown_requested: manifest.shutdown_requested,
                current_status: manifest.status,
            },
            || Ok(running_status(manifest)),
        )
    }

    fn maybe_restart_after_exit(&self, manifest: &mut KrunSandboxManifest) -> Result<bool> {
        if manifest.shutdown_requested || !manifest.conmon_layout.exit_status_file.exists() {
            return Ok(false);
        }

        let exit_code = read_exit_code(&manifest.conmon_layout.exit_status_file)?;
        if !restart_policy_allows_restart(
            manifest.spec.lifecycle.restart_policy,
            exit_code,
            manifest.restart_count,
        ) {
            return Ok(false);
        }

        manifest.last_exit_code = Some(exit_code);
        let now_millis = now_millis()?;
        let next_restart_at_millis = manifest.next_restart_at_millis.get_or_insert_with(|| {
            now_millis.saturating_add(restart_backoff_delay(manifest.restart_count).as_millis() as u64)
        });
        if now_millis < *next_restart_at_millis {
            synchronize_handle_status(manifest, SandboxStatus::Starting);
            return Ok(true);
        }

        manifest.restart_count += 1;
        manifest.next_restart_at_millis = None;
        self.reset_runtime_for_restart(manifest)?;
        self.launch_manifest(manifest, false)?;
        Ok(true)
    }

    fn launch_manifest(
        &self,
        manifest: &mut KrunSandboxManifest,
        clear_last_exit_code: bool,
    ) -> Result<()> {
        ensure_linux_host("krun")?;
        ensure_guest_user_helper_available(&self.config, manifest)?;
        spawn_background(&manifest.conmon_launch.create_command)?;
        let runtime_state = wait_for_runtime_state(
            &manifest.conmon_launch.state_command,
            self.config.start_timeout,
        )?;
        if runtime_state != "running" {
            run_status_checked(&manifest.conmon_launch.start_command)?;
        }

        manifest.shutdown_requested = false;
        manifest.next_restart_at_millis = None;
        if clear_last_exit_code {
            manifest.last_exit_code = None;
        }
        synchronize_handle_status(manifest, SandboxStatus::Starting);
        self.write_manifest(manifest)
    }

    fn reset_runtime_for_restart(&self, manifest: &KrunSandboxManifest) -> Result<()> {
        run_status_checked(&manifest.conmon_launch.delete_command)?;
        remove_if_exists(&manifest.conmon_layout.exit_status_file)?;
        remove_if_exists(&manifest.conmon_layout.pidfile)?;
        remove_if_exists(&manifest.conmon_layout.conmon_pidfile)?;
        Ok(())
    }

    pub(super) fn read_manifest(&self, id: &SandboxId) -> Result<Option<KrunSandboxManifest>> {
        let Some(manifest_path) =
            crate::artifact_paths::manifest_path_for_sandbox_id(&self.config.state_root, id)
                .map_err(|error| SandboxError::OperationFailed {
                    message: format!(
                        "failed to find krun sandbox manifest for {} under {}: {error}",
                        id,
                        self.config.state_root.display()
                    ),
                })?
        else {
            return Ok(None);
        };
        if !manifest_path.exists() {
            return Ok(None);
        }

        let contents =
            std::fs::read(&manifest_path).map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to read sandbox manifest {}: {error}",
                    manifest_path.display()
                ),
            })?;
        let manifest =
            serde_json::from_slice(&contents).map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to parse sandbox manifest {}: {error}",
                    manifest_path.display()
                ),
            })?;
        Ok(Some(manifest))
    }

    pub(super) fn write_manifest(&self, manifest: &KrunSandboxManifest) -> Result<()> {
        std::fs::create_dir_all(&manifest.conmon_layout.container_state_dir).map_err(|error| {
            SandboxError::OperationFailed {
                message: format!(
                    "failed to create manifest directory {}: {error}",
                    manifest.conmon_layout.container_state_dir.display()
                ),
            }
        })?;
        let rendered =
            serde_json::to_vec_pretty(manifest).map_err(|error| SandboxError::OperationFailed {
                message: format!("failed to serialize sandbox manifest: {error}"),
            })?;
        std::fs::write(&manifest.conmon_layout.manifest_path, rendered).map_err(|error| {
            SandboxError::OperationFailed {
                message: format!(
                    "failed to write sandbox manifest {}: {error}",
                    manifest.conmon_layout.manifest_path.display()
                ),
            }
        })
    }
}
