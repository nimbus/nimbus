//! Durable authentication for the exact gvproxy process incarnation.
//!
//! A PID is only a reusable locator. The parent records the operating-system
//! birth token of the child it actually spawned, together with the
//! parent-issued forwarder authority. Recovery may signal a PID only while both
//! identities still match.

use std::fs;
use std::io;
use std::path::Path;

use nimbus::Error;
use nimbus_machine::MachineForwarderAuthority;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct GvproxyProcessReceipt {
    forwarder_authority: MachineForwarderAuthority,
    process: MachineProcessIdentity,
}

impl GvproxyProcessReceipt {
    pub(super) fn capture(
        pid: u32,
        forwarder_authority: &MachineForwarderAuthority,
    ) -> Result<Self, Error> {
        let process = read_process_identity(pid)?.ok_or_else(|| {
            Error::Internal(format!(
                "gvproxy process {pid} exited before its exact birth identity could be recorded"
            ))
        })?;
        Ok(Self {
            forwarder_authority: forwarder_authority.clone(),
            process,
        })
    }

    pub(super) fn load_authenticated(
        path: &Path,
        expected: &MachineForwarderAuthority,
    ) -> Result<Option<Self>, Error> {
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(Error::Internal(format!(
                    "failed to read gvproxy process identity {}: {error}",
                    path.display()
                )));
            }
        };
        let receipt = serde_json::from_slice::<Self>(&bytes).map_err(|error| {
            Error::Internal(format!(
                "failed to decode gvproxy process identity {}: {error}",
                path.display()
            ))
        })?;
        expected
            .authenticate(&receipt.forwarder_authority)
            .map_err(|error| {
                Error::conflict(format!(
                    "gvproxy process identity {} belongs to a different parent forwarder \
                     incarnation: {error}",
                    path.display()
                ))
            })?;
        Ok(Some(receipt))
    }

    pub(super) fn process(&self) -> &MachineProcessIdentity {
        &self.process
    }

    #[cfg(test)]
    pub(super) fn with_substituted_birth_for_test(mut self) -> Self {
        match &mut self.process.birth {
            MachineProcessBirth::LinuxProcStartTicks { ticks } => {
                *ticks = ticks.saturating_add(1);
            }
            MachineProcessBirth::AppleBsdStartTime { microseconds, .. } => {
                *microseconds = microseconds.saturating_add(1);
            }
        }
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct MachineProcessIdentity {
    pid: u32,
    birth: MachineProcessBirth,
}

impl MachineProcessIdentity {
    pub(super) fn pid(&self) -> u32 {
        self.pid
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum MachineProcessBirth {
    LinuxProcStartTicks { ticks: u64 },
    AppleBsdStartTime { seconds: u64, microseconds: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ExactProcessObservation {
    Exact,
    Absent,
    Replaced,
}

pub(super) fn observe_exact_process(
    expected: &MachineProcessIdentity,
) -> Result<ExactProcessObservation, Error> {
    match read_process_identity(expected.pid)? {
        Some(observed) if observed == *expected => Ok(ExactProcessObservation::Exact),
        Some(_) => Ok(ExactProcessObservation::Replaced),
        None => Ok(ExactProcessObservation::Absent),
    }
}

#[cfg(target_os = "linux")]
fn read_process_identity(pid: u32) -> Result<Option<MachineProcessIdentity>, Error> {
    let path = std::path::PathBuf::from(format!("/proc/{pid}/stat"));
    let stat = match fs::read_to_string(&path) {
        Ok(stat) => stat,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(Error::Internal(format!(
                "failed to read process identity {}: {error}",
                path.display()
            )));
        }
    };
    parse_linux_process_stat(pid, &stat).map(Some)
}

#[cfg(any(target_os = "linux", test))]
fn parse_linux_process_stat(pid: u32, stat: &str) -> Result<MachineProcessIdentity, Error> {
    let fields = stat
        .rfind(") ")
        .map(|close| &stat[close + 2..])
        .ok_or_else(|| {
            Error::Internal(format!("process {pid} stat omitted its command terminator"))
        })?
        .split_ascii_whitespace()
        .collect::<Vec<_>>();
    // After the command, index 0 is field 3 (`state`) and index 19 is field 22
    // (`starttime`).
    let ticks = fields
        .get(19)
        .ok_or_else(|| Error::Internal(format!("process {pid} stat omitted its birth ticks")))?
        .parse::<u64>()
        .map_err(|error| {
            Error::Internal(format!(
                "process {pid} stat carries invalid birth ticks: {error}"
            ))
        })?;
    Ok(MachineProcessIdentity {
        pid,
        birth: MachineProcessBirth::LinuxProcStartTicks { ticks },
    })
}

#[cfg(target_os = "macos")]
fn read_process_identity(pid: u32) -> Result<Option<MachineProcessIdentity>, Error> {
    use std::mem::{MaybeUninit, size_of};

    let inspected_pid = i32::try_from(pid)
        .map_err(|_| Error::Internal(format!("gvproxy PID {pid} cannot be inspected")))?;
    let mut info = MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    let size =
        i32::try_from(size_of::<libc::proc_bsdinfo>()).expect("proc_bsdinfo size should fit c_int");
    // SAFETY: `info` is a correctly sized writable proc_bsdinfo buffer, and
    // `proc_pidinfo` initializes exactly the returned byte count.
    let returned = unsafe {
        libc::proc_pidinfo(
            inspected_pid,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    if returned == 0 {
        let inspection_error = io::Error::last_os_error();
        if matches!(inspection_error.raw_os_error(), Some(libc::ESRCH)) {
            return Ok(None);
        }
        // SAFETY: signal zero probes existence only.
        if unsafe { libc::kill(inspected_pid, 0) } != 0
            && matches!(io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH))
        {
            return Ok(None);
        }
        return Err(Error::Internal(format!(
            "proc_pidinfo could not authenticate gvproxy PID {pid}: {inspection_error}"
        )));
    }
    if returned != size {
        return Err(Error::Internal(format!(
            "proc_pidinfo returned {returned} bytes for gvproxy PID {pid}, expected {size}"
        )));
    }
    // SAFETY: the call returned the complete structure size above.
    let info = unsafe { info.assume_init() };
    if info.pbi_pid != pid {
        return Err(Error::Internal(format!(
            "proc_pidinfo returned PID {} while inspecting gvproxy PID {pid}",
            info.pbi_pid
        )));
    }
    Ok(Some(MachineProcessIdentity {
        pid,
        birth: MachineProcessBirth::AppleBsdStartTime {
            seconds: info.pbi_start_tvsec,
            microseconds: info.pbi_start_tvusec,
        },
    }))
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn read_process_identity(pid: u32) -> Result<Option<MachineProcessIdentity>, Error> {
    Err(Error::Internal(format!(
        "stable process-birth inspection for gvproxy PID {pid} is unavailable on this platform"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_stat_parser_uses_birth_ticks_after_parenthesized_command() {
        let stat = "42 (gvproxy worker) S 1 42 42 0 -1 0 0 0 0 0 0 0 0 0 20 0 1 0 987654 0";
        assert_eq!(
            parse_linux_process_stat(42, stat).expect("stat should parse"),
            MachineProcessIdentity {
                pid: 42,
                birth: MachineProcessBirth::LinuxProcStartTicks { ticks: 987654 },
            }
        );
    }
}
