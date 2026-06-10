use std::path::Path;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Deserialize;

use crate::backends::oci::command::{CommandSpec, render_command_failure};
use crate::error::{Result, SandboxError};
use crate::instance::SandboxStatus;
use crate::process::pid_is_alive;
use crate::spec::{SandboxRestartPolicy, SandboxSpec};

const DEFAULT_RESTART_BACKOFF_INITIAL_MILLIS: u64 = 1_000;
const DEFAULT_RESTART_BACKOFF_MAX_MILLIS: u64 = 60_000;

pub(crate) fn restart_policy_allows_restart(
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

pub(crate) fn restart_backoff_delay(restart_count: u32) -> Duration {
    let initial = u128::from(DEFAULT_RESTART_BACKOFF_INITIAL_MILLIS);
    let max = u128::from(DEFAULT_RESTART_BACKOFF_MAX_MILLIS);
    let multiplier = 1_u128 << restart_count.min(31);
    let millis = initial.saturating_mul(multiplier).min(max);
    Duration::from_millis(millis as u64)
}

pub(crate) fn now_millis() -> Result<u64> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| SandboxError::OperationFailed {
            message: format!("system clock is before unix epoch: {error}"),
        })?;
    u64::try_from(elapsed.as_millis()).map_err(|_| SandboxError::OperationFailed {
        message: "system clock milliseconds exceed supported range".to_owned(),
    })
}

pub(crate) fn ensure_linux_host(backend_name: &str) -> Result<()> {
    if cfg!(target_os = "linux") {
        return Ok(());
    }

    Err(SandboxError::BackendUnavailable {
        message: format!(
            "{backend_name} execution requires a Linux host; use plan-only mode for cross-platform tests"
        ),
    })
}

pub(crate) fn configured_stop_signal(stop_signal: Option<&str>) -> String {
    stop_signal
        .map(str::trim)
        .filter(|signal| !signal.is_empty())
        .unwrap_or("TERM")
        .to_owned()
}

pub(crate) fn configured_stop_timeout(spec: &SandboxSpec, fallback: Duration) -> Duration {
    spec.lifecycle.stop_timeout.unwrap_or(fallback)
}

pub(crate) struct RuntimeStatusProbe<'a> {
    pub(crate) exit_status_file: &'a Path,
    pub(crate) state_command: &'a CommandSpec,
    pub(crate) pidfile: &'a Path,
    pub(crate) shutdown_requested: bool,
    pub(crate) current_status: SandboxStatus,
}

pub(crate) fn detect_runtime_status(
    probe: RuntimeStatusProbe<'_>,
    running_status: impl FnOnce() -> Result<SandboxStatus>,
) -> Result<SandboxStatus> {
    if probe.exit_status_file.exists() {
        let exit_code = read_exit_code(probe.exit_status_file)?;
        if probe.shutdown_requested || exit_code == 0 {
            return Ok(SandboxStatus::Stopped);
        }
        return Ok(SandboxStatus::Failed);
    }

    let runtime_state = runtime_state(probe.state_command)?;
    match runtime_state.as_deref() {
        Some("running") => running_status(),
        Some("created") | Some("creating") => Ok(SandboxStatus::Starting),
        Some("stopped") => Ok(SandboxStatus::Stopped),
        Some("paused") => Ok(SandboxStatus::Stopping),
        Some(_) => Ok(SandboxStatus::Failed),
        None if probe.pidfile.exists() => {
            if pid_is_alive(read_pid(probe.pidfile)?) {
                Ok(SandboxStatus::Starting)
            } else if probe.shutdown_requested {
                Ok(SandboxStatus::Stopped)
            } else {
                Ok(SandboxStatus::Failed)
            }
        }
        None => Ok(probe.current_status),
    }
}

pub(crate) fn spawn_background(command: &CommandSpec) -> Result<()> {
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

pub(crate) fn run_status_checked(command: &CommandSpec) -> Result<()> {
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
            render_command_failure(&[], &output.stderr)
        ),
    })
}

pub(crate) fn run_status_best_effort(command: &CommandSpec) -> Result<()> {
    let _ = command
        .as_command()
        .output()
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to run sandbox cleanup command {}: {error}",
                command.program.display()
            ),
        })?;
    Ok(())
}

pub(crate) fn runtime_state(command: &CommandSpec) -> Result<Option<String>> {
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

pub(crate) fn wait_for_runtime_state(command: &CommandSpec, timeout: Duration) -> Result<String> {
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

pub(crate) fn signal_process(signal: &str, pid: u32) -> Result<()> {
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

pub(crate) fn read_pid(path: &Path) -> Result<u32> {
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

pub(crate) fn wait_for_path(path: &Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if path.exists() {
            return true;
        }
        thread::sleep(Duration::from_millis(200));
    }
    path.exists()
}

pub(crate) fn read_exit_code(path: &Path) -> Result<i32> {
    let exit_status =
        std::fs::read_to_string(path).map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to read sandbox exit status {}: {error}",
                path.display()
            ),
        })?;
    exit_status
        .trim()
        .parse::<i32>()
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to parse sandbox exit status {}: {error}",
                path.display()
            ),
        })
}

pub(crate) fn remove_if_exists(path: &Path) -> Result<()> {
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

#[derive(Debug, Deserialize)]
struct RuntimeStatePayload {
    status: String,
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use nimbus_core::TenantId;

    use super::*;
    use crate::backend::SandboxBackendKind;
    use crate::spec::{SandboxOwnerSpec, SandboxProcessSpec, SandboxRootSpec, SandboxSpec};

    #[test]
    fn configured_stop_signal_trims_and_defaults() {
        assert_eq!(configured_stop_signal(Some(" SIGQUIT ")), "SIGQUIT");
        assert_eq!(configured_stop_signal(Some("   ")), "TERM");
        assert_eq!(configured_stop_signal(None), "TERM");
    }

    #[test]
    fn configured_stop_timeout_prefers_spec_override() {
        let fallback = Duration::from_secs(5);

        assert_eq!(configured_stop_timeout(&sample_spec(), fallback), fallback);
        assert_eq!(
            configured_stop_timeout(
                &sample_spec().with_stop_timeout(Duration::from_secs(30)),
                fallback,
            ),
            Duration::from_secs(30)
        );
    }

    fn sample_spec() -> SandboxSpec {
        SandboxSpec::new(
            TenantId::new("tenant").expect("tenant id should parse"),
            SandboxOwnerSpec::standalone_named("db"),
            SandboxBackendKind::Container,
            SandboxRootSpec::rootfs("/rootfs"),
            SandboxProcessSpec::new(["/bin/server"]),
        )
    }
}
