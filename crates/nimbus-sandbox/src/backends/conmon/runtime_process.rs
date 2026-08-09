//! Stable runtime-process identity and race-free signalling for conmon-backed workloads.
//!
//! A pidfile PID is only a locator. Signal authority requires the exact OCI
//! runtime identity, creator-attempt annotation, provider-state PID, pidfile
//! PID, and operating-system process birth. Linux effects use a pidfd so a PID
//! recycled after authentication cannot redirect the signal.

use std::io::Read as _;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::lifecycle::{
    RuntimeProcessProviderObservation, runtime_process_state_for_creator_attempt,
};
use crate::backends::oci::command::CommandSpec;
use crate::error::{Result, SandboxError};

const MAX_PIDFILE_BYTES: u64 = 32;

#[cfg(test)]
#[path = "runtime_process/tests.rs"]
mod tests;

/// Persistable identity for one exact OCI runtime process incarnation.
///
/// This type deliberately contains no live file descriptor. A fresh process
/// must reopen a pidfd and reauthenticate every durable field before signal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct RuntimeProcessIdentity {
    runtime_id: String,
    creator_attempt_id: String,
    pid: u32,
    birth: RuntimeProcessBirth,
}

impl RuntimeProcessIdentity {
    #[cfg(test)]
    pub(crate) fn runtime_id(&self) -> &str {
        &self.runtime_id
    }

    #[cfg(test)]
    pub(crate) fn creator_attempt_id(&self) -> &str {
        &self.creator_attempt_id
    }

    #[cfg(test)]
    pub(crate) const fn pid(&self) -> u32 {
        self.pid
    }

    #[cfg(test)]
    pub(crate) fn fixture(runtime_id: &str, creator_attempt_id: &str, pid: u32) -> Self {
        Self {
            runtime_id: runtime_id.to_owned(),
            creator_attempt_id: creator_attempt_id.to_owned(),
            pid,
            birth: RuntimeProcessBirth::LinuxProcStartTicks { ticks: 1 },
        }
    }

    #[cfg(test)]
    fn with_substituted_birth_for_test(mut self) -> Self {
        match &mut self.birth {
            RuntimeProcessBirth::LinuxProcStartTicks { ticks } => {
                *ticks = ticks.saturating_add(1);
            }
            RuntimeProcessBirth::AppleBsdStartTime { microseconds, .. } => {
                *microseconds = microseconds.saturating_add(1);
            }
        }
        self
    }
}

/// Platform-native token that changes when a numeric PID is recycled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum RuntimeProcessBirth {
    LinuxProcStartTicks { ticks: u64 },
    AppleBsdStartTime { seconds: u64, microseconds: u64 },
}

/// Exact read-only observation of a persisted runtime process identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeProcessIdentityObservation {
    ExactLive,
    ExplicitlyAbsent,
}

/// Validated named process signal.
///
/// Numeric real-time and libc-reserved signals are deliberately rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RuntimeProcessSignal {
    number: i32,
}

impl RuntimeProcessSignal {
    pub(crate) fn parse(value: &str) -> Result<Self> {
        let canonical = value.trim().to_ascii_uppercase();
        let name = canonical.strip_prefix("SIG").unwrap_or(&canonical);
        let number = name
            .parse::<i32>()
            .ok()
            .filter(|number| is_named_signal_number(*number))
            .or_else(|| named_signal_number(name))
            .ok_or_else(|| SandboxError::OperationFailed {
                message: format!(
                    "sandbox signal {value:?} is not a supported named process signal"
                ),
            })?;
        Ok(Self { number })
    }

    pub(crate) fn kill() -> Self {
        Self {
            number: libc::SIGKILL,
        }
    }

    #[cfg(any(target_os = "linux", test))]
    pub(crate) const fn number(self) -> i32 {
        self.number
    }
}

/// Result of one signal attempt against an authenticated process handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    not(target_os = "linux"),
    allow(
        dead_code,
        reason = "non-Linux signalling always fails before an outcome"
    )
)]
pub(crate) enum RuntimeProcessSignalOutcome {
    Delivered,
    AlreadyAbsent,
}

/// Capture a durable process identity only from mutually consistent provider,
/// pidfile, and operating-system evidence.
pub(crate) fn capture_runtime_process_identity(
    state_command: &CommandSpec,
    runtime_id: &str,
    creator_attempt_id: &str,
    pidfile: &Path,
) -> Result<RuntimeProcessIdentity> {
    if runtime_id.trim().is_empty() || creator_attempt_id.trim().is_empty() {
        return Err(SandboxError::OperationFailed {
            message: "runtime and creator-attempt identities must not be empty".to_owned(),
        });
    }
    let provider_pid = match runtime_process_state_for_creator_attempt(
        state_command,
        runtime_id,
        creator_attempt_id,
    )? {
        RuntimeProcessProviderObservation::Present { pid, .. } => pid,
        RuntimeProcessProviderObservation::ExplicitlyAbsent => {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "runtime {runtime_id:?} is explicitly absent; no process identity can be captured"
                ),
            });
        }
    };
    let pidfile_pid = read_regular_pidfile(pidfile)?;
    if pidfile_pid != provider_pid {
        return Err(crossed_pid_error(
            runtime_id,
            provider_pid,
            pidfile,
            pidfile_pid,
        ));
    }
    let birth = read_process_birth(provider_pid)?.ok_or_else(|| SandboxError::OperationFailed {
        message: format!(
            "runtime {runtime_id:?} process {provider_pid} disappeared before its birth identity could be captured"
        ),
    })?;
    let identity = RuntimeProcessIdentity {
        runtime_id: runtime_id.to_owned(),
        creator_attempt_id: creator_attempt_id.to_owned(),
        pid: provider_pid,
        birth,
    };
    match inspect_runtime_process_identity(&identity, state_command, pidfile)? {
        RuntimeProcessIdentityObservation::ExactLive => Ok(identity),
        RuntimeProcessIdentityObservation::ExplicitlyAbsent => Err(SandboxError::OperationFailed {
            message: format!(
                "runtime {runtime_id:?} disappeared while its process identity was being captured"
            ),
        }),
    }
}

/// Reauthenticate a persisted runtime process without performing an effect.
pub(crate) fn inspect_runtime_process_identity(
    identity: &RuntimeProcessIdentity,
    state_command: &CommandSpec,
    pidfile: &Path,
) -> Result<RuntimeProcessIdentityObservation> {
    let pidfile_pid = read_regular_pidfile(pidfile)?;
    if pidfile_pid != identity.pid {
        return Err(crossed_pid_error(
            &identity.runtime_id,
            identity.pid,
            pidfile,
            pidfile_pid,
        ));
    }
    let provider = runtime_process_state_for_creator_attempt(
        state_command,
        &identity.runtime_id,
        &identity.creator_attempt_id,
    )?;
    let observed_birth = read_process_birth(identity.pid)?;
    match provider {
        RuntimeProcessProviderObservation::Present { pid } => {
            if pid != identity.pid {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "runtime {:?} provider state names PID {pid}, expected authenticated PID {}",
                        identity.runtime_id, identity.pid
                    ),
                });
            }
            match observed_birth {
                Some(birth) if birth == identity.birth => {
                    Ok(RuntimeProcessIdentityObservation::ExactLive)
                }
                Some(_) => Err(SandboxError::OperationFailed {
                    message: format!(
                        "runtime {:?} PID {} was recycled with a different process birth; refusing to signal",
                        identity.runtime_id, identity.pid
                    ),
                }),
                None => Err(SandboxError::OperationFailed {
                    message: format!(
                        "runtime {:?} provider state names PID {}, but that process is absent",
                        identity.runtime_id, identity.pid
                    ),
                }),
            }
        }
        RuntimeProcessProviderObservation::ExplicitlyAbsent => match observed_birth {
            None => Ok(RuntimeProcessIdentityObservation::ExplicitlyAbsent),
            Some(birth) if birth == identity.birth => Err(SandboxError::OperationFailed {
                message: format!(
                    "runtime {:?} is absent while its authenticated process {} remains live",
                    identity.runtime_id, identity.pid
                ),
            }),
            Some(_) => Err(SandboxError::OperationFailed {
                message: format!(
                    "runtime {:?} is absent but PID {} was recycled; process absence is not interchangeable with the pidfile identity",
                    identity.runtime_id, identity.pid
                ),
            }),
        },
    }
}

/// Signal only the exact process incarnation authenticated by a Linux pidfd.
///
/// The caller must durably persist that this signal may exist before calling
/// this function. No provider-command or workload state is written here.
#[cfg(target_os = "linux")]
pub(crate) fn signal_authenticated_runtime_process(
    identity: &RuntimeProcessIdentity,
    state_command: &CommandSpec,
    pidfile: &Path,
    signal: RuntimeProcessSignal,
) -> Result<RuntimeProcessSignalOutcome> {
    use std::os::fd::{AsRawFd, FromRawFd as _, OwnedFd};

    let pid = i32::try_from(identity.pid).map_err(|_| SandboxError::OperationFailed {
        message: format!("runtime process PID {} does not fit pid_t", identity.pid),
    })?;
    // SAFETY: pidfd_open receives a validated positive PID and zero flags. The
    // returned descriptor is immediately placed in OwnedFd on success.
    let raw_fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0_u32) };
    if raw_fd == -1 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH)
            && inspect_runtime_process_identity(identity, state_command, pidfile)?
                == RuntimeProcessIdentityObservation::ExplicitlyAbsent
        {
            return Ok(RuntimeProcessSignalOutcome::AlreadyAbsent);
        }
        return Err(SandboxError::OperationFailed {
            message: format!(
                "failed to open an authenticated pidfd for runtime {:?} PID {}: {error}; no signal was sent",
                identity.runtime_id, identity.pid
            ),
        });
    }
    let raw_fd = i32::try_from(raw_fd).map_err(|_| SandboxError::OperationFailed {
        message: format!("pidfd {raw_fd} does not fit a host file descriptor"),
    })?;
    // SAFETY: pidfd_open returned a new owned descriptor above.
    let pidfd = unsafe { OwnedFd::from_raw_fd(raw_fd) };

    if inspect_runtime_process_identity(identity, state_command, pidfile)?
        == RuntimeProcessIdentityObservation::ExplicitlyAbsent
    {
        return Ok(RuntimeProcessSignalOutcome::AlreadyAbsent);
    }

    // SAFETY: pidfd names the process opened above, signal was restricted to a
    // libc named signal, siginfo is null, and flags must be zero.
    let result = unsafe {
        libc::syscall(
            libc::SYS_pidfd_send_signal,
            pidfd.as_raw_fd(),
            signal.number(),
            std::ptr::null::<libc::siginfo_t>(),
            0_u32,
        )
    };
    if result == 0 {
        return Ok(RuntimeProcessSignalOutcome::Delivered);
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH)
        && inspect_runtime_process_identity(identity, state_command, pidfile)?
            == RuntimeProcessIdentityObservation::ExplicitlyAbsent
    {
        return Ok(RuntimeProcessSignalOutcome::AlreadyAbsent);
    }
    Err(SandboxError::OperationFailed {
        message: format!(
            "pidfd signal {} for runtime {:?} PID {} failed: {error}; signal outcome is unknown",
            signal.number(),
            identity.runtime_id,
            identity.pid
        ),
    })
}

/// Non-Linux hosts cannot provide the pidfd identity guarantee.
#[cfg(not(target_os = "linux"))]
pub(crate) fn signal_authenticated_runtime_process(
    identity: &RuntimeProcessIdentity,
    _state_command: &CommandSpec,
    _pidfile: &Path,
    _signal: RuntimeProcessSignal,
) -> Result<RuntimeProcessSignalOutcome> {
    Err(SandboxError::BackendUnavailable {
        message: format!(
            "race-free pidfd signalling for runtime {:?} PID {} requires Linux",
            identity.runtime_id, identity.pid
        ),
    })
}

fn read_regular_pidfile(path: &Path) -> Result<u32> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;

        options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    }
    let file = options
        .open(path)
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to open runtime pidfile {} without following links: {error}; process identity remains unknown",
                path.display()
            ),
        })?;
    let metadata = file
        .metadata()
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to inspect opened runtime pidfile {}: {error}; process identity remains unknown",
                path.display()
            ),
        })?;
    if !metadata.file_type().is_file() {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "runtime pidfile {} is not a regular file; process identity remains unknown",
                path.display()
            ),
        });
    }
    let mut bytes = Vec::new();
    file.take(MAX_PIDFILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| SandboxError::OperationFailed {
            message: format!("failed to read runtime pidfile {}: {error}", path.display()),
        })?;
    if bytes.len() as u64 > MAX_PIDFILE_BYTES {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "runtime pidfile {} exceeded {MAX_PIDFILE_BYTES} bytes; process identity remains unknown",
                path.display()
            ),
        });
    }
    let rendered = std::str::from_utf8(&bytes).map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "failed to decode runtime pidfile {} as UTF-8: {error}",
            path.display()
        ),
    })?;
    let pid = rendered
        .trim()
        .parse::<u32>()
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to parse runtime pidfile {}: {error}",
                path.display()
            ),
        })?;
    if pid == 0 {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "runtime pidfile {} names reserved PID zero; process identity remains unknown",
                path.display()
            ),
        });
    }
    Ok(pid)
}

fn crossed_pid_error(
    runtime_id: &str,
    expected: u32,
    pidfile: &Path,
    observed: u32,
) -> SandboxError {
    SandboxError::OperationFailed {
        message: format!(
            "runtime {runtime_id:?} provider state names PID {expected}, but pidfile {} names PID {observed}; refusing to signal crossed process identity",
            pidfile.display()
        ),
    }
}

#[cfg(target_os = "linux")]
fn read_process_birth(pid: u32) -> Result<Option<RuntimeProcessBirth>> {
    let path = std::path::PathBuf::from(format!("/proc/{pid}/stat"));
    let stat = match std::fs::read_to_string(&path) {
        Ok(stat) => stat,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "failed to read process identity {}: {error}",
                    path.display()
                ),
            });
        }
    };
    parse_linux_process_birth(pid, &stat).map(Some)
}

#[cfg(any(target_os = "linux", test))]
fn parse_linux_process_birth(pid: u32, stat: &str) -> Result<RuntimeProcessBirth> {
    let fields = stat
        .rfind(") ")
        .map(|close| &stat[close + 2..])
        .ok_or_else(|| SandboxError::OperationFailed {
            message: format!("process {pid} stat omitted its command terminator"),
        })?
        .split_ascii_whitespace()
        .collect::<Vec<_>>();
    // After the command, index 19 is field 22 (`starttime`).
    let ticks = fields
        .get(19)
        .ok_or_else(|| SandboxError::OperationFailed {
            message: format!("process {pid} stat omitted its birth ticks"),
        })?
        .parse::<u64>()
        .map_err(|error| SandboxError::OperationFailed {
            message: format!("process {pid} stat carries invalid birth ticks: {error}"),
        })?;
    Ok(RuntimeProcessBirth::LinuxProcStartTicks { ticks })
}

#[cfg(target_os = "macos")]
fn read_process_birth(pid: u32) -> Result<Option<RuntimeProcessBirth>> {
    use std::mem::{MaybeUninit, size_of};

    let raw_pid = i32::try_from(pid).map_err(|_| SandboxError::OperationFailed {
        message: format!("runtime PID {pid} cannot be inspected on this host"),
    })?;
    let mut info = MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    let size =
        i32::try_from(size_of::<libc::proc_bsdinfo>()).expect("proc_bsdinfo size should fit c_int");
    // SAFETY: info is a correctly sized writable proc_bsdinfo buffer.
    let returned = unsafe {
        libc::proc_pidinfo(
            raw_pid,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    if returned == 0 {
        let inspection_error = std::io::Error::last_os_error();
        // SAFETY: signal zero probes existence only and performs no mutation.
        if unsafe { libc::kill(raw_pid, 0) } != 0
            && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
        {
            return Ok(None);
        }
        return Err(SandboxError::OperationFailed {
            message: format!(
                "proc_pidinfo could not authenticate runtime PID {pid}: {inspection_error}"
            ),
        });
    }
    if returned != size {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "proc_pidinfo returned {returned} bytes for runtime PID {pid}, expected {size}"
            ),
        });
    }
    // SAFETY: proc_pidinfo returned the complete structure size above.
    let info = unsafe { info.assume_init() };
    if info.pbi_pid != pid {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "proc_pidinfo returned PID {} while inspecting runtime PID {pid}",
                info.pbi_pid
            ),
        });
    }
    Ok(Some(RuntimeProcessBirth::AppleBsdStartTime {
        seconds: info.pbi_start_tvsec,
        microseconds: info.pbi_start_tvusec,
    }))
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn read_process_birth(pid: u32) -> Result<Option<RuntimeProcessBirth>> {
    Err(SandboxError::OperationFailed {
        message: format!(
            "stable process-birth inspection for runtime PID {pid} is unavailable on this platform"
        ),
    })
}

#[cfg(unix)]
fn named_signal_number(name: &str) -> Option<i32> {
    match name {
        "ABRT" | "IOT" => Some(libc::SIGABRT),
        "ALRM" => Some(libc::SIGALRM),
        "BUS" => Some(libc::SIGBUS),
        "CHLD" | "CLD" => Some(libc::SIGCHLD),
        "CONT" => Some(libc::SIGCONT),
        "FPE" => Some(libc::SIGFPE),
        "HUP" => Some(libc::SIGHUP),
        "ILL" => Some(libc::SIGILL),
        "INT" => Some(libc::SIGINT),
        "IO" | "POLL" => Some(libc::SIGIO),
        "KILL" => Some(libc::SIGKILL),
        "PIPE" => Some(libc::SIGPIPE),
        "PROF" => Some(libc::SIGPROF),
        "QUIT" => Some(libc::SIGQUIT),
        "SEGV" => Some(libc::SIGSEGV),
        "STOP" => Some(libc::SIGSTOP),
        "SYS" => Some(libc::SIGSYS),
        "TERM" => Some(libc::SIGTERM),
        "TRAP" => Some(libc::SIGTRAP),
        "TSTP" => Some(libc::SIGTSTP),
        "TTIN" => Some(libc::SIGTTIN),
        "TTOU" => Some(libc::SIGTTOU),
        "URG" => Some(libc::SIGURG),
        "USR1" => Some(libc::SIGUSR1),
        "USR2" => Some(libc::SIGUSR2),
        "VTALRM" => Some(libc::SIGVTALRM),
        "WINCH" => Some(libc::SIGWINCH),
        "XCPU" => Some(libc::SIGXCPU),
        "XFSZ" => Some(libc::SIGXFSZ),
        _ => None,
    }
}

#[cfg(not(unix))]
fn named_signal_number(_name: &str) -> Option<i32> {
    None
}

fn is_named_signal_number(number: i32) -> bool {
    #[cfg(unix)]
    {
        [
            "ABRT", "ALRM", "BUS", "CHLD", "CONT", "FPE", "HUP", "ILL", "INT", "IO", "KILL",
            "PIPE", "PROF", "QUIT", "SEGV", "STOP", "SYS", "TERM", "TRAP", "TSTP", "TTIN", "TTOU",
            "URG", "USR1", "USR2", "VTALRM", "WINCH", "XCPU", "XFSZ",
        ]
        .into_iter()
        .filter_map(named_signal_number)
        .any(|candidate| candidate == number)
    }
    #[cfg(not(unix))]
    {
        let _ = number;
        false
    }
}
