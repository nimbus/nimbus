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
        ensure_linux_host()?;
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
        let stop_signal = configured_stop_signal(&manifest.image_metadata);
        signal_process(&stop_signal, pid)?;
        let stop_timeout = configured_stop_timeout(&manifest.spec, &self.config);
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
        if manifest.conmon_layout.exit_status_file.exists() {
            let exit_code = read_exit_code(&manifest.conmon_layout.exit_status_file)?;
            if manifest.shutdown_requested || exit_code == 0 {
                return Ok(SandboxStatus::Stopped);
            }
            return Ok(SandboxStatus::Failed);
        }

        let runtime_state = runtime_state(&manifest.conmon_launch.state_command)?;
        match runtime_state.as_deref() {
            Some("running") => Ok(running_status(manifest)),
            Some("created") | Some("creating") => Ok(SandboxStatus::Starting),
            Some("stopped") => Ok(SandboxStatus::Stopped),
            Some("paused") => Ok(SandboxStatus::Stopping),
            Some(_) => Ok(SandboxStatus::Failed),
            None if manifest.conmon_layout.pidfile.exists() => {
                if pid_is_alive(read_pid(&manifest.conmon_layout.pidfile)?) {
                    Ok(SandboxStatus::Starting)
                } else if manifest.shutdown_requested {
                    Ok(SandboxStatus::Stopped)
                } else {
                    Ok(SandboxStatus::Failed)
                }
            }
            None => Ok(manifest.status),
        }
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
        ensure_linux_host()?;
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
        let manifest_path = self
            .config
            .state_root
            .join("containers")
            .join(id.as_str())
            .join("manifest.json");
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

fn ensure_linux_host() -> Result<()> {
    if cfg!(target_os = "linux") {
        return Ok(());
    }

    Err(SandboxError::BackendUnavailable {
        message:
            "krun execution requires a Linux host; use plan-only mode for cross-platform tests"
                .to_owned(),
    })
}

pub(super) fn configured_stop_signal(image_metadata: &KrunImageMetadata) -> String {
    image_metadata
        .stop_signal
        .as_deref()
        .map(str::trim)
        .filter(|signal| !signal.is_empty())
        .unwrap_or("TERM")
        .to_owned()
}

pub(super) fn configured_stop_timeout(
    spec: &SandboxSpec,
    config: &KrunSandboxBackendConfig,
) -> Duration {
    spec.lifecycle.stop_timeout.unwrap_or(config.stop_timeout)
}

pub(super) fn restart_policy_allows_restart(
    policy: SandboxRestartPolicy,
    exit_code: i32,
    restart_count: u32,
) -> bool {
    match policy {
        SandboxRestartPolicy::Never => false,
        SandboxRestartPolicy::OnFailure { max_restarts } => {
            exit_code != 0 && restart_count < max_restarts
        }
        SandboxRestartPolicy::Always { max_restarts } => restart_count < max_restarts,
    }
}

pub(super) fn restart_backoff_delay(restart_count: u32) -> Duration {
    let initial = u128::from(DEFAULT_RESTART_BACKOFF_INITIAL_MILLIS);
    let max = u128::from(DEFAULT_RESTART_BACKOFF_MAX_MILLIS);
    let multiplier = 1_u128 << restart_count.min(31);
    let millis = initial.saturating_mul(multiplier).min(max);
    Duration::from_millis(millis as u64)
}

fn now_millis() -> Result<u64> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| SandboxError::OperationFailed {
            message: format!("system clock is before unix epoch: {error}"),
        })?;
    u64::try_from(elapsed.as_millis()).map_err(|_| SandboxError::OperationFailed {
        message: "system clock milliseconds exceed supported range".to_owned(),
    })
}

fn remove_if_exists(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }

    std::fs::remove_file(path).map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "failed to remove stale runtime artifact {}: {error}",
            path.display()
        ),
    })
}

fn spawn_background(command: &CommandSpec) -> Result<()> {
    command
        .as_command()
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to spawn sandbox lifecycle command {}: {error}",
                command.program.display()
            ),
        })?;
    Ok(())
}

fn run_status_checked(command: &CommandSpec) -> Result<()> {
    let output = command
        .as_command()
        .output()
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to run sandbox command {}: {error}",
                command.program.display()
            ),
        })?;
    if output.status.success() {
        return Ok(());
    }

    Err(SandboxError::OperationFailed {
        message: format!(
            "sandbox command {} failed: {}",
            command.program.display(),
            render_command_failure(&output.stderr)
        ),
    })
}

fn runtime_state(command: &CommandSpec) -> Result<Option<String>> {
    let output = command
        .as_command()
        .output()
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to run runtime state command {}: {error}",
                command.program.display()
            ),
        })?;
    if !output.status.success() {
        return Ok(None);
    }

    let payload: RuntimeStatePayload =
        serde_json::from_slice(&output.stdout).map_err(|error| SandboxError::OperationFailed {
            message: format!("failed to parse runtime state JSON: {error}"),
        })?;
    Ok(Some(payload.status))
}

fn wait_for_runtime_state(command: &CommandSpec, timeout: Duration) -> Result<String> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(status) = runtime_state(command)?
            && (status == "created" || status == "running")
        {
            return Ok(status);
        }
        thread::sleep(Duration::from_millis(200));
    }

    Err(SandboxError::OperationFailed {
        message: format!(
            "sandbox runtime did not reach created state before timeout via {}",
            command.program.display()
        ),
    })
}

fn signal_process(signal: &str, pid: u32) -> Result<()> {
    let status = std::process::Command::new("kill")
        .arg(format!("-{signal}"))
        .arg(pid.to_string())
        .status()
        .map_err(|error| SandboxError::OperationFailed {
            message: format!("failed to signal sandbox process {pid} with {signal}: {error}"),
        })?;
    if status.success() {
        return Ok(());
    }

    Err(SandboxError::OperationFailed {
        message: format!("kill -{signal} {pid} returned non-zero status {status}"),
    })
}

fn read_pid(path: &Path) -> Result<u32> {
    let pid = std::fs::read_to_string(path).map_err(|error| SandboxError::OperationFailed {
        message: format!("failed to read sandbox pidfile {}: {error}", path.display()),
    })?;
    pid.trim()
        .parse::<u32>()
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to parse sandbox pid from {}: {error}",
                path.display()
            ),
        })
}

fn wait_for_path(path: &Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if path.exists() {
            return true;
        }
        thread::sleep(Duration::from_millis(200));
    }
    path.exists()
}

fn read_exit_code(path: &Path) -> Result<i32> {
    let exit_status =
        std::fs::read_to_string(path).map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to read sandbox exit status {}: {error}",
                path.display()
            ),
        })?;
    let exit_code =
        exit_status
            .trim()
            .parse::<i32>()
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to parse sandbox exit status {}: {error}",
                    path.display()
                ),
            })?;
    Ok(exit_code)
}

fn render_command_failure(stderr: &[u8]) -> String {
    let rendered = String::from_utf8_lossy(stderr).trim().to_owned();
    if rendered.is_empty() {
        "stderr was empty".to_owned()
    } else {
        rendered
    }
}
