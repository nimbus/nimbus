use std::collections::BTreeMap;
use std::path::Component;
use std::path::Path;
use std::time::{Duration, Instant};
#[cfg(test)]
use std::{
    sync::{Arc, Condvar, Mutex},
    time::Duration as TestDuration,
};

use serde::Deserialize;

use crate::backends::oci::command::{CommandSpec, render_command_failure};
use crate::backends::poll::poll_until_deadline;
use crate::error::{Result, SandboxError};
use crate::instance::SandboxStatus;
use crate::process::pid_is_alive;
use crate::spec::{SandboxRestartPolicy, SandboxSpec};

const DEFAULT_RESTART_BACKOFF_INITIAL_MILLIS: u64 = 1_000;
const DEFAULT_RESTART_BACKOFF_MAX_MILLIS: u64 = 60_000;
pub(crate) const CREATOR_ATTEMPT_ANNOTATION: &str = "com.nimbus.creator-attempt";

/// Test-only semantic barrier at the provider launch entry. It lets a
/// concurrent test persist withdrawal after inspection has chosen restart but
/// before the provider effect is observed, without sleeps or a live OCI
/// runtime. Production builds contain neither this type nor the hook fields
/// that consume it.
#[cfg(test)]
#[derive(Clone)]
pub(crate) struct RestartLaunchTestProbe {
    shared: Arc<(Mutex<RestartLaunchTestState>, Condvar)>,
    timeout: TestDuration,
}

#[cfg(test)]
#[derive(Default)]
struct RestartLaunchTestState {
    entered: bool,
    released: bool,
    effects: usize,
}

#[cfg(test)]
impl RestartLaunchTestProbe {
    pub(crate) fn new(timeout: TestDuration) -> Self {
        Self {
            shared: Arc::new((
                Mutex::new(RestartLaunchTestState::default()),
                Condvar::new(),
            )),
            timeout,
        }
    }

    pub(crate) fn intercept_provider_launch(&self) -> Result<()> {
        let (lock, changed) = &*self.shared;
        let mut state = lock.lock().map_err(|_| SandboxError::OperationFailed {
            message: "restart launch test probe lock was poisoned".to_owned(),
        })?;
        state.entered = true;
        changed.notify_all();
        let (mut state, wait) = changed
            .wait_timeout_while(state, self.timeout, |state| !state.released)
            .map_err(|_| SandboxError::OperationFailed {
                message: "restart launch test probe wait was poisoned".to_owned(),
            })?;
        if wait.timed_out() && !state.released {
            return Err(SandboxError::OperationFailed {
                message: "restart launch test probe timed out awaiting release".to_owned(),
            });
        }
        state.effects += 1;
        changed.notify_all();
        Ok(())
    }

    pub(crate) fn wait_until_entered(&self) -> bool {
        let (lock, changed) = &*self.shared;
        let state = lock
            .lock()
            .expect("restart launch test probe lock should not be poisoned");
        let (state, _) = changed
            .wait_timeout_while(state, self.timeout, |state| !state.entered)
            .expect("restart launch test probe wait should not be poisoned");
        state.entered
    }

    pub(crate) fn release(&self) {
        let (lock, changed) = &*self.shared;
        let mut state = lock
            .lock()
            .expect("restart launch test probe lock should not be poisoned");
        state.released = true;
        changed.notify_all();
    }

    pub(crate) fn effect_count(&self) -> usize {
        self.shared
            .0
            .lock()
            .expect("restart launch test probe lock should not be poisoned")
            .effects
    }
}

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
    pub(crate) runtime_id: &'a str,
    pub(crate) pidfile: &'a Path,
    pub(crate) shutdown_requested: bool,
    pub(crate) current_status: SandboxStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DetectedRuntimeStatus {
    pub(crate) status: SandboxStatus,
    pub(crate) explicitly_absent: bool,
}

pub(crate) fn detect_runtime_status(
    probe: RuntimeStatusProbe<'_>,
    running_status: impl FnOnce() -> Result<SandboxStatus>,
) -> Result<SandboxStatus> {
    observe_runtime_status(probe, running_status).map(|observation| observation.status)
}

pub(crate) fn inspect_runtime_artifact_presence(path: &Path, artifact: &str) -> Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(SandboxError::OperationFailed {
            message: format!(
                "failed to inspect sandbox {artifact} {}: {error}; runtime absence remains unproven",
                path.display()
            ),
        }),
    }
}

pub(crate) fn observe_runtime_status(
    probe: RuntimeStatusProbe<'_>,
    running_status: impl FnOnce() -> Result<SandboxStatus>,
) -> Result<DetectedRuntimeStatus> {
    if inspect_runtime_artifact_presence(probe.exit_status_file, "exit-status receipt")? {
        let exit_code = read_exit_code(probe.exit_status_file)?;
        return Ok(DetectedRuntimeStatus {
            status: if probe.shutdown_requested || exit_code == 0 {
                SandboxStatus::Stopped
            } else {
                SandboxStatus::Failed
            },
            explicitly_absent: false,
        });
    }

    let runtime_state = runtime_state(probe.state_command, probe.runtime_id)?;
    let explicitly_absent = runtime_state == RuntimeStateObservation::ExplicitlyAbsent;
    let pidfile_present = if explicitly_absent {
        inspect_runtime_artifact_presence(probe.pidfile, "pidfile")?
    } else {
        false
    };
    let status = match runtime_state {
        RuntimeStateObservation::Present(status) if status == "running" => running_status(),
        RuntimeStateObservation::Present(status) if status == "created" || status == "creating" => {
            Ok(SandboxStatus::Starting)
        }
        RuntimeStateObservation::Present(status) if status == "stopped" => {
            Ok(SandboxStatus::Stopped)
        }
        RuntimeStateObservation::Present(status) if status == "paused" => {
            Ok(SandboxStatus::Stopping)
        }
        RuntimeStateObservation::Present(_) => Ok(SandboxStatus::Failed),
        RuntimeStateObservation::ExplicitlyAbsent if pidfile_present => {
            if pid_is_alive(read_pid(probe.pidfile)?) {
                Ok(SandboxStatus::Starting)
            } else if probe.shutdown_requested {
                Ok(SandboxStatus::Stopped)
            } else {
                Ok(SandboxStatus::Failed)
            }
        }
        RuntimeStateObservation::ExplicitlyAbsent => Ok(match probe.current_status {
            SandboxStatus::Stopped | SandboxStatus::Failed => probe.current_status,
            SandboxStatus::Starting
            | SandboxStatus::Ready
            | SandboxStatus::NotReady
            | SandboxStatus::Stopping => SandboxStatus::Stopping,
        }),
    }?;
    Ok(DetectedRuntimeStatus {
        status,
        explicitly_absent,
    })
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
    let output = command
        .as_command()
        .output()
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to run sandbox cleanup command {}: {error}",
                command.program.display()
            ),
        })?;
    if output.status.success() {
        return Ok(());
    }
    Err(SandboxError::OperationFailed {
        message: format!(
            "sandbox cleanup command {} failed with {}: {}",
            command.program.display(),
            output.status,
            render_command_failure(&output.stdout, &output.stderr)
        ),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RuntimeStateObservation {
    Present(String),
    ExplicitlyAbsent,
}

enum RuntimeStateCommandOutcome {
    Observation(RuntimeStateObservation),
    AmbiguousCompletedFailure(SandboxError),
}

pub(crate) fn runtime_state(
    command: &CommandSpec,
    expected_runtime_id: &str,
) -> Result<RuntimeStateObservation> {
    match run_runtime_state_command(command, expected_runtime_id, None)? {
        RuntimeStateCommandOutcome::Observation(observation) => Ok(observation),
        RuntimeStateCommandOutcome::AmbiguousCompletedFailure(error) => Err(error),
    }
}

/// Observe runtime state only when it authenticates the exact creator attempt.
pub(crate) fn runtime_state_for_creator_attempt(
    command: &CommandSpec,
    expected_runtime_id: &str,
    expected_attempt_id: &str,
) -> Result<RuntimeStateObservation> {
    match run_runtime_state_command(command, expected_runtime_id, Some(expected_attempt_id))? {
        RuntimeStateCommandOutcome::Observation(observation) => Ok(observation),
        RuntimeStateCommandOutcome::AmbiguousCompletedFailure(error) => Err(error),
    }
}

fn run_runtime_state_command(
    command: &CommandSpec,
    expected_runtime_id: &str,
    expected_attempt_id: Option<&str>,
) -> Result<RuntimeStateCommandOutcome> {
    let mut runtime_command = command.as_command();
    runtime_command.env("LC_ALL", "C");
    let output = runtime_command
        .output()
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to run runtime state command {}: {error}",
                command.program.display()
            ),
        })?;
    if !output.status.success() {
        let diagnostic = render_command_failure(&output.stdout, &output.stderr);
        if crun_state_reports_missing_runtime(&output.stdout, &output.stderr, expected_runtime_id) {
            return Ok(RuntimeStateCommandOutcome::Observation(
                RuntimeStateObservation::ExplicitlyAbsent,
            ));
        }
        return Ok(RuntimeStateCommandOutcome::AmbiguousCompletedFailure(
            SandboxError::OperationFailed {
                message: format!(
                    "runtime state command {} failed with {} without explicit absence evidence: {}",
                    command.program.display(),
                    output.status,
                    diagnostic
                ),
            },
        ));
    }
    let payload: RuntimeStatePayload =
        serde_json::from_slice(&output.stdout).map_err(|error| SandboxError::OperationFailed {
            message: format!("failed to parse runtime state JSON: {error}"),
        })?;
    if payload.id.as_deref() != Some(expected_runtime_id) {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "runtime state response identity {:?} does not match expected runtime identity \
                 {expected_runtime_id:?}",
                payload.id
            ),
        });
    }
    if let Some(expected_attempt_id) = expected_attempt_id {
        let observed_attempt = payload.annotations.get(CREATOR_ATTEMPT_ANNOTATION);
        if observed_attempt.map(String::as_str) != Some(expected_attempt_id) {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "runtime state response creator attempt {:?} does not match expected attempt \
                     {expected_attempt_id:?} for runtime {expected_runtime_id:?}",
                    observed_attempt
                ),
            });
        }
    }
    Ok(RuntimeStateCommandOutcome::Observation(
        RuntimeStateObservation::Present(payload.status),
    ))
}

/// Attempt runtime deletion and accept only an explicit post-effect absence.
///
/// Provider deletion is deliberately idempotent at the composition boundary:
/// a non-zero delete result is diagnostic, while an exact state observation is
/// the authority for retrying cleanup after an earlier delete succeeded.
pub(crate) fn delete_runtime_and_confirm_absent(
    delete_command: &CommandSpec,
    state_command: &CommandSpec,
    expected_runtime_id: &str,
) -> Result<()> {
    let delete_error = run_status_best_effort(delete_command).err();
    match runtime_state(state_command, expected_runtime_id) {
        Ok(RuntimeStateObservation::ExplicitlyAbsent) => Ok(()),
        Ok(RuntimeStateObservation::Present(status)) => Err(SandboxError::OperationFailed {
            message: format!(
                "container runtime {expected_runtime_id} remains {status:?} after delete attempt{}",
                delete_error
                    .as_ref()
                    .map(|error| format!(" ({error})"))
                    .unwrap_or_default()
            ),
        }),
        Err(observe_error) => Err(SandboxError::OperationFailed {
            message: format!(
                "cannot confirm container runtime {expected_runtime_id} absence after delete \
                 attempt: {observe_error}{}",
                delete_error
                    .as_ref()
                    .map(|error| format!("; delete diagnostic: {error}"))
                    .unwrap_or_default()
            ),
        }),
    }
}

fn crun_state_reports_missing_runtime(
    stdout: &[u8],
    stderr: &[u8],
    expected_runtime_id: &str,
) -> bool {
    if !stdout.is_empty() || !valid_runtime_id_component(expected_runtime_id) {
        return false;
    }
    let Ok(stderr) = std::str::from_utf8(stderr) else {
        return false;
    };
    // Pinned crun 1.27.1 retains the underlying ENOENT `open` context and
    // appends the C-locale strerror plus one newline. Anything outside this
    // exact single-line grammar is ambiguous and cannot authorize cleanup.
    let prefix = format!("container `{expected_runtime_id}` does not exist: open `");
    let Some(status_path) = stderr
        .strip_prefix(&prefix)
        .and_then(|rest| rest.strip_suffix("`: No such file or directory\n"))
    else {
        return false;
    };
    if status_path.is_empty()
        || status_path
            .bytes()
            .any(|byte| byte == b'`' || byte.is_ascii_control())
        || status_path
            .split('/')
            .any(|component| component == "." || component == "..")
    {
        return false;
    }
    let path = Path::new(status_path);
    if !path.is_absolute() {
        return false;
    }
    let mut components = path.components().rev();
    matches!(components.next(), Some(Component::Normal(value)) if value == "status")
        && matches!(
            components.next(),
            Some(Component::Normal(value)) if value == expected_runtime_id
        )
        && components
            .all(|component| !matches!(component, Component::CurDir | Component::ParentDir))
}

fn valid_runtime_id_component(runtime_id: &str) -> bool {
    runtime_id
        .as_bytes()
        .first()
        .is_some_and(u8::is_ascii_alphanumeric)
        && runtime_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

#[cfg(test)]
pub(crate) fn wait_for_runtime_state(
    command: &CommandSpec,
    expected_runtime_id: &str,
    timeout: Duration,
) -> Result<String> {
    wait_for_runtime_state_inner(command, expected_runtime_id, None, timeout)
}

/// Wait for a created/running runtime belonging to the exact creator attempt.
pub(crate) fn wait_for_runtime_state_for_creator_attempt(
    command: &CommandSpec,
    expected_runtime_id: &str,
    expected_attempt_id: &str,
    timeout: Duration,
) -> Result<String> {
    wait_for_runtime_state_inner(
        command,
        expected_runtime_id,
        Some(expected_attempt_id),
        timeout,
    )
}

fn wait_for_runtime_state_inner(
    command: &CommandSpec,
    expected_runtime_id: &str,
    expected_attempt_id: Option<&str>,
    timeout: Duration,
) -> Result<String> {
    let deadline = Instant::now() + timeout;
    let mut last_ambiguous_observation = None;
    let status = poll_until_deadline(Some(deadline), Duration::from_millis(200), || {
        Ok(
            match run_runtime_state_command(command, expected_runtime_id, expected_attempt_id)? {
                RuntimeStateCommandOutcome::Observation(RuntimeStateObservation::Present(
                    status,
                )) if status == "created" || status == "running" => Some(status),
                RuntimeStateCommandOutcome::Observation(
                    RuntimeStateObservation::Present(_) | RuntimeStateObservation::ExplicitlyAbsent,
                ) => None,
                RuntimeStateCommandOutcome::AmbiguousCompletedFailure(error) => {
                    last_ambiguous_observation = Some(error.to_string());
                    None
                }
            },
        )
    })?;
    status.ok_or_else(|| SandboxError::OperationFailed {
        message: format!(
            "sandbox runtime did not reach created state before timeout via {}{}",
            command.program.display(),
            last_ambiguous_observation
                .as_ref()
                .map(|error| format!("; last ambiguous observation: {error}"))
                .unwrap_or_default()
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
    let found = poll_until_deadline(Some(deadline), Duration::from_millis(200), || {
        Ok(path.exists().then_some(()))
    })
    .unwrap_or(None) // the probe here is infallible
    .is_some();
    found || path.exists()
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
    if !inspect_runtime_artifact_presence(path, "runtime artifact")? {
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
    id: Option<String>,
    status: String,
    #[serde(default)]
    annotations: BTreeMap<String, String>,
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

    #[test]
    fn runtime_state_accepts_pinned_crun_absence_diagnostic_and_rejects_unknown_failure() {
        let runtime_id = "fixture";
        let absent = CommandSpec::new("/bin/sh").args([
            "-c",
            "[ \"$LC_ALL\" = C ] || { printf '%s\\n' 'unexpected locale' >&2; exit 2; }; printf '%s\\n' 'container `fixture` does not exist: open `/run/crun/fixture/status`: No such file or directory' >&2; exit 1",
        ]);
        assert_eq!(
            runtime_state(&absent, runtime_id)
                .expect("explicit not-found evidence should be classified"),
            RuntimeStateObservation::ExplicitlyAbsent
        );

        let unknown = CommandSpec::new("/bin/sh")
            .args(["-c", "printf '%s\n' 'permission denied' >&2; exit 1"]);
        let error = runtime_state(&unknown, runtime_id)
            .expect_err("generic provider failure must not become absence evidence");
        assert!(
            error
                .to_string()
                .contains("without explicit absence evidence")
                && error.to_string().contains("permission denied"),
            "unknown observation must retain the provider diagnostic: {error}"
        );

        for misleading in [
            "runtime root does not exist",
            "container `fixture` does not exist",
            "container `foreign-fixture` does not exist",
            "container `fixture` does not exist: permission denied",
            "container `fixture` does not exist: open `/run/crun/foreign-fixture/status`: No such file or directory",
            "container `fixture` does not exist: open `/run/crun/fixture/config`: No such file or directory",
            "container `fixture` does not exist: open `relative/fixture/status`: No such file or directory",
            "container `fixture` does not exist: open `/run/crun/../fixture/status`: No such file or directory",
            "container `fixture` does not exist: open `/run/crun/fixture/status`: Permission denied",
            "crun: container `fixture` does not exist: open `/run/crun/fixture/status`: No such file or directory",
            "container `fixture` does not exist\nadditional provider failure",
        ] {
            let command = CommandSpec::new("/bin/sh")
                .args(["-c", &format!("printf '%s\\n' '{misleading}' >&2; exit 1")]);
            let error = runtime_state(&command, runtime_id)
                .expect_err("unrelated or ambiguous diagnostics must not prove absence");
            assert!(
                error
                    .to_string()
                    .contains("without explicit absence evidence"),
                "misleading diagnostic must fail closed: {error}"
            );
        }
    }

    #[test]
    fn successful_runtime_state_authenticates_the_expected_runtime_identity() {
        let matching = CommandSpec::new("/bin/sh").args([
            "-c",
            "printf '%s\\n' '{\"id\":\"fixture\",\"status\":\"running\"}'",
        ]);
        assert_eq!(
            runtime_state(&matching, "fixture")
                .expect("matching runtime identity should be authoritative"),
            RuntimeStateObservation::Present("running".to_owned())
        );

        for payload in [
            "{\"id\":\"foreign\",\"status\":\"running\"}",
            "{\"status\":\"running\"}",
        ] {
            let command =
                CommandSpec::new("/bin/sh").args(["-c", &format!("printf '%s\\n' '{payload}'")]);
            let error = runtime_state(&command, "fixture")
                .expect_err("missing or foreign runtime identity must fail closed");
            assert!(
                error.to_string().contains("runtime identity")
                    && error.to_string().contains("fixture"),
                "identity rejection must name the expected runtime: {error}"
            );
        }
    }

    #[test]
    fn creator_runtime_state_requires_the_exact_attempt_annotation() {
        let matching = CommandSpec::new("/bin/sh").args([
            "-c",
            "printf '%s\\n' '{\"id\":\"fixture\",\"status\":\"running\",\
             \"annotations\":{\"com.nimbus.creator-attempt\":\"attempt-alpha\"}}'",
        ]);
        assert_eq!(
            runtime_state_for_creator_attempt(&matching, "fixture", "attempt-alpha")
                .expect("matching runtime and creator attempt should authenticate"),
            RuntimeStateObservation::Present("running".to_owned())
        );

        for payload in [
            "{\"id\":\"fixture\",\"status\":\"running\"}",
            "{\"id\":\"fixture\",\"status\":\"running\",\"annotations\":{\
             \"com.nimbus.creator-attempt\":\"attempt-stale\"}}",
        ] {
            let command =
                CommandSpec::new("/bin/sh").args(["-c", &format!("printf '%s\\n' '{payload}'")]);
            let error = runtime_state_for_creator_attempt(&command, "fixture", "attempt-alpha")
                .expect_err("missing or stale creator attempt must fail closed");
            assert!(
                error.to_string().contains("creator attempt")
                    && error.to_string().contains("attempt-alpha"),
                "attempt rejection must name the expected identity: {error}"
            );
        }
    }

    #[test]
    fn runtime_state_absence_parser_rejects_noncanonical_raw_streams() {
        let expected =
            b"container `fixture` does not exist: open `/run/crun/fixture/status`: No such file or directory\n";
        assert!(crun_state_reports_missing_runtime(b"", expected, "fixture"));

        for (stdout, stderr, runtime_id) in [
            (&b"unexpected stdout"[..], &expected[..], "fixture"),
            (
                &b""[..],
                &b"container `fixture` does not exist: open `/run/crun/fixture/status`: No such file or directory"[..],
                "fixture",
            ),
            (
                &b""[..],
                &b"container `fixture` does not exist: open `/run/crun/fixture/status`: No such file or directory\r\n"[..],
                "fixture",
            ),
            (
                &b""[..],
                &b"container `fixture` does not exist: open `/run/crun/fixture/status`: No such file or directory\nextra\n"[..],
                "fixture",
            ),
            (&b""[..], &b"\xff\n"[..], "fixture"),
            (&b""[..], &expected[..], "../fixture"),
            (&b""[..], &expected[..], "fixture\n"),
        ] {
            assert!(
                !crun_state_reports_missing_runtime(stdout, stderr, runtime_id),
                "noncanonical runtime-state evidence must fail closed"
            );
        }
    }

    #[test]
    fn wait_for_runtime_state_retries_ambiguous_nonzero_observation() {
        let temp_dir = tempfile::TempDir::new().expect("temporary directory should exist");
        let attempts = temp_dir.path().join("attempts");
        std::fs::write(&attempts, b"0\n").expect("attempt counter should initialize");
        let command = CommandSpec::new("/bin/sh").args([
            "-c".to_owned(),
            "attempt=$(cat \"$0\"); if [ \"$attempt\" = 0 ]; then printf '1\\n' > \"$0\"; \
             printf '%s\\n' 'status file is not published yet' >&2; exit 1; fi; \
             printf '%s\\n' '{\"id\":\"fixture\",\"status\":\"created\"}'"
                .to_owned(),
            attempts.display().to_string(),
        ]);

        assert_eq!(
            wait_for_runtime_state(&command, "fixture", Duration::from_secs(1))
                .expect("ambiguous launch progress should be retried"),
            "created"
        );
        assert_eq!(
            std::fs::read_to_string(&attempts)
                .expect("attempt counter should remain readable")
                .trim(),
            "1",
            "the fixture must cross the ambiguous first observation"
        );
    }

    #[test]
    fn wait_for_runtime_state_timeout_reports_last_ambiguous_observation() {
        let command = CommandSpec::new("/bin/sh")
            .args(["-c", "printf '%s\\n' 'temporarily unavailable' >&2; exit 1"]);

        let error = wait_for_runtime_state(&command, "fixture", Duration::from_millis(25))
            .expect_err("ambiguous launch progress must remain bounded by the timeout");
        assert!(
            error.to_string().contains("last ambiguous observation")
                && error.to_string().contains("temporarily unavailable"),
            "the timeout must retain the exact last provider diagnostic: {error}"
        );
    }

    #[test]
    fn runtime_status_rejects_exit_receipt_inspection_failure() {
        let temp_dir = tempfile::TempDir::new().expect("temporary directory should exist");
        let non_directory = temp_dir.path().join("exit-receipt-parent");
        std::fs::write(&non_directory, b"not a directory")
            .expect("non-directory parent should create");
        let exit_status_file = non_directory.join("exit");
        let pidfile = temp_dir.path().join("missing-pidfile");
        let state_command = CommandSpec::new("/bin/sh").args(["-c", "exit 77"]);

        let error = observe_runtime_status(
            RuntimeStatusProbe {
                exit_status_file: &exit_status_file,
                state_command: &state_command,
                runtime_id: "fixture",
                pidfile: &pidfile,
                shutdown_requested: false,
                current_status: SandboxStatus::Ready,
            },
            || Ok(SandboxStatus::Ready),
        )
        .expect_err("an inaccessible exit receipt must fail before runtime observation");

        assert!(
            error
                .to_string()
                .contains("failed to inspect sandbox exit-status receipt")
                && error
                    .to_string()
                    .contains(&exit_status_file.display().to_string()),
            "the exact inaccessible receipt must remain explicit: {error}"
        );
    }

    #[test]
    fn runtime_status_rejects_pidfile_inspection_failure() {
        let temp_dir = tempfile::TempDir::new().expect("temporary directory should exist");
        let exit_status_file = temp_dir.path().join("missing-exit-receipt");
        let non_directory = temp_dir.path().join("pidfile-parent");
        std::fs::write(&non_directory, b"not a directory")
            .expect("non-directory parent should create");
        let pidfile = non_directory.join("pid");
        let state_command = CommandSpec::new("/bin/sh").args([
            "-c",
            "printf '%s\n' 'container `fixture` does not exist: open `/run/crun/fixture/status`: No such file or directory' >&2; exit 1",
        ]);

        let error = observe_runtime_status(
            RuntimeStatusProbe {
                exit_status_file: &exit_status_file,
                state_command: &state_command,
                runtime_id: "fixture",
                pidfile: &pidfile,
                shutdown_requested: false,
                current_status: SandboxStatus::Ready,
            },
            || Ok(SandboxStatus::Ready),
        )
        .expect_err("an inaccessible pidfile must not become explicit runtime absence");

        assert!(
            error
                .to_string()
                .contains("failed to inspect sandbox pidfile")
                && error.to_string().contains(&pidfile.display().to_string()),
            "the exact inaccessible pidfile must remain explicit: {error}"
        );
    }

    #[test]
    fn remove_if_exists_rejects_artifact_inspection_failure() {
        let temp_dir = tempfile::TempDir::new().expect("temporary directory should exist");
        let non_directory = temp_dir.path().join("artifact-parent");
        std::fs::write(&non_directory, b"not a directory")
            .expect("non-directory parent should create");
        let artifact = non_directory.join("artifact");

        let error = remove_if_exists(&artifact)
            .expect_err("inaccessible cleanup evidence must fail closed");
        assert!(
            error
                .to_string()
                .contains("failed to inspect sandbox runtime artifact")
                && error.to_string().contains(&artifact.display().to_string())
                && error.to_string().contains("absence remains unproven"),
            "cleanup must retain the exact inaccessible artifact diagnostic: {error}"
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
