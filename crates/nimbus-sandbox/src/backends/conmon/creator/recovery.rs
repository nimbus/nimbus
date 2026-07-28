//! Durable creator-process identity and fresh-process containment observation.
//!
//! A numeric PID is only a locator. The receipt pairs it with an operating
//! system birth token and the fresh process group established for the attempt.
//! Recovery may observe through this module, but provider/runtime state remains
//! owned by the container and krun adapters.

use serde::{Deserialize, Serialize};

use super::process_group_is_absent;
use crate::error::{Result, SandboxError};

#[cfg(test)]
#[path = "recovery/tests.rs"]
mod tests;

/// Durable identity for the exact creator process spawned by one logical
/// attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CreatorAttemptReceipt {
    attempt_id: String,
    process: CreatorProcessIdentity,
}

impl CreatorAttemptReceipt {
    pub(crate) fn attempt_id(&self) -> &str {
        &self.attempt_id
    }

    #[cfg(test)]
    pub(crate) fn process(&self) -> &CreatorProcessIdentity {
        &self.process
    }

    #[cfg(test)]
    pub(crate) fn for_test(attempt_id: impl Into<String>) -> Self {
        Self {
            attempt_id: attempt_id.into(),
            process: CreatorProcessIdentity {
                pid: u32::MAX,
                process_group: u32::MAX,
                birth: CreatorProcessBirth::LinuxProcStartTicks { ticks: 1 },
            },
        }
    }

    #[cfg(test)]
    pub(crate) fn with_substituted_birth_for_test(mut self) -> Self {
        match &mut self.process.birth {
            CreatorProcessBirth::LinuxProcStartTicks { ticks } => *ticks = ticks.saturating_add(1),
            CreatorProcessBirth::AppleBsdStartTime { microseconds, .. } => {
                *microseconds = microseconds.saturating_add(1)
            }
        }
        self
    }
}

/// Durable reason a creator attempt can no longer materialize provider effects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum CreatorQuiescenceProof {
    /// The operating-system spawn was rejected before any child existed.
    NeverSpawned { attempt_id: String },
    /// A wrapper process existed, but its pre-effect launch gate was never
    /// released and its exact containment was reaped.
    LaunchGateNeverReleased { attempt_id: String },
    /// The exact spawned process birth and its containment were observed dead.
    DeadContained { receipt: CreatorAttemptReceipt },
}

impl CreatorQuiescenceProof {
    pub(crate) fn never_spawned(attempt_id: impl Into<String>) -> Self {
        Self::NeverSpawned {
            attempt_id: attempt_id.into(),
        }
    }

    pub(crate) fn dead_contained(receipt: CreatorAttemptReceipt) -> Self {
        Self::DeadContained { receipt }
    }

    pub(crate) fn launch_gate_never_released(attempt_id: impl Into<String>) -> Self {
        Self::LaunchGateNeverReleased {
            attempt_id: attempt_id.into(),
        }
    }

    pub(crate) fn attempt_id(&self) -> &str {
        match self {
            Self::NeverSpawned { attempt_id } | Self::LaunchGateNeverReleased { attempt_id } => {
                attempt_id
            }
            Self::DeadContained { receipt } => receipt.attempt_id(),
        }
    }
}

/// Stable identity for one operating-system process incarnation and its
/// attempt-scoped containment group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CreatorProcessIdentity {
    pid: u32,
    process_group: u32,
    birth: CreatorProcessBirth,
}

impl CreatorProcessIdentity {
    #[cfg(test)]
    pub(crate) fn pid(&self) -> u32 {
        self.pid
    }

    #[cfg(test)]
    pub(crate) fn process_group(&self) -> u32 {
        self.process_group
    }
}

/// Platform-native token that changes when a numeric PID is recycled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum CreatorProcessBirth {
    LinuxProcStartTicks { ticks: u64 },
    AppleBsdStartTime { seconds: u64, microseconds: u64 },
}

/// Fresh-process observation of the creator containment only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CreatorContainmentObservation {
    /// The exact process incarnation remains live in its recorded process
    /// group. Recovery must not signal or supersede its owner.
    Live,
    /// The exact process is absent and the recorded group is absent.
    DeadContained,
    /// The exact process changed containment or disappeared while the group
    /// remains live. Numeric group identity is not safe to signal.
    Escaped { reason: String },
    /// Birth or containment could not be authenticated. Recovery remains
    /// fenced without guessing.
    Unknown { reason: String },
}

pub(super) fn capture_creator_attempt(
    attempt_id: &str,
    pid: u32,
    #[cfg(unix)] expected_process_group: i32,
) -> Result<CreatorAttemptReceipt> {
    if attempt_id.trim().is_empty() {
        return Err(SandboxError::OperationFailed {
            message: "creator attempt identity must not be empty".to_owned(),
        });
    }

    #[cfg(unix)]
    let expected_process_group =
        u32::try_from(expected_process_group).map_err(|_| SandboxError::OperationFailed {
            message: format!(
                "creator process group {expected_process_group} cannot be represented durably"
            ),
        })?;
    #[cfg(not(unix))]
    let expected_process_group = pid;

    let Some(observed) = read_process_identity(pid)? else {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "creator process {pid} disappeared before its birth receipt could be captured; \
                 creator handoff remains unknown"
            ),
        });
    };
    if observed.process_group != expected_process_group {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "creator process {pid} escaped expected process group {expected_process_group} \
                 into {}; creator handoff remains unknown",
                observed.process_group
            ),
        });
    }

    Ok(CreatorAttemptReceipt {
        attempt_id: attempt_id.to_owned(),
        process: observed,
    })
}

pub(crate) fn observe_creator_containment(
    receipt: &CreatorAttemptReceipt,
) -> CreatorContainmentObservation {
    let expected = &receipt.process;
    match read_process_identity(expected.pid) {
        Ok(Some(observed)) if observed.birth != expected.birth => {
            CreatorContainmentObservation::Unknown {
                reason: format!(
                    "creator PID {} was recycled with a different process birth",
                    expected.pid
                ),
            }
        }
        Ok(Some(observed)) if observed.process_group != expected.process_group => {
            CreatorContainmentObservation::Escaped {
                reason: format!(
                    "exact creator PID {} moved from process group {} to {}",
                    expected.pid, expected.process_group, observed.process_group
                ),
            }
        }
        Ok(Some(_)) => CreatorContainmentObservation::Live,
        Ok(None) => observe_group_after_leader_exit(expected.process_group),
        Err(error) => CreatorContainmentObservation::Unknown {
            reason: format!(
                "cannot authenticate creator PID {} birth: {error}",
                expected.pid
            ),
        },
    }
}

/// Authenticate the exact conmon PID receipt prepared before the creator
/// attempt, and only after the caller has independently authenticated creator
/// containment plus explicit runtime absence. The dead receipt remains durable
/// until a later acknowledged lifecycle checkpoint or the next pre-spawn
/// cleanup, so acknowledgement loss cannot erase recovery evidence.
pub(crate) fn confirm_dead_conmon_receipt(conmon_pidfile: &std::path::Path) -> Result<()> {
    let metadata =
        std::fs::symlink_metadata(conmon_pidfile).map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => SandboxError::OperationFailed {
                message: format!(
                    "creator receipt {} is absent after a spawned attempt; an escaped provider \
                     handoff remains possible",
                    conmon_pidfile.display()
                ),
            },
            _ => SandboxError::OperationFailed {
                message: format!(
                    "cannot inspect creator receipt {}: {error}; provider handoff remains unknown",
                    conmon_pidfile.display()
                ),
            },
        })?;
    if !metadata.file_type().is_file() {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "creator receipt {} is not a regular file; provider handoff remains unknown",
                conmon_pidfile.display()
            ),
        });
    }
    let pid = crate::backends::conmon::lifecycle::read_pid(conmon_pidfile)?;
    if crate::process::pid_is_alive(pid) {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "creator containment is absent but receipt {} names live PID {pid}; provider \
                 escaped the creator group",
                conmon_pidfile.display()
            ),
        });
    }
    Ok(())
}

#[cfg(unix)]
fn observe_group_after_leader_exit(process_group: u32) -> CreatorContainmentObservation {
    let Ok(process_group) = i32::try_from(process_group) else {
        return CreatorContainmentObservation::Unknown {
            reason: format!(
                "creator process group {process_group} cannot be inspected on this host"
            ),
        };
    };
    match process_group_is_absent(process_group) {
        Ok(true) => CreatorContainmentObservation::DeadContained,
        Ok(false) => CreatorContainmentObservation::Escaped {
            reason: format!(
                "creator leader is absent while process group {process_group} remains live"
            ),
        },
        Err(error) => CreatorContainmentObservation::Unknown {
            reason: error.to_string(),
        },
    }
}

#[cfg(not(unix))]
fn observe_group_after_leader_exit(process_group: u32) -> CreatorContainmentObservation {
    CreatorContainmentObservation::Unknown {
        reason: format!(
            "fresh-process containment observation is unavailable for process group \
             {process_group} on this platform"
        ),
    }
}

#[cfg(target_os = "linux")]
fn read_process_identity(pid: u32) -> Result<Option<CreatorProcessIdentity>> {
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
    parse_linux_process_stat(pid, &stat).map(Some)
}

#[cfg(any(target_os = "linux", test))]
fn parse_linux_process_stat(pid: u32, stat: &str) -> Result<CreatorProcessIdentity> {
    let fields = stat
        .rfind(") ")
        .map(|close| &stat[close + 2..])
        .ok_or_else(|| SandboxError::OperationFailed {
            message: format!("process {pid} stat omitted its command terminator"),
        })?
        .split_ascii_whitespace()
        .collect::<Vec<_>>();
    // After the command, index 0 is field 3 (`state`), index 2 is field 5
    // (`pgrp`), and index 19 is field 22 (`starttime`).
    let process_group = fields
        .get(2)
        .ok_or_else(|| SandboxError::OperationFailed {
            message: format!("process {pid} stat omitted its process group"),
        })?
        .parse::<u32>()
        .map_err(|error| SandboxError::OperationFailed {
            message: format!("process {pid} stat carries an invalid process group: {error}"),
        })?;
    let ticks = fields
        .get(19)
        .ok_or_else(|| SandboxError::OperationFailed {
            message: format!("process {pid} stat omitted its birth ticks"),
        })?
        .parse::<u64>()
        .map_err(|error| SandboxError::OperationFailed {
            message: format!("process {pid} stat carries invalid birth ticks: {error}"),
        })?;
    Ok(CreatorProcessIdentity {
        pid,
        process_group,
        birth: CreatorProcessBirth::LinuxProcStartTicks { ticks },
    })
}

#[cfg(target_os = "macos")]
fn read_process_identity(pid: u32) -> Result<Option<CreatorProcessIdentity>> {
    use std::mem::{MaybeUninit, size_of};

    let pid = i32::try_from(pid).map_err(|_| SandboxError::OperationFailed {
        message: format!("creator PID {pid} cannot be inspected on this host"),
    })?;
    let mut info = MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    let size =
        i32::try_from(size_of::<libc::proc_bsdinfo>()).expect("proc_bsdinfo size should fit c_int");
    // SAFETY: `info` points to a correctly sized writable proc_bsdinfo buffer,
    // and `proc_pidinfo` initializes exactly the returned byte count.
    let returned = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    if returned == 0 {
        let inspection_error = std::io::Error::last_os_error();
        // SAFETY: signal zero probes existence only.
        if unsafe { libc::kill(pid, 0) } != 0 {
            let existence_error = std::io::Error::last_os_error();
            if matches!(existence_error.raw_os_error(), Some(libc::ESRCH)) {
                return Ok(None);
            }
        }
        return Err(SandboxError::OperationFailed {
            message: format!(
                "proc_pidinfo could not authenticate creator PID {pid}: {inspection_error}"
            ),
        });
    }
    if returned != size {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "proc_pidinfo returned {returned} bytes for creator PID {pid}, expected {size}"
            ),
        });
    }
    // SAFETY: the call returned the complete structure size above.
    let info = unsafe { info.assume_init() };
    let observed_pid = u32::try_from(pid).expect("validated PID should fit u32");
    if info.pbi_pid != observed_pid {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "proc_pidinfo returned PID {} while inspecting creator PID {observed_pid}",
                info.pbi_pid
            ),
        });
    }
    Ok(Some(CreatorProcessIdentity {
        pid: observed_pid,
        process_group: info.pbi_pgid,
        birth: CreatorProcessBirth::AppleBsdStartTime {
            seconds: info.pbi_start_tvsec,
            microseconds: info.pbi_start_tvusec,
        },
    }))
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn read_process_identity(pid: u32) -> Result<Option<CreatorProcessIdentity>> {
    Err(SandboxError::OperationFailed {
        message: format!(
            "stable process-birth inspection for creator PID {pid} is unavailable on this platform"
        ),
    })
}
