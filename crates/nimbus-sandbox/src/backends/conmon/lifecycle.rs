use std::collections::BTreeMap;
use std::path::{Component, Path};
use std::time::{Duration, Instant};
#[cfg(test)]
use std::{
    sync::{Arc, Condvar, Mutex},
    time::Duration as TestDuration,
};

use serde::{Deserialize, Serialize};

use crate::backends::oci::command::{
    CommandSpec, render_command_failure, run_bounded_command_output,
};
use crate::backends::poll::poll_until_deadline;
use crate::error::{Result, SandboxError};
use crate::instance::SandboxStatus;
use crate::process::pid_is_alive;
use crate::spec::SandboxSpec;

pub(crate) const CREATOR_ATTEMPT_ANNOTATION: &str = "com.nimbus.creator-attempt";
const RUNTIME_STATE_OBSERVATION_TIMEOUT: Duration = Duration::from_secs(2);

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

    pub(crate) fn effect_count(&self) -> usize {
        self.shared
            .0
            .lock()
            .expect("restart launch test probe lock should not be poisoned")
            .effects
    }
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct DetectedRuntimeStatus {
    pub(crate) status: SandboxStatus,
    pub(crate) explicitly_absent: bool,
    pub(crate) provider_state: RuntimeStateObservation,
    pub(crate) provider_command_evidence: Option<RuntimeStateCommandEvidence>,
    pub(crate) pidfile_evidence: Option<Vec<u8>>,
    pub(crate) running_evidence: Vec<u8>,
}

#[cfg(test)]
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

#[cfg(test)]
pub(crate) fn observe_runtime_status(
    probe: RuntimeStatusProbe<'_>,
    running_status: impl FnOnce() -> Result<SandboxStatus>,
) -> Result<DetectedRuntimeStatus> {
    observe_runtime_status_with_evidence(probe, || {
        running_status().map(|status| (status, Vec::new()))
    })
}

pub(crate) fn observe_runtime_status_with_evidence(
    probe: RuntimeStatusProbe<'_>,
    running_status: impl FnOnce() -> Result<(SandboxStatus, Vec<u8>)>,
) -> Result<DetectedRuntimeStatus> {
    // An exit receipt decides the status only once it carries a code. Conmon
    // creates the receipt and then writes into it, so an empty receipt means
    // the exit code has not arrived, and the observation that follows this one
    // reads it. Failing here instead would report a parse error against a
    // conmon mid-publication.
    if let ExitReceipt::Published {
        exit_code,
        evidence,
    } = read_exit_receipt(probe.exit_status_file)?
    {
        return Ok(DetectedRuntimeStatus {
            status: if probe.shutdown_requested || exit_code == 0 {
                SandboxStatus::Stopped
            } else {
                SandboxStatus::Failed
            },
            explicitly_absent: false,
            provider_state: RuntimeStateObservation::Present("exit-receipt".to_owned()),
            provider_command_evidence: None,
            pidfile_evidence: None,
            // The bytes that proved the code are the evidence. Re-reading the
            // path would sample it a second time and could disagree.
            running_evidence: evidence,
        });
    }

    let (runtime_state, provider_command_evidence) =
        runtime_state_with_evidence(probe.state_command, probe.runtime_id)?;
    let explicitly_absent = runtime_state == RuntimeStateObservation::ExplicitlyAbsent;
    let pidfile_evidence =
        if explicitly_absent && inspect_runtime_artifact_presence(probe.pidfile, "pidfile")? {
            Some(
                std::fs::read(probe.pidfile).map_err(|error| SandboxError::OperationFailed {
                    message: format!(
                        "failed to read sandbox pidfile evidence {}: {error}",
                        probe.pidfile.display()
                    ),
                })?,
            )
        } else {
            None
        };
    let (status, running_evidence) = match &runtime_state {
        RuntimeStateObservation::Present(status) if status == "running" => running_status(),
        RuntimeStateObservation::Present(status) if status == "created" || status == "creating" => {
            Ok((SandboxStatus::Starting, Vec::new()))
        }
        RuntimeStateObservation::Present(status) if status == "stopped" => {
            Ok((SandboxStatus::Stopped, Vec::new()))
        }
        RuntimeStateObservation::Present(status) if status == "paused" => {
            Ok((SandboxStatus::Stopping, Vec::new()))
        }
        RuntimeStateObservation::Present(_) => Ok((SandboxStatus::Failed, Vec::new())),
        RuntimeStateObservation::ExplicitlyAbsent if pidfile_evidence.is_some() => {
            let pid_bytes = pidfile_evidence
                .as_deref()
                .expect("pidfile evidence is present in this branch");
            let pid = parse_pid_evidence(probe.pidfile, pid_bytes)?;
            if pid_is_alive(pid) {
                Ok((SandboxStatus::Starting, Vec::new()))
            } else if probe.shutdown_requested {
                Ok((SandboxStatus::Stopped, Vec::new()))
            } else {
                Ok((SandboxStatus::Failed, Vec::new()))
            }
        }
        RuntimeStateObservation::ExplicitlyAbsent => Ok((
            match probe.current_status {
                SandboxStatus::Stopped | SandboxStatus::Failed => probe.current_status,
                SandboxStatus::Starting
                | SandboxStatus::Ready
                | SandboxStatus::NotReady
                | SandboxStatus::Stopping => SandboxStatus::Stopping,
            },
            Vec::new(),
        )),
    }?;
    Ok(DetectedRuntimeStatus {
        status,
        explicitly_absent,
        provider_state: runtime_state,
        provider_command_evidence: Some(provider_command_evidence),
        pidfile_evidence,
        running_evidence,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) enum RuntimeStateObservation {
    Present(String),
    ExplicitlyAbsent,
}

/// Exact bounded provider-process evidence behind a normalized runtime state.
///
/// The typed state drives projection. These bytes participate only in the
/// inspection comparison token, so provider-output changes cannot disappear
/// merely because they normalize to the same state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct RuntimeStateCommandEvidence {
    status_success: bool,
    status_code: Option<i32>,
    status_debug: String,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

enum RuntimeStateCommandOutcome {
    Observation {
        observation: RuntimeStateObservation,
        evidence: RuntimeStateCommandEvidence,
        pid: Option<u32>,
    },
    AmbiguousCompletedFailure(SandboxError),
}

pub(crate) fn runtime_state(
    command: &CommandSpec,
    expected_runtime_id: &str,
) -> Result<RuntimeStateObservation> {
    match run_runtime_state_command(command, expected_runtime_id, None)? {
        RuntimeStateCommandOutcome::Observation { observation, .. } => Ok(observation),
        RuntimeStateCommandOutcome::AmbiguousCompletedFailure(error) => Err(error),
    }
}

fn runtime_state_with_evidence(
    command: &CommandSpec,
    expected_runtime_id: &str,
) -> Result<(RuntimeStateObservation, RuntimeStateCommandEvidence)> {
    match run_runtime_state_command(command, expected_runtime_id, None)? {
        RuntimeStateCommandOutcome::Observation {
            observation,
            evidence,
            ..
        } => Ok((observation, evidence)),
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
        RuntimeStateCommandOutcome::Observation { observation, .. } => Ok(observation),
        RuntimeStateCommandOutcome::AmbiguousCompletedFailure(error) => Err(error),
    }
}

/// Exact provider state used to authenticate a runtime process before signal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum RuntimeProcessProviderObservation {
    Present { pid: u32 },
    ExplicitlyAbsent,
}

/// Observe a runtime process only when both its runtime and creator-attempt
/// identities match. A present response without an OCI state PID is not
/// sufficient process identity and fails closed.
pub(super) fn runtime_process_state_for_creator_attempt(
    command: &CommandSpec,
    expected_runtime_id: &str,
    expected_attempt_id: &str,
) -> Result<RuntimeProcessProviderObservation> {
    match run_runtime_state_command(command, expected_runtime_id, Some(expected_attempt_id))? {
        RuntimeStateCommandOutcome::Observation {
            observation: RuntimeStateObservation::Present(_),
            pid: Some(pid),
            ..
        } => Ok(RuntimeProcessProviderObservation::Present { pid }),
        RuntimeStateCommandOutcome::Observation {
            observation: RuntimeStateObservation::Present(_),
            pid: None,
            ..
        } => Err(SandboxError::OperationFailed {
            message: format!(
                "runtime state for {expected_runtime_id:?} omitted its process identity; refusing to authorize a signal"
            ),
        }),
        RuntimeStateCommandOutcome::Observation {
            observation: RuntimeStateObservation::ExplicitlyAbsent,
            ..
        } => Ok(RuntimeProcessProviderObservation::ExplicitlyAbsent),
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
    let output =
        run_bounded_command_output(&mut runtime_command, RUNTIME_STATE_OBSERVATION_TIMEOUT)
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "runtime state command {} did not produce bounded evidence: {error}",
                    command.program.display()
                ),
            })?;
    let evidence = RuntimeStateCommandEvidence {
        status_success: output.status.success(),
        status_code: output.status.code(),
        status_debug: format!("{:?}", output.status),
        stdout: output.stdout.clone(),
        stderr: output.stderr.clone(),
    };
    if !output.status.success() {
        let diagnostic = render_command_failure(&output.stdout, &output.stderr);
        if crun_state_reports_missing_runtime(&output.stdout, &output.stderr, expected_runtime_id) {
            return Ok(RuntimeStateCommandOutcome::Observation {
                observation: RuntimeStateObservation::ExplicitlyAbsent,
                evidence,
                pid: None,
            });
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
    Ok(RuntimeStateCommandOutcome::Observation {
        observation: RuntimeStateObservation::Present(payload.status),
        evidence,
        pid: payload.pid,
    })
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
    // Supported crun releases retain the exact status path in one of these
    // C-locale ENOENT diagnostics. Anything outside these closed single-line
    // grammars is ambiguous and cannot authorize cleanup.
    let legacy_prefix = format!("container `{expected_runtime_id}` does not exist: open `");
    let status_path = stderr
        .strip_prefix(&legacy_prefix)
        .or_else(|| stderr.strip_prefix("error opening file `"))
        .and_then(|rest| rest.strip_suffix("`: No such file or directory\n"));
    let Some(status_path) = status_path else {
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
                RuntimeStateCommandOutcome::Observation {
                    observation: RuntimeStateObservation::Present(status),
                    ..
                } if status == "created" || status == "running" => Some(status),
                RuntimeStateCommandOutcome::Observation {
                    observation:
                        RuntimeStateObservation::Present(_) | RuntimeStateObservation::ExplicitlyAbsent,
                    ..
                } => None,
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
    let pid = std::fs::read(path).map_err(|error| SandboxError::OperationFailed {
        message: format!("failed to read sandbox pidfile {}: {error}", path.display()),
    })?;
    parse_pid_evidence(path, &pid)
}

fn parse_pid_evidence(path: &Path, pid: &[u8]) -> Result<u32> {
    let pid = std::str::from_utf8(pid).map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "failed to decode sandbox pid from {} as UTF-8: {error}",
            path.display()
        ),
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

/// Interval between receipt probes.
const RECEIPT_POLL_INTERVAL: Duration = Duration::from_millis(200);

/// Wait until `path` exists, or `timeout` elapses.
///
/// Existence is the whole contract, so this suits a marker whose presence is
/// the message and nothing else. Every remaining caller is a test fixture
/// gate; no production wait belongs here, because a production receipt always
/// carries a value and existence is the wrong event to wait for. Use
/// [`wait_for_receipt`] for those.
#[cfg(test)]
pub(crate) fn wait_for_path(path: &Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    let found = poll_until_deadline(Some(deadline), RECEIPT_POLL_INTERVAL, || {
        Ok(path.exists().then_some(()))
    })
    .unwrap_or(None) // the probe here is infallible
    .is_some();
    found || path.exists()
}

/// Wait until `path` holds a receipt that `read` accepts, or `timeout`
/// elapses.
///
/// A writer publishes a receipt in two steps: it opens the path, and then it
/// writes the value into it. Between those steps the file is visible and
/// empty. A reader that treats existence as publication therefore reads an
/// empty file whenever it lands inside that window, and reports a parse
/// failure against a writer that did nothing wrong. The window is short, but
/// it is not closed: a loaded host can deschedule the writer between the open
/// and the write, and the coverage lane caught exactly that.
///
/// Waiting on the value closes the window without asking the writer to publish
/// atomically. That matters, because the writers here are real runtime
/// processes -- conmon and crun -- whose publication order this code does not
/// own.
pub(crate) fn wait_for_receipt<T>(
    path: &Path,
    timeout: Duration,
    read: impl Fn(&Path) -> Result<T>,
) -> Option<T> {
    let deadline = Instant::now() + timeout;
    poll_until_deadline(Some(deadline), RECEIPT_POLL_INTERVAL, || {
        Ok(path.exists().then(|| read(path).ok()).flatten())
    })
    .unwrap_or(None) // the probe here is infallible
    .or_else(|| read(path).ok())
}

/// What an exit receipt says right now.
///
/// Conmon publishes an exit receipt in two steps: it creates the file, and then
/// it writes the code into it. Between those steps the receipt is visible and
/// empty, so presence alone cannot separate "the container has not exited" from
/// "the container exited and its code is still on the way". Naming that middle
/// state is what keeps an inspection from reporting a parse failure against a
/// conmon that did nothing wrong.
///
/// A short decimal reaches the file in one write, so a reader observes either
/// no bytes or every byte. Empty is therefore the only intermediate state, and
/// a receipt holding bytes that do not parse is corruption rather than a race.
/// [`read_exit_receipt`] still reports that as an error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExitReceipt {
    /// No receipt exists, so the container has not exited.
    Absent,
    /// The receipt exists but does not carry an exit code yet.
    Unpublished,
    /// The receipt carries `exit_code`, proved by `evidence`.
    Published { exit_code: i32, evidence: Vec<u8> },
}

impl ExitReceipt {
    /// The exit code, when this receipt carries one.
    ///
    /// Absent and unpublished both answer `None`, because neither is an
    /// observation of how the container exited.
    pub(crate) fn exit_code(&self) -> Option<i32> {
        match self {
            Self::Published { exit_code, .. } => Some(*exit_code),
            Self::Absent | Self::Unpublished => None,
        }
    }
}

/// Read `path` as an exit receipt, without treating an unfinished publication
/// as a failure.
///
/// Prefer this over [`read_exit_code`] anywhere the caller inspects a receipt
/// it did not first prove readable. `read_exit_code` remains correct after a
/// wait that already established publication.
pub(crate) fn read_exit_receipt(path: &Path) -> Result<ExitReceipt> {
    if !inspect_runtime_artifact_presence(path, "exit-status receipt")? {
        return Ok(ExitReceipt::Absent);
    }
    let exit_status = match std::fs::read(path) {
        Ok(exit_status) => exit_status,
        // A teardown can remove the receipt between the presence check and the
        // read. That is absence observed late, not a failure to read.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ExitReceipt::Absent);
        }
        Err(error) => {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "failed to read sandbox exit status {}: {error}",
                    path.display()
                ),
            });
        }
    };
    if exit_status.iter().all(u8::is_ascii_whitespace) {
        return Ok(ExitReceipt::Unpublished);
    }
    let (exit_code, evidence) = parse_exit_code_evidence(path, exit_status)?;
    Ok(ExitReceipt::Published {
        exit_code,
        evidence,
    })
}

pub(crate) fn read_exit_code(path: &Path) -> Result<i32> {
    read_exit_code_evidence(path).map(|(exit_code, _evidence)| exit_code)
}

pub(crate) fn read_exit_code_evidence(path: &Path) -> Result<(i32, Vec<u8>)> {
    let exit_status = std::fs::read(path).map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "failed to read sandbox exit status {}: {error}",
            path.display()
        ),
    })?;
    parse_exit_code_evidence(path, exit_status)
}

fn parse_exit_code_evidence(path: &Path, exit_status: Vec<u8>) -> Result<(i32, Vec<u8>)> {
    let rendered =
        std::str::from_utf8(&exit_status).map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to decode sandbox exit status {} as UTF-8: {error}",
                path.display()
            ),
        })?;
    let exit_code =
        rendered
            .trim()
            .parse::<i32>()
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to parse sandbox exit status {}: {error}",
                    path.display()
                ),
            })?;
    Ok((exit_code, exit_status))
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
    pid: Option<u32>,
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
    fn a_receipt_wait_spans_the_gap_between_creating_a_file_and_filling_it() {
        let temp_dir = tempfile::TempDir::new().expect("temporary directory should exist");
        let receipt = temp_dir.path().join("descendant.pid");
        // Publish the way a writer does: the path becomes visible first, and
        // the value arrives afterwards.
        std::fs::write(&receipt, b"").expect("receipt should be created empty");
        let filler = std::thread::spawn({
            let receipt = receipt.clone();
            move || {
                std::thread::sleep(Duration::from_millis(300));
                std::fs::write(&receipt, b"4242").expect("receipt should be filled");
            }
        });

        assert!(
            wait_for_path(&receipt, Duration::from_secs(2)),
            "the empty file already satisfies existence, which is what made it the wrong event"
        );
        assert!(
            read_pid(&receipt).is_err(),
            "and existence alone cannot yield a pid"
        );

        assert_eq!(
            wait_for_receipt(&receipt, Duration::from_secs(2), read_pid),
            Some(4242),
            "waiting on the value must outlast the writer's open-then-write gap"
        );
        filler.join().expect("filler thread should finish");
    }

    #[test]
    fn a_receipt_that_never_fills_times_out_rather_than_reporting_a_value() {
        let temp_dir = tempfile::TempDir::new().expect("temporary directory should exist");
        let receipt = temp_dir.path().join("descendant.pid");
        std::fs::write(&receipt, b"").expect("receipt should be created empty");

        assert_eq!(
            wait_for_receipt(&receipt, Duration::from_millis(300), read_pid),
            None,
            "a receipt that never carries a value must not resolve"
        );
    }

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
        for diagnostic in [
            "container `fixture` does not exist: open `/run/crun/fixture/status`: No such file or directory",
            "error opening file `/run/crun/fixture/status`: No such file or directory",
            "container `fixture` does not exist: open `/run/user/1000/crun/fixture/status`: No such file or directory",
            "error opening file `/var/lib/nimbus/crun-state/fixture/status`: No such file or directory",
        ] {
            let absent = CommandSpec::new("/bin/sh").args([
                "-c",
                &format!(
                    "[ \"$LC_ALL\" = C ] || {{ printf '%s\\n' 'unexpected locale' >&2; exit 2; }}; printf '%s\\n' '{diagnostic}' >&2; exit 1"
                ),
            ]);
            assert_eq!(
                runtime_state(&absent, runtime_id)
                    .expect("explicit not-found evidence should be classified"),
                RuntimeStateObservation::ExplicitlyAbsent
            );
        }

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
            "error opening file `/run/crun/foreign-fixture/status`: No such file or directory",
            "error opening file `/run/crun/fixture/config`: No such file or directory",
            "error opening file `relative/fixture/status`: No such file or directory",
            "error opening file `/run/crun/../fixture/status`: No such file or directory",
            "crun: error opening file `/run/crun/fixture/status`: No such file or directory",
            "error opening file `/run/crun/fixture/status`: No such file or directory\nadditional provider failure",
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
    fn runtime_state_observation_timeout_fails_closed_after_owned_termination() {
        let command = CommandSpec::new("/bin/sh").args(["-c", "exec sleep 5"]);
        let started = Instant::now();

        let error = runtime_state(&command, "fixture")
            .expect_err("a hung provider state query must not block inspection indefinitely");

        assert!(
            error
                .to_string()
                .contains("did not produce bounded evidence")
                && error.to_string().contains("provider command exceeded 2s"),
            "the timeout must remain a named ambiguity rather than absence or success: {error}"
        );
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "the two-second provider observation budget must terminate and reap its child"
        );
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
        let current = b"error opening file `/run/crun/fixture/status`: No such file or directory\n";
        let rootless =
            b"error opening file `/run/user/1000/crun/fixture/status`: No such file or directory\n";
        assert!(crun_state_reports_missing_runtime(b"", expected, "fixture"));
        assert!(crun_state_reports_missing_runtime(b"", current, "fixture"));
        assert!(crun_state_reports_missing_runtime(b"", rootless, "fixture"));

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
    fn an_exit_receipt_separates_absent_from_created_but_unwritten() {
        let temp_dir = tempfile::TempDir::new().expect("temporary directory should exist");
        let receipt = temp_dir.path().join("exit");

        assert_eq!(
            read_exit_receipt(&receipt).expect("an absent receipt should read"),
            ExitReceipt::Absent,
            "no receipt means the container has not exited"
        );

        // Publish the way conmon does: the receipt appears first, and the exit
        // code arrives afterwards.
        std::fs::write(&receipt, b"").expect("receipt should be created empty");
        assert_eq!(
            read_exit_receipt(&receipt).expect("an empty receipt should read, not fail"),
            ExitReceipt::Unpublished,
            "an empty receipt is an unfinished publication, not a parse failure"
        );
        assert!(
            read_exit_code(&receipt).is_err(),
            "and reading it as a code is what used to blame conmon for the window"
        );

        std::fs::write(&receipt, b"\n  ").expect("receipt should hold only whitespace");
        assert_eq!(
            read_exit_receipt(&receipt).expect("a whitespace receipt should read"),
            ExitReceipt::Unpublished,
            "trailing separators alone still carry no code"
        );

        std::fs::write(&receipt, b"137\n").expect("receipt should be filled");
        assert_eq!(
            read_exit_receipt(&receipt).expect("a published receipt should read"),
            ExitReceipt::Published {
                exit_code: 137,
                evidence: b"137\n".to_vec(),
            },
            "a receipt that carries a code reports it with the bytes that proved it"
        );
    }

    #[test]
    fn an_exit_receipt_holding_bytes_that_do_not_parse_stays_an_error() {
        let temp_dir = tempfile::TempDir::new().expect("temporary directory should exist");
        let receipt = temp_dir.path().join("exit");
        std::fs::write(&receipt, b"not-an-exit-code").expect("receipt should hold garbage");

        let error = read_exit_receipt(&receipt)
            .expect_err("content that is present and unparseable is corruption, not a race");
        assert!(
            error
                .to_string()
                .contains("failed to parse sandbox exit status"),
            "corruption must stay loud and name the receipt: {error}"
        );
    }

    #[test]
    fn an_unpublished_exit_receipt_does_not_decide_the_runtime_status() {
        let temp_dir = tempfile::TempDir::new().expect("temporary directory should exist");
        let exit_status_file = temp_dir.path().join("exit");
        let pidfile = temp_dir.path().join("missing-pidfile");
        let state_command = CommandSpec::new("/bin/sh").args([
            "-c",
            "printf '%s\n' 'container `fixture` does not exist: open `/run/crun/fixture/status`: No such file or directory' >&2; exit 1",
        ]);
        std::fs::write(&exit_status_file, b"").expect("receipt should be created empty");

        let observed = observe_runtime_status(
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
        .expect("an unfinished publication must not fail the observation");
        assert_ne!(
            observed.provider_state,
            RuntimeStateObservation::Present("exit-receipt".to_owned()),
            "a receipt without a code cannot be the source of the status"
        );

        std::fs::write(&exit_status_file, b"0\n").expect("receipt should be filled");
        let observed = observe_runtime_status(
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
        .expect("a published receipt should be observable");
        assert_eq!(
            observed.provider_state,
            RuntimeStateObservation::Present("exit-receipt".to_owned()),
            "once the code lands the receipt decides the status"
        );
        assert_eq!(observed.status, SandboxStatus::Stopped);
        assert_eq!(
            observed.running_evidence,
            b"0\n".to_vec(),
            "the evidence must be the bytes that proved the code, not a second sample"
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
